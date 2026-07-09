//! Inbound mapping port (hand-authored, user-owned) — the seam to the target domain module.
//!
//! Receiving an inbound EDI document (a partner's PO/invoice) means mapping it to an INTERNAL document —
//! a PO the partner sends us becomes a sales order in backbone-selling; a partner invoice becomes a
//! purchase invoice in backbone-billing. EDI never imports selling/billing — a composing service wires the
//! real target behind this port; tests drive the REAL module. Zero normal Cargo edge.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A request to map a parsed inbound document into an internal one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapRequest {
    pub company_id: Uuid,
    pub partner_id: Uuid,
    pub doc_type: String, // purchase_order | invoice | ship_notice
    pub control_number: String,
    /// Stable per-document key (the EDI document id) — the target forwards it as its own idempotency token
    /// so a re-map of a stranded document can't create a duplicate internal order.
    pub idempotency_key: String,
    pub payload: serde_json::Value,
}

/// The target accepted the document and created (or reused) an internal one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapAck {
    pub internal_ref_type: String, // sales_order | purchase_invoice
    pub internal_ref_id: Uuid,
}

/// The target rejected the mapping (unmappable payload, missing master, business-rule failure). `code` is
/// the stable contract error string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapRejected {
    pub code: String,
    pub message: String,
}

/// The inbound mapping seam. A composing service implements it over backbone-selling / backbone-billing.
#[async_trait::async_trait]
pub trait MappingPort: Send + Sync {
    async fn map(&self, req: &MapRequest) -> Result<MapAck, MapRejected>;
}
