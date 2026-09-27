use serde::{Deserialize, Serialize};

/// Optional server-supplied contact metadata, without a validation/trust claim.
/// Read from `cvmfs/info/v1/meta.json`; availability and fetch errors are represented
/// separately by [`crate::OptionalFetch`] on [`crate::PopulatedServer`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactMetadata {
    /// Administrator name or team, as reported by the server.
    pub administrator: Option<String>,
    /// Contact email text; no deliverability or address validation is performed.
    pub email: Option<String>,
    /// Organisation responsible for the server.
    pub organisation: Option<String>,
    /// Administrator-defined JSON with no fixed schema.
    pub custom: Option<serde_json::Value>,
}
