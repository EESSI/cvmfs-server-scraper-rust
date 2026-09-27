use crate::ReportedTimestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub(crate) struct StatusJSON {
    pub last_snapshot: Option<ReportedTimestamp>,
    pub last_gc: Option<ReportedTimestamp>,
}
