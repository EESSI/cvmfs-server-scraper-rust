use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::ManifestError;
use crate::models::generic::HexString;
use crate::utilities::{parse_boolean_field, parse_hex_field, parse_number_field};

/// The manifest of a repository or replica.
///
/// The fields are:
/// - c: Cryptographic hash of the repository’s current root catalog
/// - b: Size of the root file catalog in bytes
/// - a: true if the catalog should be fetched under its alternative name
/// - r: MD5 hash of the repository’s current root path (usually always d41d8cd98f00b204e9800998ecf8427e)
/// - x: Cryptographic hash of the signing certificate
/// - g: true if the repository is garbage-collectable
/// - h: Cryptographic hash of the repository’s named tag history database
/// - t: Unix timestamp of this particular revision
/// - d: Time To Live (TTL) of the root catalog
/// - s: Revision number of this published revision
/// - n: The full name of the manifested repository
/// - m: Cryptographic hash of the repository JSON metadata
/// - y: Cryptographic hash of the reflog checksum
/// - l: currently unused (reserved for micro catalogs)
/// - signature: In order to provide authoritative information about a repository publisher, the
///              repository manifest is signed by an X.509 certificate together with its private key.
///              This field is not validated by this library.
///
/// Note that the field names are lowercase, but the field names in the manifest itself are uppercase.
///
/// See https://cvmfs.readthedocs.io/en/stable/cpt-details.html#repository-manifest-cvmfspublished for
/// more information.
#[derive(Deserialize, Serialize, Clone, PartialEq)]
pub struct Manifest {
    pub c: HexString,
    pub b: i64,
    pub a: bool,
    pub r: HexString,
    pub x: HexString,
    pub g: bool,
    pub h: HexString,
    pub t: i64,
    pub d: i32,
    pub s: i32,
    pub n: String,
    pub m: HexString,
    pub y: HexString,
    pub l: String, // Currently unused
    pub signature: String,
}

/// Debug implementation for Manifest
///
/// This implementation allows the struct to be printed with debug formatting,
/// but only the fields are printed, not the signature (which is a binart blob).
impl std::fmt::Debug for Manifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manifest")
            .field("c", &self.c)
            .field("b", &self.b)
            .field("a", &self.a)
            .field("r", &self.r)
            .field("x", &self.x)
            .field("g", &self.g)
            .field("h", &self.h)
            .field("t", &self.t)
            .field("d", &self.d)
            .field("s", &self.s)
            .field("n", &self.n)
            .field("m", &self.m)
            .field("y", &self.y)
            .field("l", &self.l)
            .finish()
    }
}

impl std::str::FromStr for Manifest {
    type Err = ManifestError;

    fn from_str(content: &str) -> Result<Self, Self::Err> {
        let mut data: HashMap<char, String> = HashMap::new();
        let mut signature: String = String::new();
        let mut is_signature = false;

        for line in content.lines() {
            if line == "--" {
                is_signature = true;
                continue;
            }
            if is_signature {
                signature.push_str(line);
            } else {
                let key = line.chars().next().unwrap();
                let value = &line[1..];
                data.insert(key, value.to_string());
            }
        }

        let manifest = Manifest {
            c: parse_hex_field(&data, 'C')?,
            b: parse_number_field(&data, 'B')?,
            a: parse_boolean_field(&data, 'A')?,
            r: parse_hex_field(&data, 'R')?,
            x: parse_hex_field(&data, 'X')?,
            g: parse_boolean_field(&data, 'G')?,
            h: parse_hex_field(&data, 'H')?,
            t: parse_number_field(&data, 'T')?,
            d: parse_number_field(&data, 'D')?,
            s: parse_number_field(&data, 'S')?,
            n: data
                .get(&'N')
                .ok_or(ManifestError::MissingField('N'))?
                .clone(),
            m: parse_hex_field(&data, 'M')?,
            y: parse_hex_field(&data, 'Y')?,
            l: data.get(&'L').cloned().unwrap_or_default(),
            signature,
        };

        Ok(manifest)
    }
}

impl Manifest {
    pub fn display(&self) {
        println!("  Manifest for repository: {}", self.n);
        println!("    Root catalog hash: {}", self.c);
        println!("    Root catalog size: {}", self.b);
        println!("    Fetch under alternative name: {}", self.a);
        println!("    Root path hash: {}", self.r);
        println!("    Signing certificate hash: {}", self.x);
        println!("    Garbage-collectable: {}", self.g);
        println!("    Tag history hash: {}", self.h);
        println!("    Revision timestamp: {}", self.t);
        println!("    Root catalog TTL: {}", self.d);
        println!("    Revision number: {}", self.s);
        println!("    Metadata hash: {}", self.m);
        println!("    Reflog checksum hash: {}", self.y);
        // println!("  Signature: {}", self.signature);
    }
}
