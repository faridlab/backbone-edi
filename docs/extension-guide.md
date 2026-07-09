# backbone-edi — Extension Guide

## Public surface (stable)
- **Mapping port** (`application::service::edi_ports`): `MappingPort` + DTOs (`MapRequest`, `MapAck`,
  `MapRejected`) — the seam a composing service implements over backbone-selling / backbone-billing to turn
  an inbound document into an internal one. `MapRequest.idempotency_key` (the EDI document id) lets the
  target dedup a re-map. EDI never imports selling/billing.
- **Events** (`application::service::edi_events`): `EdiDocumentMapped`, `EdiDocumentFailed`,
  `EdiDocumentAcknowledged`, the `EdiEvent` union, and `EdiEventSink`.
- **Write path** (`application::service::edi_write_service::EdiWriteService`): `create_partner`,
  `receive_document` (idempotent intake + map), `acknowledge`, with `NewPartner` / `InboundDoc` /
  `ReceiveOutcome` DTOs.
- **Durability**: the lifecycle event is staged in this module's `edi.outbox_events` in the same tx as the
  status update; a composing service runs a relay to deliver it.

## How a consuming service uses EDI
Parse the partner's X12/EDIFACT/CSV into a structured `payload` and call `receive_document(InboundDoc {
partner_id, doc_type, control_number, raw, payload }, mapper, sink)`. Implement `MappingPort::map` over the
target module (proven: REAL backbone-selling `create_sales_order`), forwarding `idempotency_key` so a
re-map can't duplicate the internal order. Call `acknowledge(document_id)` to issue the functional ack.
Never mutate EDI's tables directly.

## Not a contract
- The 12 generated CRUD endpoints per entity are convenience scaffolding. Do **not** insert a document or
  flip a status through the generic PATCH surface — it bypasses the dedup, the mapping, and the outbox
  staging. Use `EdiWriteService`.
- `// <<< CUSTOM` blocks preserve local edits only; not a cross-module extension point.

## Invariants a consumer must not break
- One inbound document per `(partner, direction, control_number)`; `receive_document` is the only intake.
- A retransmission never re-maps (no duplicate internal order); the lifecycle event is durable.
- A mapped/failed document is acknowledged at most once (state-guarded).
