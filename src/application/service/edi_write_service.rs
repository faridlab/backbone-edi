//! The hand-authored EDI write path (user-owned; survives regen).
//!
//! B2B document interchange: receive an inbound partner document **idempotently** on (partner, direction,
//! control_number) — partners retransmit, so a redelivery must not re-map (create a duplicate internal
//! order) — map it to an internal document via a `MappingPort`, and **acknowledge** it back to the partner
//! (a functional ack, else the partner keeps retransmitting). Posts NO GL. The byte-level parsing of
//! X12/EDIFACT is the composing service's concern; this module owns the exchange lifecycle.

use backbone_orm::company_scope;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    EdiDocumentRepository, NewInboundDocRow, NewPartnerRow, TradingPartnerRepository,
};

use super::edi_events::*;
use super::edi_ports::*;

#[derive(Debug, thiserror::Error)]
pub enum EdiError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("invalid state: {0}")]
    InvalidState(&'static str),
    #[error("mapping rejected: {0}")]
    MappingRejected(String),
}

pub struct NewPartner {
    pub company_id: Uuid,
    pub name: String,
    pub partner_code: String,
    pub format: String,
    pub partner_direction: String,
}

/// An inbound EDI document as delivered by a partner (already parsed into `payload`).
pub struct InboundDoc {
    pub company_id: Uuid,
    pub partner_id: Uuid,
    pub doc_type: String, // purchase_order | invoice | ship_notice
    /// The partner's ENVELOPE control number (for the ack) — NOT the dedup key (it recycles).
    pub control_number: String,
    /// The BUSINESS document identity (the PO/invoice number) — the dedup key, stable across the partner's
    /// control-number recycling.
    pub business_key: String,
    pub raw: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveOutcome {
    pub document_id: Uuid,
    pub status: String, // mapped | failed | duplicate
    pub mapped_ref_id: Option<Uuid>,
    pub duplicate: bool,
}

pub struct EdiWriteService {
    pool: PgPool,
    partners: TradingPartnerRepository,
    documents: EdiDocumentRepository,
}

impl EdiWriteService {
    pub fn new(pool: PgPool) -> Self {
        let partners = TradingPartnerRepository::new(pool.clone());
        let documents = EdiDocumentRepository::new(pool.clone());
        Self { pool, partners, documents }
    }

