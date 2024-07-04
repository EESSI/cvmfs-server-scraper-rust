use lazy_static::lazy_static;

use crate::models::Hostname;

lazy_static! {
    pub static ref DEFAULT_GEOAPI_SERVERS: Vec<Hostname> = vec![
        "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
        "cvmfs-stratum-one.cern.ch".parse().unwrap(),
        "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
    ];
}
