use cvmfs_server_scraper::*;
use rstest::rstest;
use serde_json::json;
use std::time::Duration;

fn manifest() -> String {
    format!(
        "C{}\nRd41d8cd98f00b204e9800998ecf8427e\nD900\nS42\nNexample.org\n",
        "ab".repeat(20)
    )
}

#[rstest]
#[case::empty("")]
#[case::authority("trusted.example@127.0.0.1:8123/other?")]
#[case::path("example.org/path")]
#[case::query("example.org?x")]
#[case::unicode("éxample.org")]
#[case::empty_label("example..org")]
#[case::leading_dash("-example.org")]
#[case::trailing_dash("example-.org")]
#[case::port("example.org:80")]
fn hostname_rejects_invalid_input_through_all_constructors(#[case] value: &str) {
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
#[rstest]
#[case::traversal("../../admin?x=")]
#[case::encoded("%2e%2e")]
#[case::slash("a/b")]
#[case::backslash("a\\b")]
#[case::query("a?b")]
#[case::fragment("a#b")]
#[case::blank("")]
#[case::dot(".")]
#[case::double_dot("..")]
#[case::empty_label("repo..org")]
fn repository_name_rejects_url_syntax(#[case] value: &str) {
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
#[rstest]
#[case::credentials("http://user:password@example.org")]
#[case::path("http://example.org/cvmfs")]
#[case::query("http://example.org?x")]
#[case::fragment("http://example.org#x")]
#[case::scheme("file:///tmp/metadata")]
#[case::relative("example.org")]
#[case::whitespace(" http://example.org")]
#[case::control("http://exam\nple.org")]
#[case::invalid_dns_label("https://-example.org")]
fn endpoint_validation_cannot_be_bypassed_by_serde(#[case] value: &str) {
    assert!(value.parse::<ServerEndpoint>().is_err());
    assert!(serde_json::from_value::<ServerEndpoint>(json!(value)).is_err());
}
#[rstest]
#[case::ipv4("http://127.0.0.1:12345", "127.0.0.1")]
#[case::https("https://example.org:8443", "example.org")]
#[case::ipv6("http://[::1]:12345", "[::1]")]
#[case::idna("https://bücher.example", "xn--bcher-kva.example")]
fn endpoints_support_explicit_ports_tls_and_ipv6(#[case] text: &str, #[case] host: &str) {
    let endpoint: ServerEndpoint = text.parse().unwrap();
    assert_eq!(endpoint.host(), host);
    assert_eq!(endpoint.as_url().host_str(), Some(host));
    assert_eq!(
        serde_json::from_value::<ServerEndpoint>(json!(endpoint)).unwrap(),
        endpoint
    );
}
#[rstest]
#[case::legacy(json!({"hostname": "EXAMPLE.org"}), "http://example.org/")]
#[case::endpoint(json!({"endpoint": "https://example.org:8443"}), "https://example.org:8443/")]
fn server_configuration_accepts_legacy_and_explicit_addresses(
    #[case] mut config: serde_json::Value,
    #[case] endpoint: &str,
) {
    config["server_type"] = json!("Stratum1");
    let server: Server = serde_json::from_value(config.clone()).unwrap();
    assert_eq!(server.endpoint().to_string(), endpoint);
    assert_eq!(server.hostname(), "example.org");
    assert_eq!(server.backend_type(), ServerBackendType::AutoDetect);
    config["backend_type"] = json!("S3");
    let explicit: Server = serde_json::from_str(&config.to_string()).unwrap();
    assert_eq!(explicit.backend_type(), ServerBackendType::S3);
    let canonical = serde_json::to_value(&server).unwrap();
    assert_eq!(canonical["endpoint"], endpoint);
    assert!(canonical.get("hostname").is_none());
    assert_eq!(serde_json::from_value::<Server>(canonical).unwrap(), server);
}

#[rstest]
#[case::both(json!({"hostname":"example.org", "endpoint":"http://example.org"}))]
#[case::missing(json!({}))]
#[case::hostname_with_scheme(json!({"hostname":"https://example.org"}))]
#[case::hostname_with_port(json!({"hostname":"example.org:8443"}))]
#[case::invalid_hostname(json!({"hostname":"bad..example"}))]
#[case::invalid_endpoint(json!({"endpoint":"example.org"}))]
#[case::no_fallback_from_invalid_endpoint(json!({"hostname":"example.org", "endpoint":"invalid"}))]
fn server_configuration_rejects_ambiguous_or_invalid_addresses(
    #[case] mut config: serde_json::Value,
) {
    config["server_type"] = json!("Stratum1");
    assert!(serde_json::from_value::<Server>(config).is_err());
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
    for (suffix, algorithm) in [
        ("", HashAlgorithm::Sha1),
        ("-rmd160", HashAlgorithm::Rmd160),
        ("-shake128", HashAlgorithm::Shake128),
    ] {
        let text = format!("{}{suffix}", "AB".repeat(20));
        let hash: ContentHash = text.parse().unwrap();
        assert_eq!(hash.algorithm(), algorithm);
        assert_eq!(hash.digest(), &[0xab; 20]);
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
        &"zz".repeat(20),
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

#[rstest]
#[case::catalog('C')]
#[case::root_path('R')]
#[case::ttl('D')]
#[case::revision('S')]
fn missing_manifest_fields_identify_the_required_key(#[case] key: char) {
    let input = manifest()
        .lines()
        .filter(|line| !line.starts_with(key))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        input.parse::<Manifest>(),
        Err(ManifestError::MissingField(key))
    );
}

#[rstest]
#[case::lf("\n", true)]
#[case::crlf("\r\n", true)]
#[case::no_final_newline("\n", false)]
fn optional_manifest_fields_survive_parsing_and_serde(
    #[case] newline: &str,
    #[case] final_newline: bool,
) {
    let digest = "0123456789abcdef0123456789abcdef01234567";
    let input = format!(
        "{}B18446744073709551615\nAyes\nGyes\nT1718984402\nX{digest}\nH{digest}-rmd160\nM{digest}-shake128\nY{digest}\nL{digest}\n",
        manifest()
    );
    let input = if final_newline {
        input.as_str()
    } else {
        input.trim_end()
    };
    let parsed = Manifest::from_bytes(input.replace('\n', newline).as_bytes()).unwrap();
    assert_eq!(parsed.catalog_hash().digest(), &[0xab; 20]);
    assert_eq!(parsed.catalog_size().get(), u64::MAX);
    assert!(parsed.alternative_catalog_path());
    assert!(parsed.garbage_collectable());
    assert_eq!(
        parsed.root_path_hash().digest(),
        &[
            0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
            0x42, 0x7e,
        ]
    );
    assert_eq!(parsed.certificate_hash().unwrap().to_string(), digest);
    assert_eq!(
        parsed.history_hash().unwrap().algorithm(),
        HashAlgorithm::Rmd160
    );
    assert_eq!(
        parsed.metadata_hash().unwrap().algorithm(),
        HashAlgorithm::Shake128
    );
    assert_eq!(parsed.reflog_hash(), parsed.micro_catalog_hash());
    assert_eq!(parsed.reflog_hash(), parsed.certificate_hash());
    assert_eq!(
        parsed
            .published_at()
            .unwrap()
            .datetime()
            .unwrap()
            .to_rfc3339(),
        "2024-06-21T15:40:02+00:00"
    );
    assert_eq!(
        serde_json::from_value::<Manifest>(json!(parsed)).unwrap(),
        parsed
    );
}

#[rstest]
#[case::catalog("c", json!("zz".repeat(20)))]
#[case::root_path("r", json!("ab".repeat(20)))]
#[case::catalog_size("b", json!(-1))]
#[case::ttl("d", json!(u64::from(u32::MAX) + 1))]
#[case::revision("s", json!(-1))]
#[case::timestamp("t", json!(-1))]
#[case::repository("n", json!("../private"))]
#[case::optional_hash("h", json!("unsupported"))]
#[case::signature("signature", json!("not a byte array"))]
fn manifest_serde_enforces_scalar_invariants(
    #[case] field: &str,
    #[case] value: serde_json::Value,
) {
    let mut data = json!(manifest().parse::<Manifest>().unwrap());
    data[field] = value;
    assert!(serde_json::from_value::<Manifest>(data).is_err());
}

#[rstest]
#[case::true_flags("yes", true)]
#[case::false_flags("no", false)]
fn explicit_manifest_flags_are_not_confused_with_presence(
    #[case] value: &str,
    #[case] expected: bool,
) {
    let parsed: Manifest = format!("{}A{value}\nG{value}\n", manifest())
        .parse()
        .unwrap();
    assert_eq!(parsed.alternative_catalog_path(), expected);
    assert_eq!(parsed.garbage_collectable(), expected);
}

#[rstest]
#[case::absent(None, Ok(None))]
#[case::utc(
    Some("Fri Jun 21 17:40:02 UTC 2024"),
    Ok(Some("2024-06-21T17:40:02+00:00"))
)]
#[case::offset(
    Some("Fri, 21 Jun 2024 19:40:02 +0200"),
    Ok(Some("2024-06-21T17:40:02+00:00"))
)]
#[case::unresolved_zone(Some("Fri Jun 21 17:40:02 CEST 2024"), Err(()))]
#[case::empty(Some(""), Err(()))]
fn compatibility_timestamps_preserve_absent_parsed_and_unresolved_states(
    #[case] raw: Option<&str>,
    #[case] expected: Result<Option<&str>, ()>,
) {
    let timestamp = MaybeRfc2822DateTime::new(raw.map(str::to_owned));
    let parsed = timestamp
        .try_into_datetime()
        .map(|instant| instant.map(|instant| instant.to_rfc3339()))
        .map_err(|_| ());
    assert_eq!(parsed, expected.map(|instant| instant.map(str::to_owned)));
    assert_eq!(timestamp.is_some(), raw.is_some());
    assert_eq!(timestamp.is_none(), raw.is_none());
    assert_eq!(timestamp.as_ref().map(ReportedTimestamp::as_str), raw);
    assert_eq!(timestamp.to_string(), raw.unwrap_or_default());
    assert_eq!(serde_json::to_value(&timestamp).unwrap(), json!(raw));
    assert_eq!(
        serde_json::from_value::<MaybeRfc2822DateTime>(json!(raw)).unwrap(),
        timestamp
    );
}

