//! Golden cases — the exchange-lifecycle oracle: receive an inbound document, map it to an internal one,
//! dedup a retransmission, surface an unmappable document, and acknowledge.

mod common;
use common::*;

use backbone_edi::application::service::edi_events::LoggingSink;
use backbone_edi::application::service::edi_write_service::*;
use serde_json::json;
use uuid::Uuid;

async fn partner(svc: &EdiWriteService, company: Uuid) -> Uuid {
    svc.create_partner(NewPartner {
        company_id: company, name: "Acme".into(), partner_code: format!("ACME-{}", Uuid::new_v4()),
        format: "custom_json".into(), partner_direction: "both".into(),
    }).await.unwrap()
}

fn po(company: Uuid, partner_id: Uuid, control: &str) -> InboundDoc {
    InboundDoc {
        company_id: company, partner_id, doc_type: "purchase_order".into(), control_number: control.into(), business_key: control.into(),
        raw: "{...raw edi...}".into(),
        payload: json!({"customer_id": Uuid::new_v4().to_string(), "lines": [{"item_id": Uuid::new_v4().to_string(), "qty": "3", "price": "1000"}]}),
    }
}

// EGC-1 — an inbound PO maps to an internal document and publishes EdiDocumentMapped.
#[tokio::test]
async fn egc1_inbound_po_maps() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc, company).await;
    let mapper = FakeMapper::new();
    let sink = CapturingSink::new();

    let out = svc.receive_document(po(company, p, "CTRL-1"), &mapper, &sink).await.unwrap();
    assert!(!out.duplicate);
    assert_eq!(out.status, "mapped");
    assert!(out.mapped_ref_id.is_some());
    assert_eq!(sink.mapped(), 1);
    assert_eq!(mapper.count(), 1);
}

// EGC-2 — a retransmission (same partner + control_number) is idempotent: no re-map, no duplicate order.
#[tokio::test]
async fn egc2_retransmission_idempotent() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc, company).await;
    let mapper = FakeMapper::new();
    let sink = CapturingSink::new();

    let first = svc.receive_document(po(company, p, "CTRL-2"), &mapper, &sink).await.unwrap();
    let second = svc.receive_document(po(company, p, "CTRL-2"), &mapper, &sink).await.unwrap();
    assert!(!first.duplicate);
    assert!(second.duplicate);
    assert_eq!(first.document_id, second.document_id);
    assert_eq!(mapper.count(), 1, "mapped once — no duplicate internal order from a retransmission");
    assert_eq!(sink.mapped(), 1);
}

// EGC-3 — an unmappable document is recorded 'failed' with the reason and publishes EdiDocumentFailed.
#[tokio::test]
async fn egc3_unmappable_document_fails() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc, company).await;
    let mapper = FakeMapper::rejecting("bad_payload", "missing customer");
    let sink = CapturingSink::new();

    let out = svc.receive_document(po(company, p, "CTRL-3"), &mapper, &sink).await.unwrap();
    assert_eq!(out.status, "failed");
    assert_eq!(sink.failed(), 1);
    let (status, err): (String, Option<String>) = sqlx::query_as(
        "SELECT status::text, error_detail FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "failed");
    assert_eq!(err.as_deref(), Some("missing customer"));
}

// EGC-4 — acknowledge issues a functional ack (state-guarded, idempotent) and publishes.
#[tokio::test]
async fn egc4_acknowledge() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc, company).await;
    let sink = CapturingSink::new();

    let out = svc.receive_document(po(company, p, "CTRL-4"), &FakeMapper::new(), &sink).await.unwrap();
    assert!(svc.acknowledge(out.document_id, &sink).await.unwrap());
    assert!(!svc.acknowledge(out.document_id, &sink).await.unwrap(), "second ack is a no-op");
    assert_eq!(sink.acknowledged(), 1);
    let status: String = sqlx::query_scalar("SELECT status::text FROM edi.edi_documents WHERE id=$1")
        .bind(out.document_id).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "acknowledged");
    let _ = LoggingSink;
}

// EGC-5 — the acknowledgement event carries the 997 polarity + reason (completeness council 2026-07-09),
// so a consumer generates the correct positive/negative wire ack from the event alone.
#[tokio::test]
async fn egc5_acknowledge_event_carries_polarity() {
    use backbone_edi::application::service::edi_events::EdiEvent;
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc, company).await;
    let sink = CapturingSink::new();

    // A REJECTED document → negative ack carrying the reason.
    let bad = svc.receive_document(po(company, p, "CTRL-R"), &FakeMapper::rejecting("bad", "missing customer"), &sink).await.unwrap();
    svc.acknowledge(bad.document_id, &sink).await.unwrap();
    // An ACCEPTED document → positive ack, no reason.
    let good = svc.receive_document(po(company, p, "CTRL-G"), &FakeMapper::new(), &sink).await.unwrap();
    svc.acknowledge(good.document_id, &sink).await.unwrap();

    let acks: Vec<(Uuid, bool, Option<String>)> = sink.events.lock().unwrap().iter().filter_map(|e| match e {
        EdiEvent::EdiDocumentAcknowledged { document_id, accepted, error_detail, .. } =>
            Some((*document_id, *accepted, error_detail.clone())),
        _ => None,
    }).collect();
    let rej = acks.iter().find(|(id, ..)| *id == bad.document_id).expect("reject ack");
    let acc = acks.iter().find(|(id, ..)| *id == good.document_id).expect("accept ack");
    assert!(!rej.1, "rejected document → negative ack");
    assert_eq!(rej.2.as_deref(), Some("missing customer"), "negative ack carries the reason");
    assert!(acc.1, "mapped document → positive ack");
    assert_eq!(acc.2, None, "positive ack has no reason");
}
