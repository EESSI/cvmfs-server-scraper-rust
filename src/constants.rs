use crate::Hostname;
use std::sync::LazyLock;

/// Default ordered host list sent to GeoAPI probes: FNAL, CERN, and IHEP.
/// These names are probe inputs, not a list of servers the scraper automatically
/// visits. A response contains a permutation of their one-based positions.
pub static DEFAULT_GEOAPI_SERVERS: LazyLock<Vec<Hostname>> = LazyLock::new(|| {
    [
        "cvmfs-s1fnal.opensciencegrid.org",
        "cvmfs-stratum-one.cern.ch",
        "cvmfs-stratum-one.ihep.ac.cn",
    ]
    .into_iter()
    .map(|host| host.parse().expect("constant hostname is valid"))
    .collect()
});
