use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "edi_doc_type", rename_all = "snake_case")]
pub enum EdiDocType {
    PurchaseOrder,
    Invoice,
    ShipNotice,
    FunctionalAck,
}

impl std::fmt::Display for EdiDocType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PurchaseOrder => write!(f, "purchase_order"),
            Self::Invoice => write!(f, "invoice"),
            Self::ShipNotice => write!(f, "ship_notice"),
            Self::FunctionalAck => write!(f, "functional_ack"),
        }
    }
}

impl FromStr for EdiDocType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "purchase_order" => Ok(Self::PurchaseOrder),
            "invoice" => Ok(Self::Invoice),
            "ship_notice" => Ok(Self::ShipNotice),
            "functional_ack" => Ok(Self::FunctionalAck),
            _ => Err(format!("Unknown EdiDocType variant: {}", s)),
        }
    }
}

impl Default for EdiDocType {
    fn default() -> Self {
        Self::PurchaseOrder
    }
}
