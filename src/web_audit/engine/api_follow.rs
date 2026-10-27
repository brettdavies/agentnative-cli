//! The API catalog's share of the follow slice, a mirror of the site's
//! `follow-api.ts` over the request gate of `follow-requests.ts` and the
//! refusals of `follow-trail.ts` (`api_trail`).
//!
//! An anchor host off the audited origin is admitted (one of the slice's
//! host slots), which is what lets wave 2 send it the hygiene probes; it
//! gets no request here. A `service-desc` target is read once, under the
//! slice's clock, request cap and host slots, and kept for the OpenAPI row.
//! A description host that gives no response, or whose answer is a
//! redirect the slice does not take (its one redirect hop redirecting
//! again), records unreachable; any other answer records followed. The
//! local engine follows no MCP declaration, so the API batch is the slice's
//! only batch and has its slots and requests to itself.

use std::time::{Duration, Instant};

use url::Url;

use super::api_catalog::{ApiDeclaration, ApiKind};
use super::api_trail::{Outcome, Settled, TrailEntry, declaration_key, host_of, refusal, upfront};
use super::discovery::resolve_url;
use super::discovery_documents::same_origin;
use super::types::FetchHandle;
use super::waves::is_edge_error_status;
use crate::web_audit::fetch::body::OPENAPI_MAX_BODY_BYTES;
use crate::web_audit::fetch::redirect::is_redirect;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::locality::Locality;

/// The follow slice's share of the per-audit deadline.
pub const FOLLOW_SLICE: Duration = Duration::from_millis(6_000);
/// Distinct hosts the slice admits.
pub const MAX_FOLLOWED_HOSTS: usize = 4;
/// Requests the slice sends.
pub const MAX_FOLLOW_REQUESTS: usize = 12;
const DESCRIPTION_TIMEOUT: Duration = Duration::from_millis(3_000);

/// A description the slice read: where it landed and what came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    /// The URL that answered, after at most one redirect hop.
    pub url: String,
    /// Its response.
    pub response: ProbeResponse,
}

/// What the slice settled for the catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiFollowResult {
    /// Trail entries in declaration order: the anchors, then the descriptions.
    pub entries: Vec<TrailEntry>,
    /// The descriptions read, with their declaration keys, in declaration order.
    pub descriptions: Vec<(String, Fetched)>,
}

/// The slice's inputs.
#[derive(Debug)]
pub struct SliceInput<'a> {
    /// The audited origin.
    pub base: &'a str,
    /// The run's transport and resolver.
    pub fetch: &'a FetchHandle,
    /// The target's locality; a declaration of another class is blocked.
    pub target_locality: Option<Locality>,
    /// The per-request timeout.
    pub timeout: Duration,
    /// The per-audit deadline.
    pub deadline: Instant,
}

/// The request gate: host slots, the request cap, and the slice clock.
struct Gate<'a> {
    input: &'a SliceInput<'a>,
    deadline_at: Instant,
    hosts: Vec<String>,
    requests: usize,
}

impl Gate<'_> {
    fn slice(&self) -> Option<Duration> {
        let left = self.deadline_at.saturating_duration_since(Instant::now());
        (!left.is_zero()).then(|| self.input.timeout.min(left))
    }

    fn enter(&mut self, url: &str) -> Result<(), &'static str> {
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

    /// Charges one GET and returns its timeout.
    fn request(&mut self) -> Result<Duration, &'static str> {
        let slice = self.slice().ok_or("slice")?;
        if self.requests >= MAX_FOLLOW_REQUESTS {
            return Err("per-audit-cap");
        }
        self.requests += 1;
        Ok(slice.min(DESCRIPTION_TIMEOUT))
    }

    /// A description's first GET, admitted in declaration order.
    fn first(&mut self, d: &ApiDeclaration) -> Step {
        if let Some(settled) = upfront(d, self.input.fetch, self.input.target_locality) {
            return Step::Settled(settled);
        }
        match self.enter(&d.url).and_then(|()| self.request()) {
            Ok(timeout) => Step::Get {
                url: d.url.clone(),
                timeout,
            },
            Err(cause) => Step::Settled(Settled::budget(cause)),
        }
    }

    /// Where a first answer leads: itself, or the one redirect hop the
    /// guard, the host slots and the request cap admit, as a GET still to
    /// send or the way the hop settled.
    fn hop(&mut self, declared: &str, first: Fetched) -> Result<Fetched, Step> {
        let location = first.response.headers.get("location").map(str::to_string);
        let (Some(location), Some(true)) = (location, first.response.status.map(is_redirect))
        else {
            return Ok(first);
        };
        let hop = resolve_url(declared, &location);
        let refused = if hop.is_empty() {
            Some(Settled::of(Outcome::ReciprocityRefused))
        } else {
            refusal(&hop, self.input.fetch, self.input.target_locality)
        };
        if let Some(refused) = refused {
            let final_url = host_of(&hop).map(|_| hop.clone());
            return Err(Step::Settled(Settled {
                final_url,
                ..refused
            }));
        }
        Err(match self.enter(&hop).and_then(|()| self.request()) {
            Ok(timeout) => Step::Get { url: hop, timeout },
            Err(cause) => Step::Settled(Settled::budget(cause)),
        })
    }
}

