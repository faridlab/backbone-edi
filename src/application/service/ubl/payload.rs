//! The canonical UBL order payload (hand-authored, user-owned).
//!
//! THE de-facto host contract: this exact JSON shape is what a host-side `MappingPort` adapter
//! consumes, recorded verbatim in
//! docs/adr/ADR-002-ubl-bis3-inbound-order-import.md. Evolve it by bumping `schema` and adding
//! fields — never by changing a key's meaning or type in place.
//!
//! Decimal fidelity: every amount and quantity is a JSON STRING produced by
//! `rust_decimal::Decimal::to_string` (scale-preserving — `"12500.00"` stays `"12500.00"`).
//! `f64` is never involved anywhere on the path. Absent optionals are present as `null` so the
//! key set is stable regardless of what the sender omitted.

use serde_json::{json, Value};

use super::model::{ItemIdSource, ParsedUblOrder};

/// Canonical payload builder: [`ParsedUblOrder`] → the versioned, self-describing JSON contract.
pub fn to_payload(o: &ParsedUblOrder) -> Value {
    json!({
        "kind": "ubl_bis3_order",
        "schema": 1,
        "document": {
            "id": o.document_id,
            "uuid": o.document_uuid,
            "customization_id": o.customization_id,
            "profile_id": o.profile_id,
            "issue_date": o.issue_date.format("%Y-%m-%d").to_string(),
            "issue_time": o.issue_time,
            "currency": o.currency,
            "buyer_reference": o.buyer_reference,
            "requested_delivery_date": date_str(&o.requested_delivery_date),
            "notes": o.notes,
        },
        "buyer": {
            "id": o.buyer.id,
            "scheme": o.buyer.scheme,
            "endpoint_id": o.buyer.endpoint_id,
            "endpoint_scheme": o.buyer.endpoint_scheme,
            "name": o.buyer.name,
        },
        "seller": {
            "id": o.seller.id,
            "scheme": o.seller.scheme,
            "name": o.seller.name,
        },
        "lines": o.lines.iter().map(|l| json!({
            "line_number": l.line_number,
            "item_id": l.item_id,
            "item_id_source": match l.item_id_source {
                ItemIdSource::SellerAssigned => "seller_assigned",
                ItemIdSource::StandardGtin => "standard_gtin",
            },
            "gtin": l.gtin,
            "item_name": l.item_name,
            "description": l.description,
            "qty": l.quantity.to_string(),
            "uom": l.uom,
            "unit_price": l.unit_price.to_string(),
            "price_currency": l.price_currency,
            "base_qty": opt_decimal(&l.base_quantity),
            "base_uom": l.base_uom,
            "line_amount": opt_decimal(&l.line_amount),
            "requested_delivery_date": date_str(&l.requested_delivery_date),
        })).collect::<Vec<_>>(),
        "totals": {
            "line_extension": o.totals.as_ref().and_then(|t| opt_decimal(&t.line_extension)),
            "tax_exclusive": o.totals.as_ref().and_then(|t| opt_decimal(&t.tax_exclusive)),
            "tax_inclusive": o.totals.as_ref().and_then(|t| opt_decimal(&t.tax_inclusive)),
            "payable": o.totals.as_ref().and_then(|t| opt_decimal(&t.payable)),
        },
        "warnings": o.warnings,
    })
}

fn opt_decimal(d: &Option<rust_decimal::Decimal>) -> Option<String> {
    d.as_ref().map(|v| v.to_string())
}

fn date_str(d: &Option<chrono::NaiveDate>) -> Option<String> {
    d.map(|v| v.format("%Y-%m-%d").to_string())
}
