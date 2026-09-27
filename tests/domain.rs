use cvmfs_server_scraper::*;
use serde_json::json;
use std::time::Duration;
use yare::parameterized;

fn manifest() -> String {
    format!(
        "C{}\nRd41d8cd98f00b204e9800998ecf8427e\nD900\nS42\nNexample.org\n",
        "ab".repeat(20)
    )
}

#[parameterized(empty = {""}, authority = {"trusted.example@127.0.0.1:8123/other?"}, path = {"example.org/path"}, query = {"example.org?x"}, unicode = {"éxample.org"}, empty_label = {"example..org"}, dash = {"-example.org"}, port = {"example.org:80"})]
fn hostname_rejects_invalid_input_through_all_constructors(value: &str) {
    assert!(value.parse::<Hostname>().is_err());
    assert!(Hostname::try_from(value.to_owned()).is_err());
    assert!(serde_json::from_value::<Hostname>(json!(value)).is_err());
    assert!(serde_json::from_str::<Hostname>(&serde_json::to_string(value).unwrap()).is_err());
}
#[test]
fn hostname_normalizes_case_accepts_punycode_and_checks_lengths() {
    let host: Hostname = "XN--BCHER-KVA.Example".parse().unwrap();
    assert_eq!(host.as_str(), "xn--bcher-kva.example");
    assert_eq!(
        serde_json::from_value::<Hostname>(json!(host)).unwrap(),
        host
    );
    assert!(format!("{}.org", "a".repeat(64))
        .parse::<Hostname>()
        .is_err());
    assert!(format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "a".repeat(63),
        "a".repeat(63),
        "a".repeat(62)
    )
    .parse::<Hostname>()
    .is_err());
}
#[parameterized(traversal = {"../../admin?x="}, encoded = {"%2e%2e"}, slash = {"a/b"}, backslash = {"a\\b"}, query = {"a?b"}, fragment = {"a#b"}, blank = {""}, dot = {"."}, double_dot = {".."}, empty_label = {"repo..org"})]
fn repository_name_rejects_url_syntax(value: &str) {
    assert!(value.parse::<RepositoryName>().is_err());
    assert!(serde_json::from_value::<RepositoryName>(json!(value)).is_err());
}
#[test]
fn repository_names_roundtrip_owned_and_escaped_json() {
    let name: RepositoryName = "software.eessi.io".parse().unwrap();
    assert_eq!(
        serde_json::from_value::<RepositoryName>(json!(name)).unwrap(),
        name
    );
    assert_eq!(
        serde_json::from_str::<RepositoryName>(r#""\u0073oftware.eessi.io""#).unwrap(),
        name
    );
    assert!("a".repeat(256).parse::<RepositoryName>().is_err());
}
#[parameterized(credentials = {"http://user:password@example.org"}, path = {"http://example.org/cvmfs"}, query = {"http://example.org?x"}, fragment = {"http://example.org#x"}, scheme = {"file:///tmp/metadata"}, relative = {"example.org"}, whitespace = {" http://example.org"}, control = {"http://exam\nple.org"})]
fn endpoint_validation_cannot_be_bypassed_by_serde(value: &str) {
    assert!(value.parse::<ServerEndpoint>().is_err());
    assert!(serde_json::from_value::<ServerEndpoint>(json!(value)).is_err());
}
#[test]
fn endpoints_support_explicit_ports_tls_and_ipv6() {
    for text in [
        "http://127.0.0.1:12345",
        "https://example.org:8443",
        "http://[::1]:12345",
    ] {
        let endpoint: ServerEndpoint = text.parse().unwrap();
        assert_eq!(
            serde_json::from_value::<ServerEndpoint>(json!(endpoint)).unwrap(),
            endpoint
        );
    }
}
#[test]
fn malformed_manifest_input_returns_errors_without_panicking() {
    for input in [
        b"\n".as_slice(),
        "évalue\n".as_bytes(),
        b"C\xff\n",
        b"Cabc\nCdef\n",
        b"cabc\n",
        b"\r\n",
    ] {
        assert!(Manifest::from_bytes(input).is_err());
    }
    // All possible first bytes and arbitrary bytes throughout a metadata line.
    for byte in 0..=255u8 {
        let mut bytes = vec![byte; 200];
        for offset in [0, 1, 12, 199] {
            bytes[offset] = b'\n';
        }
        let _ = Manifest::from_bytes(&bytes);
    }
    assert!(Manifest::from_bytes(&vec![b'A'; 1024 * 1024 + 1]).is_err());
}
#[test]
fn manifests_accept_optional_fields_and_large_unsigned_values() {
    let input = manifest()
        .replace("S42", "S18446744073709551615")
        .replace("D900", "D4294967295");
    let parsed: Manifest = input.parse().unwrap();
    assert_eq!(parsed.revision().get(), u64::MAX);
    assert_eq!(parsed.ttl().get(), u32::MAX);
    assert_eq!(parsed.catalog_size().get(), 0);
    assert!(!parsed.alternative_catalog_path());
    assert!(!parsed.garbage_collectable());
    assert!(parsed.history_hash().is_none());
    assert!(parsed.metadata_hash().is_none());
    assert!(parsed.reflog_hash().is_none());
    assert!(parsed.signature().is_none());
    for (key, value) in [
        ('B', "-1"),
        ('D', "-1"),
        ('S', "-1"),
        ('T', "-1"),
        ('A', "invalid"),
        ('G', "invalid"),
    ] {
        let input = manifest()
            .lines()
            .filter(|line| !line.starts_with(key))
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            + &format!("{key}{value}\n");
        assert!(input.parse::<Manifest>().is_err(), "{key} accepted {value}");
    }
    assert!(manifest()
        .replace("S42", "S18446744073709551616")
        .parse::<Manifest>()
        .is_err());
    assert!(UnixTimestamp::new(u64::MAX).datetime().is_err());
}
#[test]
fn hash_types_accept_supported_algorithms_and_require_exact_sizes() {
    for suffix in ["", "-rmd160", "-shake128"] {
        let text = format!("{}{suffix}", "AB".repeat(20));
        let hash: ContentHash = text.parse().unwrap();
        assert_eq!(hash.to_string(), text.to_ascii_lowercase());
        assert_eq!(
            serde_json::from_value::<ContentHash>(json!(hash)).unwrap(),
            hash
        );
        assert!(manifest()
            .replacen(&"ab".repeat(20), &text, 1)
            .parse::<Manifest>()
            .is_ok());
    }
    for invalid in [
        "",
        "ab",
        "zz",
        &"ab".repeat(19),
        &format!("{}-sha256", "ab".repeat(32)),
    ] {
        assert!(invalid.parse::<ContentHash>().is_err());
        assert!(serde_json::from_value::<ContentHash>(json!(invalid)).is_err());
    }
    assert!("ab".repeat(20).parse::<RootPathMd5>().is_err());
    assert!(manifest()
        .replacen(&"ab".repeat(20), "", 1)
        .parse::<Manifest>()
        .is_err());
}
#[test]
fn serde_owned_values_escaped_strings_and_manifest_roundtrips_work() {
    let hex = serde_json::from_str::<HexString>(r#""\u0061b""#).unwrap();
    assert_eq!(
        serde_json::from_value::<HexString>(json!(hex)).unwrap(),
        hex
    );
    let parsed: Manifest = manifest().parse().unwrap();
    assert_eq!(
        serde_json::from_value::<Manifest>(json!(parsed)).unwrap(),
        parsed
    );
    let mut invalid = json!(parsed);
    invalid["s"] = json!(-1);
    assert!(serde_json::from_value::<Manifest>(invalid).is_err());
}
#[test]
fn signature_bytes_are_preserved_including_invalid_utf8_and_newlines() {
    let tail = b"checksum\n\xff\x00\r\n--\n\xfe\n";
    let bytes = [manifest().as_bytes(), b"--\n", tail].concat();
    let parsed = Manifest::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.signature().unwrap().as_bytes(), tail);
    let roundtrip: Manifest = serde_json::from_value(json!(parsed)).unwrap();
    assert_eq!(roundtrip.signature().unwrap().as_bytes(), tail);
}
#[test]
fn repository_binding_checks_identity_and_preserves_missing_claims() {
    let parsed: Manifest = manifest().parse().unwrap();
    assert!(parsed
        .clone()
        .bind_to_repository("other.org".parse().unwrap())
        .is_err());
    let bound = parsed
        .bind_to_repository("example.org".parse().unwrap())
        .unwrap();
    assert_eq!(bound.identity(), RepositoryIdentity::Matched);
    let without_name: Manifest = manifest().replace("Nexample.org\n", "").parse().unwrap();
    assert_eq!(
        without_name
            .bind_to_repository("example.org".parse().unwrap())
            .unwrap()
            .identity(),
        RepositoryIdentity::Unspecified
    );
}
#[test]
fn timestamps_preserve_unknown_zones_and_convert_known_offsets() {
    for raw in [
        "Fri Jun 21 17:40:02 CEST 2024",
        "localized date",
        "Fri Jun 21 17:40:02 2024",
    ] {
        let timestamp = ReportedTimestamp::new(raw.into());
        assert!(timestamp.datetime().is_none());
        assert!(timestamp.try_into_datetime().is_err());
        assert_eq!(timestamp.as_str(), raw);
        assert_eq!(
            serde_json::from_value::<ReportedTimestamp>(json!(timestamp)).unwrap(),
            timestamp
        );
    }
    for raw in [
        "Fri Jun 21 17:40:02 UTC 2024",
        "Fri Jun 21 17:40:02 GMT 2024",
        "Fri Jun 21 19:40:02 +0200 2024",
        "Fri, 21 Jun 2024 19:40:02 +0200",
    ] {
        let timestamp = ReportedTimestamp::new(raw.into());
        assert_eq!(
            timestamp.datetime().unwrap().to_rfc3339(),
            "2024-06-21T17:40:02+00:00"
        );
    }
}
fn hosts() -> Vec<Hostname> {
    ["one.org", "two.org", "three.org"]
        .into_iter()
        .map(|h| h.parse().unwrap())
        .collect()
}
#[test]
fn geoapi_ordering_converts_one_based_ids_and_validates_the_permutation() {
    let ordering = GeoapiOrdering::from_response(hosts(), "3,1,2\n").unwrap();
    assert_eq!(
        ordering
            .ordered_hosts()
            .map(Hostname::as_str)
            .collect::<Vec<_>>(),
        ["three.org", "one.org", "two.org"]
    );
    assert_eq!(
        serde_json::from_value::<GeoapiOrdering>(json!(ordering)).unwrap(),
        ordering
    );
    for invalid in [
        "0,1,2", "1,1,2", "1,2,4", "1,2", "1,2,3,4", "", "1,-1,3", "1,wat,3",
    ] {
        assert!(
            GeoapiOrdering::from_response(hosts(), invalid).is_err(),
            "accepted {invalid}"
        );
    }
    let invalid = json!({"hosts": hosts(), "response": [1, 1, 2]});
    assert!(serde_json::from_value::<GeoapiOrdering>(invalid).is_err());
    assert!(serde_json::from_value::<GeoapiHostId>(json!(0)).is_err());
    assert!(GeoapiHosts::new(vec![]).is_err());
    assert!(GeoapiHosts::new(vec!["one.org".parse().unwrap(); 2]).is_err());
}
#[test]
fn limits_reject_zero_and_extreme_values() {
    assert!(RequestTimeout::new(Duration::ZERO).is_err());
    assert!(RequestTimeout::new(Duration::from_secs(3601)).is_err());
    assert!(ConcurrencyLimit::new(0).is_err());
    assert!(ConcurrencyLimit::new(1025).is_err());
    assert!(ResponseByteLimit::new(0).is_err());
    assert!(ResponseByteLimit::new(usize::MAX).is_err());
    assert!(RepositoryLimit::new(0).is_err());
}
