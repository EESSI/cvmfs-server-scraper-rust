use std::collections::HashMap;
use std::num::ParseIntError;

use chrono::{DateTime, Utc};

use crate::errors::ManifestError;
use crate::models::HexString;

/// Parse a boolean field from a manifest.
///
/// This function parses a boolean field from a manifest. Anything other than "yes" (or "YES")
/// is considered false.
pub fn parse_boolean_field(data: &HashMap<char, String>, key: char) -> Result<bool, ManifestError> {
    data.get(&key)
        .ok_or(ManifestError::MissingField(key))
        .map(|v| v.to_lowercase() == "yes")
}

/// Parse a hexadecimal field from a manifest.
///
/// This function parses a hexadecimal field from a manifest. Rules for a valid hexadecimal field
/// are:
///
/// - The field must only contain hexadecimal characters (0-9, a-f).
/// - The field must have an even number of characters.
pub fn parse_hex_field(
    data: &HashMap<char, String>,
    key: char,
) -> Result<HexString, ManifestError> {
    let value = data.get(&key).ok_or(ManifestError::MissingField(key))?;
    value.parse().map_err(|e: ManifestError| e)
}

pub fn parse_number_field<T>(data: &HashMap<char, String>, key: char) -> Result<T, ManifestError>
where
    T: std::str::FromStr<Err = ParseIntError>,
{
    data.get(&key)
        .ok_or(ManifestError::MissingField(key))?
        .parse()
        .map_err(|e: ParseIntError| ManifestError::ParseError(key, e.to_string()))
}

#[allow(dead_code)]
pub fn parse_timestamp_field(
    data: &HashMap<char, String>,
    key: char,
) -> Result<DateTime<Utc>, ManifestError> {
    data.get(&key)
        .ok_or(ManifestError::MissingField(key))
        .and_then(|v| {
            let timestamp = v
                .parse::<i64>()
                .map_err(|e| ManifestError::ParseError(key, e.to_string()))?;
            DateTime::from_timestamp(timestamp, 0)
                .ok_or_else(|| ManifestError::ParseError(key, "Invalid timestamp".to_string()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use yare::parameterized;

    #[parameterized(
        yes = { "yes", true },        
        no = { "no", false },
        empty = { "", false },
        not_yes_no = { "invalid", false }
    )]
    fn test_parse_boolean_field(value: &str, expected: bool) {
        let mut data = HashMap::new();
        data.insert('A', value.to_string());
        assert_eq!(parse_boolean_field(&data, 'A').unwrap(), expected);
    }

    #[parameterized(
        chars = { "aabbcc" },
        uppercasechars = { "AABBCC" },
        numbers = { "123456" },
        all = { "1234567890abcdef" },
        empty = { "" },
    )]
    fn test_parse_valid_hex_field(value: &str) {
        let mut data = HashMap::new();
        data.insert('R', value.to_string());
        assert_eq!(
            parse_hex_field(&data, 'R').unwrap().to_string(),
            value.to_lowercase() // HexString stores the value in lowercase
        );
    }

    #[parameterized(
        odd = { "B" },
        invalid = { "invalid" },
    )]
    fn test_parse_invalid_hex_field(value: &str) {
        let mut data = HashMap::new();
        data.insert('R', value.to_string());
        assert!(parse_hex_field(&data, 'R').is_err());
    }
}
