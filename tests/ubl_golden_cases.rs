//! UBL golden cases — the inbound-order-import oracle against a live database: the partner gate,
//! the idempotent claim, the durable negative ack for refused-but-identifiable documents, the
//! fail-fast for unidentifiable ones, and the re-drive semantics for crash-stranded and corrected
//! retransmissions.

mod common;
use common::*;

use backbone_edi::application::service::edi_events::EdiEvent;
use backbone_edi::application::service::edi_write_service::*;
use uuid::Uuid;

const VALID: &str = include_str!("fixtures/ubl/valid_order.xml");
const VALID_UUID: &str = "6b7c8d9e-0f1a-2b3c-4d5e-6f7a8b9c0d1e";

/// A valid order under a per-test-unique business key (the dedup key is per partner, but unique
/// ids keep tests independent even against a reused partner row).
fn order(id: &str) -> String {
    VALID.replace("PO-2026-1001", id)
}

async fn ubl_partner(svc: &EdiWriteService, company: Uuid) -> Uuid {
    svc.create_partner(NewPartner {
        company_id: company,
        name: "BIS3 Buyer".into(),
        partner_code: format!("BIS3-{}", Uuid::new_v4()),
        format: "ubl_bis3".into(),
        partner_direction: "inbound".into(),
    })
    .await
    .unwrap()
}

