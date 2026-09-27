use serde::{Deserialize, Serialize};

/// Optional server-supplied contact metadata, without a validation/trust claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactMetadata {
    pub administrator: Option<String>,
    pub email: Option<String>,
    pub organisation: Option<String>,
    pub custom: Option<serde_json::Value>,
}
