use crate::{
    CatalogSize, CatalogTtl, ContentHash, ManifestError, RepositoryName, Revision, RootPathMd5,
    UnixTimestamp,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};

/// Hard parser bound, also applied when Manifest::from_bytes is called directly.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Exact bytes following the manifest separator, including the checksum line.
/// Presence does not imply that the signature is valid or trusted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignatureBytes(Vec<u8>);
impl SignatureBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
impl std::fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignatureBytes({} bytes)", self.0.len())
    }
}

/// Parsed, unverified repository metadata. All scalar fields obey their protocol
/// ranges. This type makes no cryptographic authenticity claim.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Manifest {
    c: ContentHash,
    #[serde(default = "zero_size")]
    b: CatalogSize,
    #[serde(default)]
    a: bool,
    r: RootPathMd5,
    x: Option<ContentHash>,
    #[serde(default)]
    g: bool,
    h: Option<ContentHash>,
    t: Option<UnixTimestamp>,
    d: CatalogTtl,
    s: Revision,
    n: Option<RepositoryName>,
    m: Option<ContentHash>,
    y: Option<ContentHash>,
    l: Option<ContentHash>,
    signature: Option<SignatureBytes>,
}
fn zero_size() -> CatalogSize {
    CatalogSize::new(0)
}

impl FromStr for Manifest {
    type Err = ManifestError;
    fn from_str(content: &str) -> Result<Self, Self::Err> {
        Self::from_bytes(content.as_bytes())
    }
}
impl Manifest {
    /// Parse the ASCII metadata envelope without decoding or changing binary signature bytes.
    pub fn from_bytes(content: &[u8]) -> Result<Self, ManifestError> {
        if content.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge(MAX_MANIFEST_BYTES));
        }
        let mut data = BTreeMap::new();
        let mut signature = None;
        let mut offset = 0;
        let mut line_number = 0;
        while offset < content.len() {
            line_number += 1;
            let rest = &content[offset..];
            let length = rest
                .iter()
                .position(|b| *b == b'\n')
                .map_or(rest.len(), |i| i + 1);
            let line = rest[..length]
                .strip_suffix(b"\n")
                .unwrap_or(&rest[..length]);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            offset += length;
            if line == b"--" {
                signature = Some(SignatureBytes(content[offset..].to_vec()));
                break;
            }
            let (&key, value) = line
                .split_first()
                .ok_or_else(|| ManifestError::InvalidLine {
                    line: line_number,
                    reason: "empty metadata line".into(),
                })?;
            if !key.is_ascii_uppercase() {
                return Err(ManifestError::InvalidLine {
                    line: line_number,
                    reason: "field key must be an uppercase ASCII letter".into(),
                });
            }
            let value = std::str::from_utf8(value).map_err(|_| ManifestError::InvalidLine {
                line: line_number,
                reason: "metadata value is not UTF-8".into(),
            })?;
            if data.insert(char::from(key), value).is_some() {
                return Err(ManifestError::DuplicateField(char::from(key)));
            }
        }
        Ok(Self {
            c: required(&data, 'C')?,
            b: optional(&data, 'B')?.unwrap_or_else(zero_size),
            a: flag(&data, 'A')?,
            r: required(&data, 'R')?,
            x: optional(&data, 'X')?,
            g: flag(&data, 'G')?,
            h: optional(&data, 'H')?,
            t: optional(&data, 'T')?,
            d: required(&data, 'D')?,
            s: required(&data, 'S')?,
            n: optional(&data, 'N')?,
            m: optional(&data, 'M')?,
            y: optional(&data, 'Y')?,
            l: optional(&data, 'L')?,
            signature,
        })
    }
    pub fn catalog_hash(&self) -> &ContentHash {
        &self.c
    }
    pub fn catalog_size(&self) -> CatalogSize {
        self.b
    }
    pub fn alternative_catalog_path(&self) -> bool {
        self.a
    }
    pub fn root_path_hash(&self) -> &RootPathMd5 {
        &self.r
    }
    pub fn certificate_hash(&self) -> Option<&ContentHash> {
        self.x.as_ref()
    }
    pub fn garbage_collectable(&self) -> bool {
        self.g
    }
    pub fn history_hash(&self) -> Option<&ContentHash> {
        self.h.as_ref()
    }
    pub fn published_at(&self) -> Option<UnixTimestamp> {
        self.t
    }
    pub fn ttl(&self) -> CatalogTtl {
        self.d
    }
    pub fn revision(&self) -> Revision {
        self.s
    }
    pub fn repository_name(&self) -> Option<&RepositoryName> {
        self.n.as_ref()
    }
    pub fn metadata_hash(&self) -> Option<&ContentHash> {
        self.m.as_ref()
    }
    pub fn reflog_hash(&self) -> Option<&ContentHash> {
        self.y.as_ref()
    }
    pub fn micro_catalog_hash(&self) -> Option<&ContentHash> {
        self.l.as_ref()
    }
    pub fn signature(&self) -> Option<&SignatureBytes> {
        self.signature.as_ref()
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
    /// Bind the server's identity claim to the requested repository. Missing N
    /// remains explicitly Unspecified and never becomes a matched identity.
    pub fn bind_to_repository(
        self,
        requested: RepositoryName,
    ) -> Result<RepositoryManifest, ManifestError> {
        let identity = match &self.n {
            Some(actual) if actual != &requested => {
                return Err(ManifestError::RepositoryMismatch {
                    expected: requested.to_string(),
                    actual: actual.to_string(),
                })
            }
            Some(_) => RepositoryIdentity::Matched,
            None => RepositoryIdentity::Unspecified,
        };
        Ok(RepositoryManifest {
            requested,
            manifest: self,
            identity,
        })
    }
}
fn required<T: FromStr>(data: &BTreeMap<char, &str>, key: char) -> Result<T, ManifestError>
where
    T::Err: std::fmt::Display,
{
    optional(data, key)?.ok_or(ManifestError::MissingField(key))
}
fn optional<T: FromStr>(data: &BTreeMap<char, &str>, key: char) -> Result<Option<T>, ManifestError>
where
    T::Err: std::fmt::Display,
{
    data.get(&key)
        .map(|value| {
            value
                .parse()
                .map_err(|e: T::Err| ManifestError::ParseError(key, e.to_string()))
        })
        .transpose()
}
fn flag(data: &BTreeMap<char, &str>, key: char) -> Result<bool, ManifestError> {
    match data.get(&key) {
        None | Some(&"no") => Ok(false),
        Some(&"yes") => Ok(true),
        Some(_) => Err(ManifestError::ParseError(
            key,
            "expected 'yes' or 'no'".into(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RepositoryIdentity {
    Matched,
    Unspecified,
}

/// Proof that the manifest did not claim a different repository. Constructed
/// only by Manifest::bind_to_repository; it cannot be forged with Deserialize.
///
/// ```compile_fail
/// use cvmfs_server_scraper::RepositoryManifest;
/// let forged: RepositoryManifest = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryManifest {
    requested: RepositoryName,
    manifest: Manifest,
    identity: RepositoryIdentity,
}
impl RepositoryManifest {
    pub fn repository_name(&self) -> &RepositoryName {
        &self.requested
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn identity(&self) -> RepositoryIdentity {
        self.identity
    }
}
