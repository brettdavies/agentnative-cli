//! The contracts between the engine and its probe handlers, a mirror of the
//! site's `handlers/types.ts`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::web_audit::fetch::{FetchInit, FetchOptions, Fetcher, ProbeResponse};
use crate::web_audit::locality::{ClassifyError, Locality, Resolver, classify_host};
use crate::web_audit::scorecard::{EvidenceItem, NaReason, ScorecardStatus};
use crate::web_audit::transport::Transport;

/// A handler's verdict before the engine finalizes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeStatus {
    /// The surface is present and valid.
    Pass,
    /// The surface works while violating a spec detail.
    Noncompliant,
    /// The surface is present but invalid.
    Broken,
    /// The surface is not there.
    Absent,
    /// The handler had nothing to probe, or a posture rules the row out.
    Na,
    /// An operational failure: transport error or timeout.
    Error,
}

impl ProbeStatus {
    /// The scorecard status this verdict becomes.
    pub fn to_scorecard(self) -> ScorecardStatus {
        match self {
            ProbeStatus::Pass => ScorecardStatus::Pass,
            ProbeStatus::Noncompliant => ScorecardStatus::Noncompliant,
            ProbeStatus::Broken => ScorecardStatus::Broken,
            ProbeStatus::Absent => ScorecardStatus::Absent,
            ProbeStatus::Na => ScorecardStatus::NA,
            ProbeStatus::Error => ScorecardStatus::Error,
        }
    }
}

/// What a handler returns for one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// The verdict.
    pub status: ProbeStatus,
    /// Structured evidence rows, first row leading.
    pub evidence: Vec<EvidenceItem>,
    /// A handler-stated reason for an `Na` verdict.
    pub na_reason: Option<NaReason>,
    /// The row settled without a request, from an antecedent the run observed.
    pub unprobed: bool,
    /// The handler exhausted the remaining budget mid-probe.
    pub incomplete: bool,
}

impl ProbeOutcome {
    /// A verdict with its evidence and nothing else set.
    pub fn new(status: ProbeStatus, evidence: Vec<EvidenceItem>) -> Self {
        ProbeOutcome {
            status,
            evidence,
            na_reason: None,
            unprobed: false,
            incomplete: false,
        }
    }

    /// The `na` verdict with one `why` line and no reason.
    pub fn na(why: &str) -> Self {
        ProbeOutcome::new(ProbeStatus::Na, vec![why_item(why)])
    }

    /// The `error` verdict with one `why` line.
    pub fn error(why: &str) -> Self {
        ProbeOutcome::new(ProbeStatus::Error, vec![why_item(why)])
    }
}

/// An evidence row carrying only a `why` list.
pub fn why_item(why: &str) -> EvidenceItem {
    let mut item = EvidenceItem::new();
    item.insert(
        "why".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String(why.to_string())]),
    );
    item
}

/// Whether the target serves the modern MCP era, read from the wave-1
/// `server/discover` probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpModernLane {
    /// `server/discover` answered with a JSON-RPC result.
    Present,
    /// It answered some other way.
    Unevidenced,
    /// It never got an answer.
    Unknown,
}

/// Era-lane facts the wave-1 MCP probes establish for the wave-2 rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpLaneEvidence {
    /// The modern lane's presence.
    pub modern: McpModernLane,
    /// Capability groups the legacy `initialize` result advertised.
    pub legacy_advertised: Vec<String>,
    /// Capability groups the modern `server/discover` result advertised.
    pub modern_advertised: Vec<String>,
}

impl Default for McpLaneEvidence {
    fn default() -> Self {
        McpLaneEvidence {
            modern: McpModernLane::Unknown,
            legacy_advertised: Vec::new(),
            modern_advertised: Vec::new(),
        }
    }
}

/// The transport, resolver and identity every probe in a run shares.
#[derive(Clone)]
pub struct FetchHandle {
    transport: Arc<dyn Transport + Send + Sync>,
    resolver: Arc<dyn Resolver + Send + Sync>,
    user_agent: Arc<str>,
}

impl std::fmt::Debug for FetchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchHandle")
            .field("user_agent", &self.user_agent)
            .finish_non_exhaustive()
    }
}

