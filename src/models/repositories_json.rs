use crate::{ReportedTimestamp, RepositoryName};
use serde::Deserialize;

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
    pub name: RepositoryName,
}
