<!-- front-matter: date=2026-08-04 | origin=maturity council review (2026-08-04-module-backbone-edi-maturity.md) | scope=framework-level follow-ups that the module cannot resolve itself -->

# Framework follow-ups from the backbone-edi maturity review

## Why this exists

The maturity council (`2026-08-04-module-backbone-edi-maturity.md`) produced four recommendations.
**#2 and #4 are done** (skeleton dead-weight removed; identity copy refreshed — commits on
`chore/edi-shed-skeleton-dead-weight`). **#1, #3, and the `EdiQueryService` removal are blocked at the
module level**: every one of them runs into the same wall — the codegen regen model leaves no
extension point where the fix can land without being reverted by the next `metaphor schema generate`,
and one of them (#3) is based on a trait that is never invoked. This document routes those items to
the framework, where they can actually be fixed, so they are not lost.

**One piece of good news that changes the framing:** the module's real write path
(`EdiWriteService`) is *already* correctly enforced — SQL `WHERE status=...` guards on
`mark_mapped` / `mark_failed` / `acknowledge` (`src/infrastructure/persistence/edi_document_repository.rs`).
So the EDI lifecycle is sound on the validated path; the gaps below are about the *generic CRUD*
surface and the *defaults*, not the core domain logic.

---

## 1. Generator template — safe route default + `impl Module` CUSTOM zone
**Repo:** `metaphor-plugin-codegen` (module skeleton / `lib.rs` template). **Priority: high.**

- **(a)** The scaffolded `{Module}Module::routes()` should default to **read-only / guarded**, not
  unguarded full CRUD. Today `EdiModule::routes()` → `all_crud_routes()` → full 16-endpoint CRUD
  (`src/lib.rs:75-94`), so a naive composer bypasses `EdiWriteService` entirely — skipping business-key
  dedup and producing duplicate `sales_orders` on partner retransmit (PRD idempotency criterion).
- **(b)** Add a `// <<< CUSTOM` zone **inside** the `impl {Module}Module` block (and one at module
  scope for `pub use`/`pub mod`). There is currently no CUSTOM zone there, so a module **cannot**
  regen-safely add a method like `guarded_routes()` or re-export the existing guarded composer.

**Why module-level fix failed:** `routes()` / `all_crud_routes()` are generated *outside* any CUSTOM
zone; `impl EdiModule` has none; there is no module-scope CUSTOM zone for new re-exports. Hand-edits
revert on regen. (Consumer probe found **N=0 external callers** today, so there is a clean migration
window to change the default.)

---

## 2. Schema DSL — a `@readonly` / `@exclude_from_dto` field attribute
**Repo:** `metaphor-plugin-schema` (schema DSL → DTO generation). **Priority: high.**

Add a field attribute that excludes a field from the generated **Create / Update / Patch** DTOs
(the only existing auto-exclusion is `@audit_metadata`, which is a composition, not a per-field knob —
see `docs/schema/RULE_FORMAT_MODELS.md` Field Attributes).

**Why:** `EdiDocument.status` (and `mapped_ref_type`, `mapped_ref_id`, `acknowledged_at`,
`error_detail`) are **freely settable via generic CRUD** today — `CreateEdiDocumentDto.status`
(required), `UpdateEdiDocumentDto.status` (required), `PatchEdiDocumentDto.status` (optional)
(`src/presentation/dto/edi_document_dto.rs:53,94,137`). So a CRUD client can stamp any status and
bypass the lifecycle. Marking these fields non-writable-via-CRUD in `schema/models/edi_document.model.yaml`
would drop them from the write DTOs at the source — the regen-safe way to force all status changes
through `EdiWriteService`. (This is the clean alternative to framework trait-wiring for closing the
status-bypass hole.)

---

## 3. `DomainPolicy` is dead — fix the trait wiring or the generated guidance
**Repos:** `backbone-core` (`src/policy.rs`, `src/service.rs`) + `metaphor-plugin-schema`
(`*_domain_policy.rs` template). **Priority: medium.**

`DomainPolicy` (`backbone-core/src/policy.rs`: `can_create` / `can_update` / `can_delete`) is **never
invoked**. `GenericCrudService` enforces via a *different* trait, `ServiceLifecycle`
(`before_create` / `before_update` / `before_delete`). The generated
`src/domain/services/edi_document_domain_policy.rs` nevertheless instructs: *"To add domain rules,
replace this alias with a struct implementing `backbone_core::DomainPolicy<EdiDocument>` in the
`// <<< CUSTOM` zone"* — but doing so is **dead code** (the council's rec #3 was a misdiagnosis on
this basis). `EdiDocumentDomainPolicy` is referenced only by its own definition + one `pub use`.