    /// Register a trading partner.
    pub async fn create_partner(&self, p: NewPartner) -> Result<Uuid, EdiError> {
        if p.name.trim().is_empty() || p.partner_code.trim().is_empty() {
            return Err(EdiError::Invalid("partner needs a name and code".into()));
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — bind it so the INSERT satisfies the WITH CHECK
        // fence on `app.company_id`.
        let company = p.company_id;
        let r = company_scope::with_company_scope(
            Some(company),
            self.partners.insert_partner(&self.pool, &NewPartnerRow {
                id,
                company_id: p.company_id,
                name: &p.name,
                partner_code: &p.partner_code,
                format: &p.format,
                partner_direction: &p.partner_direction,
            }),
        )
        .await;
        match r {
            Ok(_) => Ok(id),
            Err(e) if e.as_database_error().map(|d| d.is_unique_violation()).unwrap_or(false) =>
                Err(EdiError::Invalid("a partner with this code already exists".into())),
            Err(e) => Err(e.into()),
        }
    }

    /// Receive an inbound document: dedup on (partner, inbound, control_number), record it, map it to an
    /// internal document via the `MappingPort`, and record the outcome. A redelivered document (a partner
    /// retransmission) returns the original with `duplicate=true` — it never re-maps. Emits
    /// `EdiDocumentMapped` or `EdiDocumentFailed`.
    pub async fn receive_document(
        &self,
        d: InboundDoc,
        mapper: &dyn MappingPort,
        events: &dyn EdiEventSink,
    ) -> Result<ReceiveOutcome, EdiError> {
        if d.control_number.trim().is_empty() {
            return Err(EdiError::Invalid("an inbound document needs a control number".into()));
        }
        if d.business_key.trim().is_empty() {
            return Err(EdiError::Invalid("an inbound document needs a business key (the PO/invoice number)".into()));
        }

        // Claim the (partner, inbound, business_key) dedup slot — a retransmission of the SAME business
        // document conflicts here; a new document that reuses a recycled control number does not.
        // RLS scope (ADR-0008): the inbound document carries its company on the DTO — bind it explicitly
        // on every statement, so the dedup INSERT passes the WITH CHECK fence and the redelivery lookup
        // sees the original. Partner retransmissions arrive off-request (no ambient scope), so an
        // explicit company here is what keeps the dedup honest rather than failing closed into a
        // duplicate internal order.
        let company = d.company_id;
        let inserted: Option<Uuid> = company_scope::with_company_scope(
            Some(company),
            self.documents.claim_inbound(&self.pool, &NewInboundDocRow {
                id: Uuid::new_v4(),
                company_id: d.company_id,
                partner_id: d.partner_id,
                doc_type: &d.doc_type,
                control_number: &d.control_number,
                business_key: &d.business_key,
                raw: &d.raw,
            }),
        )
        .await?;

        let Some(document_id) = inserted else {
            let row = company_scope::with_company_scope(
                Some(company),
                self.documents.fetch_inbound_by_business_key(&self.pool, d.partner_id, &d.business_key),
            )
            .await?;
            return Ok(ReceiveOutcome {
                document_id: row.id, status: row.status,
                mapped_ref_id: row.mapped_ref_id, duplicate: true,
            });
        };

        // Map to an internal document via the target module (external — creates a real sales order/invoice).
        let req = MapRequest {
            company_id: d.company_id, partner_id: d.partner_id, doc_type: d.doc_type.clone(),
            control_number: d.control_number.clone(), idempotency_key: document_id.to_string(),
            payload: d.payload.clone(),
        };
        match mapper.map(&req).await {
            Ok(ack) => {
                let event = EdiEvent::EdiDocumentMapped(EdiDocumentMapped {
                    document_id, company_id: d.company_id, partner_id: d.partner_id, doc_type: d.doc_type.clone(),
                    control_number: d.control_number.clone(),
                    internal_ref_type: ack.internal_ref_type.clone(), internal_ref_id: ack.internal_ref_id,
                });
                let mut tx = self.pool.begin().await?;
                // RLS scope (ADR-0008): bind this document's own company onto the tx (the outbox stage
                // rides it too).
                company_scope::bind_company_on(&mut tx, company).await?;
                self.documents
                    .mark_mapped(&mut tx, document_id, &ack.internal_ref_type, ack.internal_ref_id)
                    .await?;
                stage(&mut tx, &event).await?;
                tx.commit().await?;
                events.publish(&event);
                Ok(ReceiveOutcome { document_id, status: "mapped".into(), mapped_ref_id: Some(ack.internal_ref_id), duplicate: false })
            }
            Err(rej) => {
                let event = EdiEvent::EdiDocumentFailed {
                    document_id, company_id: d.company_id, partner_id: d.partner_id,
                    control_number: d.control_number.clone(), reason: rej.code.clone(),
                };
                let mut tx = self.pool.begin().await?;
                // RLS scope (ADR-0008): bind this document's own company onto the tx.
                company_scope::bind_company_on(&mut tx, company).await?;
                self.documents.mark_failed(&mut tx, document_id, &rej.message).await?;
                stage(&mut tx, &event).await?;
                tx.commit().await?;
                events.publish(&event);
                Ok(ReceiveOutcome { document_id, status: "failed".into(), mapped_ref_id: None, duplicate: false })
            }
        }
    }

    /// Issue a functional acknowledgement to the partner for a mapped/failed document. Idempotent
    /// (state-guarded); emits `EdiDocumentAcknowledged`.
    pub async fn acknowledge(&self, document_id: Uuid, events: &dyn EdiEventSink) -> Result<bool, EdiError> {
        // Capture the PRE-update status (mapped → accepted, failed → rejected) + the error via a CTE, so
        // the emitted event carries the 997 polarity the consumer needs to generate the wire ack.
        // RLS scope (ADR-0008), ID-only pattern: identified by the document id alone, with no company
        // argument to bind. Under HTTP this rides the request-dedicated connection (which carries the
        // caller's `app.company_id`), so another company's document simply is not found. A non-request
        // caller (an ack job / an event-driven sink) MUST wrap this in
        // `with_company_scope(Some(company_id))` — otherwise it fails closed and returns Ok(false).
        let row = self.documents.acknowledge(&self.pool, document_id).await?;
        let Some(row) = row else { return Ok(false) };
        let accepted = row.accepted;
        events.publish(&EdiEvent::EdiDocumentAcknowledged {
            document_id, partner_id: row.partner_id, control_number: row.control_number,
            accepted, error_detail: if accepted { None } else { row.error_detail },
        });
        Ok(true)
    }
}

async fn stage(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, event: &EdiEvent) -> Result<(), EdiError> {
    let (etype, agg_id) = match event {
        EdiEvent::EdiDocumentMapped(m) => ("EdiDocumentMapped", m.document_id),
        EdiEvent::EdiDocumentFailed { document_id, .. } => ("EdiDocumentFailed", *document_id),
        EdiEvent::EdiDocumentAcknowledged { document_id, .. } => ("EdiDocumentAcknowledged", *document_id),
    };
    let record = backbone_outbox::OutboxRecord::new(
        etype, "EdiDocument", agg_id.to_string(),
        serde_json::to_value(event).map_err(|e| EdiError::Invalid(e.to_string()))?,
        Utc::now(),
    );
    backbone_outbox::outbox::stage(&mut **tx, "edi", &record)
        .await
        .map_err(|e| EdiError::Invalid(format!("outbox stage: {e}")))?;
    Ok(())
}
