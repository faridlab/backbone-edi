//! UBL refusal taxonomy (hand-authored, user-owned).
//!
//! Every refusal carries one or more [`UblFieldError`]s — ALL collected problems are reported
//! together, never one at a time — plus the business key (`cbc:ID`) when the document carried a
//! usable one, so the caller can persist a durable negative acknowledgement for a document it can
//! identify and fail fast for one it cannot.
//!
//! The `code` strings are STABLE snake_case contract values: they surface in the
//! `edi_documents.error_detail` column and in the `EdiDocumentFailed` event, and a partner/support
//! engineer reads them. Never rename one in place — a renamed code is a broken contract.

/// One field-level problem in a refused UBL document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UblFieldError {
    /// Stable snake_case contract code (see the constants on [`super::super::ubl`] usage sites).
    pub code: &'static str,
    /// XPath-ish locator of the offending element, e.g. `Order/cac:OrderLine[2]/cac:LineItem/cbc:Quantity`.
    pub field: String,
    /// Human-readable detail; includes the source line number when the parser can supply one.
    pub detail: String,
}

impl UblFieldError {
    pub fn new(code: &'static str, field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { code, field: field.into(), detail: detail.into() }
    }
}

/// A refused UBL document: every problem found, plus the business key when one was usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UblRefusal {
    pub errors: Vec<UblFieldError>,
    /// `Some(cbc:ID)` only when the document carried an identifier that is present, non-empty and
    /// within the business-key column budget — the caller may persist a durable negative ack for it.
    pub business_key: Option<String>,
}

impl UblRefusal {
    pub fn new(business_key: Option<String>) -> Self {
        Self { errors: Vec::new(), business_key }
    }

    pub fn push(&mut self, code: &'static str, field: impl Into<String>, detail: impl Into<String>) {
        self.errors.push(UblFieldError::new(code, field, detail));
    }

    /// Single-line rendering shaped for the `edi_documents.error_detail` column (max 1000 chars):
    /// `code[field]: detail` items joined by `"; "`. When the joined form does not fit, the tail is
    /// dropped and `"+N more"` records how many problems were elided.
    pub fn detail_line(&self) -> String {
        const CAP: usize = 1000;
        let mut out = String::new();
        let mut shown = 0usize;
        for e in &self.errors {
            let item = format!("{}[{}]: {}", e.code, e.field, e.detail);
            if out.is_empty() {
                out.push_str(&item);
                shown += 1;
            } else {
                // The separator only fits if the item after it does.
                if out.len() + 2 + item.len() <= CAP {
                    out.push_str("; ");
                    out.push_str(&item);
                    shown += 1;
                } else {
                    break;
                }
            }
        }
        let hidden = self.errors.len().saturating_sub(shown);
        if hidden > 0 {
            let suffix = format!("; +{hidden} more");
            if !out.is_empty() {
                let room = CAP.saturating_sub(suffix.len());
                if out.len() > room {
                    out.truncate(room);
                }
                out.push_str(&suffix);
            } else {
                // Nothing fit at all (a single over-long item): keep the code prefix so the line
                // is never empty.
                let first = &self.errors[0];
                out = format!("{}[{}]: +{hidden} more", first.code, first.field);
            }
        }
        out
    }
}

// Stable contract codes. Grouped exactly as the refusal taxonomy is documented in
// docs/adr/ADR-002-ubl-bis3-inbound-order-import.md.

/// Well-formedness (also the code for non-UTF-8-looking input and truncated documents).
pub const XML_MALFORMED: &str = "xml_malformed";
/// The root element is not `Order` in the UBL 2.1 Order namespace.
pub const WRONG_ROOT_ELEMENT: &str = "wrong_root_element";
/// `cbc:CustomizationID` present but not the BIS 3 order transaction.
pub const UNSUPPORTED_CUSTOMIZATION: &str = "unsupported_customization";
/// The raw document exceeds the inbound XML size budget.
pub const DOCUMENT_TOO_LARGE: &str = "document_too_large";
/// More order lines than the inbound budget allows.
pub const TOO_MANY_LINES: &str = "too_many_lines";
/// An identifier/name/code exceeded its length budget (document id: 120 chars — the
/// `business_key`/`control_number` column cap; other ids/names/codes: 256 chars).
pub const ID_TOO_LONG: &str = "id_too_long";
/// `cbc:ID` absent or empty.
pub const MISSING_DOCUMENT_ID: &str = "missing_document_id";
/// `cbc:IssueDate` absent or empty.
pub const MISSING_ISSUE_DATE: &str = "missing_issue_date";
/// `cbc:IssueDate` present but not strict ISO `yyyy-mm-dd`.
pub const INVALID_ISSUE_DATE: &str = "invalid_issue_date";
/// No `cac:OrderLine` at all (or an `OrderLine` without its `cac:LineItem`).
pub const MISSING_ORDER_LINE: &str = "missing_order_line";
/// A line's item carries neither a seller-assigned nor a standard (GTIN) identifier — a bare name
/// is not enough for the host to resolve an item.
pub const MISSING_ITEM_IDENTIFIER: &str = "missing_item_identifier";
/// `cbc:Quantity` absent on a line.
pub const MISSING_LINE_QUANTITY: &str = "missing_line_quantity";
/// `cbc:Quantity` unparseable, zero, or negative.
pub const INVALID_LINE_QUANTITY: &str = "invalid_line_quantity";
/// `cac:Price/cbc:PriceAmount` absent — a missing price would default the internal order line to
/// free, so it always refuses.
pub const MISSING_LINE_PRICE: &str = "missing_line_price";
/// `cbc:PriceAmount` unparseable or negative (an explicit zero is allowed).
pub const INVALID_LINE_PRICE: &str = "invalid_line_price";
/// A monetary amount (line extension or document total) present but unparseable. The `field`
/// locator names the exact element, including `cac:AnticipatedMonetaryTotal` members.
pub const INVALID_LINE_AMOUNT: &str = "invalid_line_amount";
/// A currency code present but not `^[A-Z]{3}$`.
pub const INVALID_CURRENCY: &str = "invalid_currency";
/// A requested delivery date present but not strict ISO `yyyy-mm-dd`.
pub const INVALID_DELIVERY_DATE: &str = "invalid_delivery_date";
/// `cbc:BaseQuantity` unparseable, zero, or negative.
pub const INVALID_BASE_QUANTITY: &str = "invalid_base_quantity";
