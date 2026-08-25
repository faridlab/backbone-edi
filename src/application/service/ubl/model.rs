//! The parsed UBL BIS 3 order model (hand-authored, user-owned).
//!
//! Pure data — no XML crate imports here. Amounts and quantities are
//! `rust_decimal::Decimal` from the moment of extraction (text → Decimal; `f64` is never touched),
//! so scale is preserved end to end: `"12500.00"` in the XML is `"12500.00"` in the canonical
//! payload. Dates are strict-ISO `NaiveDate`.

use chrono::NaiveDate;
use rust_decimal::Decimal;

/// A whole inbound UBL 2.1 BIS 3 order, extracted and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUblOrder {
    /// `cbc:ID` — the buyer's order number; becomes the dedup `business_key`.
    pub document_id: String,
    /// `cbc:UUID` — the instance id; becomes the `control_number` when present.
    pub document_uuid: Option<String>,
    pub customization_id: Option<String>,
    pub profile_id: Option<String>,
    /// `cbc:IssueDate`, strict ISO `yyyy-mm-dd`.
    pub issue_date: NaiveDate,
    /// `cbc:IssueTime`, carried verbatim (no timezone arithmetic).
    pub issue_time: Option<String>,
    /// `cbc:DocumentCurrencyCode`, `^[A-Z]{3}$` when present.
    pub currency: Option<String>,
    pub buyer_reference: Option<String>,
    /// Document-level `cac:Delivery/cbc:RequestedDeliveryDate`.
    pub requested_delivery_date: Option<NaiveDate>,
    /// `cbc:Note` (capped in count and length by the parser).
    pub notes: Vec<String>,
    pub buyer: UblParty,
    pub seller: UblParty,
    pub lines: Vec<ParsedUblLine>,
    pub totals: Option<UblTotals>,
    /// Non-fatal observations, e.g. `missing_customization_id`.
    pub warnings: Vec<String>,
}

/// One party (buyer from `cac:BuyerCustomerParty/cac:Party`, seller from
/// `cac:SellerSupplierParty/cac:Party`). Every member is optional — the partner row already binds
/// the tenant and the default customer, so an unidentified party is extracted as-is and the host
/// may still reject via `MapRejected` if it needs the id.
#[derive(Debug, Clone, PartialEq)]
pub struct UblParty {
    /// First `cac:PartyIdentification/cbc:ID`, falling back to `cbc:EndpointID`, then
    /// `cac:PartyLegalEntity/cbc:CompanyID`.
    pub id: Option<String>,
    /// `@schemeID` of whichever element supplied [`Self::id`].
    pub scheme: Option<String>,
    pub endpoint_id: Option<String>,
    pub endpoint_scheme: Option<String>,
    /// `cac:PartyLegalEntity/cbc:RegistrationName`, falling back to `cac:PartyName/cbc:Name`.
    pub name: Option<String>,
}

/// Which element supplied a line's `item_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemIdSource {
    /// `cac:Item/cbc:SellersItemIdentification/cbc:ID` (preferred when present).
    SellerAssigned,
    /// `cac:Item/cbc:StandardItemIdentification/cbc:ID` (a GTIN).
    StandardGtin,
}

/// One `cac:OrderLine/cac:LineItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUblLine {
    /// `cac:LineItem/cbc:ID` (the buyer's line number, verbatim).
    pub line_number: String,
    /// The seller-assigned code when present, else the GTIN.
    pub item_id: String,
    pub item_id_source: ItemIdSource,
    /// `cbc:StandardItemIdentification/cbc:ID` — carried separately even when the seller id won.
    pub gtin: Option<String>,
    pub item_name: Option<String>,
    pub description: Option<String>,
    /// `cbc:Quantity` — always > 0.
    pub quantity: Decimal,
    /// `cbc:Quantity/@unitCode`.
    pub uom: Option<String>,
    /// `cac:Price/cbc:PriceAmount` — never negative (explicit zero allowed).
    pub unit_price: Decimal,
    /// `cbc:PriceAmount/@currencyID`.
    pub price_currency: Option<String>,
    /// `cac:Price/cbc:BaseQuantity` (> 0 when present).
    pub base_quantity: Option<Decimal>,
    /// `cbc:BaseQuantity/@unitCode`.
    pub base_uom: Option<String>,
    /// `cbc:LineExtensionAmount`.
    pub line_amount: Option<Decimal>,
    /// Line-level `cac:Delivery/cbc:RequestedDeliveryDate`.
    pub requested_delivery_date: Option<NaiveDate>,
}

/// `cac:AnticipatedMonetaryTotal` — all members optional as sent.
#[derive(Debug, Clone, PartialEq)]
pub struct UblTotals {
    pub line_extension: Option<Decimal>,
    pub tax_exclusive: Option<Decimal>,
    pub tax_inclusive: Option<Decimal>,
    pub payable: Option<Decimal>,
}