#[rstest]
#[case::concurrency(1024, |n| ConcurrencyLimit::new(n).map(ConcurrencyLimit::get))]
#[case::response_bytes(64 * 1024 * 1024, |n| ResponseByteLimit::new(n).map(ResponseByteLimit::get))]
#[case::repositories(100_000, |n| RepositoryLimit::new(n).map(RepositoryLimit::get))]
fn bounded_limits_accept_both_edges_and_reject_values_outside(
    #[case] maximum: usize,
    #[case] construct: fn(usize) -> Result<usize, ConfigurationError>,
) {
    assert_eq!(construct(1).unwrap(), 1);
    assert_eq!(construct(maximum).unwrap(), maximum);
    assert!(construct(0).is_err());
    assert!(construct(maximum + 1).is_err());
}

#[rstest]
#[case::minimum(Duration::from_nanos(1), true)]
#[case::maximum(Duration::from_secs(3600), true)]
#[case::zero(Duration::ZERO, false)]
#[case::over_maximum(Duration::from_secs(3600) + Duration::from_nanos(1), false)]
fn request_timeout_enforces_exact_boundaries(#[case] value: Duration, #[case] accepted: bool) {
    let result = RequestTimeout::new(value);
    assert_eq!(result.is_ok(), accepted);
    if let Ok(timeout) = result {
        assert_eq!(timeout.get(), value);
    }
}

