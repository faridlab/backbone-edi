//! Integrity probes — the exchange invariants: control number required, and the lifecycle event is
//! durable (staged in the outbox, survives a lost in-proc publish).
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — no tenant key on any write. The one
//! tenancy-adjacent dependency is the durable-event path: `outbox_events` is a framework-owned,
//! still-company-keyed surface, so probes that run a real write wrap it in an org request scope
//! carrying a legacy company — the same thing a composing service's scope-resolving auth
//! middleware provides. Everything asserted is domain; fence behavior is the composing service's
//! to prove. (The former "one partner per code" probe pinned the per-company code unique — that
//! unique is composition posture now, so the invariant is proved host-side and was deleted here.)

mod common;
use common::*;

use backbone_edi::application::service::edi_write_service::*;
use serde_json::json;
use uuid::Uuid;

async fn partner(svc: &EdiWriteService) -> Uuid {
    svc.create_partner(NewPartner {
        name: "Acme".into(), partner_code: format!("ACME-{}", Uuid::new_v4()),
        format: "custom_json".into(), partner_direction: "both".into(),
    }).await.unwrap()
}

fn doc(partner_id: Uuid, control: &str) -> InboundDoc {
    InboundDoc {
        partner_id, doc_type: "purchase_order".into(), control_number: control.into(), business_key: control.into(),
        raw: "{}".into(), payload: json!({}),
    }
}

// EIP-1 — an inbound document needs a control number (the dedup key).
#[tokio::test]
async fn eip1_control_number_required() {
    let pool = pool().await;
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc).await;
    let r = scoped(&pool, async {
        svc.receive_document(doc(p, "  "), &FakeMapper::new(), &CapturingSink::new()).await
    }).await;
    assert!(matches!(r, Err(EdiError::Invalid(_))));
}

// EIP-3 — the lifecycle event is durable: with the in-proc publish lost (dropping sink), EdiDocumentMapped
// is still staged in the outbox for the relay.
#[tokio::test]
async fn eip3_lifecycle_event_durable_via_outbox() {
    let pool = pool().await;
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc).await;
    let out = scoped(&pool, async {
        svc.receive_document(doc(p, &format!("CTRL-{}", Uuid::new_v4())), &FakeMapper::new(), &DroppingSink).await
    }).await.unwrap();
    let staged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM edi.outbox_events WHERE aggregate_id=$1 AND event_type='EdiDocumentMapped'")
        .bind(out.document_id.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(staged, 1, "EdiDocumentMapped durably staged despite the lost publish");
}

// EIP-4 — a partner reissuing a RECYCLED control number for a DIFFERENT business document must map as new,
// not be dropped as a "retransmission" (maturity council 2026-07-09). Dedup is on the business key (the
// PO number), not the envelope control number that wraps.
#[tokio::test]
async fn eip4_recycled_control_number_maps_as_new() {
    let pool = pool().await;
    let svc = EdiWriteService::new(pool.clone());
    let p = partner(&svc).await;
    let mapper = FakeMapper::new();
    let sink = CapturingSink::new();

    // Two DIFFERENT purchase orders that happen to carry the SAME recycled control number.
    let mk = |bk: &str| InboundDoc {
        partner_id: p, doc_type: "purchase_order".into(),
        control_number: "000000042".into(), business_key: bk.into(),
        raw: "{}".into(), payload: json!({"po": bk}),
    };
    let (a, b) = scoped(&pool, async {
        let a = svc.receive_document(mk("PO-A"), &mapper, &sink).await.unwrap();
        let b = svc.receive_document(mk("PO-B"), &mapper, &sink).await.unwrap();
        (a, b)
    }).await;

    assert!(!a.duplicate);
    assert!(!b.duplicate, "PO-B is a new business document, not a retransmission");
    assert_ne!(a.document_id, b.document_id);
    assert_eq!(mapper.count(), 2, "both POs mapped — the second is not silently dropped");
    assert_eq!(sink.mapped(), 2);
}
