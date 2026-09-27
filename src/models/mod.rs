mod cvmfs_published;
pub(crate) mod cvmfs_status_json;
mod generic;
mod geoapi;
mod manifest_values;
mod meta_json;
pub(crate) mod repositories_json;
mod servers;

pub(crate) use cvmfs_published::MAX_MANIFEST_BYTES;
pub use cvmfs_published::{Manifest, RepositoryIdentity, RepositoryManifest, SignatureBytes};
pub use generic::{HexString, Hostname, MaybeRfc2822DateTime, ReportedTimestamp, RepositoryName};
pub use geoapi::{
    GeoapiHostId, GeoapiOrdering, GeoapiOutcome, GeoapiServerQuery, GeoapiSkipReason,
};
pub use manifest_values::{
    CatalogSize, CatalogTtl, ContentHash, HashAlgorithm, Revision, RootPathMd5, UnixTimestamp,
};
pub use meta_json::ContactMetadata;
pub use servers::{
    BackendResolution, FailedServer, OptionalFetch, PopulatedRepositoryOrReplica, PopulatedServer,
    ScrapedServer, Server, ServerBackendType, ServerMetadata, ServerType,
};
