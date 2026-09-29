use crate::{Hostname, ScrapeError, ServerEndpoint};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// One-based GeoAPI wire index. The ordering validates its upper bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GeoapiHostId(NonZeroUsize);
impl GeoapiHostId {
    pub fn new(value: usize) -> Result<Self, ScrapeError> {
        NonZeroUsize::new(value).map(Self).ok_or_else(|| {
            ScrapeError::GeoAPIFailure("host IDs are one-based; 0 is invalid".into())
        })
    }
    pub fn get(self) -> usize {
        self.0.get()
    }
}

/// Validated permutation bound to its immutable query host list.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(try_from = "RawOrdering")]
pub struct GeoapiOrdering {
    hosts: Vec<Hostname>,
    response: Vec<GeoapiHostId>,
}
#[derive(Deserialize)]
struct RawOrdering {
    hosts: Vec<Hostname>,
    response: Vec<GeoapiHostId>,
}
impl TryFrom<RawOrdering> for GeoapiOrdering {
    type Error = ScrapeError;
    fn try_from(raw: RawOrdering) -> Result<Self, Self::Error> {
        Self::new(raw.hosts, raw.response)
    }
}
impl GeoapiOrdering {
    pub fn new(hosts: Vec<Hostname>, response: Vec<GeoapiHostId>) -> Result<Self, ScrapeError> {
        crate::GeoapiHosts::new(hosts.clone())?;
        if hosts.len() != response.len() {
            return Err(ScrapeError::GeoAPIFailure(format!(
                "expected {} nonempty host IDs, received {}",
                hosts.len(),
                response.len()
            )));
        }
        let mut seen = vec![false; hosts.len()];
        for id in &response {
            let index = id.get() - 1;
            let entry = seen.get_mut(index).ok_or_else(|| {
                ScrapeError::GeoAPIFailure(format!(
                    "host ID {} exceeds host count {}",
                    id.get(),
                    hosts.len()
                ))
            })?;
            if *entry {
                return Err(ScrapeError::GeoAPIFailure(format!(
                    "duplicate host ID {}",
                    id.get()
                )));
            }
            *entry = true;
        }
        Ok(Self { hosts, response })
    }
    pub fn from_response(hosts: Vec<Hostname>, text: &str) -> Result<Self, ScrapeError> {
        if text.len() > 16 * 1024 {
            return Err(ScrapeError::GeoAPIFailure(
                "response exceeds 16384 bytes".into(),
            ));
        }
        let response = text
            .trim()
            .split(',')
            .map(|s| {
                let id = s.trim().parse::<usize>().map_err(|_| {
                    ScrapeError::GeoAPIFailure(format!(
                        "invalid host ID {s:?}; expected a positive integer"
                    ))
                })?;
                GeoapiHostId::new(id)
            })
            .collect::<Result<_, _>>()?;
        Self::new(hosts, response)
    }
    pub fn hosts(&self) -> &[Hostname] {
        &self.hosts
    }
    pub fn response(&self) -> &[GeoapiHostId] {
        &self.response
    }
    pub fn ordered_hosts(&self) -> impl Iterator<Item = &Hostname> {
        self.response.iter().map(|id| &self.hosts[id.get() - 1])
    }
}

/// A completed query to a server's GeoAPI, bound to the queried host list.
///
/// GeoAPI URLs sit below a repository, but the response orders hosts rather than
/// repository contents. The scraper uses the first successful repository's endpoint
/// and retains the server origin here. Wire IDs are one-based; [`GeoapiOrdering`]
/// validates that they form a complete permutation before exposing ordered hosts.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct GeoapiServerQuery {
    endpoint: ServerEndpoint,
    ordering: GeoapiOrdering,
}
impl GeoapiServerQuery {
    pub(crate) fn new(endpoint: ServerEndpoint, ordering: GeoapiOrdering) -> Self {
        Self { endpoint, ordering }
    }
    pub fn endpoint(&self) -> &ServerEndpoint {
        &self.endpoint
    }
    pub fn ordering(&self) -> &GeoapiOrdering {
        &self.ordering
    }
    pub fn output(&self) {
        println!("{self:#?}");
    }
    pub fn check_against_expected_order_by_id(&self, expected: &[GeoapiHostId]) -> bool {
        self.ordering.response() == expected
    }
    pub fn check_against_expected_order_by_hostname(&self, expected: &[Hostname]) -> bool {
        self.ordering.ordered_hosts().eq(expected.iter())
    }
    pub fn map_response_order_to_geoapi_hostnames(&self) -> Vec<Hostname> {
        self.ordering.ordered_hosts().cloned().collect()
    }
}

/// Outcome of the optional GeoAPI probe, independent of repository success.
#[derive(Debug, Clone)]
pub enum GeoapiOutcome {
    /// A valid ordering bound to the exact hosts sent in the query.
    Available(GeoapiServerQuery),
    /// The resolved backend uses S3, which this scraper does not probe for GeoAPI.
    Unsupported,
    /// No request was made for the given reason.
    Skipped(GeoapiSkipReason),
    /// The request, decoding, or permutation validation failed.
    Failed(ScrapeError),
}
/// Reason a GeoAPI request was not attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoapiSkipReason {
    /// Disabled explicitly in configuration.
    Disabled,
    /// The configured server is a Stratum0.
    Stratum0,
    /// No selected repository provides a path under which to query GeoAPI.
    NoRepositories,
}
