<!-- front-matter: date=2026-08-04 | repo_type=module | unit=backbone-edi | focus=maturity | roster=chair, skeptic, steelman, yagni-business (standing) + ddd-bounded-context, contract-seat (context) + domain-expert (invited). Skeptic/steelman/chair ran as isolated subagents. -->

# Chair's Report — backbone-edi maturity review (post-regen)

**Trigger:** author re-ran `metaphor-schema`, flagged "some files not safe," asked "is this module complete, nothing missing?"

**Framing answers (up front, because they shape the call):**
- **Are the regen'd files safe?** Yes, in the narrow sense: cargo check is clean (0/0) and the two regressions deleted zero-caller code (`value_objects/` had no references; `EdiQueryServiceImpl` had no impl and no callers). What the regen did was *expose* pre-existing maturity gaps — it is not itself a defect.
- **Is the module complete?** No. Four gaps remain, in this severity order: (1) the default route composer ships unguarded storage mutation as the supported contract; (2) the EDI lifecycle is documented (ADR-001) but unenforced (`PermitAllPolicy`); (3) dead weight from an unfinished skeleton-extraction (5 orphaned `example_*` files + 2 wired stragglers + `value_objects/` + impl-less `EdiQueryService` trait); (4) stale "Minimal … module skeleton" copy in `Cargo.toml:5` / `docs/README.md`.

---

## 1. Best call (one move)

**Flip the default route composer so unguarded CRUD is opt-in, not the default.**

Concretely: `EdiModule::routes()` and `routes::create_stateless_routes` mount `create_readonly_edi_routes` (+ the validated `EdiWriteService` write router where one exists), and the current `all_crud_routes()` is renamed to an explicit opt-in (e.g. `unguarded_admin_routes`) that no default path calls. The `#[deprecated]` on `routes()` is kept but its body becomes the guarded composer, not the unguarded one.

