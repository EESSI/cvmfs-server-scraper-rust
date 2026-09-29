use crate::{ReportedTimestamp, RepositoryName};
use serde::Deserialize;

// The index also supplies server metadata. For example:
// {"schema":1,"cvmfs_version":"2.11.3-1","os_id":"rhel",
//  "repositories":[],"replicas":[{"name":"software.eessi.io","url":"/cvmfs/software.eessi.io"}]}
// Version/OS details and last_geodb_update may be omitted; in particular a
// Stratum0 need not report a GeoIP database update time. Server::validate_index
// checks schema, counts, and the correlation between server role and list contents.
#[derive(Debug, Deserialize)]
pub(crate) struct RepositoriesJSON {
    pub schema: u32,
    pub last_geodb_update: Option<ReportedTimestamp>,
    pub cvmfs_version: Option<semver::Version>,
    pub os_id: Option<String>,
    pub os_version_id: Option<String>,
    pub os_pretty_name: Option<String>,
    pub repositories: Vec<RepositoriesJSONRepo>,
    pub replicas: Vec<RepositoriesJSONRepo>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct RepositoriesJSONRepo {
    // Reported `url` fields are deliberately ignored: resource URLs are built
    // from this validated name and the caller's configured origin.
    pub name: RepositoryName,
}
