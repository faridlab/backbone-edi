# backbone-edi — BRD

## Documents
TradingPartner (code + format + direction) · EdiDocument (one exchanged document + its lifecycle). Own
Postgres schema `edi`. Posts **no GL**. Publishes lifecycle events over the event bus.

## Business rules

**BR-1 (partner).** `create_partner` registers a trading partner — unique per `(company, partner_code)`.

**BR-2 (idempotent inbound — the invariant).** `receive_document` records an inbound document, deduped on
`(partner, direction, control_number)`; a partner retransmission returns the original (`duplicate=true`)
and **never re-maps** — no duplicate internal order. A control number is required (the dedup key).

**BR-3 (mapping).** An inbound document is mapped to an internal document (a PO → a sales order, an invoice
→ a purchase invoice) via the `MappingPort`, carrying the document's idempotency key so a re-map can't
duplicate the internal order. On success: `mapped` with the internal ref recorded + `EdiDocumentMapped`. On
failure: `failed` with the reason (recorded, not swallowed) + `EdiDocumentFailed`. The lifecycle event is
staged in the same tx as the status update (durable).

**BR-4 (acknowledgement).** `acknowledge` issues a functional acknowledgement to the partner for a mapped/
failed document (state-guarded, idempotent) — else the partner keeps retransmitting. Emits
`EdiDocumentAcknowledged`.

## Events
`EdiDocumentMapped` (document_id, company/partner, doc_type, control_number, internal_ref_type/id),
`EdiDocumentFailed` (document_id, control_number, reason), `EdiDocumentAcknowledged` (document_id,
control_number, **accepted, error_detail?** — the 997 polarity + reason, so a consumer generates the correct
positive/negative wire ack from the event alone; completeness council 2026-07-09).

## Deferred (with reason)
The byte-level X12/EDIFACT parser (the composing service parses; this owns the lifecycle), the transport
(AS2/SFTP/VAN), full outbound generation (tier5-deferred §5 — EDI is deferred-hard).
