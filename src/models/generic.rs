use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::errors::{HostnameError, ManifestError, ScrapeError};

/// A hostname string.
///
/// This type is used to represent a hostname string. It is a wrapper around a `String` and
/// provides validation for hostnames.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Hostname(pub String);

impl std::str::FromStr for Hostname {
    type Err = HostnameError;

    fn from_str(s: &str) -> Result<Self, HostnameError> {
        if s.len() > 255 {
            return Err(HostnameError::TooLong(s.to_string()));
        }

        let labels: Vec<&str> = s.split('.').collect();
        for label in &labels {
            if label.len() > 63 {
                return Err(HostnameError::LabelTooLong(label.to_string()));
            }
            if !label.chars().all(|c| c.is_alphanumeric() || c == '-') {
                return Err(HostnameError::InvalidChar(label.to_string()));
            }
            // This will also catch empty labels
            if !label.chars().next().unwrap_or_default().is_alphanumeric()
                || !label.chars().last().unwrap_or_default().is_alphanumeric()
            {
                return Err(HostnameError::InvalidLabelFormat(format!(
                    "First and last character of '{}' is not alphanumeric.",
                    label
                )));
            }
            if label.contains("--") {
                return Err(HostnameError::ConsecutiveDashes(label.to_string()));
            }
        }

        Ok(Hostname(s.to_string()))
    }
}

impl std::fmt::Display for Hostname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Hostname {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_string(&self) -> String {
        self.0.clone()
    }
}

/// A hexadecimal string.
///
/// This type is used to represent a hexadecimal string. It is a wrapper around a `String` and
/// provides validation for hexadecimal strings. A valid hexadecimal string must:
///
/// - Contain only hexadecimal characters (0-9, a-f).
/// - Have an even number of characters.
///
/// The string is stored in lowercase.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct HexString(String);

impl HexString {
    pub fn new(s: &str) -> Result<Self, ManifestError> {
        if s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(HexString(s.to_string().to_lowercase()))
        } else {
            Err(ManifestError::InvalidHex(s.to_string()))
        }
    }
}

impl std::str::FromStr for HexString {
    type Err = ManifestError;

    fn from_str(s: &str) -> Result<Self, ManifestError> {
        HexString::new(s)
    }
}

impl std::fmt::Display for HexString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for HexString {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        HexString::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct MaybeRfc2822DateTime(pub Option<String>);

impl Default for MaybeRfc2822DateTime {
    fn default() -> Self {
        MaybeRfc2822DateTime(None)
    }
}

impl std::fmt::Display for MaybeRfc2822DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(date_str) => write!(f, "{}", date_str),
            None => write!(f, ""),
        }
    }
}

impl MaybeRfc2822DateTime {
    pub fn try_into_datetime(&self) -> Result<Option<DateTime<Utc>>, ScrapeError> {
        match &self.0 {
            Some(date_str) => {
                // Try parsing the date string with the format
                let naive_dt = NaiveDateTime::parse_from_str(date_str, "%a %b %d %H:%M:%S %Z %Y")
                    .map_err(|_| ScrapeError::ConversionError(date_str.clone()))?;
                // Convert NaiveDateTime to DateTime<Utc>
                Ok(Some(DateTime::<Utc>::from_naive_utc_and_offset(
                    naive_dt, Utc,
                )))
            }
            None => Ok(None),
        }
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

pub struct Rfc2822DateTime(String);

impl From<&str> for Rfc2822DateTime {
    fn from(s: &str) -> Self {
        Rfc2822DateTime(s.to_string())
    }
}

impl TryFrom<Rfc2822DateTime> for DateTime<Utc> {
    type Error = ScrapeError;

    fn try_from(value: Rfc2822DateTime) -> Result<Self, Self::Error> {
        Ok(DateTime::parse_from_rfc2822(&value.0)?.with_timezone(&Utc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yare::parameterized;

    #[parameterized(
        example_com = { "example.com" },
        foo_dash_example_com = { "foo-example.com" },
        numeric_example_com = { "123-example.com" },
    )]
    fn test_valid_hostname(hostname_str: &str) {
        let hostname: Hostname = hostname_str.parse().unwrap();
        assert_eq!(hostname.to_string(), hostname_str);
    }

    #[parameterized(
        empty_str = { "" },
        too_long_str = { &"a".repeat(256) },
        invalid_char_str = { "example.com!" },
        invalid_label_format_str = { "-example.com" },
        consecutive_dashes_str = { "foo--example.com" },
        double_dot = { "example..com" },
        ends_with_dash_str = { "example-.com" },
        ends_with_dot = { "example.com." },
        label_ends_with_dash = { "example-.com" },
        label_ends_with_underscore = { "example_.com" },
        label_starts_with_underscore = { "_example.com" },
    )]
    fn test_invalid_hostname(hostname_str: &str) {
        assert!(hostname_str.parse::<Hostname>().is_err());
    }

    #[parameterized(
        deadbeef = { "deadbeef" },
        abcdef = { "abcdef" },
        abcdef123456 = { "abcdef123456" },
        uppercase = { "AAABBB" },
        empty = { "" },
    )]
    fn test_valid_hexstrings(hex_str: &str) {
        let hexstring = HexString::new(hex_str).unwrap();
        assert_eq!(hexstring.to_string(), hex_str.to_lowercase());
    }

    #[parameterized(
        deadbeefg = { "deadbeefg" },
        abcdefg = { "abcdefg" },
        abcdef123456g = { "abcdef123456g" },
    )]

    fn test_invalid_hexstrings(hex_str: &str) {
        match HexString::new(hex_str) {
            Err(ManifestError::InvalidHex(s)) => {
                assert_eq!(s, hex_str);
            }
            _ => panic!("Unexpected success from {:?}", hex_str),
        }
    }

    // Note that we test comparison against UTC time, so the input string must be in UTC
    #[parameterized(
        one = { "Fri, 21 Jun 2024 17:40:02 +0000" },
        two = { "Sun, 16 Jun 2024 00:00:59 +0000" },
        three = { "Tue, 18 Jun 2024 13:40:04 +0000" },
    )]
    fn test_valid_rfc2822datetime(date: &str) {
        let rfc2822 = Rfc2822DateTime::from(date);
        let datetime: DateTime<Utc> = rfc2822.try_into().unwrap();
        assert_eq!(datetime.to_rfc2822(), date);
    }

    #[parameterized(
        empty_str = { "" },
        invalid_char = { "foo" },
        missing_lots = { "Fri, 21 Jun 2024" },
        missing_timezone = { "Fri, 21 Jun 2024 17:40:02" },
    )]
    fn test_invalid_rfc2822datetime(date: &str) {
        let rfc2822 = Rfc2822DateTime::from(date);
        let result: Result<DateTime<Utc>, ScrapeError> = rfc2822.try_into();
        match result {
            Err(_) => {}
            _ => panic!("Unexpected success from {:?}", date),
        }
    }

    #[test]
    fn test_hostname_as_str() {
        let hostname = Hostname("example.com".to_string());
        assert_eq!(hostname.as_str(), "example.com");
    }

    #[test]
    fn test_hostname_as_string() {
        let hostname = Hostname("example.com".to_string());
        assert_eq!(hostname.as_string(), "example.com");
    }
}
