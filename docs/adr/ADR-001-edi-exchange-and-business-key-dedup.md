# ADR-001 — The EDI exchange lifecycle, business-key dedup, and durable events

Status: accepted · 2026-07-09 · Thin-channels pillar (Tier 5; posts no GL; deferred-hard)

## Context
An enterprise partner may mandate EDI. This module owns the **exchange lifecycle** — receive a partner's
document, map it to an internal document (a PO becomes a sales order), acknowledge it — not the byte-level
X12/EDIFACT grammar (the composing service parses and hands a structured payload). Low SMB value in
Indonesia; promoted only when a partner mandates it (tier5-deferred §5).

## Decision
1. **Idempotency keys on the BUSINESS document identity, not the envelope control number.** EDI control
   numbers recycle (they wrap), so deduping on them silently drops a legitimate new document that reuses a
   recycled number. The dedup key is `business_key` (the PO/invoice number); `control_number` is kept for
   the ack/audit (maturity council 2026-07-09).
2. **Mapping is a PORT, not a dependency.** An inbound document is mapped to an internal one via
   `MappingPort` (implemented over backbone-selling/billing). Zero Cargo edge — proven by ESEAM-1 creating
   a REAL sales order. `MapRequest.idempotency_key` (the document id) lets the target dedup a re-map.
3. **The lifecycle event is DURABLE — staged in the outbox in the same tx as the status update.** A crash
   between commit and the in-proc publish can't drop it.
4. **Failures are first-class.** An unmappable document is recorded `failed` with the reason + an
   `EdiDocumentFailed` event (the partner needs a negative acknowledgement).
5. **Acknowledgement stops the retransmission loop.** `acknowledge` issues a functional ack (997/CONTRL),
   state-guarded and idempotent.
6. **Posts no GL.**

## Consequences
- Turn EDI off and no partner document enters; it is the one place partner traffic becomes internal
  documents. Proven against REAL backbone-selling; durable across a lost publish; survives regen (§5).

## Parking lot (each with a gate)
- **Ack event couldn't drive the 997** — FIXED (completeness council 2026-07-09): `EdiDocumentAcknowledged` dropped the accept/reject outcome + error, so a consumer couldn't generate a positive-vs-negative functional ack; added `accepted` + `error_detail` to the event, derived from the pre-update status (EGC-5, proven-by-revert).
- **Recycled control number silently dropped a legitimate PO** — FIXED (maturity council 2026-07-09):
  deduped on `business_key` (the PO number), not the recycling control number (EIP-4, proven-by-revert).
- **Map/UPDATE non-atomicity + no reaper** — a crash after the order is created but before the status
  UPDATE strands the document `received` with an orphaned order; the dedup then blocks re-mapping. Gate: a
  `remap_pending` reaper re-driving `received` docs via the idempotent mapper.
- **Corrected re-versioned documents** — a corrected doc under the same business_key dedups (won't re-map).
  Gate: an upsert-on-newer policy.
- **The byte-level parser + transport (AS2/SFTP/VAN) + full outbound generation** — deferred (PRD non-goals).