async fn row_count(pool: &sqlx::PgPool, partner_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM edi.edi_documents WHERE partner_id=$1")
        .bind(partner_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// UGC-1 — a valid UBL order maps: business key = cbc:ID, control number = cbc:UUID, raw stored
// verbatim, the canonical payload handed to the mapper, EdiDocumentMapped emitted.
#[tokio::test]
async fn ugc1_valid_ubl_order_maps() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let mapper = FakeMapper::new();
    let sink = CapturingSink::new();

    let raw = order("PO-UGC1-A");
    let out = svc.receive_ubl_order(company, partner_id, &raw, &mapper, &sink).await.unwrap();
    assert!(!out.duplicate);
    assert_eq!(out.status, "mapped");
    assert!(out.mapped_ref_id.is_some());
    assert_eq!(sink.mapped(), 1);

    let (business_key, control_number, stored_raw): (String, String, String) = sqlx::query_as(
        "SELECT business_key, control_number, payload FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(business_key, "PO-UGC1-A", "the buyer's order number is the dedup key");
    assert_eq!(control_number, VALID_UUID, "the instance UUID is the control number");

    // The raw XML is stored verbatim and the mapper saw the canonical contract.
    assert_eq!(stored_raw, raw);
    assert_eq!(mapper.count(), 1);
    let req = &mapper.maps.lock().unwrap()[0];
    assert_eq!(req.payload["kind"], "ubl_bis3_order");
    assert_eq!(req.payload["schema"], 1);
    assert_eq!(req.payload["document"]["id"], "PO-UGC1-A");
    assert_eq!(req.payload["lines"][0]["unit_price"], "12500.00");
    assert_eq!(req.idempotency_key, out.document_id.to_string());
    assert_eq!(req.doc_type, "purchase_order");
}

// UGC-2 — a retransmission of the same cbc:ID is idempotent: no re-map, same document id.
#[tokio::test]
async fn ugc2_retransmission_idempotent() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let mapper = FakeMapper::new();
    let sink = CapturingSink::new();

    let raw = order("PO-UGC2-A");
    let first = svc.receive_ubl_order(company, partner_id, &raw, &mapper, &sink).await.unwrap();
    let second = svc.receive_ubl_order(company, partner_id, &raw, &mapper, &sink).await.unwrap();
    assert!(!first.duplicate);
    assert!(second.duplicate);
    assert_eq!(first.document_id, second.document_id);
    assert_eq!(mapper.count(), 1, "mapped once — a replayed UBL order cannot double-create");
    assert_eq!(sink.mapped(), 1);
}

// UGC-3 — correction re-drive: a refused-but-identifiable order lands `failed` with a negative
// ack path; a corrected retransmission of the SAME business key re-drives to `mapped` on the SAME
// row — no second document. Once the negative ack is ISSUED the row is terminal: a correction
// after that must arrive under a new business key (the deliberate v0.6.0 gate), so the re-drive
// here rides an UNacknowledged refusal.
#[tokio::test]
async fn ugc3_correction_redrive_after_refusal() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let sink = CapturingSink::new();

    // A refusal is durable: failed + joined codes + EdiDocumentFailed, mapper untouched.
    let refused = include_str!("fixtures/ubl/missing_price.xml").replace("PO-NOPRICE-1", "PO-UGC3-A");
    let rejecting = FakeMapper::rejecting("unreachable", "never called");
    let out = svc.receive_ubl_order(company, partner_id, &refused, &rejecting, &sink).await.unwrap();
    assert_eq!(out.status, "failed");
    assert!(!out.duplicate);
    assert_eq!(rejecting.count(), 0, "a parse refusal never calls the mapper");
    assert_eq!(sink.failed(), 1);
    let (status, detail): (String, Option<String>) = sqlx::query_as(
        "SELECT status::text, error_detail FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "failed");
    assert!(detail.as_deref().unwrap_or_default().contains("missing_line_price"), "joined codes stored: {detail:?}");

    // The negative functional ack flows through the existing acknowledge surface.
    assert!(svc.acknowledge(out.document_id, company, &sink).await.unwrap());
    let ack = sink.events.lock().unwrap().iter().rev().find_map(|e| match e {
        EdiEvent::EdiDocumentAcknowledged { document_id, accepted, error_detail, .. } if *document_id == out.document_id =>
            Some((*accepted, error_detail.clone())),
        _ => None,
    }).expect("ack event");
    assert!(!ack.0, "refused document → negative ack");
    assert!(ack.1.as_deref().unwrap_or_default().contains("missing_line_price"));

    // Acknowledged is terminal: a correction under the SAME key dedups, it does not re-drive.
    let corrected_late = order("PO-UGC3-A");
    let out_late = svc.receive_ubl_order(company, partner_id, &corrected_late, &FakeMapper::new(), &sink).await.unwrap();
    assert!(out_late.duplicate);
    assert_eq!(out_late.status, "acknowledged");

    // The re-drive proper: a SECOND refusal, NOT yet acknowledged, corrected on retransmission.
    let refused2 = include_str!("fixtures/ubl/missing_price.xml").replace("PO-NOPRICE-1", "PO-UGC3-B");
    let out2 = svc.receive_ubl_order(company, partner_id, &refused2, &FakeMapper::new(), &sink).await.unwrap();
    assert_eq!(out2.status, "failed");
    let corrected = order("PO-UGC3-B");
    let accepting = FakeMapper::new();
    let out3 = svc.receive_ubl_order(company, partner_id, &corrected, &accepting, &sink).await.unwrap();
    assert!(!out3.duplicate, "a re-drive that does work is not a duplicate");
    assert_eq!(out3.status, "mapped");
    assert_eq!(out3.document_id, out2.document_id, "same row — no second document");
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM edi.edi_documents WHERE partner_id=$1 AND business_key='PO-UGC3-B'")
        .bind(partner_id).fetch_one(&pool).await.unwrap();
    assert_eq!(rows, 1);
    assert_eq!(accepting.count(), 1);
}

// UGC-4 — crash-stranded re-drive: a row stuck `received` with no mapped_ref (crash between the
// order creation and the status UPDATE) settles on retransmission, and the re-drive's
// idempotency_key is the SAME document id — the target hands back the same order, not a new one.
#[tokio::test]
async fn ugc4_crash_stranded_redrive() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let sink = CapturingSink::new();

    let raw = order("PO-UGC4-A");
    let out = svc.receive_ubl_order(company, partner_id, &raw, &FakeMapper::new(), &sink).await.unwrap();
    assert_eq!(out.status, "mapped");

    // Force the crash-stranded shape: settled back to received with no mapped ref.
    sqlx::query("UPDATE edi.edi_documents SET status='received', mapped_ref_type=NULL, mapped_ref_id=NULL WHERE id=$1")
        .bind(out.document_id).execute(&pool).await.unwrap();

    let re = FakeMapper::new();
    let out2 = svc.receive_ubl_order(company, partner_id, &raw, &re, &sink).await.unwrap();
    assert!(!out2.duplicate);
    assert_eq!(out2.status, "mapped");
    assert_eq!(out2.document_id, out.document_id, "same row");
    assert_eq!(re.count(), 1);
    assert_eq!(
        re.maps.lock().unwrap()[0].idempotency_key,
        out.document_id.to_string(),
        "the re-drive carries the SAME idempotency key — no duplicate internal order"
    );
    assert_eq!(row_count(&pool, partner_id).await, 1);
}

// UGC-5 — a parse refusal WITH a business key is durable: recorded failed, joined codes in
// error_detail, EdiDocumentFailed staged, mapper untouched.
#[tokio::test]
async fn ugc5_parse_refusal_with_id_is_durable() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let sink = CapturingSink::new();

    let raw = include_str!("fixtures/ubl/invalid_qty_zero.xml").replace("PO-QTYZERO-1", "PO-UGC5-A");
    let mapper = FakeMapper::new();
    let out = svc.receive_ubl_order(company, partner_id, &raw, &mapper, &sink).await.unwrap();
    assert_eq!(out.status, "failed");
    assert!(!out.duplicate);
    assert_eq!(mapper.count(), 0);
    assert_eq!(sink.failed(), 1);
    let (status, key, detail): (String, String, Option<String>) = sqlx::query_as(
        "SELECT status::text, business_key, error_detail FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "failed");
    assert_eq!(key, "PO-UGC5-A", "identified by the refused document's own cbc:ID");
    assert!(detail.unwrap_or_default().contains("invalid_line_quantity"));
}

