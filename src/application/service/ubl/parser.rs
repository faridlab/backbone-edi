//! UBL 2.1 BIS 3 order extraction and structural validation (hand-authored, user-owned).
//!
//! The only file in the crate that touches `roxmltree`. Strictness lives at the ROOT: the document
//! must be well-formed XML whose root is `Order` in the UBL 2.1 Order namespace — prefix choice is
//! irrelevant because roxmltree resolves namespaces. `cbc:CustomizationID`, when present, must name
//! the BIS 3 order transaction; when absent the document is ACCEPTED with a recorded warning
//! (hand-rolled senders routinely omit it — the root QName is the reliable discriminator).
//!
//! Amounts and quantities go text → `rust_decimal::Decimal` directly (`f64` is never involved), so
//! `"12500.00"` keeps its scale all the way into the canonical payload. Dates are strict ISO
//! `yyyy-mm-dd`. Every problem is collected and reported together in one [`UblRefusal`].

use chrono::NaiveDate;
use roxmltree::Node;
use rust_decimal::Decimal;
use std::str::FromStr;

use super::error::{
    UblRefusal, DOCUMENT_TOO_LARGE, ID_TOO_LONG, INVALID_BASE_QUANTITY, INVALID_CURRENCY,
    INVALID_DELIVERY_DATE, INVALID_ISSUE_DATE, INVALID_LINE_AMOUNT, INVALID_LINE_PRICE,
    INVALID_LINE_QUANTITY, MISSING_DOCUMENT_ID, MISSING_ISSUE_DATE, MISSING_ITEM_IDENTIFIER,
    MISSING_LINE_PRICE, MISSING_LINE_QUANTITY, MISSING_ORDER_LINE, TOO_MANY_LINES,
    UNSUPPORTED_CUSTOMIZATION, WRONG_ROOT_ELEMENT, XML_MALFORMED,
};
use super::model::{ItemIdSource, ParsedUblLine, ParsedUblOrder, UblParty, UblTotals};
use super::{MAX_NOTE_TEXT, MAX_NOTES, MAX_ORDER_LINES, MAX_TEXT};

/// The UBL 2.1 Order document namespace (the root-element gate).
const NS_ORDER: &str = "urn:oasis:names:specification:ubl:schema:xsd:Order-2";
/// The UBL 2.1 common basic components namespace (`cbc:`).
const NS_CBC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2";
/// The UBL 2.1 common aggregate components namespace (`cac:`).
const NS_CAC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2";

/// The BIS order transaction customizations this importer accepts. A `cbc:CustomizationID`
/// matching none of them is not an order import this module supports. Making the customization
/// mandatory instead of warning-on-absent is a one-line change here — deliberate, documented
/// posture.
///
/// The canonical PEPPOL BIS order identifier is
/// `urn:www.cenbii.eu:transaction:biitrns001:ver2.0:extended:urn:www.peppol.eu:bis:peppol03a:ver2.0`
/// (OpenPEPPOL `peppol-bis` ruleset `peppolbis-trdm001-2.0-order`); national profiles such as EHF
/// extend the same `biitrns001` core, so the accepted form is the core URN up to and including the
/// transaction token, with any `:ver…[:extended:…]` suffix. The bare `trns001` form is accepted
/// for senders that emit the CEN BII core identifier without the `bii` prefix.
const BIS3_ORDER_CUSTOMIZATION_CORES: &[&str] = &[
    "urn:www.cenbii.eu:transaction:biitrns001",
    "urn:www.cenbii.eu:transaction:trns001",
];

/// Does a `cbc:CustomizationID` name an accepted order transaction? The core must end the
/// match at a `:` boundary so a hypothetical longer transaction token (e.g. `trns0010`) is
/// not swept up by the bare `trns001` core.
fn is_supported_order_customization(customization: &str) -> bool {
    BIS3_ORDER_CUSTOMIZATION_CORES
        .iter()
        .any(|core| customization == *core || customization.starts_with(&format!("{core}:")))
}

