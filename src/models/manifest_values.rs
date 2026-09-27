use crate::ManifestError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

macro_rules! unsigned_value {
    ($name:ident, $inner:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name($inner);
        impl $name {
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }
            pub const fn get(self) -> $inner {
                self.0
            }
        }
        impl FromStr for $name {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
unsigned_value!(Revision, u64);
unsigned_value!(CatalogSize, u64);
unsigned_value!(CatalogTtl, u32);
unsigned_value!(UnixTimestamp, u64);
impl UnixTimestamp {
    pub fn datetime(self) -> Result<DateTime<Utc>, ManifestError> {
        i64::try_from(self.0)
            .ok()
            .and_then(|s| DateTime::from_timestamp(s, 0))
            .ok_or_else(|| {
                ManifestError::ParseError('T', "timestamp is outside the datetime range".into())
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha1,
    Rmd160,
    Shake128,
}

/// A supported CVMFS content digest. Its bytes and algorithm cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash {
    algorithm: HashAlgorithm,
    digest: [u8; 20],
}
impl ContentHash {
    pub fn new(algorithm: HashAlgorithm, digest: [u8; 20]) -> Self {
        Self { algorithm, digest }
    }
    pub fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }
    pub fn digest(&self) -> &[u8; 20] {
        &self.digest
    }
}
fn decode<const N: usize>(s: &str) -> Result<[u8; N], ManifestError> {
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ManifestError::InvalidDigest(format!(
            "expected {} hexadecimal characters",
            N * 2
        )));
    }
    let mut bytes = [0; N];
    for (out, pair) in bytes.iter_mut().zip(s.as_bytes().as_chunks::<2>().0) {
        fn nibble(b: u8) -> u8 {
            match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => b - b'A' + 10,
            }
        }
        *out = nibble(pair[0]) * 16 + nibble(pair[1]);
    }
    Ok(bytes)
}
impl FromStr for ContentHash {
    type Err = ManifestError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (hex, algorithm) = if let Some(hex) = s.strip_suffix("-rmd160") {
            (hex, HashAlgorithm::Rmd160)
        } else if let Some(hex) = s.strip_suffix("-shake128") {
            (hex, HashAlgorithm::Shake128)
        } else {
            (s, HashAlgorithm::Sha1)
        };
        Ok(Self::new(algorithm, decode(hex)?))
    }
}
impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.digest {
            write!(f, "{byte:02x}")?;
        }
        f.write_str(match self.algorithm {
            HashAlgorithm::Sha1 => "",
            HashAlgorithm::Rmd160 => "-rmd160",
            HashAlgorithm::Shake128 => "-shake128",
        })
    }
}
impl TryFrom<String> for ContentHash {
    type Error = ManifestError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}
impl From<ContentHash> for String {
    fn from(value: ContentHash) -> Self {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RootPathMd5([u8; 16]);
impl RootPathMd5 {
    pub fn digest(&self) -> &[u8; 16] {
        &self.0
    }
}
impl FromStr for RootPathMd5 {
    type Err = ManifestError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        decode(s).map(Self)
    }
}
impl fmt::Display for RootPathMd5 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl TryFrom<String> for RootPathMd5 {
    type Error = ManifestError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}
impl From<RootPathMd5> for String {
    fn from(value: RootPathMd5) -> Self {
        value.to_string()
    }
}
