# backbone-edi — business flows & golden cases

## Flow: partner document → dedup → map → acknowledge (durably)
```
receive_document (parsed partner document, retransmitted)
   │
   ▼  dedup on (partner, direction, business_key) — same PO again → duplicate=true, no re-map
   │        (a recycled control number on a NEW PO is NOT a duplicate — it maps as new)
   │
   ▼  INSERT received → map via MappingPort (creates a real sales order / purchase invoice)
   │        ├─ Ok  → status=mapped + mapped_ref + STAGE EdiDocumentMapped (same tx) → commit
   │        └─ Err → status=failed + reason  + STAGE EdiDocumentFailed  (same tx) → commit
   │
   ▼  acknowledge → functional ack (997/CONTRL), state-guarded → EdiDocumentAcknowledged
```
Posts NO GL. The composing service parses the X12/EDIFACT bytes; this module owns the lifecycle.

## Golden cases (`tests/edi_golden_cases.rs`)
- **EGC-1 — inbound PO maps.** A PO → mapped, an internal ref recorded, `EdiDocumentMapped` published.
- **EGC-2 — retransmission idempotent.** The same business document twice → mapped once, no duplicate order.
- **EGC-3 — unmappable fails.** A rejected map → `failed` + reason recorded + `EdiDocumentFailed`.
- **EGC-4 — acknowledge.** A functional ack is issued once (state-guarded); a second call is a no-op.
- **EGC-5 — ack event carries the 997 polarity.** A rejected document → `accepted=false` + reason; an accepted
  document → `accepted=true`, so a consumer builds the correct positive/negative wire ack from the event alone.

## Integrity probes (`tests/integrity_probes.rs`)
- **EIP-1 — control number required.**
- **EIP-2 — one partner per code.**
- **EIP-3 — lifecycle event durable.** With the in-proc publish lost (dropping sink), `EdiDocumentMapped`
  is still staged in the outbox.
- **EIP-4 — recycled control number maps as new.** Two different POs sharing a recycled control number
  `000000042` → both map (dedup is on the business key, not the envelope control number). Proven-by-revert.

## Seam (`tests/edi_selling_seam.rs`)
- **ESEAM-1 — inbound PO becomes a REAL sales order.** An inbound EDI PO → `MappingPort` over REAL
  backbone-selling → a genuine sales order (customer + total match); the EDI document records the link back.
  Zero normal Cargo edge.

## §5 round-trip (`scripts/edi_selling_seam_roundtrip.sh`)
Regen (`--force`) leaves the seam files (`edi_ports.rs`, `edi_events.rs`, `edi_write_service.rs`)
byte-identical; the oracle + seam re-run green.
