# backbone-edi — FSD

## Entities
TradingPartner (`company_id`, `name`, `partner_code`, `format`, `partner_direction`, `is_active`; unique
`(company_id, partner_code)`) · EdiDocument (`company_id`, `partner_id` FK, `doc_type`, `direction`,
`control_number` (envelope/ack), `business_key` (the dedup key — the PO/invoice number), `status`,
`payload`, `mapped_ref_type?`/`mapped_ref_id?` logical, `error_detail?`, `acknowledged_at?`; unique
`(partner_id, direction, business_key)`; index `(company_id, status)`). Enums: EdiFormat {custom_json, x12,
edifact, csv}, PartnerDirection {both, inbound, outbound}, EdiDocType {purchase_order, invoice, ship_notice,
functional_ack}, EdiDirection {inbound, outbound}, EdiStatus {received, mapped, acknowledged, generated,
sent, failed}.

## Write path (`EdiWriteService`, hand-authored, user-owned)
- `create_partner(NewPartner)` → a trading partner (one per code)
- `receive_document(InboundDoc, &dyn MappingPort, &dyn EdiEventSink)` → dedup on (partner, direction,
  business_key); record `received`; map via the port; `mapped`/`failed` + **stage the lifecycle event in
  the same tx (outbox)** + publish; returns `ReceiveOutcome {document_id, status, mapped_ref_id, duplicate}`
- `acknowledge(document_id, sink)` → state-guarded functional ack; emits `EdiDocumentAcknowledged`

Errors: `EdiError {Db, NotFound, Invalid, InvalidState, MappingRejected}`.

## Seams (ports — zero normal Cargo edge)
- **Map → target module (proven, ESEAM-1):** an inbound PO is mapped to a real sales order via
  `MappingPort` (implemented over REAL backbone-selling `create_sales_order`); `MapRequest.idempotency_key`
  lets the target dedup a re-map. EDI never imports selling/billing.
- **Outbound events:** `EdiDocumentMapped`/`Failed`/`Acknowledged` staged to the outbox + published.

## Test oracle
`edi_golden_cases` (5: EGC-1 inbound PO maps, EGC-2 retransmission idempotent, EGC-3 unmappable fails,
EGC-4 acknowledge, EGC-5 the ack event carries the 997 accept/reject + reason),
`integrity_probes` (4: EIP-1 control number required, EIP-2 one partner per code, EIP-3 lifecycle event
durable via outbox, EIP-4 recycled control number maps as new),
`edi_selling_seam` (1: ESEAM-1 inbound PO becomes a REAL sales order) + §5 round-trip. **10 tests.**

> The generated `integration_tests.rs` hits an external HTTP server and is environmental scaffolding, not
> part of this module's correctness gate.
