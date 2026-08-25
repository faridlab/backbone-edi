//! UBL parser cases — the pure oracle: valid extraction, the canonical payload contract, and one
//! test per refusal code. No DB, no server: these run armed everywhere.

use backbone_edi::application::service::ubl::{
    self, parse_ubl_order, to_payload, ItemIdSource, UblRefusal,
};
use rust_decimal::Decimal;
use std::str::FromStr;

const VALID: &str = include_str!("fixtures/ubl/valid_order.xml");
const MINIMAL_NO_CUSTOMIZATION: &str = include_str!("fixtures/ubl/valid_minimal_no_customization.xml");
const MISSING_OPTIONAL: &str = include_str!("fixtures/ubl/valid_missing_optional_fields.xml");

fn refuse(raw: &str) -> UblRefusal {
    parse_ubl_order(raw).expect_err("document must refuse")
}

fn codes(r: &UblRefusal) -> Vec<&'static str> {
    r.errors.iter().map(|e| e.code).collect()
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// UBP-01 — a full minimal BIS 3 order parses: every typed field, both party shapes, both
// item-identification shapes, exact decimals.
#[test]
fn ubp01_valid_parse() {
    let o = parse_ubl_order(VALID).expect("valid fixture parses");
    assert_eq!(o.document_id, "PO-2026-1001");
    assert_eq!(o.document_uuid.as_deref(), Some("6b7c8d9e-0f1a-2b3c-4d5e-6f7a8b9c0d1e"));
    assert_eq!(
        o.customization_id.as_deref(),
        Some("urn:www.cenbii.eu:transaction:biitrns001:ver2.0:extended:urn:www.peppol.eu:bis:peppol28a:ver1.0")
    );
    assert_eq!(o.profile_id.as_deref(), Some("urn:www.cenbii.eu:profile:bii28:ver2.0"));
    assert_eq!(o.issue_date, chrono::NaiveDate::from_ymd_opt(2026, 8, 25).unwrap());
    assert_eq!(o.issue_time.as_deref(), Some("10:30:00"));
    assert_eq!(o.currency.as_deref(), Some("IDR"));
    assert_eq!(o.buyer_reference.as_deref(), Some("BUYER-REF-77"));
    assert_eq!(
        o.requested_delivery_date,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
    );
    assert_eq!(o.notes, vec!["Please deliver before Friday".to_string()]);

    // Buyer: first PartyIdentification/ID wins; EndpointID carried separately; PartyName fallback.
    assert_eq!(o.buyer.id.as_deref(), Some("BUYER-CO-001"));
    assert_eq!(o.buyer.scheme.as_deref(), Some("CO"));
    assert_eq!(o.buyer.endpoint_id.as_deref(), Some("1234567890123"));
    assert_eq!(o.buyer.endpoint_scheme.as_deref(), Some("GLN"));
    assert_eq!(o.buyer.name.as_deref(), Some("Buyer Retail Group"));
    // Seller: PartyIdentification/ID + RegistrationName.
    assert_eq!(o.seller.id.as_deref(), Some("SELLER-CO-001"));
    assert_eq!(o.seller.scheme.as_deref(), Some("CO"));
    assert_eq!(o.seller.name.as_deref(), Some("Acme Supplies"));

    let t = o.totals.as_ref().expect("totals present");
    assert_eq!(t.line_extension, Some(dec("25000.10")));
    assert_eq!(t.tax_exclusive, Some(dec("25000.10")));
    assert_eq!(t.tax_inclusive, Some(dec("27500.11")));
    assert_eq!(t.payable, Some(dec("27500.11")));

    assert_eq!(o.lines.len(), 2);
    let l1 = &o.lines[0];
    assert_eq!(l1.line_number, "1");
    assert_eq!(l1.item_id, "SKU-001");
    assert_eq!(l1.item_id_source, ItemIdSource::SellerAssigned);
    assert_eq!(l1.gtin.as_deref(), Some("12345678901234"));
    assert_eq!(l1.item_name.as_deref(), Some("Industrial Widget"));
    assert_eq!(l1.description.as_deref(), Some("Widget, industrial grade"));
    assert_eq!(l1.quantity, dec("2"));
    assert_eq!(l1.uom.as_deref(), Some("EA"));
    assert_eq!(l1.unit_price, dec("12500.00"));
    assert_eq!(l1.price_currency.as_deref(), Some("IDR"));
    assert_eq!(l1.base_quantity, Some(dec("1")));
    assert_eq!(l1.base_uom.as_deref(), Some("EA"));
    assert_eq!(l1.line_amount, Some(dec("12500.00")));
    assert_eq!(
        l1.requested_delivery_date,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
    );

    let l2 = &o.lines[1];
    assert_eq!(l2.item_id, "98765432109876");
    assert_eq!(l2.item_id_source, ItemIdSource::StandardGtin);
    assert_eq!(l2.gtin.as_deref(), Some("98765432109876"));
    assert_eq!(l2.base_quantity, None);
    assert_eq!(l2.requested_delivery_date, None);

    assert!(o.warnings.is_empty(), "a conformant customization produces no warnings");
}

