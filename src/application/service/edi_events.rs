//! EDI domain events (hand-authored, user-owned) — the exchange-lifecycle surface.
//!
//! An inbound document that maps to an internal one emits `EdiDocumentMapped` (the target created a sales
//! order/invoice); a document that can't be mapped emits `EdiDocumentFailed` (the partner needs a negative
//! acknowledgement); an acknowledged document emits `EdiDocumentAcknowledged`. A consuming service supplies
//! the sink.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An inbound document mapped to an internal document (a sales order / purchase invoice was created).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdiDocumentMapped {
    pub document_id: Uuid,
    pub company_id: Uuid,
    pub partner_id: Uuid,
    pub doc_type: String,
    pub control_number: String,
    pub internal_ref_type: String,
    pub internal_ref_id: Uuid,
}

/// The EDI domain-event union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum EdiEvent {
    EdiDocumentMapped(EdiDocumentMapped),
    EdiDocumentFailed { document_id: Uuid, company_id: Uuid, partner_id: Uuid, control_number: String, reason: String },
    /// A functional acknowledgement was issued. `accepted` is the 997/CONTRL polarity (mapped → accepted,
    /// failed → rejected); `error_detail` is the rejection reason for a negative ack — so a consumer can
    /// generate the correct positive/negative wire ack from THIS event alone (completeness council
    /// 2026-07-09).
    EdiDocumentAcknowledged { document_id: Uuid, partner_id: Uuid, control_number: String, accepted: bool, error_detail: Option<String> },
}

/// Sink the write path publishes to. A consuming service supplies its own (bus, outbox, …).
pub trait EdiEventSink: Send + Sync {
    fn publish(&self, event: &EdiEvent);
}

/// A no-op/logging sink for tests and single-process composition.
#[derive(Debug, Default, Clone)]
pub struct LoggingSink;

impl EdiEventSink for LoggingSink {
    fn publish(&self, event: &EdiEvent) {
        tracing::info!(?event, "edi event");
    }
}
