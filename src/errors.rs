use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("Failed to fetch manifest: {0}")]
    FetchError(#[from] reqwest::Error),

    #[error("Missing field {0}")]
    MissingField(char),

    #[error("Parse error for field {0}: {1}")]
    ParseError(char, String),

    #[error("Invalid hex string: {0}")]
    InvalidHex(String),

    #[error("Invalid certificate: {0}")]
    InvalidCertificate(String),
}

#[derive(Error, Debug)]
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

#[derive(Error, Debug)]
pub enum ScrapeError {
    #[error("Failed to scrape: {0}")]
    FetchError(#[from] reqwest::Error),

    #[error("Failed to parse scrape result: {0}")]
    ParseError(#[from] serde_json::Error),

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
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Scrape error: {0}")]
    ScrapeError(#[from] ScrapeError),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] ManifestError),

    #[error("Hostname error: {0}")]
    HostnameError(#[from] HostnameError),
}
