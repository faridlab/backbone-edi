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
use super::ubl::{self, UblRefusal};

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
    /// A UBL document was refused before it could enter the exchange. Carries the joined refusal
    /// detail line (stable codes + locators) — nothing was persisted for it.
    #[error("ubl refused: {0}")]
    UblRefused(String),
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
        self.claim_and_settle(d, mapper, events).await
    }

    /// Receive an inbound UBL BIS 3 order (raw XML bytes). The DECLARED partner row — never payload
    /// sniffing — selects the UBL path: the partner must be active, handle inbound, and declare
    /// `format='ubl_bis3'`. Flow:
    ///
    /// - Oversized or structurally refused document with no usable business key (`cbc:ID` absent,
    ///   empty, or over the column budget) → `Err(EdiError::UblRefused(..))`. Nothing persisted, no
    ///   events — a retransmission re-parses cleanly (idempotent by construction).
    /// - Structurally refused document WITH a usable business key → the durable negative-ack path:
    ///   the dedup slot is claimed, the row is recorded `failed` with the joined refusal codes, and
    ///   `EdiDocumentFailed` is staged/published — the partner gets a real negative functional ack
    ///   through the existing `acknowledge` surface.
    /// - Valid document → the shared idempotent claim/settle core (same one
    ///   [`Self::receive_document`] runs): business key = `cbc:ID` (the buyer's order number),
    ///   control number = `cbc:UUID` when present else `cbc:ID` (UBL has no envelope control
    ///   number — the instance UUID stands in), payload = the canonical parsed contract.
    pub async fn receive_ubl_order(
        &self,
        company_id: Uuid,
        partner_id: Uuid,
        raw: &str,
        mapper: &dyn MappingPort,
        events: &dyn EdiEventSink,
    ) -> Result<ReceiveOutcome, EdiError> {
        // Partner gate — typed refusals, nothing persisted.
        let gate = company_scope::with_company_scope(
            Some(company_id),
            self.partners.fetch_partner_gate(&self.pool, company_id, partner_id),
        )
        .await?;
        let Some(gate) = gate else {
            return Err(EdiError::NotFound("trading partner"));
        };
        if gate.status != "active" {
            return Err(EdiError::Invalid(format!("trading partner {partner_id} is not active")));
        }
        if gate.partner_direction != "inbound" && gate.partner_direction != "both" {
            return Err(EdiError::Invalid(format!(
                "trading partner {partner_id} does not accept inbound documents (direction: {})",
                gate.partner_direction
            )));
        }
        if gate.format != "ubl_bis3" {
            return Err(EdiError::Invalid(format!(
                "trading partner {partner_id} does not declare ubl_bis3 (found: {})",
                gate.format
            )));
        }

        // Size guard — refuses before any parsing or persistence.
        if raw.len() > ubl::MAX_XML_BYTES {
            return Err(EdiError::UblRefused(
                ubl::too_large_refusal(raw.len(), ubl::MAX_XML_BYTES).detail_line(),
            ));
        }

        match ubl::parse_ubl_order(raw) {
            Ok(order) => {
                let control_number = order
                    .document_uuid
                    .clone()
                    .unwrap_or_else(|| order.document_id.clone());
                let d = InboundDoc {
                    company_id,
                    partner_id,
                    doc_type: "purchase_order".into(),
                    control_number,
                    business_key: order.document_id.clone(),
                    raw: raw.to_string(),
                    payload: ubl::to_payload(&order),
                };
                self.claim_and_settle(d, mapper, events).await
            }
            Err(refusal) => match refusal.business_key.clone() {
                None => Err(EdiError::UblRefused(refusal.detail_line())),
                Some(business_key) => self.record_ubl_refusal(company_id, partner_id, &business_key, raw, &refusal, events).await,
            },
        }
    }

    /// Durable negative-ack for a refused-but-identifiable UBL document: claim the dedup slot on
    /// the refused document's own business key, then record `failed` with the joined refusal
    /// detail. On a losing claim (a prior copy exists) the row re-drives when it never completed
    /// mapping, so a corrected retransmission after a negative ack can still land; settled rows
    /// return their stored outcome untouched.
    async fn record_ubl_refusal(
        &self,
        company_id: Uuid,
        partner_id: Uuid,
        business_key: &str,
        raw: &str,
        refusal: &UblRefusal,
        events: &dyn EdiEventSink,
    ) -> Result<ReceiveOutcome, EdiError> {
        // No envelope control number survives a refused parse — the business key stands in for the
        // audit column (the dedup is on business_key; control_number is display/audit only).
        let inserted: Option<Uuid> = company_scope::with_company_scope(
            Some(company_id),
            self.documents.claim_inbound(&self.pool, &NewInboundDocRow {
                id: Uuid::new_v4(),
                company_id,
                partner_id,
                doc_type: "purchase_order",
                control_number: business_key,
                business_key,
                raw,
            }),
        )
        .await?;

        let document_id = match inserted {
            Some(id) => id,
            None => {
                let row = company_scope::with_company_scope(
                    Some(company_id),
                    self.documents.fetch_inbound_by_business_key(&self.pool, partner_id, business_key),
                )
                .await?;
                if Self::redrivable(&row) {
                    let reset = company_scope::with_company_scope(
                        Some(company_id),
                        self.documents.reset_for_redrive(&self.pool, row.id, business_key, raw),
                    )
                    .await?;
                    if reset {
                        return self.record_parse_failure(company_id, partner_id, row.id, business_key, refusal, events).await;
                    }
                }
                return Ok(ReceiveOutcome {
                    document_id: row.id,
                    status: row.status,
                    mapped_ref_id: row.mapped_ref_id,
                    duplicate: true,
                });
            }
        };
        self.record_parse_failure(company_id, partner_id, document_id, business_key, refusal, events)
            .await
    }

    /// Record `failed` + the staged `EdiDocumentFailed` for a refused document (never calls the
    /// mapper — there is nothing mappable). The event is staged/published only when the guarded
    /// UPDATE actually transitioned the row, so racing re-drives cannot double-emit.
    async fn record_parse_failure(
        &self,
        company_id: Uuid,
        partner_id: Uuid,
        document_id: Uuid,
        control_number: &str,
        refusal: &UblRefusal,
        events: &dyn EdiEventSink,
    ) -> Result<ReceiveOutcome, EdiError> {
        let reason = refusal.errors.first().map(|e| e.code).unwrap_or("ubl_refused");
        let event = EdiEvent::EdiDocumentFailed {
            document_id,
            company_id,
            partner_id,
            control_number: control_number.to_string(),
            reason: reason.to_string(),
        };
        let mut tx = self.pool.begin().await?;
        // RLS scope (ADR-0008): bind this document's own company onto the tx (the outbox stage
        // rides it too).
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let settled = self.documents.mark_failed(&mut tx, document_id, &refusal.detail_line()).await?;
        if settled {
            stage(&mut tx, &event).await?;
        }
        tx.commit().await?;
        if settled {
            events.publish(&event);
        }
        Ok(ReceiveOutcome { document_id, status: "failed".into(), mapped_ref_id: None, duplicate: false })
    }

    /// A stored inbound row may re-drive when it never completed mapping: crash-stranded
    /// `received` with no mapped_ref, or any prior `failed` (a refusal). Settled rows
    /// (`mapped`/`acknowledged`) never do — `acknowledged` stays terminal, so a correction after a
    /// negative ack must arrive under a new business key.
    fn redrivable(row: &crate::infrastructure::persistence::DocOutcomeRow) -> bool {
        (row.status == "received" || row.status == "failed") && row.mapped_ref_id.is_none()
    }

    /// The shared inbound core: validate the envelope fields, claim the (partner, inbound,
    /// business_key) dedup slot, and settle. A losing claim re-drives when the stored row never
    /// completed mapping; otherwise it returns the stored outcome with `duplicate=true`.
    async fn claim_and_settle(
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
            // Re-drive: a retransmission against a row that never completed mapping — crash-stranded
            // between map and mark, or any prior refusal — settles now instead of returning a bare
            // duplicate. The re-drive's MapRequest carries the SAME idempotency_key (the document id),
            // so the target hands back the same internal_ref_id it already created: no duplicate order.
            if Self::redrivable(&row) {
                let reset = company_scope::with_company_scope(
                    Some(company),
                    self.documents.reset_for_redrive(&self.pool, row.id, &d.control_number, &d.raw),
                )
                .await?;
                if reset {
                    return self.settle_inbound(&d, row.id, mapper, events).await;
                }
            }
            return Ok(ReceiveOutcome {
                document_id: row.id, status: row.status,
                mapped_ref_id: row.mapped_ref_id, duplicate: true,
            });
        };

        self.settle_inbound(&d, document_id, mapper, events).await
    }

    /// Map a claimed document to an internal one via the target module (external — creates a real
    /// sales order/invoice) and record the outcome. Shared by the fresh-claim path and the
    /// re-drive path. The lifecycle event is staged in the SAME tx as the status UPDATE (durable)
    /// and published after commit — but only when the guarded UPDATE actually transitioned the
    /// row, which kills the duplicate event under racing re-drives.
    async fn settle_inbound(
        &self,
        d: &InboundDoc,
        document_id: Uuid,
        mapper: &dyn MappingPort,
        events: &dyn EdiEventSink,
    ) -> Result<ReceiveOutcome, EdiError> {
        let req = MapRequest {
            company_id: d.company_id, partner_id: d.partner_id, doc_type: d.doc_type.clone(),
            control_number: d.control_number.clone(), idempotency_key: document_id.to_string(),
            payload: d.payload.clone(),
        };
        let company = d.company_id;
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
                let settled = self
                    .documents
                    .mark_mapped(&mut tx, document_id, &ack.internal_ref_type, ack.internal_ref_id)
                    .await?;
                if settled {
                    stage(&mut tx, &event).await?;
                }
                tx.commit().await?;
                if !settled {
                    // A racing re-drive settled this row first — report the stored state, no second event.
                    return self.stored_outcome_for(company, d.partner_id, &d.business_key).await;
                }
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
                let settled = self.documents.mark_failed(&mut tx, document_id, &rej.message).await?;
                if settled {
                    stage(&mut tx, &event).await?;
                }
                tx.commit().await?;
                if !settled {
                    return self.stored_outcome_for(company, d.partner_id, &d.business_key).await;
                }
                events.publish(&event);
                Ok(ReceiveOutcome { document_id, status: "failed".into(), mapped_ref_id: None, duplicate: false })
            }
        }
    }

    /// Re-read a stored row after losing a guarded transition to a racing writer.
    async fn stored_outcome_for(
        &self,
        company: Uuid,
        partner_id: Uuid,
        business_key: &str,
    ) -> Result<ReceiveOutcome, EdiError> {
        let row = company_scope::with_company_scope(
            Some(company),
            self.documents.fetch_inbound_by_business_key(&self.pool, partner_id, business_key),
        )
        .await?;
        Ok(ReceiveOutcome {
            document_id: row.id, status: row.status,
            mapped_ref_id: row.mapped_ref_id, duplicate: false,
        })
    }

    /// Issue a functional acknowledgement to the partner for a mapped/failed document. Idempotent
    /// (state-guarded); emits `EdiDocumentAcknowledged`.
    ///
    /// `company_id` scopes the guarded update for the same reason as [`Self::receive_document`]: the
    /// caller's tenant must own the row. An event/job caller can no longer forget to scope — passing
    /// the event's company here is what fences the `UPDATE`, so another company's document is
    /// indistinguishable from a missing/already-acknowledged one (`Ok(false)`).
    pub async fn acknowledge(
        &self,
        document_id: Uuid,
        company_id: Uuid,
        events: &dyn EdiEventSink,
    ) -> Result<bool, EdiError> {
        // Capture the PRE-update status (mapped → accepted, failed → rejected) + the error via a CTE, so
        // the emitted event carries the 997 polarity the consumer needs to generate the wire ack.
        // RLS scope (ADR-0008): company on the parameter — scope the guarded update so it runs with
        // `app.company_id` set. The repository holds the statement; the scope wrapper stays here, in
        // the service.
        company_scope::with_company_scope(Some(company_id), async move {
            let row = self.documents.acknowledge(&self.pool, document_id).await?;
            let Some(row) = row else { return Ok(false) };
            let accepted = row.accepted;
            events.publish(&EdiEvent::EdiDocumentAcknowledged {
                document_id, partner_id: row.partner_id, control_number: row.control_number,
                accepted, error_detail: if accepted { None } else { row.error_detail },
            });
            Ok(true)
        })
        .await
    }
}

async fn stage(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, event: &EdiEvent) -> Result<(), EdiError> {
    let (etype, agg_id) = match event {
        EdiEvent::EdiDocumentMapped(m) => ("EdiDocumentMapped", m.document_id),
        EdiEvent::EdiDocumentFailed { document_id, .. } => ("EdiDocumentFailed", *document_id),
        EdiEvent::EdiDocumentAcknowledged { document_id, .. } => ("EdiDocumentAcknowledged", *document_id),
    };
    let payload = serde_json::to_value(event).map_err(|e| EdiError::Invalid(e.to_string()))?;
    let company_id: Uuid = payload
        .get("company_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EdiError::Invalid("edi event missing company_id".into()))?
        .parse()
        .map_err(|e| EdiError::Invalid(format!("company_id parse: {e}")))?;
    let record = backbone_outbox::OutboxRecord::new(
        etype, "EdiDocument", agg_id.to_string(), company_id, payload, Utc::now(),
    );
    backbone_outbox::outbox::stage(&mut **tx, "edi", &record)
        .await
        .map_err(|e| EdiError::Invalid(format!("outbox stage: {e}")))?;
    Ok(())
}
