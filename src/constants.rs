use crate::Hostname;
use std::sync::LazyLock;

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
