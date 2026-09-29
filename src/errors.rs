use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    #[error("Missing manifest field {0}")]
    MissingField(char),
    #[error("Invalid manifest field {0}: {1}")]
    ParseError(char, String),
    #[error("Invalid manifest line {line}: {reason}")]
    InvalidLine { line: usize, reason: String },
    #[error("Duplicate manifest field {0}")]
    DuplicateField(char),
    #[error("Invalid hex string: {0}")]
    InvalidHex(String),
    #[error("Invalid digest: {0}")]
    InvalidDigest(String),
    #[error("Manifest exceeds {0} bytes")]
    TooLarge(usize),
    #[error("Repository identity mismatch: requested {expected}, manifest names {actual}")]
    RepositoryMismatch { expected: String, actual: String },
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum HostnameError {
    #[error("Hostname is {0} bytes; expected 1..=253 ASCII bytes")]
    InvalidLength(usize),
    #[error("Invalid hostname label {0:?}: expected 1..=63 ASCII letters, digits or hyphens, starting and ending with a letter or digit; use punycode for international names")]
    InvalidLabel(String),
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("Invalid repository name {value:?}: {reason}")]
pub struct RepositoryNameError {
    pub value: String,
    pub reason: &'static str,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("Invalid {field}: {reason}")]
pub struct ConfigurationError {
    pub field: &'static str,
    pub reason: String,
}

#[derive(Error, Debug, Clone)]
pub enum ScrapeError {
    #[error("Request failed for {url}: {source}")]
    Fetch {
        url: String,
        #[source]
        source: Arc<reqwest::Error>,
    },
    #[error("HTTP {status} from {url}")]
    HttpStatus { url: String, status: u16 },
    #[error("Invalid JSON from {url}: {source}")]
    Json {
        url: String,
        #[source]
        source: Arc<serde_json::Error>,
    },
    #[error("Response from {url} exceeds {limit} bytes")]
    BodyTooLarge { url: String, limit: usize },
    #[error("Server scrape deadline exceeded for {0}")]
    Timeout(String),
    #[error("Repository count {actual} exceeds configured limit {limit}")]
    RepositoryLimit { actual: usize, limit: usize },
    #[error("Empty effective repository list for S3 backend: {0}")]
    EmptyRepositoryList(String),
    #[error("Server type mismatch: {0}")]
    ServerTypeMismatch(String),
    #[error("Unsupported repository index schema {0}; supported schema is 1")]
    UnsupportedSchema(u32),
    #[error("Conversion error: {0}")]
    ConversionError(String),
    #[error("GeoAPI failure: {0}")]
    GeoAPIFailure(String),
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
}

impl ScrapeError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 404, .. })
    }
}

#[derive(Error, Debug, Clone)]
pub enum GenericError {
    #[error("Type error: {0}")]
    TypeError(String),
}

#[derive(Error, Debug, Clone)]
pub enum CVMFSScraperError {
    #[error(transparent)]
    Scrape(#[from] ScrapeError),
    #[error("Manifest at {url}: {source}")]
    Manifest {
        url: String,
        #[source]
        source: ManifestError,
    },
    #[error(transparent)]
    Hostname(#[from] HostnameError),
    #[error(transparent)]
    RepositoryName(#[from] RepositoryNameError),
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    #[error(transparent)]
    Generic(#[from] GenericError),
}
