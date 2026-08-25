//! UBL BIS 3 inbound order import (hand-authored, user-owned; survives regen).
//!
//! The wire representation is a DECLARED [`crate::domain::entity::EdiFormat`] value on the trading
//! partner row (`ubl_bis3`) — never payload sniffing. This module owns the pure half: parse the
//! XML, validate the structure, and render the canonical JSON payload the host-side
//! `MappingPort` adapter consumes. The durable half (partner gate, idempotent claim, settle,
//! negative-ack) lives in [`super::edi_write_service::EdiWriteService::receive_ubl_order`].

pub mod error;
pub mod model;
pub mod parser;
pub mod payload;

pub use error::{
    UblFieldError, UblRefusal, DOCUMENT_TOO_LARGE, ID_TOO_LONG, INVALID_BASE_QUANTITY,
    INVALID_CURRENCY, INVALID_DELIVERY_DATE, INVALID_ISSUE_DATE, INVALID_LINE_AMOUNT,
    INVALID_LINE_PRICE, INVALID_LINE_QUANTITY, MISSING_DOCUMENT_ID, MISSING_ISSUE_DATE,
    MISSING_ITEM_IDENTIFIER, MISSING_LINE_PRICE, MISSING_LINE_QUANTITY, MISSING_ORDER_LINE,
    TOO_MANY_LINES, UNSUPPORTED_CUSTOMIZATION, WRONG_ROOT_ELEMENT, XML_MALFORMED,
};
pub use model::{ItemIdSource, ParsedUblLine, ParsedUblOrder, UblParty, UblTotals};
pub use parser::{parse_ubl_order, too_large_refusal};
pub use payload::to_payload;

/// Hard refuse beyond this many raw bytes (256 KiB) — inbound orders are small documents; a
/// partner streaming something larger is a misconfiguration or an attack, not an order.
pub const MAX_XML_BYTES: usize = 262_144;
/// Hard refuse beyond this many `cac:OrderLine` elements.
pub const MAX_ORDER_LINES: usize = 500;
/// Post-trim budget for extracted ids, names, and codes (the document id has a tighter,
/// column-backed budget — see the parser).
pub const MAX_TEXT: usize = 256;
/// At most this many `cbc:Note` elements are carried into the canonical payload.
pub const MAX_NOTES: usize = 10;
/// Per-note character budget (advisory text — truncated, never refused).
pub const MAX_NOTE_TEXT: usize = 500;