// UGC-6 — a parse refusal WITHOUT a business key fails fast: Err(UblRefused), nothing persisted,
// no events; the same holds for an oversized document.
#[tokio::test]
async fn ugc6_parse_refusal_without_id_fails_fast() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let sink = CapturingSink::new();

    let no_id = include_str!("fixtures/ubl/missing_id.xml");
    let err = svc.receive_ubl_order(company, partner_id, no_id, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::UblRefused(ref d) if d.contains("missing_document_id")), "{err}");

    let oversized = format!("{}{}", order("PO-UGC6-B"), "\n".repeat(300_000));
    let err = svc.receive_ubl_order(company, partner_id, &oversized, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::UblRefused(ref d) if d.contains("document_too_large")), "{err}");

    assert_eq!(row_count(&pool, partner_id).await, 0, "nothing persisted for either");
    assert_eq!(sink.events.lock().unwrap().len(), 0, "no events for either");
}

// UGC-7 — the partner gate: unknown partner, non-UBL format, outbound-only, and inactive partners
// all refuse typed, before any parsing side effects touch the exchange.
#[tokio::test]
async fn ugc7_partner_gate() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let sink = CapturingSink::new();

    // Unknown partner.
    let err = svc.receive_ubl_order(company, Uuid::new_v4(), VALID, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::NotFound(_)), "{err}");

    // Declared format is not ubl_bis3 — payload sniffing must not rescue it.
    let json_partner = svc.create_partner(NewPartner {
        company_id: company, name: "JSON partner".into(), partner_code: format!("JSON-{}", Uuid::new_v4()),
        format: "custom_json".into(), partner_direction: "both".into(),
    }).await.unwrap();
    let err = svc.receive_ubl_order(company, json_partner, VALID, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::Invalid(ref m) if m.contains("ubl_bis3")), "{err}");

    // Outbound-only partner.
    let out_partner = svc.create_partner(NewPartner {
        company_id: company, name: "Outbound only".into(), partner_code: format!("OUT-{}", Uuid::new_v4()),
        format: "ubl_bis3".into(), partner_direction: "outbound".into(),
    }).await.unwrap();
    let err = svc.receive_ubl_order(company, out_partner, VALID, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::Invalid(ref m) if m.contains("inbound")), "{err}");

    // Inactive partner.
    let inactive = ubl_partner(&svc, company).await;
    sqlx::query("UPDATE edi.trading_partners SET status='inactive' WHERE id=$1")
        .bind(inactive).execute(&pool).await.unwrap();
    let err = svc.receive_ubl_order(company, inactive, VALID, &FakeMapper::new(), &sink).await.unwrap_err();
    assert!(matches!(err, EdiError::Invalid(ref m) if m.contains("active")), "{err}");

    assert_eq!(row_count(&pool, json_partner).await, 0);
    assert_eq!(row_count(&pool, out_partner).await, 0);
    assert_eq!(row_count(&pool, inactive).await, 0);
    assert_eq!(sink.events.lock().unwrap().len(), 0);
}

// UGC-8 — a settled row is immune to re-drive: a mapped retransmission returns the stored
// duplicate outcome, the reset matches zero rows, and no second event fires.
#[tokio::test]
async fn ugc8_mapped_retransmission_immune_to_redrive() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let partner_id = ubl_partner(&svc, company).await;
    let sink = CapturingSink::new();

    let raw = order("PO-UGC8-A");
    let out = svc.receive_ubl_order(company, partner_id, &raw, &FakeMapper::new(), &sink).await.unwrap();
    assert_eq!(out.status, "mapped");

    let mapper = FakeMapper::new();
    let again = svc.receive_ubl_order(company, partner_id, &raw, &mapper, &sink).await.unwrap();
    assert!(again.duplicate, "a mapped row is returned untouched");
    assert_eq!(again.status, "mapped");
    assert_eq!(again.document_id, out.document_id);
    assert_eq!(mapper.count(), 0, "the reset guard matched zero rows — no re-map");
    assert_eq!(sink.mapped(), 1);

    // Acknowledged stays terminal: even a forced reset-shape probe cannot re-open it via receive.
    assert!(svc.acknowledge(out.document_id, company, &sink).await.unwrap());
    let raw2 = order("PO-UGC8-A");
    let third = svc.receive_ubl_order(company, partner_id, &raw2, &mapper, &sink).await.unwrap();
    assert!(third.duplicate);
    assert_eq!(third.status, "acknowledged");
    assert_eq!(mapper.count(), 0);
}
