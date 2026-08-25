# backbone-edi — FSD

## Entities
TradingPartner (`company_id`, `name`, `partner_code`, `format`, `partner_direction`, `is_active`; unique
`(company_id, partner_code)`) · EdiDocument (`company_id`, `partner_id` FK, `doc_type`, `direction`,
`control_number` (envelope/ack), `business_key` (the dedup key — the PO/invoice number), `status`,
`payload`, `mapped_ref_type?`/`mapped_ref_id?` logical, `error_detail?`, `acknowledged_at?`; unique
`(partner_id, direction, business_key)`; index `(company_id, status)`). Enums: EdiFormat {custom_json, x12,
edifact, csv, ubl_bis3}, PartnerDirection {both, inbound, outbound}, EdiDocType {purchase_order, invoice,
ship_notice, functional_ack}, EdiDirection {inbound, outbound}, EdiStatus {received, mapped, acknowledged,
generated, sent, failed}.

## Write path (`EdiWriteService`, hand-authored, user-owned)
- `create_partner(NewPartner)` → a trading partner (one per code)
- `receive_document(InboundDoc, &dyn MappingPort, &dyn EdiEventSink)` → dedup on (partner, direction,
  business_key); record `received`; map via the port; `mapped`/`failed` + **stage the lifecycle event in
  the same tx (outbox)** + publish; returns `ReceiveOutcome {document_id, status, mapped_ref_id, duplicate}`
- `receive_ubl_order(company_id, partner_id, raw_xml, &dyn MappingPort, &dyn EdiEventSink)` → the UBL BIS 3
  inbound verb (see addendum): partner gate, parse, then the same claim/map/settle path
- `acknowledge(document_id, sink)` → state-guarded functional ack; emits `EdiDocumentAcknowledged`

Errors: `EdiError {Db, NotFound, Invalid, InvalidState, MappingRejected, UblRefused}`.

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
`edi_selling_seam` (2: ESEAM-1 inbound PO becomes a REAL sales order; USEAM-1 the canonical UBL payload
alone creates a REAL sales order) + §5 round-trip,
`ubl_parser_cases` (24: UBP-01..24 — pure parser oracle, no DB), `ubl_golden_cases` (8: UGC-1..8 —
map/re-drive/refusal/gate behaviors) + §5 round-trip. **43 tests.**

> The generated `integration_tests.rs` hits an external HTTP server and is environmental scaffolding, not
> part of this module's correctness gate.

---

# Addendum (2026-08-25): UBL BIS 3 inbound order import

Scope: **inbound UBL 2.1 BIS 3.0 orders only** (`edi_format = 'ubl_bis3'`), mapped to an internal order via
the existing `MappingPort`. Outbound UBL generation is a non-goal. Design record: ADR-002.

## Parser (`src/application/service/ubl/`, user-owned)
`roxmltree` (zero transitive deps). Budgets: `MAX_XML_BYTES = 262_144` (payload column grew to 200 000
chars), `MAX_ORDER_LINES = 500`, `MAX_TEXT = 256`, notes ≤ 10 × 500 chars (advisory truncation).
Strict 3-layer detection: well-formed XML → root `Order` in
`urn:oasis:names:specification:ubl:schema:xsd:Order-2` → `cbc:CustomizationID` naming the BIS order
transaction (`urn:www.cenbii.eu:transaction:biitrns001` or bare `trns001`, `:`-bounded, any
`:ver…[:extended:…]` suffix). Absent customization id ⇒ accepted with a
`missing_customization_id` warning (the root QName is the reliable discriminator). All problems in a
document are collected together.

19 refusal codes (code + XPath locator + detail): `xml_malformed`, `wrong_root_element`,
`unsupported_customization`, `document_too_large`, `too_many_lines`, `id_too_long`, `missing_document_id`,
`missing_issue_date`, `invalid_issue_date`, `missing_order_line`, `missing_item_identifier`,
`missing_line_quantity`, `invalid_line_quantity`, `missing_line_price`, `invalid_line_price`,
`invalid_line_amount`, `invalid_currency`, `invalid_delivery_date`, `invalid_base_quantity`.
Edge rulings: unparseable `AnticipatedMonetaryTotal` members refuse under `invalid_line_amount` (totals
locator); quantity must be strictly positive; explicit zero price allowed; `BaseQuantity` must be strictly
positive; `id_too_long` voids the business key.

## Canonical payload contract (`to_payload`)
`{"kind":"ubl_bis3_order","schema":1, document, buyer, seller, lines, totals, warnings}` — every decimal
field a JSON **string** (scale-preserving; f64 never touched), absent optionals `null` (stable key set),
dates ISO strings. The host adapter contract — USEAM-1 proves a `MappingPort` over this object alone
creates a real backbone-selling sales order with exact decimals.

## Keys, refusals, re-drive
`business_key = cbc:ID`; `control_number = cbc:UUID` (falls back to `cbc:ID`). Partner gate: active +
direction `inbound`/`both` + format `ubl_bis3`. Refusal WITHOUT a business key ⇒ fail-fast
`Err(UblRefused)` (nothing stored). Refusal WITH a business key ⇒ durable: claim, `failed` +
`EdiDocumentFailed` negative-ack event staged in the same tx. A corrected retransmission under the same
business key re-drives (`received`/`failed` + no `mapped_ref_id`; `mark_mapped` also clears
`error_detail`); an ACKNOWLEDGED document is terminal (duplicates dedup, never re-map).
