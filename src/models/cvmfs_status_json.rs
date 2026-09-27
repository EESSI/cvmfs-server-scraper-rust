use crate::ReportedTimestamp;
use serde::{Deserialize, Serialize};

// The status file is required by the scrape, but individual timestamps may be
// omitted or null. Present text is preserved even when its locale/zone is unknown.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub(crate) struct StatusJSON {
    pub last_snapshot: Option<ReportedTimestamp>,
    pub last_gc: Option<ReportedTimestamp>,
}