// UBP-02 — the canonical payload contract: self-describing kind + schema, every decimal field a
// JSON STRING (recursive), absent optionals present as null so the key set is stable.
#[test]
fn ubp02_canonical_payload_snapshot() {
    let o = parse_ubl_order(VALID).unwrap();
    let p = to_payload(&o);
    assert_eq!(p["kind"], "ubl_bis3_order");
    assert_eq!(p["schema"], 1);
    assert_eq!(p["document"]["id"], "PO-2026-1001");
    assert_eq!(p["document"]["issue_date"], "2026-08-25");
    assert_eq!(p["buyer"]["id"], "BUYER-CO-001");
    assert_eq!(p["lines"][0]["item_id"], "SKU-001");
    assert_eq!(p["lines"][0]["item_id_source"], "seller_assigned");
    assert_eq!(p["lines"][1]["item_id_source"], "standard_gtin");
    assert_eq!(p["totals"]["payable"], "27500.11");

    // Recursive: every decimal-bearing key is a string (never a JSON number).
    assert_decimals_are_strings(&p);

    // Absent optionals are null, never missing keys.
    let m = parse_ubl_order(MISSING_OPTIONAL).unwrap();
    let mp = to_payload(&m);
    assert!(mp["document"]["uuid"].is_null());
    assert!(mp["document"]["currency"].is_null());
    assert!(mp["buyer"]["id"].is_null());
    assert!(mp["seller"]["name"].is_null());
    assert!(mp["lines"][0]["base_qty"].is_null());
    assert!(mp["lines"][0]["line_amount"].is_null());
    assert!(mp["lines"][0]["uom"].is_null());
    assert!(mp["totals"]["payable"].is_null(), "no AnticipatedMonetaryTotal → all-null totals object, still present");
}

const DECIMAL_KEYS: &[&str] = &[
    "qty", "unit_price", "base_qty", "line_amount",
    "line_extension", "tax_exclusive", "tax_inclusive", "payable",
];

fn assert_decimals_are_strings(v: &serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for (k, child) in map {
            if DECIMAL_KEYS.contains(&k.as_str()) {
                assert!(
                    child.is_string() || child.is_null(),
                    "payload key {k} must be a JSON string or null, found {child}"
                );
            }
            assert_decimals_are_strings(child);
        }
    } else if let serde_json::Value::Array(items) = v {
        for child in items {
            assert_decimals_are_strings(child);
        }
    }
}

// UBP-03 — X12 text and truncated XML both refuse as xml_malformed.
#[test]
fn ubp03_xml_malformed() {
    let x12 = include_str!("fixtures/ubl/not_xml.txt");
    let r = refuse(x12);
    assert_eq!(codes(&r), vec!["xml_malformed"]);
    assert!(r.business_key.is_none());
    let r = refuse(include_str!("fixtures/ubl/malformed.xml"));
    assert_eq!(codes(&r), vec!["xml_malformed"]);
}

