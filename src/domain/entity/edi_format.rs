use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "edi_format", rename_all = "snake_case")]
pub enum EdiFormat {
    CustomJson,
    X12,
    Edifact,
    Csv,
}

impl std::fmt::Display for EdiFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CustomJson => write!(f, "custom_json"),
            Self::X12 => write!(f, "x12"),
            Self::Edifact => write!(f, "edifact"),
            Self::Csv => write!(f, "csv"),
        }
    }
}

impl FromStr for EdiFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "custom_json" => Ok(Self::CustomJson),
            "x12" => Ok(Self::X12),
            "edifact" => Ok(Self::Edifact),
            "csv" => Ok(Self::Csv),
            _ => Err(format!("Unknown EdiFormat variant: {}", s)),
        }
    }
}

impl Default for EdiFormat {
    fn default() -> Self {
        Self::CustomJson
    }
}