#[test]
fn geoapi_bounds_apply_to_direct_parsing_and_deserialization() {
    let maximum_hosts: Vec<Hostname> = (0..128)
        .map(|n| format!("host{n}.example").parse().unwrap())
        .collect();
    assert!(GeoapiHosts::new(maximum_hosts.clone()).is_ok());
    let mut too_many = maximum_hosts;
    too_many.push("extra.example".parse().unwrap());
    assert!(GeoapiHosts::new(too_many.clone()).is_err());
    let data = json!({"hosts": too_many, "response": (1..=129).collect::<Vec<_>>()});
    assert!(serde_json::from_value::<GeoapiOrdering>(data).is_err());
    assert!(GeoapiOrdering::from_response(hosts(), &"1".repeat(16 * 1024 + 1)).is_err());
    // Padding is accepted only while the complete response fits the parser bound.
    let padded = format!("{}1,2,3", " ".repeat(16 * 1024 - 5));
    let ordering = GeoapiOrdering::from_response(hosts(), &padded).unwrap();
    assert_eq!(ordering.hosts(), hosts());
    assert_eq!(
        ordering
            .response()
            .iter()
            .map(|id| id.get())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[rstest]
#[case::odd_length("a")]
#[case::invalid_digit("gg")]
#[case::unicode("é")]
fn general_hex_validation_also_applies_during_deserialization(#[case] text: &str) {
    assert!(HexString::new(text).is_err());
    assert!(serde_json::from_value::<HexString>(json!(text)).is_err());
}
