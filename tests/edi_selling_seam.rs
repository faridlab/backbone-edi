//! The mapping seam against the REAL backbone-selling module. An inbound EDI purchase order is mapped to a
//! genuine sales order via the `MappingPort` implemented over REAL selling. Proves the exchange lands a
//! real internal document. ZERO normal Cargo edge — selling is reached through the port, a dev-dependency
//! only in the test.
//!
//! Tenancy (ADR-0029): the edi side of the seam is tenant-agnostic. Selling is still a company-fenced
//! sibling, so the seam keeps the port contract's explicit `company_id` on `MapRequest` — the edi write
//! service fills it from the ambient org scope's legacy company (fail-closed), which is why every probe
//! below wraps its writes in the scoped helper.

mod common;
use common::*;

use backbone_edi::application::service::edi_ports::{MapAck, MapRejected, MapRequest, MappingPort};
use backbone_edi::application::service::edi_write_service::*;
use serde_json::json;
use std::str::FromStr;
use uuid::Uuid;

// ESEAM-1 — an inbound EDI PO becomes a REAL sales order in backbone-selling.
#[tokio::test]
async fn eseam1_inbound_po_becomes_real_sales_order() {
    let pool = pool().await;
    let svc = EdiWriteService::new(pool.clone());
    let mapper = RealSellingMapper::new(pool.clone());
    let sink = CapturingSink::new();

    let customer = Uuid::new_v4();
    let item = Uuid::new_v4();
    let control = format!("PO-{}", Uuid::new_v4());
    let out = scoped(&pool, async {
        let partner_id = svc.create_partner(NewPartner {
            name: "Acme Retail".into(), partner_code: format!("ACME-{}", Uuid::new_v4()),
            format: "custom_json".into(), partner_direction: "inbound".into(),
        }).await.unwrap();

        svc.receive_document(InboundDoc {
            partner_id, doc_type: "purchase_order".into(), control_number: control.clone(), business_key: control.clone(),
            raw: "ISA*...".into(),
            payload: json!({"customer_id": customer.to_string(), "lines": [
                {"item_id": item.to_string(), "qty": "5", "price": "20000"}
            ]}),
        }, &mapper, &sink).await.unwrap()
    }).await;

    assert_eq!(out.status, "mapped");
    let order_id = out.mapped_ref_id.expect("mapped to a sales order");

    // A REAL sales order exists in backbone-selling, for the PO's customer, with the mapped total.
    let (cust, total, status): (Uuid, rust_decimal::Decimal, String) = sqlx::query_as(
        "SELECT customer_id, total, status::text FROM selling.sales_orders WHERE id=$1")
        .bind(order_id).fetch_one(&pool).await.unwrap();
    assert_eq!(cust, customer, "the sales order is for the PO's customer");
    assert_eq!(total, rust_decimal::Decimal::new(100000, 0), "5 × 20000 = 100,000");
    assert_eq!(status, "draft");

    // The EDI document records the link back to the internal order.
    let mapped_ref: Option<Uuid> = sqlx::query_scalar(
        "SELECT mapped_ref_id FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(mapped_ref, Some(order_id));
}

/// A FUTURE-HOST adapter over the canonical UBL payload: exactly what a composing service's
/// `MappingPort` implementation will do — resolve buyer→customer and item_id→item master (stubbed
/// here to fixed test ids), parse the decimal STRINGS back via `Decimal::from_str`, and create a
/// real sales order. Proves the canonical contract ALONE is sufficient to land an internal order.
pub struct UblSellingMapper {
    pub selling: backbone_selling::application::service::selling_write_service::SellingWriteService,
    /// Stub of the buyer→customer resolution.
    pub customer_id: Uuid,
    /// Stub of the item_id→item-master resolution.
    pub item_id: Uuid,
}

#[async_trait::async_trait]
impl MappingPort for UblSellingMapper {
    async fn map(&self, req: &MapRequest) -> Result<MapAck, MapRejected> {
        use backbone_selling::application::service::selling_write_service::{NewLine, NewSalesOrder};
        let p = &req.payload;
        if p["kind"] != "ubl_bis3_order" || p["schema"] != 1 {
            return Err(MapRejected { code: "bad_payload".into(), message: "not a ubl_bis3_order schema:1 payload".into() });
        }
        let business_key = p["document"]["id"].as_str()
            .ok_or(MapRejected { code: "bad_payload".into(), message: "missing document.id".into() })?;
        let order_date = p["document"]["issue_date"].as_str()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .ok_or(MapRejected { code: "bad_payload".into(), message: "missing/invalid document.issue_date".into() })?;
        let delivery_date = match p["document"]["requested_delivery_date"].as_str() {
            Some(s) => Some(chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|e| MapRejected { code: "bad_payload".into(), message: format!("invalid delivery date: {e}") })?),
            None => None,
        };
        let lines: Vec<NewLine> = p["lines"].as_array().map(|arr| arr.iter().map(|l| {
            let qty = l["qty"].as_str().and_then(|s| s.parse().ok());
            let price = l["unit_price"].as_str().and_then(|s| s.parse().ok());
            NewLine {
                item_id: self.item_id,
                revenue_account_id: None,
                description: l["item_name"].as_str().map(|s| s.to_string()),
                quantity: qty.unwrap_or(rust_decimal::Decimal::ZERO),
                unit_price: price.unwrap_or(rust_decimal::Decimal::ZERO),
                line_discount: rust_decimal::Decimal::ZERO,
                invoice_policy: None,
                is_downpayment: None,
            }
        }).collect()).unwrap_or_default();
        if lines.is_empty() {
            return Err(MapRejected { code: "bad_payload".into(), message: "no lines".into() });
        }
        let order = NewSalesOrder {
            order_number: format!("UBL-{business_key}"),
            quotation_id: None,
            delivery_carrier_id: None,
            // The port contract keeps the owning tenant: selling is still a company-fenced sibling.
            company_id: req.company_id,
            branch_id: None,
            customer_id: self.customer_id,
            order_date,
            delivery_date,
            currency: p["document"]["currency"].as_str().map(|s| s.to_string()),
            tax_rate: rust_decimal::Decimal::ZERO,
            notes: Some("Created from inbound UBL BIS 3 order".into()),
            lines,
        };
        match self.selling.create_sales_order(order).await {
            Ok(id) => Ok(MapAck { internal_ref_type: "sales_order".into(), internal_ref_id: id }),
            Err(e) => Err(MapRejected { code: "selling_rejected".into(), message: e.to_string() }),
        }
    }
}