// UBP-04 — a non-Order root (Invoice) and a wrong-namespace Order both refuse carrying the QName.
#[test]
fn ubp04_wrong_root_element() {
    let r = refuse(include_str!("fixtures/ubl/wrong_root_invoice.xml"));
    assert_eq!(codes(&r), vec!["wrong_root_element"]);
    assert!(r.errors[0].field.contains("Invoice"));
    assert!(r.business_key.is_none());
    let r = refuse(include_str!("fixtures/ubl/wrong_ns_order.xml"));
    assert_eq!(codes(&r), vec!["wrong_root_element"]);
    assert!(r.errors[0].field.contains("Order"));
}

// UBP-05 — a customization naming a different transaction refuses.
#[test]
fn ubp05_unsupported_customization() {
    let r = refuse(include_str!("fixtures/ubl/unsupported_customization.xml"));
    assert_eq!(codes(&r), vec!["unsupported_customization"]);
}

// UBP-06 — a document id over the business-key column budget refuses and is NOT a usable key.
#[test]
fn ubp06_id_too_long() {
    let r = refuse(include_str!("fixtures/ubl/id_too_long.xml"));
    assert_eq!(codes(&r), vec!["id_too_long"]);
    assert!(r.business_key.is_none(), "an unstorable id must not become a business key");
}

// UBP-07 — no cbc:ID: no business key, nothing identifiable.
#[test]
fn ubp07_missing_document_id() {
    let r = refuse(include_str!("fixtures/ubl/missing_id.xml"));
    assert_eq!(codes(&r), vec!["missing_document_id"]);
    assert!(r.business_key.is_none());
}

// UBP-08 / UBP-09 — issue date missing, and present but not strict ISO.
#[test]
fn ubp08_missing_issue_date() {
    let r = refuse(include_str!("fixtures/ubl/missing_issue_date.xml"));
    assert_eq!(codes(&r), vec!["missing_issue_date"]);
}

#[test]
fn ubp09_invalid_issue_date() {
    let r = refuse(include_str!("fixtures/ubl/invalid_issue_date.xml"));
    assert_eq!(codes(&r), vec!["invalid_issue_date"]);
}

// UBP-10 — an order with no lines at all.
#[test]
fn ubp10_missing_order_line() {
    let r = refuse(include_str!("fixtures/ubl/no_lines.xml"));
    assert_eq!(codes(&r), vec!["missing_order_line"]);
}

// UBP-11 — a name-only item cannot be resolved by the host.
#[test]
fn ubp11_missing_item_identifier() {
    let r = refuse(include_str!("fixtures/ubl/missing_item_identifier.xml"));
    assert_eq!(codes(&r), vec!["missing_item_identifier"]);
}

// UBP-12 — a line without quantity.
#[test]
fn ubp12_missing_line_quantity() {
    let r = refuse(include_str!("fixtures/ubl/missing_qty.xml"));
    assert_eq!(codes(&r), vec!["missing_line_quantity"]);
}

// UBP-13 — unparseable, zero, and negative quantities all refuse under invalid_line_quantity.
#[test]
fn ubp13_invalid_line_quantity() {
    for f in ["invalid_qty_zero.xml", "invalid_qty_negative.xml", "qty_not_decimal.xml"] {
        let raw = std::fs::read_to_string(format!("tests/fixtures/ubl/{f}")).unwrap();
        let r = refuse(&raw);
        assert_eq!(codes(&r), vec!["invalid_line_quantity"], "{f}");
    }
}

// UBP-14 — a missing price would default the internal line to free: always refuse.
#[test]
fn ubp14_missing_line_price() {
    let r = refuse(include_str!("fixtures/ubl/missing_price.xml"));
    assert_eq!(codes(&r), vec!["missing_line_price"]);
}

