use log::warn;
use serde::{Deserialize, Serialize};

use crate::errors::ScrapeError;
use crate::Hostname;

/// A query to the GeoAPI endpoints of the host.
///
/// GeoAPI endpoints in CVMFS lie under each repository, but the repository
/// is irrelevant to the functionality of the endpoint. As such, we ignore
/// the repository structure.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct GeoapiServerQuery {
    pub hostname: Hostname,
    pub geoapi_hosts: Vec<Hostname>,
    pub response: Vec<u32>,
}

impl GeoapiServerQuery {
    pub fn display(&self) {
        println!(
            "Geoapi Hosts: {} -> {:?}",
            self.geoapi_hosts
                .iter()
                .enumerate()
                .map(|(i, x)| format!("[{}] {}", i + 1, x))
                .collect::<Vec<String>>()
                .join(", "),
            self.response
        )
    }

    pub fn check_against_expected_order_by_id(&self, expected_order: Vec<u32>) -> bool {
        if self.response != expected_order {
            return false;
        }
        true
    }

    pub fn check_against_expected_order_by_hostname(
        &self,
        expected_order: Vec<Hostname>,
    ) -> Result<bool, ScrapeError> {
        // Check that the hostnames we are checking against are the same that are in geoapi_hosts
        let geoapi_hosts_not_checked = self
            .geoapi_hosts
            .iter()
            .filter(|x| !expected_order.contains(x))
            .collect::<Vec<&Hostname>>();
        let target_hosts_not_in_geoapi_response = expected_order
            .iter()
            .filter(|x| !self.geoapi_hosts.contains(x))
            .collect::<Vec<&Hostname>>();

        if !geoapi_hosts_not_checked.is_empty() {
            warn!(
                "GeoAPI: Host missing from expected_order: {:?} ",
                geoapi_hosts_not_checked
            );
        }

        if !target_hosts_not_in_geoapi_response.is_empty() {
            warn!(
                "GeoAPI: Host nn expected_order but not in geoapi_hosts: {:?} ",
                target_hosts_not_in_geoapi_response
            );
        }

        if !geoapi_hosts_not_checked.is_empty() || !target_hosts_not_in_geoapi_response.is_empty() {
            return Ok(false);
        }

        let response_order = self.map_response_order_to_geoapi_hostnames()?;
        Ok(response_order == expected_order)
    }

    fn map_order_to_geoapi_hostname(&self, order: Vec<u32>) -> Vec<Hostname> {
        order
            .iter()
            .map(|x| self.geoapi_hosts[*x as usize].clone())
            .collect()
    }

    pub fn map_response_order_to_geoapi_hostnames(&self) -> Result<Vec<Hostname>, ScrapeError> {
        if self.response.len() != self.geoapi_hosts.len() {
            return Err(ScrapeError::GeoAPIFailure(format!(
                "GeoAPI response count mismatch for repository {}: expected {}, got {}",
                self.hostname,
                self.geoapi_hosts.len(),
                self.response.len()
            )));
        }

        Ok(self.map_order_to_geoapi_hostname(self.response.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yare::parameterized;

    fn create_geoapi_server_query() -> GeoapiServerQuery {
        GeoapiServerQuery {
            hostname: "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
            geoapi_hosts: vec![
                "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
                "cvmfs-stratum-one.cern.ch".parse().unwrap(),
                "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
            ],
            response: vec![0, 1, 2],
        }
    }

    #[parameterized(
        failing_120 = { vec![1,2,0] },        
        failing_102 = { vec![1,0,2] },
        failing_021 = { vec![0,2,1] },
        failing_201 = { vec![2,0,1] },
        failing_210 = { vec![2,1,0] },
        failing_empty_response = { vec![] },
        failing_not_enough_response = { vec![0,1] },
        failing_too_many_responses = { vec![0,1,2,3] } 
    )]
    fn test_check_against_expected_order_by_id_failure(res: Vec<u32>) {
        let geoapi = create_geoapi_server_query();

        assert!(!geoapi.check_against_expected_order_by_id(res));
    }

    #[test]
    fn test_check_against_expected_order_by_id_ok() {
        let geoapi = create_geoapi_server_query();
        assert!(geoapi.check_against_expected_order_by_id(vec![0, 1, 2]));
    }

    #[test]
    fn test_check_against_expected_order_by_hostname_ok() {
        let geoapi = create_geoapi_server_query();
        assert!(geoapi
            .check_against_expected_order_by_hostname(vec![
                "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
                "cvmfs-stratum-one.cern.ch".parse().unwrap(),
                "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap()
            ])
            .unwrap());
    }

    #[parameterized(
        failing_missing_host = { vec![
            "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
            "cvmfs-stratum-one.cern.ch".parse().unwrap(),
        ] },
        failing_extra_host = { vec![
            "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
            "cvmfs-stratum-one.cern.ch".parse().unwrap(),
            "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
            "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
        ] },
        failing_wrong_order = { vec![
            "cvmfs-stratum-one.cern.ch".parse().unwrap(),
            "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
            "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
        ] },
        failing_empty_response = { vec![] },
        failing_not_enough_response = { vec![
            "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
        ] },
        failing_too_many_responses = { vec![
            "cvmfs-s1fnal.opensciencegrid.org".parse().unwrap(),
            "cvmfs-stratum-one.cern.ch".parse().unwrap(),
            "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(),
            "cvmfs-stratum-one.ihep.ac.cn".parse().unwrap(), 
        ]})]
        fn test_check_against_expected_order_by_hostname_failure(res: Vec<Hostname>) {
            let geoapi = create_geoapi_server_query();
            assert!(!geoapi.check_against_expected_order_by_hostname(res).unwrap());
        }
    
}