impl FetchHandle {
    /// Bind a run to its transport, resolver and User-Agent.
    pub fn new(
        transport: Arc<dyn Transport + Send + Sync>,
        resolver: Arc<dyn Resolver + Send + Sync>,
        user_agent: &str,
    ) -> Self {
        FetchHandle {
            transport,
            resolver,
            user_agent: Arc::from(user_agent),
        }
    }

    /// Fetch through the guard. Never fails; failures come back as a
    /// response with `status: None`.
    pub fn fetch(&self, url: &str, init: &FetchInit, opts: &FetchOptions) -> ProbeResponse {
        Fetcher::new(&*self.transport, &*self.resolver, &self.user_agent).fetch(url, init, opts)
    }

    /// Classify a host with the run's resolver.
    pub fn classify(&self, host: &str) -> Result<Locality, ClassifyError> {
        classify_host(host, &*self.resolver)
    }

    /// Validate a URL the way the guard validates a target, without
    /// fetching it: the class it would connect to, or the `blocked:` reason.
    pub fn validate(&self, url: &str) -> Result<Locality, String> {
        crate::web_audit::fetch::redirect::validate_target(url, &*self.resolver).map(|v| v.class)
    }
}

/// Everything a handler sees for one check.
#[derive(Clone, Debug)]
pub struct HandlerContext {
    /// Normalized base URL: scheme, host and a trailing slash.
    pub base: String,
    /// Target hostname, for `{host}` substitution.
    pub host: String,
    /// Discovered MCP endpoint, if any.
    pub mcp_endpoint: Option<String>,
    /// The legacy protocol version the registry pins.
    pub protocol_version: &'static str,
    /// Per-request timeout when a check sets none, already bounded by the budget.
    pub default_timeout: Duration,
    /// The single canonical root fetch, shared by every root-reading check.
    pub root: Option<Arc<ProbeResponse>>,
    /// Section directories for the scoped llms.txt probes.
    pub scoped_dirs: Arc<Vec<String>>,
    /// Wave-1 bodies keyed by check id.
    pub retained_bodies: Arc<HashMap<String, String>>,
    /// The run's transport, resolver and identity.
    pub fetch: FetchHandle,
    /// Session id from wave-1 `initialize`, when the server issued one.
    pub mcp_session_id: Option<String>,
    /// Era-lane evidence from the wave-1 MCP probes.
    pub mcp_lanes: McpLaneEvidence,
    /// The per-audit deadline.
    pub deadline: Instant,
    /// Whether checks that query external DNS resolvers may run.
    pub external_dns: bool,
    /// The target's locality class, when it could be classified; nested
    /// probes must stay within it.
    pub target_locality: Option<Locality>,
}

impl HandlerContext {
    /// Budget left before the per-audit deadline, never below one millisecond.
    pub fn remaining(&self) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
    }

    /// A check's `with.timeout` (seconds) or the default, bounded by the budget.
    pub fn timeout_for(&self, check_timeout_seconds: Option<f64>) -> Duration {
        let wanted = match check_timeout_seconds {
            Some(secs) => Duration::from_millis((secs * 1000.0).round() as u64),
            None => self.default_timeout,
        };
        wanted.min(self.remaining())
    }

    /// The default fetch options for this check.
    pub fn fetch_options(&self, check_timeout_seconds: Option<f64>) -> FetchOptions {
        FetchOptions {
            timeout: self.timeout_for(check_timeout_seconds),
            ..FetchOptions::default()
        }
    }

    /// Validate a URL a handler found in the target's own documents. A
    /// class-crossing candidate is refused with a `blocked:` reason: a
    /// local run never egresses to a public host it was not pointed at,
    /// and a public run never probes the auditor's private network.
    pub fn validate_nested(&self, url: &str) -> Result<(), String> {
        let class = self.fetch.validate(url)?;
        match self.target_locality {
            Some(target) if class != target => {
                let host = url::Url::parse(url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
                    .unwrap_or_default();
                let reason =
                    crate::web_audit::locality::site_block_reason(&host).unwrap_or_else(|| {
                        format!("{host} is a {class} host outside the {target} target")
                    });
                Err(format!("blocked: {reason}"))
            }
            _ => Ok(()),
        }
    }
}