**Fix (one of):** (a) wire `DomainPolicy` into `GenericCrudService` so the generated policy actually
enforces; or (b) regenerate `*_domain_policy.rs` around `ServiceLifecycle` (the real enforcement
trait) and correct the guidance text. Either way, stop shipping a generated file that points authors
at a no-op trait.

---

## 4. Exports template — stop emitting the impl-less `EdiQueryService` trait
**Repo:** `metaphor-plugin-schema` (exports template). **Priority: low.**

The exports template emits a public `EdiQueryService` read-API trait with no implementation and no
consumer. `src/exports/services.rs:23` ships the trait; regen stripped the hand-added
`EdiQueryServiceImpl` but kept the trait; there are zero callers anywhere. An unreachable public
contract that masquerades as an inter-module API.

**Fix:** drop `EdiQueryService` from the exports template, or only emit it when a backing impl is
configured.

---

## 5. Meta-finding — the regen model needs more CUSTOM zones
**Repos:** `metaphor-plugin-codegen` + `metaphor-plugin-schema`. **Priority: medium (systemic enabler).**

Every module-level route/policy rec from this review hit the same wall: the codegen CUSTOM zones do
not cover the places a module needs to customize for safety. Specifically, add regen-stable CUSTOM
zones at:

- inside `impl {Module}Module` (for added methods like `guarded_routes()`);
- around the generated `routes()` / `all_crud_routes()` methods (or make the default delegate to a
  CUSTOM-overridable composer);
- at module scope in `lib.rs` (for `pub mod` / `pub use` additions — e.g. re-exporting a guarded
  composer at the crate root).

Without these, modules cannot make safe defaults or add safe entry points without editing the
framework templates — which blocks exactly the kind of hardening the maturity review asks for.

---

## Suggested sequencing

1. **#2 (schema `@exclude_from_dto`)** + **#5 (CUSTOM zones)** — highest leverage, unblocks
   module-level safety without touching runtime traits. Enables rec #1's intent (safe route surface)
   and rec #3's real fix (CRUD can't mutate lifecycle fields) at the source.
2. **#1 (template route default)** — flip the scaffolded default once #5 lands; clean N=0 window.
3. **#3 (DomainPolicy ↔ ServiceLifecycle)** — correctness of generated guidance; prevents the next
   author from writing dead policy code.
4. **#4 (`EdiQueryService`)** — low-priority cleanup.

## Evidence index
- Council report: `docs/council/2026-08-04-module-backbone-edi-maturity.md`
- Write-path enforcement (sound): `src/infrastructure/persistence/edi_document_repository.rs` (`mark_mapped` / `mark_failed` / `acknowledge` SQL state guards); `src/application/service/edi_write_service.rs`
- Status CRUD-writable (the gap): `src/presentation/dto/edi_document_dto.rs:53,94,137`
- Dead `DomainPolicy`: `src/domain/services/edi_document_domain_policy.rs:15`; `backbone-core/src/policy.rs`; `GenericCrudService` uses `ServiceLifecycle` (`backbone-core/src/service.rs`)
- Impl-less `EdiQueryService`: `src/exports/services.rs:23`
- Generated route default outside CUSTOM: `src/lib.rs:75-94`; `src/routes/mod.rs:50` (`create_readonly_edi_routes` already exists but is opt-in/unwired)