/// The `edi_documents.business_key`/`control_number` column budget — the document id (`cbc:ID`)
/// must fit it to be storable, which is what makes it a usable business key.
const MAX_DOCUMENT_ID: usize = 120;

/// Parse and structurally validate an inbound UBL BIS 3 order document.
///
/// Pure: no database, no clock, no network. The byte-size guard (`MAX_XML_BYTES`) lives in the
/// caller, which has the raw bytes before parsing.
pub fn parse_ubl_order(raw: &str) -> Result<ParsedUblOrder, UblRefusal> {
    // (i) well-formedness. X12/EDIFACT text and truncated or non-UTF-8-shaped documents fail here.
    let doc = match roxmltree::Document::parse(raw) {
        Ok(d) => d,
        Err(e) => {
            return Err(UblRefusal::new(None).tap(XML_MALFORMED, "document", format!("not well-formed XML: {e}")));
        }
    };

    // (ii) root element: local name `Order` in the UBL 2.1 Order namespace. Prefix choice is
    // irrelevant (roxmltree resolves namespaces); a no-namespace or wrong-namespace Order, and an
    // Invoice/DespatchAdvice root alike, are wrong_root_element carrying the found QName.
    let root = doc.root_element();
    if root.tag_name().name() != "Order" || root.tag_name().namespace() != Some(NS_ORDER) {
        let found = qname(root);
        return Err(UblRefusal::new(None).tap(
            WRONG_ROOT_ELEMENT,
            found.clone(),
            format!("root element must be Order in {NS_ORDER}"),
        ));
    }

    let mut r = UblRefusal::new(None);
    let mut warnings: Vec<String> = Vec::new();

    // (iii) customization: present ⇒ must be the BIS 3 order transaction; absent ⇒ accepted with a
    // recorded warning.
    match child_text(root, NS_CBC, "CustomizationID") {
        Some(c) => {
            if !is_supported_order_customization(&c) {
                r.push(UNSUPPORTED_CUSTOMIZATION, "Order/cbc:CustomizationID", format!("expected a biitrns001/trns001 order transaction, found {c}"));
            }
        }
        None => warnings.push("missing_customization_id".to_string()),
    }

    // Document identity. cbc:ID is the buyer's order number — the business-key material. It is
    // only a USABLE business key when it fits the column budget; over-long ids refuse.
    let document_id = child_text(root, NS_CBC, "ID");
    let mut usable_id: Option<String> = None;
    match document_id {
        None => r.push(MISSING_DOCUMENT_ID, "Order/cbc:ID", "the buyer's order number is required (it is the business key)"),
        Some(v) if v.chars().count() > MAX_DOCUMENT_ID => r.push(
            ID_TOO_LONG,
            "Order/cbc:ID",
            format!("{} chars exceeds the {}-char business-key budget", v.chars().count(), MAX_DOCUMENT_ID),
        ),
        Some(v) => usable_id = Some(v),
    }
    r.business_key = usable_id.clone();

    let document_uuid = child_text(root, NS_CBC, "UUID");

    // Issue date — strict ISO yyyy-mm-dd, no other accepted form.
    let issue_date = match child_text(root, NS_CBC, "IssueDate") {
        None => {
            r.push(MISSING_ISSUE_DATE, "Order/cbc:IssueDate", "required");
            None
        }
        Some(s) => match NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => {
                r.push(INVALID_ISSUE_DATE, "Order/cbc:IssueDate", format!("\"{s}\" is not a strict ISO yyyy-mm-dd date"));
                None
            }
        },
    };

    let issue_time = child_text(root, NS_CBC, "IssueTime");
    let buyer_reference = child_text(root, NS_CBC, "BuyerReference");

    // Document currency — validated when present, never defaulted by the parser.
    let currency = child_text(root, NS_CBC, "DocumentCurrencyCode");
    if let Some(c) = &currency {
        if !is_currency_code(c) {
            r.push(INVALID_CURRENCY, "Order/cbc:DocumentCurrencyCode", format!("\"{c}\" is not a three-letter uppercase currency code"));
        }
    }

    // Notes: capped in count and per-note length (advisory text — truncation over refusal).
    let mut notes = Vec::new();
    for n in root.children().filter(|c| is_el(*c, NS_CBC, "Note")) {
        if notes.len() >= MAX_NOTES {
            break;
        }
        if let Some(t) = node_text(n) {
            notes.push(t.chars().take(MAX_NOTE_TEXT).collect());
        }
    }

    // Document-level requested delivery date.
    let requested_delivery_date =
        parse_delivery_date(child(root, NS_CAC, "Delivery"), "Order/cac:Delivery/cbc:RequestedDeliveryDate", &mut r);

    let buyer = parse_party(child(root, NS_CAC, "BuyerCustomerParty").and_then(|p| child(p, NS_CAC, "Party")), "BuyerCustomerParty", &mut r);
    let seller = parse_party(child(root, NS_CAC, "SellerSupplierParty").and_then(|p| child(p, NS_CAC, "Party")), "SellerSupplierParty", &mut r);

    let totals = parse_totals(root, &mut r);

    // Order lines: at least one, never more than the inbound budget.
    let order_lines: Vec<Node> = root.children().filter(|c| is_el(*c, NS_CAC, "OrderLine")).collect();
    if order_lines.is_empty() {
        r.push(MISSING_ORDER_LINE, "Order/cac:OrderLine", "an order must carry at least one line");
    } else if order_lines.len() > MAX_ORDER_LINES {
        r.push(TOO_MANY_LINES, "Order/cac:OrderLine", format!("{} lines exceeds the {}-line budget", order_lines.len(), MAX_ORDER_LINES));
    }

    let mut lines: Vec<ParsedUblLine> = Vec::new();
    for (idx, ol) in order_lines.iter().enumerate() {
        let n = idx + 1;
        let loc = format!("Order/cac:OrderLine[{n}]");
        let Some(li) = child(*ol, NS_CAC, "LineItem") else {
            // An OrderLine without its LineItem is an unusable line — the closest taxonomy code is
            // the missing-line one, with the locator naming the exact element.
            r.push(MISSING_ORDER_LINE, format!("{loc}/cac:LineItem"), "an order line must carry a cac:LineItem");
            continue;
        };
        if let Some(line) = parse_line(li, &loc, &mut r) {
            lines.push(line);
        }
    }

    if !r.errors.is_empty() {
        return Err(r);
    }

    Ok(ParsedUblOrder {
        document_id: usable_id.expect("no MISSING_DOCUMENT_ID/ID_TOO_LONG error means the id is present"),
        document_uuid,
        customization_id: child_text(root, NS_CBC, "CustomizationID"),
        profile_id: child_text(root, NS_CBC, "ProfileID"),
        issue_date: issue_date.expect("no date error means the date parsed"),
        issue_time,
        currency,
        buyer_reference,
        requested_delivery_date,
        notes,
        buyer,
        seller,
        lines,
        totals,
        warnings,
    })
}

