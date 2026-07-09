//! The mapping seam against the REAL backbone-selling module. An inbound EDI purchase order is mapped to a
//! genuine sales order via the `MappingPort` implemented over REAL selling. Proves the exchange lands a
//! real internal document. ZERO normal Cargo edge — selling is reached through the port, a dev-dependency
//! only in the test.

mod common;
use common::*;

use backbone_edi::application::service::edi_write_service::*;
use serde_json::json;
use uuid::Uuid;

// ESEAM-1 — an inbound EDI PO becomes a REAL sales order in backbone-selling.
#[tokio::test]
async fn eseam1_inbound_po_becomes_real_sales_order() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let mapper = RealSellingMapper::new(pool.clone());
    let sink = CapturingSink::new();

    let partner_id = svc.create_partner(NewPartner {
        company_id: company, name: "Acme Retail".into(), partner_code: format!("ACME-{}", Uuid::new_v4()),
        format: "custom_json".into(), partner_direction: "inbound".into(),
    }).await.unwrap();

    let customer = Uuid::new_v4();
    let item = Uuid::new_v4();
    let control = format!("PO-{}", Uuid::new_v4());
    let out = svc.receive_document(InboundDoc {
        company_id: company, partner_id, doc_type: "purchase_order".into(), control_number: control.clone(), business_key: control.clone(),
        raw: "ISA*...".into(),
        payload: json!({"customer_id": customer.to_string(), "lines": [
            {"item_id": item.to_string(), "qty": "5", "price": "20000"}
        ]}),
    }, &mapper, &sink).await.unwrap();

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