// UBP-15 — an unparseable price refuses (an explicit zero would be allowed).
#[test]
fn ubp15_invalid_line_price() {
    let r = refuse(include_str!("fixtures/ubl/invalid_price.xml"));
    assert_eq!(codes(&r), vec!["invalid_line_price"]);
}

// UBP-16 — an unparseable line extension amount refuses.
#[test]
fn ubp16_invalid_line_amount() {
    let r = refuse(include_str!("fixtures/ubl/invalid_line_amount.xml"));
    assert_eq!(codes(&r), vec!["invalid_line_amount"]);
}

// UBP-17 — a lowercase currency code refuses.
#[test]
fn ubp17_invalid_currency() {
    let r = refuse(include_str!("fixtures/ubl/invalid_currency.xml"));
    assert_eq!(codes(&r), vec!["invalid_currency"]);
}

// UBP-18 — a non-ISO requested delivery date refuses.
#[test]
fn ubp18_invalid_delivery_date() {
    let r = refuse(include_str!("fixtures/ubl/invalid_delivery_date.xml"));
    assert_eq!(codes(&r), vec!["invalid_delivery_date"]);
}

// UBP-19 — a zero base quantity is unusable (it prices nothing) and refuses.
#[test]
fn ubp19_invalid_base_quantity() {
    let r = refuse(include_str!("fixtures/ubl/invalid_base_quantity.xml"));
    assert_eq!(codes(&r), vec!["invalid_base_quantity"]);
}

// UBP-20 — an absent customization is ACCEPTED with a recorded warning (hand-rolled senders
// routinely omit it; the root QName is the reliable discriminator).
#[test]
fn ubp20_missing_customization_accepted_with_warning() {
    let o = parse_ubl_order(MINIMAL_NO_CUSTOMIZATION).expect("accepted despite missing customization");
    assert_eq!(o.warnings, vec!["missing_customization_id".to_string()]);
    assert_eq!(o.document_id, "PO-MIN-0001");
    let p = to_payload(&o);
    assert_eq!(p["warnings"][0], "missing_customization_id");
}

// UBP-21 — business key / control number derivation: business_key is always cbc:ID; the control
// number is cbc:UUID when present and falls back to cbc:ID (the service applies this exact rule).
#[test]
fn ubp21_business_key_and_control_number_derivation() {
    let with_uuid = parse_ubl_order(VALID).unwrap();
    assert_eq!(with_uuid.document_id, "PO-2026-1001");
    let control = with_uuid.document_uuid.clone().unwrap_or_else(|| with_uuid.document_id.clone());
    assert_eq!(control, "6b7c8d9e-0f1a-2b3c-4d5e-6f7a8b9c0d1e");

    let minimal = parse_ubl_order(MISSING_OPTIONAL).unwrap();
    let control = minimal.document_uuid.clone().unwrap_or_else(|| minimal.document_id.clone());
    assert_eq!(control, "PO-BARE-0001", "no UUID → the order number stands in");
}

// UBP-22 — the line-count and size budgets: 501 lines refuse as too_many_lines; an oversized raw
// document refuses as document_too_large with no business key.
#[test]
fn ubp22_size_and_line_count_limits() {
    let mut doc = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Order xmlns="urn:oasis:names:specification:ubl:schema:xsd:Order-2"
       xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
       xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>PO-BIG-1</cbc:ID>
  <cbc:IssueDate>2026-08-25</cbc:IssueDate>
"#,
    );
    for i in 1..=501 {
        doc.push_str(&format!(
            r#"  <cac:OrderLine><cac:LineItem><cbc:ID>{i}</cbc:ID><cbc:Quantity>1</cbc:Quantity>
      <cac:Price><cbc:PriceAmount>1</cbc:PriceAmount></cac:Price>
      <cac:Item><cac:SellersItemIdentification><cbc:ID>SKU-{i}</cbc:ID></cac:SellersItemIdentification></cac:Item>
  </cac:LineItem></cac:OrderLine>
"#
        ));
    }
    doc.push_str("</Order>\n");
    let r = refuse(&doc);
    assert_eq!(codes(&r), vec!["too_many_lines"]);
    // Exactly 500 lines is within budget.
    let mut within = doc.clone();
    if let Some(pos) = within.rfind("<cac:OrderLine>") {
        if let Some(end) = within.rfind("</cac:OrderLine>") {
            within.replace_range(pos..end + "</cac:OrderLine>".len(), "");
        }
    }
    assert!(parse_ubl_order(&within).is_ok(), "500 lines are within the budget");

    let oversized = "x".repeat(ubl::MAX_XML_BYTES + 1);
    let r = ubl::too_large_refusal(oversized.len(), ubl::MAX_XML_BYTES);
    assert_eq!(r.errors[0].code, "document_too_large");
    assert!(r.business_key.is_none());
    assert!(r.detail_line().contains("document_too_large"));
}