/// One `cac:LineItem` → [`ParsedUblLine`]; `None` when the line must be dropped from the built
/// order. Every problem in the line is collected before returning — a line missing both quantity
/// and price reports both, not whichever the parser noticed first.
fn parse_line(li: Node, loc: &str, r: &mut UblRefusal) -> Option<ParsedUblLine> {
    let line_number = child_text(li, NS_CBC, "ID").unwrap_or_default();
    let item = child(li, NS_CAC, "Item");
    let mut usable = true;

    // Item identity: seller-assigned code preferred, GTIN as fallback and additionally carried
    // alongside. A bare name is not enough for the host to resolve an item.
    let seller_id = item
        .and_then(|it| child(it, NS_CAC, "SellersItemIdentification"))
        .and_then(|s| child_text(s, NS_CBC, "ID"));
    let gtin = item
        .and_then(|it| child(it, NS_CAC, "StandardItemIdentification"))
        .and_then(|s| child_text(s, NS_CBC, "ID"));
    let (item_id, item_id_source) = if let Some(s) = seller_id.clone() {
        (s, ItemIdSource::SellerAssigned)
    } else if let Some(g) = gtin.clone() {
        (g, ItemIdSource::StandardGtin)
    } else {
        r.push(MISSING_ITEM_IDENTIFIER, format!("{loc}/cac:LineItem/cac:Item"), "the item needs a seller-assigned id or a standard (GTIN) id — a name alone cannot be resolved");
        usable = false;
        (String::new(), ItemIdSource::SellerAssigned)
    };
    let item_name = item.and_then(|it| child_text(it, NS_CBC, "Name"));
    let description = item.and_then(|it| child_text(it, NS_CBC, "Description"));

    // Quantity: required, and unparseable / zero / negative all refuse (a negative or zero order
    // line is not something an internal order can carry). Placeholder values keep later fields
    // collecting their own problems; the line is dropped below when unusable.
    let qty_node = child(li, NS_CBC, "Quantity");
    let quantity = match qty_node.and_then(node_text) {
        None => {
            r.push(MISSING_LINE_QUANTITY, format!("{loc}/cac:LineItem/cbc:Quantity"), with_line("required".into(), qty_node));
            usable = false;
            Decimal::ZERO
        }
        Some(s) => match Decimal::from_str(&s) {
            Ok(q) if q > Decimal::ZERO => q,
            _ => {
                r.push(INVALID_LINE_QUANTITY, format!("{loc}/cac:LineItem/cbc:Quantity"), with_line(format!("\"{s}\" is not a positive decimal quantity"), qty_node));
                usable = false;
                Decimal::ZERO
            }
        },
    };
    let uom = qty_node.and_then(|n| node_attr(n, "unitCode"));

    // Price: a missing price would default the internal order line to free — always refuse.
    let price_node = child(li, NS_CAC, "Price").and_then(|p| child(p, NS_CBC, "PriceAmount"));
    let unit_price = match price_node.and_then(node_text) {
        None => {
            r.push(MISSING_LINE_PRICE, format!("{loc}/cac:LineItem/cac:Price/cbc:PriceAmount"), with_line("required — a missing price would default the internal order line to free".into(), price_node));
            usable = false;
            Decimal::ZERO
        }
        Some(s) => match Decimal::from_str(&s) {
            // Explicit zero is an explicit statement (a free sample line); only unparseable or
            // negative refuse.
            Ok(p) if p >= Decimal::ZERO => p,
            _ => {
                r.push(INVALID_LINE_PRICE, format!("{loc}/cac:LineItem/cac:Price/cbc:PriceAmount"), with_line(format!("\"{s}\" is not a non-negative decimal price"), price_node));
                usable = false;
                Decimal::ZERO
            }
        },
    };
    let price_currency = price_node.and_then(|n| node_attr(n, "currencyID"));
    if let Some(c) = &price_currency {
        if !is_currency_code(c) {
            r.push(INVALID_CURRENCY, format!("{loc}/cac:LineItem/cac:Price/cbc:PriceAmount/@currencyID"), format!("\"{c}\" is not a three-letter uppercase currency code"));
        }
    }

    // Base quantity (units the price applies to): optional, but unusable when sent.
    let base_node = child(li, NS_CAC, "Price").and_then(|p| child(p, NS_CBC, "BaseQuantity"));
    let base_quantity = match base_node.and_then(node_text) {
        None => None,
        Some(s) => match Decimal::from_str(&s) {
            Ok(q) if q > Decimal::ZERO => Some(q),
            _ => {
                r.push(INVALID_BASE_QUANTITY, format!("{loc}/cac:LineItem/cac:Price/cbc:BaseQuantity"), format!("\"{s}\" is not a positive decimal base quantity"));
                None
            }
        },
    };
    let base_uom = base_node.and_then(|n| node_attr(n, "unitCode"));

    // Line extension amount: optional; unparseable refuses (the amount-parse code with the line
    // locator).
    let line_amount = match child_text(li, NS_CBC, "LineExtensionAmount") {
        None => None,
        Some(s) => match Decimal::from_str(&s) {
            Ok(a) => Some(a),
            Err(_) => {
                r.push(INVALID_LINE_AMOUNT, format!("{loc}/cac:LineItem/cbc:LineExtensionAmount"), format!("\"{s}\" is not a decimal amount"));
                None
            }
        },
    };

    let requested_delivery_date =
        parse_delivery_date(child(li, NS_CAC, "Delivery"), &format!("{loc}/cac:LineItem/cac:Delivery/cbc:RequestedDeliveryDate"), r);

    // Length budgets on the identifiers/codes this line contributes to the canonical payload.
    check_text_budget(r, &format!("{loc}/cac:LineItem/cbc:ID"), Some(&line_number));
    check_text_budget(r, &format!("{loc}/cac:LineItem/cac:Item (id)"), Some(&item_id));
    check_text_budget(r, &format!("{loc}/cac:LineItem/cac:Item (gtin)"), gtin.as_ref());
    check_text_budget(r, &format!("{loc}/cac:LineItem/cac:Item/cbc:Name"), item_name.as_ref());

    if !usable {
        return None;
    }
    Some(ParsedUblLine {
        line_number,
        item_id,
        item_id_source,
        gtin,
        item_name,
        description,
        quantity,
        uom,
        unit_price,
        price_currency,
        base_quantity,
        base_uom,
        line_amount,
        requested_delivery_date,
    })
}