**Why this one, not the alternatives:** it is the smallest move that closes the load-bearing production hazard (skeptic's failure scenario: composer calls `routes()` → bypasses `EdiWriteService` → business-key dedup skipped → partner PO retransmit inserts duplicate `edi_documents` row → duplicate `sales_orders`, PRD idempotency silently fails). It also turns contract-seat's "storage-row mutation leaking past the domain interface" into a compile-time choice. It does not preclude the dead-code deletion (yagni) or the lifecycle `DomainPolicy` (domain-expert) — those become additive follow-ups rather than blockers.

**Residual negative value (concrete):** N consumers of `routes()` / `create_stateless_routes` for EdiDocument writes will fail to compile and must add one explicit `.merge(unguarded_admin_routes(..))` or `.merge(validated_writes)` line. If N=0 today (only the saga test consumes `EdiWriteService` directly), cost is zero. If N>0, cost is N × (read deprecation note + one line) — bounded, loud, immediate. The domain hole (`PermitAllPolicy`) and the dead weight remain open as separate items.

**Reversibility:** Easy — one-line revert per call site. Not a one-way door.

**Evidence that would flip the call:** A current consumer that *intentionally* relies on `routes()` for production EdiDocument writes (trusted/admin path). That converts the leak from "default bug" to "supported contract" and shifts the Best call to **replace `PermitAllPolicy` with a transition-table `DomainPolicy<EdiDocument>`** (domain-expert's move), because the bypass would then have to be sealed inside the aggregate, not at the composer.

---

## 2. Disagreement map

**Tension A — Skeleton vs. production pillar (the framing fight).**
- Crux: is backbone-edi a framework reference module or a Tier-5 production pillar?
- Steelman (alone) held the skeleton framing. Everyone else (skeptic, ddd, contract, domain) produced PRD/FSD/`tests/edi_selling_seam.rs:42-53` showing a real cross-module read from `selling.sales_orders`, ADR-001/0008/0011, v0.4.1 with RLS+outbox parity.
- Resolution: steelman overturned on evidence. This is the load-bearing decision — it converts every "transitional state is fine for a skeleton" argument into "this is a hole in production."

**Tension B — Delete-first vs. enforce-first (sequencing).**
- Crux: what is the cheapest first move — remove the dead weight (yagni) or close the lifecycle hole (domain-expert)?
- yagni-business: an afternoon of `git rm` removes reader drag and frees attention. domain-expert: a 15-line transition table is the actual production hole.
- Resolution: neither is the Best call. The composer flip (contract-seat) subsumes both — it is smaller than the policy and addresses a bigger blast radius than the dead code. Deletion and `DomainPolicy` follow.

**Tension C — Delete vs. implement for `EdiQueryService` + `example_*`.**
- Crux: is the leaked/compiled-in surface teaching-skeleton residue (delete) or a pending API (implement)?
- ddd-bounded-context says delete/fence (boundary cleanup). contract-seat says implement-or-remove (contract honesty). Both directions converge on **remove** — there is no seat arguing the trait or the example code is load-bearing for a real consumer. No fake merge; the sides agree on the action, disagree only on the framing word.

---

## 3. Recommendations (ranked by leverage)

| # | Move | Leverage | Residual negative | Reversibility | Evidence to flip |
|---|------|----------|-------------------|---------------|------------------|
| 1 | **Flip the default composer** — `routes()` / `create_stateless_routes` mount read-only + validated writes; rename `all_crud_routes` → explicit `unguarded_admin_routes` opt-in. | Highest. Converts the single biggest production hazard into a compile-time choice; bounds contract-seat's leak at the boundary. | N consumers add one opt-in line. N today appears to be 0. | Easy. | A consumer intentionally using `routes()` for production writes → DomainPolicy becomes #1. |
| 2 | **Delete the dead weight** — `git rm` the 5 unwired `example_*` files + `example_saga_workflow.rs` + `example_dto.rs` + `value_objects/`; drop the impl-less `EdiQueryService` trait. | High, cheap. Removes reader drag, kills the steelman trap, stops regen re-orphaning. ~half-day. | Zero production impact (all dead code). Some example/saga reference material lost — recoverable from git. | Easy (git revert). | A consumer of `EdiQueryService` discovered → re-add as impl, not trait. |
| 3 | **Replace `PermitAllPolicy` with a transition-table `DomainPolicy<EdiDocument>`** enforcing ADR-001's 6-state lifecycle at the aggregate root. | Closes domain-expert's hole *once the composer bypass is sealed*. ~15-line transition table + tests. | Mutations that skip the policy (storage-bypass routes) are unaffected — which is exactly why this is #3, not #1. | Easy. | Discovery that status is set only through `EdiWriteService` already → policy becomes redundant documentation and this drops to parking lot. |
| 4 | **Refresh stale skeleton copy** in `Cargo.toml:5` and `docs/README.md`. | Low effort, kills the steelman framing trap at the source so future reviewers don't relitigate Tension A. | None. | Trivial. | None — always do. |

---

## 4. Maturity scorecard

| Seat | Maturity axis | Score | One-sentence justification |
|------|--------------|-------|---------------------------|
| ddd-bounded-context | bounded-context language consistent + contracts stable under change | **2 / 5** | Two bounded contexts (generic Example skeleton + real EDI exchange) coexist in one crate's public surface and the boundary was never finished — `example_dto` and `example_saga_workflow` are compiled into the EDI module's exports while five sibling `example_*` files sit orphaned on disk. |
| contract-seat | outward contract explicit/minimal, internals free to change, consumers depend only on deliberate promises | **2 / 5** | The default route composer exports unguarded storage-row mutation as if it were the supported write contract, and `EdiQueryService` is shipped as a cross-module read API with no implementation — two non-deliberate promises leaking from the module. |
| domain-expert | ubiquitous language consistent end-to-end + model can represent every real business state/rule incl. edge cases | **2 / 5** | The ubiquitous language is present (`EdiStatus` enumerates 6 states, ADR-001 documents the transitions), but `PermitAllPolicy` means the aggregate cannot refuse an illegal transition — the language describes rules the model does not enforce. |

Aggregate read: the module sits at "production pillar still finishing its skeleton-extraction migration." All three axes are recoverable in days, not weeks — none of the 2/5s are structural.

---

## 5. Parking lot (out of maturity focus)

- **Stale skeleton framing text** in `Cargo.toml:5` and `docs/README.md` ("Minimal Backbone Framework module skeleton") — Recommendation #4; parked here because it is a doc fix, not a maturity axis.
- **Whether event sourcing + snapshot store is premature** at current EDI volume — separate from this review; revisit when inbound throughput numbers exist.
- **Whether to spin the validated write router into its own first-class compose helper** (e.g. `create_validated_edi_routes`) so consumers compose read + validated-writes without naming `EdiWriteService` directly — a contract-tidiness item, not blocking.
- **OpenAPI / gRPC feature flags** — declared in `Cargo.toml`; actual consumer uptake unverified. Track as contract surface audit.
- **Pre-existing example_* orphaning predates this regen** — the regen made it more visible but did not cause it; track separately so it isn't mis-attributed to the next schema-generator run.

---

## Key file anchors

- `src/lib.rs` (lines 75-94: default unguarded CRUD + `#[deprecated] routes`)
- `src/routes/mod.rs` (lines 39-54: `create_stateless_routes` vs `create_readonly_edi_routes`)
- `src/domain/services/edi_document_domain_policy.rs` (line 15: `PermitAllPolicy`)
- `src/domain/mod.rs` (value_objects missing — regen regression 1)
- `src/exports/services.rs` (`EdiQueryService` trait, impl-less — regen regression 2)
- `src/application/workflows/mod.rs:1` + `src/application/dto/mod.rs:8` (compiled-in example stragglers)
- `src/application/service/edi_write_service.rs:65` (the validated write path that the default composer bypasses)

**Verdict:** Regen is safe to ship; the module is not complete. The Best call is the composer flip (Recommendation #1) — it is the only move that closes a production hazard at zero residual cost if N=0, and it makes the other three recommendations additive rather than blocking.
