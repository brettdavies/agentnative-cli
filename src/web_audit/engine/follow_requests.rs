//! The follow slice's request gate, a mirror of the site's
//! `follow-requests.ts`; the declaration-level reads over it, with their
//! one redirect hop, live in `follow_hops`. Every GET the slice sends to a declared host is
//! admitted here before it starts: the host takes one of the slice's host
//! slots, and the request counts toward the slice's request cap and clock.
//! A declared URL may take one redirect hop, which the guard validates and
//! the slice charges the same way; a second hop is never taken. Requests
//! run with redirects disabled, so nothing reaches a host this gate did not
//! admit. A local run shares no budget with other audits, so no domain
//! reservation is drawn.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use url::Url;

use super::follow_trail::Settled;
use super::types::FetchHandle;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::locality::Locality;

/// The follow slice's share of the per-audit deadline.
pub const FOLLOW_SLICE: Duration = Duration::from_millis(6_000);
/// Distinct hosts the slice admits.
pub const MAX_FOLLOWED_HOSTS: usize = 4;
/// Requests the slice sends.
pub const MAX_FOLLOW_REQUESTS: usize = 12;

/// What stopped a request before it started.
pub type Cause = &'static str;

/// How one GET reads its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    /// The body cap.
    pub max_body_bytes: usize,
    /// The `Accept` sent, when one is.
    pub accept: Option<&'static str>,
    /// A cap on the GET's timeout below the slice's.
    pub timeout_cap: Option<Duration>,
}

/// Where a declared URL landed after at most one redirect hop, and its response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    /// The URL that answered.
    pub url: String,
    /// Its response.
    pub response: ProbeResponse,
}

/// The slice's inputs the gate reads.
#[derive(Clone, Copy, Debug)]
pub struct SliceClock {
    /// The per-request timeout.
    pub timeout: Duration,
    /// When the slice's share of the deadline runs out.
    pub deadline_at: Instant,
}

/// A GET the gate admitted, still to send.
#[derive(Clone, Debug)]
pub(super) struct Admitted {
    url: String,
    key: String,
    opts: ReadOptions,
    timeout: Duration,
}

/// One declaration's next step: settled, or a GET to send.
pub(super) enum Step {
    Settled(Settled),
    Get(Admitted),
    Cached(String, Result<ProbeResponse, Cause>),
}

/// The request gate: host slots, the request cap, the slice clock, and the
/// answers already read.
pub struct Gate<'a> {
    pub(super) fetch: &'a FetchHandle,
    pub(super) target_locality: Option<Locality>,
    clock: SliceClock,
    hosts: Vec<String>,
    requests: usize,
    cache: HashMap<String, Result<ProbeResponse, Cause>>,
}

impl std::fmt::Debug for Gate<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gate")
            .field("hosts", &self.hosts)
            .field("requests", &self.requests)
            .finish_non_exhaustive()
    }
}

impl<'a> Gate<'a> {
    /// A gate over a slice clock.
    pub fn new(
        fetch: &'a FetchHandle,
        target_locality: Option<Locality>,
        clock: SliceClock,
    ) -> Self {
        Gate {
            fetch,
            target_locality,
            clock,
            hosts: Vec::new(),
            requests: 0,
            cache: HashMap::new(),
        }
    }

    fn slice(&self) -> Option<Duration> {
        let left = self
            .clock
            .deadline_at
            .saturating_duration_since(Instant::now());
        (!left.is_zero()).then(|| self.clock.timeout.min(left))
    }

    /// Admits a host to the slice, or names the cause that refuses it.
    pub fn enter(&mut self, url: &str) -> Result<(), Cause> {
        self.slice().ok_or("slice")?;
        let host = Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        if !self.hosts.contains(&host) {
            if self.hosts.len() >= MAX_FOLLOWED_HOSTS {
                return Err("per-audit-cap");
            }
            self.hosts.push(host);
        }
        Ok(())
    }

    /// Charges one GET to the cap and clock, unless the same read is on
    /// record: a document read the same way twice is requested once.
    pub(super) fn admit(&mut self, url: &str, opts: ReadOptions) -> Step {
        let key = format!(
            "{url} {} {}",
            opts.max_body_bytes,
            opts.accept.unwrap_or("")
        );
        if let Some(read) = self.cache.get(&key) {
            return Step::Cached(url.to_string(), read.clone());
        }
        let charged = self.slice().ok_or("slice").and_then(|slice| {
            if self.requests >= MAX_FOLLOW_REQUESTS {
                return Err("per-audit-cap");
            }
            self.requests += 1;
            Ok(slice.min(opts.timeout_cap.unwrap_or(slice)))
        });
        match charged {
            Ok(timeout) => Step::Get(Admitted {
                url: url.to_string(),
                key,
                opts,
                timeout,
            }),
            Err(cause) => {
                self.cache.insert(key, Err(cause));
                Step::Settled(Settled::budget(cause))
            }
        }
    }

    /// Sends the admitted GETs side by side and keeps their answers.
    pub(super) fn send(&mut self, steps: Vec<Step>) -> Vec<Result<Fetched, Settled>> {
        let jobs: Vec<&Admitted> = steps
            .iter()
            .filter_map(|s| match s {
                Step::Get(admitted) => Some(admitted),
                _ => None,
            })
            .collect();
        let mut responses = get_all(self.fetch, &jobs).into_iter();
        steps
            .into_iter()
            .map(|step| match step {
                Step::Settled(settled) => Err(settled),
                Step::Cached(url, Ok(response)) => Ok(Fetched { url, response }),
                Step::Cached(_, Err(cause)) => Err(Settled::budget(cause)),
                Step::Get(admitted) => {
                    let response = responses.next().expect("one response per GET");
                    self.cache.insert(admitted.key, Ok(response.clone()));
                    Ok(Fetched {
                        url: admitted.url,
                        response,
                    })
                }
            })
            .collect()
    }

    /// One GET through the gate.
    pub fn get(&mut self, url: &str, opts: ReadOptions) -> Result<ProbeResponse, Cause> {
        let step = self.admit(url, opts);
        match self.send(vec![step]).pop().expect("one read") {
            Ok(fetched) => Ok(fetched.response),
            Err(settled) => Err(settled.cause.unwrap_or("slice")),
        }
    }
}

/// The admitted GETs, side by side, with redirects off.
fn get_all(fetch: &FetchHandle, jobs: &[&Admitted]) -> Vec<ProbeResponse> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|job| {
                let opts = FetchOptions {
                    timeout: job.timeout,
                    max_body_bytes: Some(job.opts.max_body_bytes),
                    follow_redirects: false,
                    ..FetchOptions::default()
                };
                let init = FetchInit {
                    headers: job
                        .opts
                        .accept
                        .map(|accept| vec![("accept".to_string(), accept.to_string())])
                        .unwrap_or_default(),
                    ..FetchInit::default()
                };
                let url = job.url.clone();
                scope.spawn(move || fetch.fetch(&url, &init, &opts))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| ProbeResponse::failed("panic".into(), 0))
            })
            .collect()
    })
}