/// `cac:AnticipatedMonetaryTotal` → [`UblTotals`]. Members are optional; an unparseable member
/// refuses under the amount-parse code with the element's locator.
fn parse_totals(root: Node, r: &mut UblRefusal) -> Option<UblTotals> {
    let t = child(root, NS_CAC, "AnticipatedMonetaryTotal")?;
    let mut amount = |local: &str| -> Option<Decimal> {
        let text = child_text(t, NS_CBC, local)?;
        match Decimal::from_str(&text) {
            Ok(a) => Some(a),
            Err(_) => {
                r.push(INVALID_LINE_AMOUNT, format!("Order/cac:AnticipatedMonetaryTotal/cbc:{local}"), format!("\"{text}\" is not a decimal amount"));
                None
            }
        }
    };
    Some(UblTotals {
        line_extension: amount("LineExtensionAmount"),
        tax_exclusive: amount("TaxExclusiveAmount"),
        tax_inclusive: amount("TaxInclusiveAmount"),
        payable: amount("PayableAmount"),
    })
}

/// A buyer/seller `cac:Party` → [`UblParty`] (all members optional).
fn parse_party(party: Option<Node>, loc: &str, r: &mut UblRefusal) -> UblParty {
    let Some(p) = party else {
        return UblParty { id: None, scheme: None, endpoint_id: None, endpoint_scheme: None, name: None };
    };
    // id: first PartyIdentification/ID, fallback EndpointID, fallback PartyLegalEntity/CompanyID —
    // whichever supplied it also supplies the scheme.
    let ident = child(p, NS_CAC, "PartyIdentification").and_then(|i| child(i, NS_CBC, "ID"));
    let endpoint = child(p, NS_CBC, "EndpointID");
    let legal = child(p, NS_CAC, "PartyLegalEntity").and_then(|l| child(l, NS_CBC, "CompanyID"));
    let source = ident.or(endpoint).or(legal);
    let id = source.and_then(node_text);
    let scheme = source.and_then(|n| node_attr(n, "schemeID"));
    let endpoint_id = endpoint.and_then(node_text);
    let endpoint_scheme = endpoint.and_then(|n| node_attr(n, "schemeID"));
    let name = child(p, NS_CAC, "PartyLegalEntity")
        .and_then(|l| child_text(l, NS_CBC, "RegistrationName"))
        .or_else(|| child(p, NS_CAC, "PartyName").and_then(|n| child_text(n, NS_CBC, "Name")));

    if let Some(id) = &id {
        check_text_budget(r, &format!("Order/cac:{loc}/cac:Party (id)"), Some(id));
    }
    if let Some(name) = &name {
        check_text_budget(r, &format!("Order/cac:{loc}/cac:Party (name)"), Some(name));
    }

    UblParty { id, scheme, endpoint_id, endpoint_scheme, name }
}