// UBP-23 — decimal fidelity: scale survives text → Decimal → payload exactly ("12500.00" never
// becomes "12500.0" or a JSON number; "0.1" stays "0.1").
#[test]
fn ubp23_decimal_fidelity() {
    let o = parse_ubl_order(VALID).unwrap();
    let p = to_payload(&o);
    assert_eq!(p["lines"][0]["unit_price"].as_str().unwrap(), "12500.00");
    assert_eq!(p["lines"][0]["line_amount"].as_str().unwrap(), "12500.00");
    assert_eq!(p["lines"][1]["unit_price"].as_str().unwrap(), "0.1");
    assert_eq!(p["totals"]["line_extension"].as_str().unwrap(), "25000.10");
    // And a host recovers the exact value through Decimal::from_str.
    assert_eq!(
        Decimal::from_str(p["lines"][0]["unit_price"].as_str().unwrap()).unwrap(),
        Decimal::from_str("12500.00").unwrap()
    );
}

// UBP-24 — every refusal reports ALL collected problems together, and the detail line renders the
// stable code[field]: detail shape capped for the error_detail column.
#[test]
fn ubp24_all_errors_collected_and_detail_line() {
    // Missing quantity AND missing price on the same line: both reported.
    let raw = r#"<?xml version="1.0" encoding="UTF-8"?>
<Order xmlns="urn:oasis:names:specification:ubl:schema:xsd:Order-2"
       xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
       xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>PO-MULTI-1</cbc:ID>
  <cbc:IssueDate>2026-08-25</cbc:IssueDate>
  <cac:OrderLine>
    <cac:LineItem>
      <cbc:ID>1</cbc:ID>
      <cac:Item><cac:SellersItemIdentification><cbc:ID>SKU-1</cbc:ID></cac:SellersItemIdentification></cac:Item>
    </cac:LineItem>
  </cac:OrderLine>
</Order>
"#;
    let r = refuse(raw);
    assert!(codes(&r).contains(&"missing_line_quantity"));
    assert!(codes(&r).contains(&"missing_line_price"));
    let line = r.detail_line();
    assert!(line.contains("missing_line_quantity[") && line.contains("missing_line_price["));

    // The cap: a document with many problems renders a bounded line with "+N more".
    let mut doc = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Order xmlns="urn:oasis:names:specification:ubl:schema:xsd:Order-2"
       xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
       xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>PO-MANY-1</cbc:ID>
  <cbc:IssueDate>2026-08-25</cbc:IssueDate>
"#,
    );
    for i in 0..80 {
        doc.push_str(&format!(
            r#"  <cac:OrderLine><cac:LineItem><cbc:ID>{i}</cbc:ID>
      <cac:Item><cbc:Name>name only</cbc:Name></cac:Item>
  </cac:LineItem></cac:OrderLine>
"#
        ));
    }
    doc.push_str("</Order>\n");
    let r = refuse(&doc);
    let line = r.detail_line();
    assert!(line.len() <= 1000, "detail line capped at 1000 chars, got {}", line.len());
    assert!(line.contains("+"), "elided problems are counted");
}
