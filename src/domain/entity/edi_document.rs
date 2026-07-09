use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::EdiDocType;
use super::EdiDirection;
use super::EdiStatus;
use super::AuditMetadata;

/// Strongly-typed ID for EdiDocument
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdiDocumentId(pub Uuid);

impl EdiDocumentId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for EdiDocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for EdiDocumentId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for EdiDocumentId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<EdiDocumentId> for Uuid {
    fn from(id: EdiDocumentId) -> Self { id.0 }
}

impl AsRef<Uuid> for EdiDocumentId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for EdiDocumentId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EdiDocument {
    pub id: Uuid,
    pub company_id: Uuid,
    pub partner_id: Uuid,
    pub doc_type: EdiDocType,
    pub direction: EdiDirection,
    pub control_number: String,
    pub business_key: String,
    pub status: EdiStatus,
    pub payload: String,
    pub mapped_ref_type: Option<String>,
    pub mapped_ref_id: Option<Uuid>,
    pub error_detail: Option<String>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl EdiDocument {
    /// Create a builder for EdiDocument
    pub fn builder() -> EdiDocumentBuilder {
        EdiDocumentBuilder::default()
    }

    /// Create a new EdiDocument with required fields
    pub fn new(company_id: Uuid, partner_id: Uuid, doc_type: EdiDocType, direction: EdiDirection, control_number: String, business_key: String, status: EdiStatus, payload: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            partner_id,
            doc_type,
            direction,
            control_number,
            business_key,
            status,
            payload,
            mapped_ref_type: None,
            mapped_ref_id: None,
            error_detail: None,
            acknowledged_at: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> EdiDocumentId {
        EdiDocumentId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &EdiStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the mapped_ref_type field (chainable)
    pub fn with_mapped_ref_type(mut self, value: String) -> Self {
        self.mapped_ref_type = Some(value);
        self
    }

    /// Set the mapped_ref_id field (chainable)
    pub fn with_mapped_ref_id(mut self, value: Uuid) -> Self {
        self.mapped_ref_id = Some(value);
        self
    }

    /// Set the error_detail field (chainable)
    pub fn with_error_detail(mut self, value: String) -> Self {
        self.error_detail = Some(value);
        self
    }

    /// Set the acknowledged_at field (chainable)
    pub fn with_acknowledged_at(mut self, value: DateTime<Utc>) -> Self {
        self.acknowledged_at = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "partner_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.partner_id = v; }
                }
                "doc_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.doc_type = v; }
                }
                "direction" => {
                    if let Ok(v) = serde_json::from_value(value) { self.direction = v; }
                }
                "control_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.control_number = v; }
                }
                "business_key" => {
                    if let Ok(v) = serde_json::from_value(value) { self.business_key = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "payload" => {
                    if let Ok(v) = serde_json::from_value(value) { self.payload = v; }
                }
                "mapped_ref_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mapped_ref_type = v; }
                }
                "mapped_ref_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mapped_ref_id = v; }
                }
                "error_detail" => {
                    if let Ok(v) = serde_json::from_value(value) { self.error_detail = v; }
                }
                "acknowledged_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.acknowledged_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for EdiDocument {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "EdiDocument"
    }
}

impl backbone_core::PersistentEntity for EdiDocument {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for EdiDocument {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("partner_id".to_string(), "uuid".to_string());
        m.insert("mapped_ref_id".to_string(), "uuid".to_string());
        m.insert("doc_type".to_string(), "edi_doc_type".to_string());
        m.insert("direction".to_string(), "edi_direction".to_string());
        m.insert("status".to_string(), "edi_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["control_number", "business_key", "payload"]
    }
}

/// Builder for EdiDocument entity
///
/// Provides a fluent API for constructing EdiDocument instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct EdiDocumentBuilder {
    company_id: Option<Uuid>,
    partner_id: Option<Uuid>,
    doc_type: Option<EdiDocType>,
    direction: Option<EdiDirection>,
    control_number: Option<String>,
    business_key: Option<String>,
    status: Option<EdiStatus>,
    payload: Option<String>,
    mapped_ref_type: Option<String>,
    mapped_ref_id: Option<Uuid>,
    error_detail: Option<String>,
    acknowledged_at: Option<DateTime<Utc>>,
}

impl EdiDocumentBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the partner_id field (required)
    pub fn partner_id(mut self, value: Uuid) -> Self {
        self.partner_id = Some(value);
        self
    }

    /// Set the doc_type field (required)
    pub fn doc_type(mut self, value: EdiDocType) -> Self {
        self.doc_type = Some(value);
        self
    }

    /// Set the direction field (required)
    pub fn direction(mut self, value: EdiDirection) -> Self {
        self.direction = Some(value);
        self
    }

    /// Set the control_number field (required)
    pub fn control_number(mut self, value: String) -> Self {
        self.control_number = Some(value);
        self
    }

    /// Set the business_key field (required)
    pub fn business_key(mut self, value: String) -> Self {
        self.business_key = Some(value);
        self
    }

    /// Set the status field (default: `EdiStatus::default()`)
    pub fn status(mut self, value: EdiStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the payload field (required)
    pub fn payload(mut self, value: String) -> Self {
        self.payload = Some(value);
        self
    }

    /// Set the mapped_ref_type field (optional)
    pub fn mapped_ref_type(mut self, value: String) -> Self {
        self.mapped_ref_type = Some(value);
        self
    }

    /// Set the mapped_ref_id field (optional)
    pub fn mapped_ref_id(mut self, value: Uuid) -> Self {
        self.mapped_ref_id = Some(value);
        self
    }

    /// Set the error_detail field (optional)
    pub fn error_detail(mut self, value: String) -> Self {
        self.error_detail = Some(value);
        self
    }

    /// Set the acknowledged_at field (optional)
    pub fn acknowledged_at(mut self, value: DateTime<Utc>) -> Self {
        self.acknowledged_at = Some(value);
        self
    }

    /// Build the EdiDocument entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<EdiDocument, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let partner_id = self.partner_id.ok_or_else(|| "partner_id is required".to_string())?;
        let doc_type = self.doc_type.ok_or_else(|| "doc_type is required".to_string())?;
        let direction = self.direction.ok_or_else(|| "direction is required".to_string())?;
        let control_number = self.control_number.ok_or_else(|| "control_number is required".to_string())?;
        let business_key = self.business_key.ok_or_else(|| "business_key is required".to_string())?;
        let payload = self.payload.ok_or_else(|| "payload is required".to_string())?;

        Ok(EdiDocument {
            id: Uuid::new_v4(),
            company_id,
            partner_id,
            doc_type,
            direction,
            control_number,
            business_key,
            status: self.status.unwrap_or(EdiStatus::default()),
            payload,
            mapped_ref_type: self.mapped_ref_type,
            mapped_ref_id: self.mapped_ref_id,
            error_detail: self.error_detail,
            acknowledged_at: self.acknowledged_at,
            metadata: AuditMetadata::default(),
        })
    }
}
