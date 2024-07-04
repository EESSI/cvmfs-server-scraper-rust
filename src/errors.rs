use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ManifestError {
    #[error("Failed to fetch manifest: {0}")]
    FetchError(Arc<reqwest::Error>),

    #[error("Missing field {0}")]
    MissingField(char),

    #[error("Parse error for field {0}: {1}")]
    ParseError(char, String),

    #[error("Invalid hex string: {0}")]
    InvalidHex(String),

    #[error("Invalid certificate: {0}")]
    InvalidCertificate(String),
}

#[derive(Error, Debug, Clone)]
pub enum HostnameError {
    #[error("Invalid hostname length: {0} > 255")]
    TooLong(String),

    #[error("Invalid label length: {0} > 63")]
    LabelTooLong(String),

    #[error("Invalid character in label: {0}")]
    InvalidChar(String),

    #[error("Invalid label format: {0}")]
    InvalidLabelFormat(String),

    #[error("Label contains consecutive dashes: {0}")]
    ConsecutiveDashes(String),
}

#[derive(Error, Debug, Clone)]
pub enum ScrapeError {
    #[error("Failed to scrape: {0}")]
    FetchError(Arc<reqwest::Error>),

    #[error("Failed to parse scrape result: {0}")]
    ParseError(Arc<serde_json::Error>),

    #[error("Failed to parse scrape result: {0}")]
    InvalidJson(String),

    #[error("Empty repository list with S3 backend: {0}")]
    EmptyRepositoryList(String),

    #[error("Server type mismatch: {0}")]
    ServerTypeMismatch(String),

    #[error("Chrono parsing error: {0}")]
    ChronoParseError(#[from] chrono::ParseError),

    #[error("Conversion error: {0}")]
    ConversionError(String),

    #[error("GeoAPI failure: {0}")]
    GeoAPIFailure(String),
}

#[derive(Error, Debug, Clone)]
pub enum GenericError {
    #[error("Type error: {0}")]
    TypeError(String),
}

#[allow(clippy::enum_variant_names)]
#[derive(Error, Debug, Clone)]
pub enum CVMFSScraperError {
    #[error("Scrape error: {0}")]
    ScrapeError(#[from] ScrapeError),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] ManifestError),

    #[error("Hostname error: {0}")]
    HostnameError(#[from] HostnameError),

    #[error("Generic error: {0}")]
    GenericError(#[from] GenericError),
}

impl From<reqwest::Error> for ManifestError {
    fn from(error: reqwest::Error) -> Self {
        ManifestError::FetchError(Arc::new(error))
    }
}

impl From<reqwest::Error> for ScrapeError {
    fn from(error: reqwest::Error) -> Self {
        ScrapeError::FetchError(Arc::new(error))
    }
}

impl From<serde_json::Error> for ScrapeError {
    fn from(error: serde_json::Error) -> Self {
        ScrapeError::ParseError(Arc::new(error))
    }
}

// For when we try to convert Hostname to Hostname. We get an infallible
// conversion error, which we can safely ignore.
impl From<std::convert::Infallible> for HostnameError {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!("Infallible conversions cannot fail")
    }
}