/// The admitted GETs, side by side, with redirects off.
fn get_all(fetch: &FetchHandle, jobs: &[(String, Duration)]) -> Vec<ProbeResponse> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(url, timeout)| {
                let opts = FetchOptions {
                    timeout: *timeout,
                    max_body_bytes: Some(OPENAPI_MAX_BODY_BYTES),
                    follow_redirects: false,
                    ..FetchOptions::default()
                };
                scope.spawn(move || fetch.fetch(url, &FetchInit::default(), &opts))
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

enum Step {
    Settled(Settled),
    Get { url: String, timeout: Duration },
}

/// Runs the admitted GETs of one round, in declaration order.
fn run_round(fetch: &FetchHandle, steps: Vec<Step>) -> Vec<Result<Fetched, Settled>> {
    let jobs: Vec<(String, Duration)> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Get { url, timeout } => Some((url.clone(), *timeout)),
            Step::Settled(_) => None,
        })
        .collect();
    let mut responses = get_all(fetch, &jobs).into_iter();
    steps
        .into_iter()
        .map(|s| match s {
            Step::Settled(settled) => Err(settled),
            Step::Get { url, .. } => Ok(Fetched {
                url,
                response: responses.next().expect("one response per GET"),
            }),
        })
        .collect()
}

/// A description's trail outcome from where its read landed.
fn described(read: &Result<Fetched, Settled>, declared: &str) -> Settled {
    let fetched = match read {
        Ok(fetched) => fetched,
        Err(settled) => return settled.clone(),
    };
    let status = fetched.response.status;
    let unread =
        status.is_none() || is_edge_error_status(status) || status.is_some_and(is_redirect);
    Settled {
        final_url: (fetched.url != declared).then(|| fetched.url.clone()),
        ..Settled::of(if unread {
            Outcome::Unreachable
        } else {
            Outcome::Followed
        })
    }
}

/// Settles the catalog's off-origin declarations: the anchors are admitted
/// in declaration order, then the descriptions are read side by side, each
/// first GET and then each redirect hop admitted in declaration order.
pub fn settle_api_declarations(
    input: &SliceInput<'_>,
    declarations: &[ApiDeclaration],
) -> ApiFollowResult {
    let mut seen: Vec<String> = Vec::new();
    let declared: Vec<&ApiDeclaration> = declarations
        .iter()
        .filter(|d| !same_origin(&d.url, input.base))
        .filter(|d| {
            let key = declaration_key(d.kind, &d.url);
            let first = !seen.contains(&key);
            seen.push(key);
            first
        })
        .collect();
    let mut gate = Gate {
        input,
        deadline_at: input.deadline.min(Instant::now() + FOLLOW_SLICE),
        hosts: Vec::new(),
        requests: 0,
    };
    let mut result = ApiFollowResult::default();
    let entry = |d: &ApiDeclaration, settled: Settled| TrailEntry {
        kind: d.kind,
        url: d.url.clone(),
        source: d.source.clone(),
        settled,
    };
    for anchor in declared.iter().filter(|d| d.kind == ApiKind::Anchor) {
        let settled = upfront(anchor, input.fetch, input.target_locality).unwrap_or_else(|| {
            match gate.enter(&anchor.url) {
                Ok(()) => Settled::of(Outcome::Followed),
                Err(cause) => Settled::budget(cause),
            }
        });
        result.entries.push(entry(anchor, settled));
    }

    let wanted: Vec<&ApiDeclaration> = declared
        .into_iter()
        .filter(|d| d.kind == ApiKind::Description)
        .collect();
    let firsts: Vec<Step> = wanted.iter().map(|d| gate.first(d)).collect();
    let mut landings: Vec<Option<Result<Fetched, Settled>>> = Vec::new();
    let mut hops: Vec<Step> = Vec::new();
    for (d, first) in wanted.iter().zip(run_round(input.fetch, firsts)) {
        match first.map(|fetched| gate.hop(&d.url, fetched)) {
            Ok(Err(hop)) => {
                landings.push(None);
                hops.push(hop);
            }
            Ok(Ok(fetched)) => landings.push(Some(Ok(fetched))),
            Err(settled) => landings.push(Some(Err(settled))),
        }
    }
    let mut hop_reads = run_round(input.fetch, hops).into_iter();
    for (d, landing) in wanted.iter().zip(landings) {
        let read = landing.unwrap_or_else(|| hop_reads.next().expect("one read per hop"));
        let settled = described(&read, &d.url);
        if let Ok(fetched) = read
            && settled.outcome == Outcome::Followed
        {
            result
                .descriptions
                .push((declaration_key(d.kind, &d.url), fetched));
        }
        result.entries.push(entry(d, settled));
    }
    result
}
