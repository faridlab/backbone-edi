# backbone-edi — PRD

Thin-channels pillar (Tier 5) · **B2B document interchange** · posts **no GL** · **deferred-hard**.

## Why
An enterprise trading partner may **mandate EDI**: exchange purchase orders, invoices, and ship notices as
structured documents over X12/EDIFACT/a partner format, with functional acknowledgements. This is the lean
EDI core: receive a partner's document, map it to an internal document (a PO becomes a sales order), and
acknowledge it — the exchange lifecycle, not a byte-level parser. **Low SMB value in Indonesia** — promoted
only when a partner mandates it (tier5-deferred §5), so this is a structure-first, promote-on-demand module.

## Scope (KEEP — tier5-deferred.md §5)
- **TradingPartner** — a partner we exchange with: code, wire format (X12/EDIFACT/JSON/CSV), directions.
- **EdiDocument** — one exchanged document with its lifecycle (`received → mapped → acknowledged` inbound;
  `generated → sent → acknowledged` outbound; `failed`).
- **Idempotent inbound** — `receive_document` dedups on `(partner, direction, control_number)`; a partner
  retransmission never re-maps (no duplicate internal order).
- **Mapping** — an inbound document is mapped to an internal one (a sales order / purchase invoice) via a
  `MappingPort`; a failure is recorded (the partner needs a negative acknowledgement).
- **Acknowledgement** — `acknowledge` issues a functional ack (997/CONTRL) so the partner stops
  retransmitting.

## Non-goals (CUT / DEFER — tier5-deferred.md §5)
- **The byte-level X12/EDIFACT parser** — the composing service parses the wire format and hands
  `receive_document` a structured payload; this module owns the exchange lifecycle, not the grammar.
- The transport (AS2/SFTP/VAN) — wired by the composing service.
- Full outbound generation (rendering internal documents to a partner's format) — structured but minimal.

## Success criteria
- An inbound document maps **exactly once** under partner retransmission (no duplicate internal order), and
  the lifecycle event is **durable**.
- An inbound PO becomes a real sales order in backbone-selling (proven against the REAL module).
- Zero normal Cargo edge; survives a full codegen regen (§5). Posts no GL.