// USEAM-1 — the canonical UBL payload alone creates a REAL sales order with exact quantities,
// prices, currency, and delivery date (the future host adapter, proven in-tree).
#[tokio::test]
async fn useam1_canonical_ubl_payload_creates_real_sales_order() {
    let pool = pool().await;
    let svc = EdiWriteService::new(pool.clone());
    let customer = Uuid::new_v4();
    let item = Uuid::new_v4();
    let mapper = UblSellingMapper {
        selling: backbone_selling::application::service::selling_write_service::SellingWriteService::new(pool.clone()),
        customer_id: customer,
        item_id: item,
    };
    let sink = CapturingSink::new();

    // selling.sales_orders.order_number is unique GLOBALLY (not per company), so the business key
    // carries a per-run suffix to survive repeated executions against the same database.
    let business_key = format!("PO-USEAM-{}", &Uuid::new_v4().to_string()[..8]);
    let raw = include_str!("fixtures/ubl/valid_order.xml").replace("PO-2026-1001", &business_key);
    let out = scoped(&pool, async {
        let partner_id = svc.create_partner(NewPartner {
            name: "BIS3 Buyer".into(), partner_code: format!("BIS3-{}", Uuid::new_v4()),
            format: "ubl_bis3".into(), partner_direction: "inbound".into(),
        }).await.unwrap();
        svc.receive_ubl_order(partner_id, &raw, &mapper, &sink).await
    }).await.unwrap();
    assert_eq!(out.status, "mapped");
    let order_id = out.mapped_ref_id.expect("mapped to a sales order");

    // A REAL sales order exists, derived from the canonical contract fields alone.
    let (number, cust, currency, delivery, total, status): (String, Uuid, String, Option<chrono::NaiveDate>, rust_decimal::Decimal, String) =
        sqlx::query_as("SELECT order_number, customer_id, currency, delivery_date, total, status::text FROM selling.sales_orders WHERE id=$1")
            .bind(order_id).fetch_one(&pool).await.unwrap();
    assert_eq!(number, format!("UBL-{business_key}"));
    assert_eq!(cust, customer);
    assert_eq!(currency, "IDR");
    assert_eq!(delivery, Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()));
    assert_eq!(total, rust_decimal::Decimal::from_str("25000.1").unwrap(), "2×12500.00 + 1×0.1");
    assert_eq!(status, "draft");

    // Line-level decimal exactness, straight from the payload's decimal STRINGS.
    // Item ids are random UUIDs, so order deterministically by price for the exactness check.
    let lines: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT quantity, unit_price FROM selling.sales_order_items WHERE order_id=$1 ORDER BY unit_price DESC")
        .bind(order_id).fetch_all(&pool).await.unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], (rust_decimal::Decimal::from_str("2").unwrap(), rust_decimal::Decimal::from_str("12500.00").unwrap()));
    assert_eq!(lines[1], (rust_decimal::Decimal::from_str("1").unwrap(), rust_decimal::Decimal::from_str("0.1").unwrap()));
}