/// A `cbc:RequestedDeliveryDate` under a `cac:Delivery`; absent → `None`, malformed → refusal.
fn parse_delivery_date(delivery: Option<Node>, locator: &str, r: &mut UblRefusal) -> Option<NaiveDate> {
    let text = delivery.and_then(|d| child_text(d, NS_CBC, "RequestedDeliveryDate"))?;
    match NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        Ok(d) => Some(d),
        Err(_) => {
            r.push(INVALID_DELIVERY_DATE, locator.to_string(), format!("\"{text}\" is not a strict ISO yyyy-mm-dd date"));
            None
        }
    }
}

// ---- small XML helpers (namespace-aware; the only roxmltree-touching code in the crate) ----

fn is_el(n: Node, ns: &str, local: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(ns) && n.tag_name().name() == local
}

/// First child element matching namespace + local name.
fn child<'a, 'i>(n: Node<'a, 'i>, ns: &str, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is_el(*c, ns, local))
}

/// Trimmed, non-empty text of the first matching child element; empty/whitespace text counts as
/// absent (senders emit empty elements for omitted fields).
fn child_text<'a, 'i>(n: Node<'a, 'i>, ns: &str, local: &str) -> Option<String> {
    child(n, ns, local).and_then(node_text)
}

fn node_text(n: Node) -> Option<String> {
    n.text().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

fn node_attr(n: Node, name: &str) -> Option<String> {
    n.attribute(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Clark-notation QName of an element, for refusal details.
fn qname(n: Node) -> String {
    match n.tag_name().namespace() {
        Some(ns) => format!("{{{}}}:{}", ns, n.tag_name().name()),
        None => format!("{} (no namespace)", n.tag_name().name()),
    }
}

fn is_currency_code(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase())
}

/// Append the element's source line to a refusal detail when the node is at hand.
fn with_line(detail: String, node: Option<Node>) -> String {
    match node {
        Some(n) => {
            let pos = n.document().text_pos_at(n.range().start);
            format!("{detail} (line {})", pos.row)
        }
        None => detail,
    }
}

/// Enforce the shared id/name/code length budget. The document id has its own tighter budget
/// (column-backed); everything else that identifies or names refuses over-long text rather than
/// silently truncating identity.
fn check_text_budget(r: &mut UblRefusal, field: &str, value: Option<&String>) {
    if let Some(v) = value {
        if v.chars().count() > MAX_TEXT {
            r.push(ID_TOO_LONG, field, format!("{} chars exceeds the {}-char budget", v.chars().count(), MAX_TEXT));
        }
    }
}

// A tiny builder convenience so the early-return refusals read declaratively.
impl UblRefusal {
    fn tap(mut self, code: &'static str, field: impl Into<String>, detail: impl Into<String>) -> Self {
        self.push(code, field, detail);
        self
    }
}

// Documented size guard shared with the write service (kept here so the refusal taxonomy and the
// limit it enforces live together).
pub fn too_large_refusal(len: usize, budget: usize) -> UblRefusal {
    UblRefusal::new(None).tap(
        DOCUMENT_TOO_LARGE,
        "document",
        format!("{len} bytes exceeds the {budget}-byte inbound budget"),
    )
}
