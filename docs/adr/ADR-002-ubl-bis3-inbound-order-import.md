# ADR-002 — UBL BIS 3 inbound order import

Status: accepted · 2026-08-25 · Thin-channels pillar (Tier 5; posts no GL; inbound only)

## Context
A large buyer may mandate UBL 2.1 BIS 3.0 XML purchase orders (the EU peppol-adjacent norm). This module
already owns the exchange lifecycle (ADR-001) but had no byte-level grammar — the composing service was
expected to parse. To accept BIS 3 orders directly, the module now parses the XML itself, strictly, and
maps it through the existing `MappingPort` into an internal order. Outbound UBL generation (order
responses, invoices) is explicitly deferred.

## Decision

1. **Format representation is a single enum value.** `edi_format` gains `'ubl_bis3'` via
   `ALTER TYPE ... ADD VALUE IF NOT EXISTS` (idempotent; enum values cannot be dropped, so the down
   migration is a documented no-op). The partner row gates format + direction: only `active` partners
   with direction `inbound` (or `both`) and format `ubl_bis3` may deliver orders.

2. **Parsing uses `roxmltree` (zero additional transitive dependencies), in-process, with hard budgets.**
   `MAX_XML_BYTES = 262_144` (the payload column grew 20 000 → 200 000 chars), `MAX_ORDER_LINES = 500`,
   text fields capped at `MAX_TEXT = 256` (notes: 10 notes × 500 chars, advisory truncation). Budgets
   exist so a hostile or broken sender cannot balloon memory or the canonical payload.

3. **Detection is strict 3-layer.** Well-formed XML → root element `Order` in
   `urn:oasis:names:specification:ubl:schema:xsd:Order-2` → `cbc:CustomizationID` must name the BIS
   order transaction: `urn:www.cenbii.eu:transaction:biitrns001` or the bare CEN BII core
   `urn:www.cenbii.eu:transaction:trns001`, ending the match at a `:` boundary, with any
   `:ver…[:extended:…]` suffix accepted. The canonical full form (OpenPEPPOL `peppol-bis` ruleset
   `peppolbis-trdm001-2.0-order` buildconfig) is
   `urn:www.cenbii.eu:transaction:biitrns001:ver2.0:extended:urn:www.peppol.eu:bis:peppol03a:ver2.0`;
   national profiles such as EHF extend the same core. Anything else is a typed
   refusal. An ABSENT customization id is ACCEPTED with a `missing_customization_id` warning — hand-rolled
   senders routinely omit it and the root QName is the reliable discriminator.

4. **The refusal taxonomy is 19 stable codes**, each carrying a code, an XPath-style field locator, and a
   human detail line; every problem in a document is collected together (a line missing both quantity and
   price reports both). Codes: `xml_malformed`, `wrong_root_element`, `unsupported_customization`,
   `document_too_large`, `too_many_lines`, `id_too_long` (> 120 chars, the business-key column budget),
   `missing_document_id`, `missing_issue_date`, `invalid_issue_date`, `missing_order_line`,
   `missing_item_identifier`, `missing_line_quantity`, `invalid_line_quantity`, `missing_line_price`,
   `invalid_line_price`, `invalid_line_amount`, `invalid_currency`, `invalid_delivery_date`,
   `invalid_base_quantity`. Taxonomy edge rulings (recorded here because the code list alone doesn't say):
   - An unparseable `AnticipatedMonetaryTotal` member refuses under `invalid_line_amount` with the totals
     element locator (the amount-parse code; there is no separate totals code).
   - Quantity must be present AND strictly positive (zero/negative refuse under `invalid_line_quantity`).
   - Price must be present; an EXPLICIT zero price is allowed (a free-sample line is a statement);
     unparseable or negative refuses.
   - `BaseQuantity`, when present, must be strictly positive (a zero base quantity prices nothing).
   - `id_too_long` also voids the business key — an unstorable id must not become a dedup key.

5. **The canonical payload is the module-to-host contract** (a JSON object stored in `edi_documents.payload`
   and handed to `MappingPort`): `{"kind":"ubl_bis3_order","schema":1, document, buyer, seller, lines,
   totals, warnings}`. EVERY decimal-bearing field is a JSON **string** (scale-preserving text →
   `rust_decimal` → `to_string()`; f64 is never touched — `"12500.00"` never becomes `12500.0` or a JSON
   number), absent optionals are `null` (the key set is stable), and dates are ISO strings. A future host
   adapter implements `MappingPort` over this object alone — proven in-tree by USEAM-1 creating a REAL
   backbone-selling sales order with exact quantities/prices.

6. **Keys reuse the ADR-001 rules.** `business_key` = `cbc:ID` (the buyer's order number — dedup survives
   control-number recycling); `control_number` = `cbc:UUID`, falling back to `cbc:ID` when absent.

7. **Refusals split by identifiability, and acknowledgement is terminal.**
   - A refusal WITHOUT a usable business key (`xml_malformed`, `wrong_root_element`, `id_too_long`,
     `missing_document_id`, `document_too_large`) fails fast: `Err(UblRefused)` — nothing is stored, the
     caller owns retry noise.
   - A refusal WITH a business key is DURABLE: the document is claimed, marked `failed` with the joined
     detail line, and an `EdiDocumentFailed` event (negative ack, `reason` = first code) is staged in the
     outbox in the same transaction — the partner must learn their order was rejected.
   - Re-drive: a later corrected transmission under the same business key re-enters `received`
     (status `received`/`failed` AND no `mapped_ref_id`); an ACKNOWLEDGED document is terminal — a late
     duplicate transmission dedups and never re-maps.
   - `mark_mapped` now clears `error_detail` and guards on `status IN ('received','failed')` with no
     mapped ref, so a re-driven correction cannot resurrect a settled document.

8. **`MapRequest`/`MappingPort`/`EdiEventSink` are unchanged.** UBL rides the existing seams;
   `receive_ubl_order` is a new verb on the write service, not a new port.

## Consequences
- The module now accepts the BIS 3 order transaction end-to-end (strictly), with replay safety inherited
  from business-key dedup + idempotency-key mapping: a replayed UBL order can never double-create an
  internal order (UGC-2/UGC-8).
- Negative acknowledgements are durable and polarity-carrying (the `EdiDocumentFailed` path), so a BIS 3
  sender's retransmission loop stops on refusal, not on silence.
- The enum value is permanent once shipped (Postgres cannot drop it); the down migration documents this.
- Outbound UBL (order response / OISUBL 3, invoice generation) remains deferred; the partner's negative
  ack is the module's own `EdiDocumentFailed` event, not a UBL response document.

## Parking lot (each with a gate)
- **UBL response document generation** (peppol BIS order response) — deferred; consumers currently build
  acks from `EdiDocumentAcknowledged`/`EdiDocumentFailed` events. Gate: a partner that mandates a UBL
  response envelope.
- **Attachment / additional-document reference handling** — parsed only as far as well-formedness; not
  carried in the canonical payload. Gate: a partner that sends order attachments that matter.
- **Line-level price-base normalization** (price per base quantity ≠ 1) is carried as data
  (`base_qty`/`base_uom`); the host normalizes. Gate: a host that wants module-side normalization.
