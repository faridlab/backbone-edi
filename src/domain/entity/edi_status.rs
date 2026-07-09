use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "edi_status", rename_all = "snake_case")]
pub enum EdiStatus {
    Received,
    Mapped,
    Acknowledged,
    Generated,
    Sent,
    Failed,
}

impl std::fmt::Display for EdiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Received => write!(f, "received"),
            Self::Mapped => write!(f, "mapped"),
            Self::Acknowledged => write!(f, "acknowledged"),
            Self::Generated => write!(f, "generated"),
            Self::Sent => write!(f, "sent"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for EdiStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "received" => Ok(Self::Received),
            "mapped" => Ok(Self::Mapped),
            "acknowledged" => Ok(Self::Acknowledged),
            "generated" => Ok(Self::Generated),
            "sent" => Ok(Self::Sent),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Unknown EdiStatus variant: {}", s)),
        }
    }
}

impl Default for EdiStatus {
    fn default() -> Self {
        Self::Received
    }
}
