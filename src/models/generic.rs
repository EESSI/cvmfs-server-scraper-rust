use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

use crate::errors::{HostnameError, ManifestError, RepositoryNameError, ScrapeError};

/// Canonical lowercase ASCII DNS name. International names must use IDNA punycode.
/// Syntax validation does not authorize a network destination.
///
/// Names must contain 1..=253 bytes. Each dot-separated label contains 1..=63
/// ASCII letters, digits, or hyphens and starts and ends with a letter or digit.
/// Parsing, `TryFrom`, and serde all apply these rules and normalize case.
/// Ports, URL syntax, and trailing dots are rejected; use [`crate::ServerEndpoint`]
/// when configuring an HTTP(S) origin with a port.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct Hostname(String);

impl FromStr for Hostname {
    type Err = HostnameError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 253 {
            return Err(HostnameError::InvalidLength(s.len()));
        }
        for label in s.split('.') {
            if label.is_empty()
                || label.len() > 63
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || !label.as_bytes()[0].is_ascii_alphanumeric()
                || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            {
                return Err(HostnameError::InvalidLabel(label.into()));
            }
        }
        Ok(Self(s.to_ascii_lowercase()))
    }
}

/// CVMFS repository identifier, never a URL or a path.
///
/// Accepts 1..=255 ASCII bytes consisting of letters, digits, dots, underscores,
/// and hyphens, with no empty dot-separated components. Case is preserved.
/// The same checks apply through parsing, `TryFrom`, and serde.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct RepositoryName(String);
impl FromStr for RepositoryName {
    type Err = RepositoryNameError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let reason = if s.is_empty() || s.len() > 255 {
            Some("expected 1..=255 bytes")
        } else if !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            Some("only ASCII letters, digits, '.', '_' and '-' are allowed; paths, escapes and queries are forbidden")
        } else if s.split('.').any(str::is_empty) {
            Some("dot-separated components must not be empty")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(RepositoryNameError {
                value: s.into(),
                reason,
            });
        }
        Ok(Self(s.into()))
    }
}

macro_rules! string_accessors {
    ($ty:ident, $error:ty) => {
        impl $ty {
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn to_str(&self) -> &str {
                self.as_str()
            }
        }
        impl TryFrom<String> for $ty {
            type Error = $error;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                s.parse()
            }
        }
        impl TryFrom<&str> for $ty {
            type Error = $error;
            fn try_from(s: &str) -> Result<Self, Self::Error> {
                s.parse()
            }
        }
        impl From<$ty> for String {
            fn from(value: $ty) -> Self {
                value.0
            }
        }
        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
string_accessors!(Hostname, HostnameError);
string_accessors!(RepositoryName, RepositoryNameError);

/// General even-length hex text. Manifest digests use the stricter ContentHash type.
///
/// Accepts only ASCII hexadecimal characters (`0`–`9`, `a`–`f`, `A`–`F`) and
/// stores them in lowercase. The empty string is valid general hex text; it is
/// not a valid [`crate::ContentHash`] or [`crate::RootPathMd5`].
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct HexString(String);
impl HexString {
    pub fn new(s: &str) -> Result<Self, ManifestError> {
        if s.len().is_multiple_of(2) && s.bytes().all(|c| c.is_ascii_hexdigit()) {
            Ok(Self(s.to_ascii_lowercase()))
        } else {
            Err(ManifestError::InvalidHex(s.into()))
        }
    }
}
impl FromStr for HexString {
    type Err = ManifestError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}
string_accessors!(HexString, ManifestError);

/// A server's timestamp claim. Unrecognized/localized text is retained without
/// pretending it specifies a UTC instant. Serde always recomputes the state.
///
/// CVMFS JSON timestamps are often produced by the server's `date` command, so
/// their formatting and timezone names can depend on that server's locale.
/// This wrapper preserves the original text even when it cannot be interpreted.
/// [`Self::datetime`] accepts recognized RFC 2822 dates and GNU date-style strings
/// with UTC, GMT, or numeric offsets. Unknown or ambiguous zone names stay unparsed.
/// [`Self::try_into_datetime`] reports an error when no instant is available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct ReportedTimestamp(TimestampState);
#[derive(Debug, Clone, PartialEq, Eq)]
enum TimestampState {
    Parsed { raw: String, instant: DateTime<Utc> },
    Unparsed { raw: String },
}
impl ReportedTimestamp {
    pub fn new(raw: String) -> Self {
        let parsed = DateTime::parse_from_rfc2822(&raw).or_else(|_| {
            // GNU date emits a named zone by default. Only UTC/GMT are
            // unambiguous here; numeric offsets are accepted as well.
            let normalized = raw.replace(" UTC ", " +0000 ").replace(" GMT ", " +0000 ");
            DateTime::parse_from_str(&normalized, "%a %b %d %H:%M:%S %z %Y")
        });
        Self(match parsed {
            Ok(value) => TimestampState::Parsed {
                raw,
                instant: value.with_timezone(&Utc),
            },
            Err(_) => TimestampState::Unparsed { raw },
        })
    }
    pub fn as_str(&self) -> &str {
        match &self.0 {
            TimestampState::Parsed { raw, .. } | TimestampState::Unparsed { raw } => raw,
        }
    }
    pub fn datetime(&self) -> Option<DateTime<Utc>> {
        match &self.0 {
            TimestampState::Parsed { instant, .. } => Some(*instant),
            TimestampState::Unparsed { .. } => None,
        }
    }
    pub fn try_into_datetime(&self) -> Result<DateTime<Utc>, ScrapeError> {
        self.datetime().ok_or_else(|| ScrapeError::ConversionError(format!(
            "timestamp {:?} has an unsupported format or unresolved timezone; use UTC or a numeric offset", self.as_str())))
    }
}
impl From<String> for ReportedTimestamp {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}
impl From<ReportedTimestamp> for String {
    fn from(value: ReportedTimestamp) -> Self {
        match value.0 {
            TimestampState::Parsed { raw, .. } | TimestampState::Unparsed { raw } => raw,
        }
    }
}
impl fmt::Display for ReportedTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Compatibility wrapper; new models use `Option<ReportedTimestamp>` directly.
///
/// Retains the old optional-string shape while distinguishing absent data from
/// a present but unparseable value. [`Self::try_into_datetime`] returns `Ok(None)`
/// for absence, `Ok(Some(_))` for a recognized instant, and an error for unresolved
/// text. [`Self::as_ref`] always permits access to the original text when present.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(transparent)]
pub struct MaybeRfc2822DateTime(Option<ReportedTimestamp>);
impl MaybeRfc2822DateTime {
    pub fn new(value: Option<String>) -> Self {
        Self(value.map(ReportedTimestamp::new))
    }
    pub fn as_ref(&self) -> Option<&ReportedTimestamp> {
        self.0.as_ref()
    }
    pub fn try_into_datetime(&self) -> Result<Option<DateTime<Utc>>, ScrapeError> {
        self.0
            .as_ref()
            .map(ReportedTimestamp::try_into_datetime)
            .transpose()
    }
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}
impl fmt::Display for MaybeRfc2822DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(value) = &self.0 {
            value.fmt(f)
        } else {
            Ok(())
        }
    }
}
