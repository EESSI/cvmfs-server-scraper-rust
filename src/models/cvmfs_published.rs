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
/// Debug output shows only the byte count so binary signature data is not printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignatureBytes(Vec<u8>);
impl SignatureBytes {
    /// Borrow the original signature section without text decoding or newline changes.
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
///
/// A repository or replica publishes this data in `.cvmfspublished`. The wire
/// format uses uppercase one-letter keys; serde retains lowercase keys for this
/// struct. Accessors describe their meanings:
///
/// | Wire key | Accessor | Meaning |
/// | --- | --- | --- |
/// | `C` | [`catalog_hash`](Self::catalog_hash) | Current root catalog digest. |
/// | `B` | [`catalog_size`](Self::catalog_size) | Root catalog size in bytes. |
/// | `A` | [`alternative_catalog_path`](Self::alternative_catalog_path) | Whether the catalog uses an alternative path. |
/// | `R` | [`root_path_hash`](Self::root_path_hash) | MD5 digest of the repository root path. |
/// | `X` | [`certificate_hash`](Self::certificate_hash) | Signing certificate digest. |
/// | `G` | [`garbage_collectable`](Self::garbage_collectable) | Whether garbage collection is enabled. |
/// | `H` | [`history_hash`](Self::history_hash) | Named-tag history database digest. |
/// | `T` | [`published_at`](Self::published_at) | Revision publication time as Unix seconds. |
/// | `D` | [`ttl`](Self::ttl) | Root catalog time to live in seconds. |
/// | `S` | [`revision`](Self::revision) | Published revision number. |
/// | `N` | [`repository_name`](Self::repository_name) | Repository name claimed by the manifest. |
/// | `M` | [`metadata_hash`](Self::metadata_hash) | Repository JSON metadata digest. |
/// | `Y` | [`reflog_hash`](Self::reflog_hash) | Reflog checksum digest. |
/// | `L` | [`micro_catalog_hash`](Self::micro_catalog_hash) | Reserved micro-catalog digest. |
///
/// Parsing requires `C`, `R`, `D`, and `S`. Missing `B` defaults to zero; missing
/// `A` and `G` default to false. Other fields remain optional. The bytes after
/// the `--` separator are retained in [`Self::signature`], including the checksum
/// line and binary signature, without certificate or signature verification.
///
/// See the [CVMFS manifest format](https://cvmfs.readthedocs.io/en/stable/cpt-details/#repository-manifest-cvmfspublished)
/// for the protocol description.
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
    ///
    /// Accepts LF or CRLF metadata lines and an unterminated final line. Unknown
    /// uppercase keys are ignored after checking line structure and uniqueness.
    /// Use this byte entry point for network data because the signature may not be UTF-8.
    ///
    /// # Errors
    ///
    /// Rejects input larger than 1 MiB, empty or malformed metadata lines,
    /// duplicate keys, missing required fields, and invalid field values.
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
    /// Digest of the current root catalog (`C`).
    pub fn catalog_hash(&self) -> &ContentHash {
        &self.c
    }
    /// Root catalog size in bytes (`B`), defaulting to zero when omitted.
    pub fn catalog_size(&self) -> CatalogSize {
        self.b
    }
    /// Whether the catalog should be fetched under its alternative name (`A`).
    pub fn alternative_catalog_path(&self) -> bool {
        self.a
    }
    /// MD5 digest of the root path (`R`), usually the digest of an empty path.
    pub fn root_path_hash(&self) -> &RootPathMd5 {
        &self.r
    }
    /// Optional signing certificate digest (`X`); no certificate is fetched or verified.
    pub fn certificate_hash(&self) -> Option<&ContentHash> {
        self.x.as_ref()
    }
    /// Whether the manifest enables garbage collection (`G`).
    pub fn garbage_collectable(&self) -> bool {
        self.g
    }
    /// Optional named-tag history database digest (`H`).
    pub fn history_hash(&self) -> Option<&ContentHash> {
        self.h.as_ref()
    }
    /// Optional Unix timestamp of the published revision (`T`).
    pub fn published_at(&self) -> Option<UnixTimestamp> {
        self.t
    }
    /// Root catalog time to live in seconds (`D`).
    pub fn ttl(&self) -> CatalogTtl {
        self.d
    }
    /// Published revision number (`S`).
    pub fn revision(&self) -> Revision {
        self.s
    }
    /// Optional name claimed in `N`, before binding to a requested repository.
    pub fn repository_name(&self) -> Option<&RepositoryName> {
        self.n.as_ref()
    }
    /// Optional repository JSON metadata digest (`M`).
    pub fn metadata_hash(&self) -> Option<&ContentHash> {
        self.m.as_ref()
    }
    /// Optional reflog checksum digest (`Y`).
    pub fn reflog_hash(&self) -> Option<&ContentHash> {
        self.y.as_ref()
    }
    /// Optional digest in the reserved micro-catalog field (`L`).
    pub fn micro_catalog_hash(&self) -> Option<&ContentHash> {
        self.l.as_ref()
    }
    /// Exact bytes following `--`, or `None` if no separator was present.
    pub fn signature(&self) -> Option<&SignatureBytes> {
        self.signature.as_ref()
    }
    /// Print debug metadata to stdout, showing only the signature's byte count.
    pub fn output(&self) {
        println!("{self:#?}");
    }
    /// Bind the server's identity claim to the requested repository. Missing N
    /// remains explicitly Unspecified and never becomes a matched identity.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::RepositoryMismatch`] when `N` differs from the
    /// requested name. A successful binding does not authenticate the publisher.
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

/// Relationship between a manifest's optional name claim and the requested name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RepositoryIdentity {
    /// The manifest's `N` field matches the requested repository.
    Matched,
    /// No `N` field was supplied; no name agreement is claimed.
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
    /// Requested repository name used to establish this binding.
    pub fn repository_name(&self) -> &RepositoryName {
        &self.requested
    }
    /// Parsed, unverified manifest associated with the request.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    /// Whether the manifest explicitly matched the requested name or omitted it.
    pub fn identity(&self) -> RepositoryIdentity {
        self.identity
    }
}
