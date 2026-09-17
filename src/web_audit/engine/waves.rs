//! The run: root fetch, discovery, the reachability gate, wave 1 over the
//! antecedent-source checks, the antecedent context, wave 2 over the rest,
//! and scorecard assembly. A mirror of the site's `runWebAudit`.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use url::Url;

use super::antecedents::{self, AntecedentContext, Resolution};
use super::discovery::{DiscoveryResult, discover_mcp_endpoint};
use super::handlers::{Dispatch, HandlerSet};
use super::mcp_wire::{
    advertised_capabilities, modern_lane_from, notify_mcp_initialized, session_id_from,
};
use super::pool::run_bounded;
use super::retained::enumerate_scoped_dirs;
use super::summary::summarize_evidence;
use super::types::{FetchHandle, HandlerContext, McpLaneEvidence, ProbeOutcome, why_item};
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::locality::Locality;
use crate::web_audit::registry::{
    CATEGORIES, CATEGORY_ORDER, CHECKS, HandlerBinding, MCP_DISCOVERY, WebCheck, WebCheckKeyword,
};
use crate::web_audit::score::ScoreConfig;
use crate::web_audit::scorecard::{
    DeclaredSiteType, EngineResult, EvidenceItem, NaReason, ScorecardMeta, ScorecardStatus,
    WebScorecard, build_web_scorecard,
};

/// Worker threads per wave.
pub const DEFAULT_CONCURRENCY: usize = 6;
/// Per-request timeout when the root answered.
pub const DEFAULT_PER_CHECK_TIMEOUT: Duration = Duration::from_millis(8_000);
/// Wall-clock budget for the whole run.
pub const DEFAULT_PER_AUDIT_DEADLINE: Duration = Duration::from_millis(25_000);
/// Per-request timeout once the root fetch has failed at the network level.
pub const DEGRADED_PER_CHECK_TIMEOUT: Duration = Duration::from_millis(2_000);

/// The evidence line on a DNS row the locality gate withheld.
pub const EXTERNAL_DNS_WITHHELD: &str = "needs public DNS: verified on anc.dev";

/// Receives results as the run finalizes them.
pub trait ProgressSink {
    /// Discovery finished.
    fn discovery(&mut self, _endpoint: Option<&str>) {}
    /// One row finalized.
    fn result(&mut self, _result: &EngineResult) {}
}

/// Everything a run needs.
pub struct RunInput<'a> {
    /// The target URL.
    pub url: String,
    /// The declared site type; `None` runs everything.
    pub site_type: Option<DeclaredSiteType>,
    /// The spec version to record on the scorecard.
    pub spec_version: String,
    /// Worker threads per wave.
    pub concurrency: usize,
    /// Per-request timeout while the root answered.
    pub per_check_timeout: Duration,
    /// Wall-clock budget for the whole run.
    pub per_audit_deadline: Duration,
    /// Let checks that query external DNS resolvers run against a local target.
    pub external_dns: bool,
    /// The run's transport, resolver and identity.
    pub fetch: FetchHandle,
    /// The probe handlers.
    pub handlers: Arc<HandlerSet>,
    /// Where finalized rows go as they land.
    pub progress: Option<&'a mut dyn ProgressSink>,
}

impl<'a> RunInput<'a> {
    /// A run with the site's defaults.
    pub fn new(url: &str, fetch: FetchHandle, handlers: Arc<HandlerSet>) -> Self {
        RunInput {
            url: url.to_string(),
            site_type: None,
            spec_version: String::new(),
            concurrency: DEFAULT_CONCURRENCY,
            per_check_timeout: DEFAULT_PER_CHECK_TIMEOUT,
            per_audit_deadline: DEFAULT_PER_AUDIT_DEADLINE,
            external_dns: false,
            fetch,
            handlers,
            progress: None,
        }
    }
}

/// A check whose handler kind or eval rule the engine has not ported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unported {
    /// The check id.
    pub check_id: &'static str,
    /// The unported kind or rule.
    pub kind: &'static str,
}

/// A completed run.
#[derive(Debug)]
pub struct RunReport {
    /// The scorecard.
    pub scorecard: WebScorecard,
    /// Every finalized row, in registry order.
    pub results: Vec<EngineResult>,
    /// The run finished inside its budget with no handler cut short.
    pub complete: bool,
    /// Rows skipped because their handler is not ported.
    pub unported: Vec<Unported>,
    /// The target's locality class, when it could be classified.
    pub target_locality: Option<Locality>,
    /// The discovery trail.
    pub discovery: DiscoveryResult,
}

/// How a run ended.
#[derive(Debug)]
pub enum RunOutcome {
    /// Nothing from the target answered, so there is nothing to score.
    Unreachable {
        /// The site's wording for why.
        reason: String,
        /// The discovery trail.
        discovery: Vec<EvidenceItem>,
    },
    /// The run scored.
    Complete(Box<RunReport>),
}

struct Base {
    base: String,
    host: String,
    domain: String,
}

fn normalize_base(raw: &str) -> Option<Base> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_string();
    let domain = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    Some(Base {
        base: format!("{}://{domain}/", url.scheme()),
        host,
        domain,
    })
}

// Cloudflare answers on the origin's behalf with these when the origin never
// spoke: 52x for connection and timeout failures, 530 when the host does not
// resolve. They carry the auditor's edge, not the target.
fn is_edge_error_status(status: Option<u16>) -> bool {
    matches!(status, Some(530) | Some(520..=527))
}

fn to_result(check: &'static WebCheck, outcome: ProbeOutcome) -> EngineResult {
    EngineResult {
        check,
        status: outcome.status.to_scorecard(),
        na_reason: outcome.na_reason,
        unprobed: outcome.unprobed,
        evidence: summarize_evidence(check, &outcome),
        raw_evidence: outcome.evidence,
    }
}

fn na_result(check: &'static WebCheck, reason: NaReason, evidence: &str) -> EngineResult {
    EngineResult {
        check,
        status: ScorecardStatus::NA,
        na_reason: Some(reason),
        unprobed: false,
        evidence: evidence.to_string(),
        raw_evidence: vec![why_item(evidence)],
    }
}

fn error_result(check: &'static WebCheck, message: &str) -> EngineResult {
    let mut item = EvidenceItem::new();
    item.insert(
        "error".to_string(),
        serde_json::Value::String(message.to_string()),
    );
    EngineResult {
        check,
        status: ScorecardStatus::Error,
        na_reason: None,
        unprobed: false,
        evidence: message.to_string(),
        raw_evidence: vec![item],
    }
}

fn skip_result(check: &'static WebCheck, why: &str, evidence: &str) -> EngineResult {
    EngineResult {
        check,
        status: ScorecardStatus::Skip,
        na_reason: None,
        unprobed: false,
        evidence: evidence.to_string(),
        raw_evidence: vec![why_item(why)],
    }
}

fn deadline_skip(check: &'static WebCheck) -> EngineResult {
    skip_result(
        check,
        "per-audit deadline exceeded",
        "skipped: per-audit deadline exceeded",
    )
}

fn unported_skip(check: &'static WebCheck, kind: &str) -> EngineResult {
    let why = format!("handler `{kind}` not yet ported to the local engine");
    skip_result(check, &why, &why)
}

/// An applicable MAY that is simply absent is optional, not a miss.
fn finalize_optional(check: &WebCheck, mut result: EngineResult) -> EngineResult {
    if check.keyword == WebCheckKeyword::May && result.status == ScorecardStatus::Absent {
        result.status = ScorecardStatus::NA;
        result.na_reason = Some(NaReason::OptionalAbsent);
    }
    result
}

struct Probed {
    check: &'static WebCheck,
    outcome: Option<ProbeOutcome>,
    result: EngineResult,
    incomplete: bool,
    unported: Option<&'static str>,
}

fn probe_one(
    check: &'static WebCheck,
    ctx: &HandlerContext,
    handlers: &HandlerSet,
    deadline: Instant,
    local_target: bool,
) -> Probed {
    let done = |result: EngineResult, incomplete: bool| Probed {
        check,
        outcome: None,
        result,
        incomplete,
        unported: None,
    };
    if Instant::now() >= deadline {
        return done(deadline_skip(check), true);
    }
    if check.handler == HandlerBinding::DnsDoh && local_target && !ctx.external_dns {
        return done(
            EngineResult {
                check,
                status: ScorecardStatus::NA,
                na_reason: None,
                unprobed: false,
                evidence: EXTERNAL_DNS_WITHHELD.to_string(),
                raw_evidence: vec![why_item(EXTERNAL_DNS_WITHHELD)],
            },
            false,
        );
    }
    let dispatched = catch_unwind(AssertUnwindSafe(|| handlers.dispatch(check, ctx)));
    match dispatched {
        Ok(Dispatch::Outcome(outcome)) => {
            let incomplete = outcome.incomplete || Instant::now() >= deadline;
            Probed {
                check,
                result: to_result(check, outcome.clone()),
                outcome: Some(outcome),
                incomplete,
                unported: None,
            }
        }
        Ok(Dispatch::Unported(kind)) => Probed {
            check,
            outcome: None,
            result: unported_skip(check, kind),
            incomplete: false,
            unported: Some(kind),
        },
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "handler panicked".to_string());
            done(
                error_result(check, &format!("handler error: {message}")),
                false,
            )
        }
    }
}

fn run_wave(
    checks: Vec<&'static WebCheck>,
    ctx: &HandlerContext,
    handlers: &Arc<HandlerSet>,
    concurrency: usize,
    deadline: Instant,
    local_target: bool,
) -> Vec<Probed> {
    let job_ctx = ctx.clone();
    let job_handlers = Arc::clone(handlers);
    let returned = run_bounded(checks.clone(), concurrency, deadline, move |check| {
        probe_one(check, &job_ctx, &job_handlers, deadline, local_target)
    });
    checks
        .into_iter()
        .zip(returned)
        .map(|(check, probed)| {
            probed.unwrap_or_else(|| Probed {
                check,
                outcome: None,
                result: deadline_skip(check),
                incomplete: true,
                unported: None,
            })
        })
        .collect()
}

/// Run the audit.
pub fn run_web_audit(mut input: RunInput<'_>) -> RunOutcome {
    let Some(Base { base, host, domain }) = normalize_base(&input.url) else {
        return RunOutcome::Unreachable {
            reason: format!("blocked: unparseable url: {}", input.url),
            discovery: Vec::new(),
        };
    };
    let deadline = Instant::now() + input.per_audit_deadline;
    let target_locality = input.fetch.classify(&host).ok();
    let local_target = target_locality == Some(Locality::Local);

    // The single canonical root fetch every root-HTML check and several
    // antecedents read. It runs before discovery because it doubles as the
    // reachability probe: a network-dead root drops every later probe to
    // the degraded timeout so a tarpitting target cannot spend the whole
    // deadline on a handful of fetches.
    let root_resp = input.fetch.fetch(
        &base,
        &FetchInit::default(),
        &FetchOptions {
            timeout: input.per_check_timeout,
            ..FetchOptions::default()
        },
    );
    let root: Option<Arc<ProbeResponse>> = root_resp.status.map(|_| Arc::new(root_resp.clone()));
    let per_check_timeout = if root.is_none() {
        input.per_check_timeout.min(DEGRADED_PER_CHECK_TIMEOUT)
    } else {
        input.per_check_timeout
    };

    let discovery = discover_mcp_endpoint(
        &input.fetch,
        &base,
        &MCP_DISCOVERY,
        per_check_timeout,
        deadline,
    );
    if let Some(sink) = input.progress.as_deref_mut() {
        sink.discovery(discovery.endpoint.as_deref());
    }

    // Nothing from the target itself answered: unreachable from this vantage
    // point. Any real response, even a 401 or 404, is auditable evidence.
    let root_from_target = root
        .as_ref()
        .is_some_and(|r| !is_edge_error_status(r.status));
    let discovery_status = |item: &EvidenceItem| {
        item.get("status")
            .and_then(|s| s.as_u64())
            .map(|s| s as u16)
    };
    let any_target_response = discovery
        .evidence
        .iter()
        .any(|e| discovery_status(e).is_some_and(|s| !is_edge_error_status(Some(s))));
    if !root_from_target && discovery.endpoint.is_none() && !any_target_response {
        let only_edge_errors = root.is_some()
            || discovery
                .evidence
                .iter()
                .any(|e| discovery_status(e).is_some_and(|s| is_edge_error_status(Some(s))));
        let reason = if only_edge_errors {
            format!(
                "{base} did not answer any probe (every response was a Cloudflare edge error, \
                 which means the host did not resolve or never replied). \
                 The site may be down, its DNS may be misconfigured, or it may block requests from datacenter IP ranges \
                 such as the auditor’s."
            )
        } else {
            format!(
                "{base} did not answer any probe (no HTTP response from the root fetch or MCP discovery). \
                 The site may be down, or it may block requests from datacenter IP ranges such as the auditor’s."
            )
        };
        return RunOutcome::Unreachable {
            reason,
            discovery: discovery.evidence,
        };
    }

    let mut incomplete = false;
    let mut unported: Vec<Unported> = Vec::new();
    let handler_ctx = |scoped_dirs: Arc<Vec<String>>,
                       retained: Arc<HashMap<String, String>>,
                       session: Option<String>,
                       lanes: McpLaneEvidence| HandlerContext {
        base: base.clone(),
        host: host.clone(),
        mcp_endpoint: discovery.endpoint.clone(),
        protocol_version: MCP_DISCOVERY.protocol_version,
        default_timeout: per_check_timeout.min(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ),
        root: root.clone(),
        scoped_dirs,
        retained_bodies: retained,
        fetch: input.fetch.clone(),
        mcp_session_id: session,
        mcp_lanes: lanes,
        deadline,
        external_dns: input.external_dns,
        target_locality,
    };

    // Wave 1: probe the antecedent-source checks unconditionally.
    let wave1: Vec<&'static WebCheck> = CHECKS
        .iter()
        .filter(|c| antecedents::is_wave1(c.id))
        .collect();
    let wave2: Vec<&'static WebCheck> = CHECKS
        .iter()
        .filter(|c| !antecedents::is_wave1(c.id))
        .collect();
    let ctx1 = handler_ctx(
        Arc::new(Vec::new()),
        Arc::new(HashMap::new()),
        None,
        McpLaneEvidence::default(),
    );
    let mut sources: HashMap<String, ProbeOutcome> = HashMap::new();
    let mut wave1_results: HashMap<&'static str, (EngineResult, Option<&'static str>)> =
        HashMap::new();
    for probed in run_wave(
        wave1.clone(),
        &ctx1,
        &input.handlers,
        input.concurrency,
        deadline,
        local_target,
    ) {
        incomplete |= probed.incomplete;
        if let Some(outcome) = probed.outcome {
            sources.insert(probed.check.id.to_string(), outcome);
        }
        wave1_results.insert(probed.check.id, (probed.result, probed.unported));
    }

    let retained_body = |id: &str| -> String {
        sources
            .get(id)
            .and_then(|o| {
                o.evidence
                    .iter()
                    .find_map(|item| item.get("body").and_then(|b| b.as_str()))
            })
            .unwrap_or("")
            .to_string()
    };
    let llms_txt_body = retained_body("llms-txt");
    let sitemap_body = retained_body("sitemap");
    let openapi_body = retained_body("openapi");
    let scoped_dirs = Arc::new(enumerate_scoped_dirs(&llms_txt_body, &sitemap_body, &base));
    let mut retained: HashMap<String, String> = HashMap::new();
    if !llms_txt_body.is_empty() {
        retained.insert("llms-txt".to_string(), llms_txt_body);
    }
    if !openapi_body.is_empty() {
        retained.insert("openapi".to_string(), openapi_body);
    }
    let session = session_id_from(sources.get("mcp-initialize"));
    let lanes = McpLaneEvidence {
        modern: modern_lane_from(sources.get("mcp-server-discover")),
        legacy_advertised: advertised_capabilities(
            sources
                .get("mcp-initialize")
                .map_or(&[][..], |o| o.evidence.as_slice()),
        ),
        modern_advertised: advertised_capabilities(
            sources
                .get("mcp-server-discover")
                .map_or(&[][..], |o| o.evidence.as_slice()),
        ),
    };
    if let (Some(session_id), Some(endpoint)) = (&session, &discovery.endpoint) {
        let timeout = per_check_timeout.min(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        );
        notify_mcp_initialized(&input.fetch, endpoint, session_id, timeout);
        if Instant::now() >= deadline {
            incomplete = true;
        }
    }

    let actx = AntecedentContext {
        site_type: input.site_type,
        mcp_endpoint: discovery.endpoint.as_deref(),
        discovery_evidence: &discovery.evidence,
        root: root.as_deref(),
        sources: &sources,
    };
    // Gate: declared-type filter first, then the antecedent token.
    let gate = |check: &'static WebCheck| -> Option<EngineResult> {
        if !antecedents::site_type_applies(check.site_types, &actx) {
            return Some(na_result(
                check,
                NaReason::AntecedentUnmet,
                "not applicable to the declared site type",
            ));
        }
        match antecedents::resolve(check.antecedent, &actx) {
            Resolution::NotApplicable => Some(na_result(
                check,
                NaReason::AntecedentUnmet,
                antecedents::unmet_evidence(check.antecedent),
            )),
            Resolution::Error => Some(error_result(
                check,
                "antecedent unresolvable: root fetch failed",
            )),
            Resolution::Apply => None,
        }
    };

    let mut finalized: HashMap<&'static str, EngineResult> = HashMap::new();
    for check in &wave1 {
        // A wave-1 row its gate rejects keeps the gate's verdict, so an
        // unported handler only counts where its skip is the row.
        let result = match gate(check) {
            Some(gated) => gated,
            None => match wave1_results.remove(check.id) {
                Some((result, unported_kind)) => {
                    if let Some(kind) = unported_kind {
                        unported.push(Unported {
                            check_id: check.id,
                            kind,
                        });
                    }
                    result
                }
                None => error_result(check, "missing wave-1 result"),
            },
        };
        let result = finalize_optional(check, result);
        if let Some(sink) = input.progress.as_deref_mut() {
            sink.result(&result);
        }
        finalized.insert(check.id, result);
    }

    // Wave 2: gated checks resolve immediately; applicable ones probe with
    // the root fetch and wave-1 signals reused.
    let mut applicable: Vec<&'static WebCheck> = Vec::new();
    for check in &wave2 {
        match gate(check) {
            Some(gated) => {
                let result = finalize_optional(check, gated);
                if let Some(sink) = input.progress.as_deref_mut() {
                    sink.result(&result);
                }
                finalized.insert(check.id, result);
            }
            None => applicable.push(check),
        }
    }
    let ctx2 = handler_ctx(scoped_dirs, Arc::new(retained), session, lanes);
    for probed in run_wave(
        applicable,
        &ctx2,
        &input.handlers,
        input.concurrency,
        deadline,
        local_target,
    ) {
        incomplete |= probed.incomplete;
        if let Some(kind) = probed.unported {
            unported.push(Unported {
                check_id: probed.check.id,
                kind,
            });
        }
        let result = finalize_optional(probed.check, probed.result);
        if let Some(sink) = input.progress.as_deref_mut() {
            sink.result(&result);
        }
        finalized.insert(probed.check.id, result);
    }

    let results: Vec<EngineResult> = CHECKS
        .iter()
        .filter_map(|check| finalized.remove(check.id))
        .collect();
    let meta = ScorecardMeta {
        target_url: base.clone(),
        domain,
        mcp_endpoint: discovery.endpoint.clone(),
        discovery_evidence: discovery.evidence.clone(),
        spec_version: input.spec_version.clone(),
        site_type: input.site_type,
        public_listing: false,
    };
    let scorecard = build_web_scorecard(
        &results,
        &meta,
        CHECKS.iter().map(|c| c.keyword),
        CATEGORY_ORDER,
        CATEGORIES,
        &ScoreConfig::default(),
    );
    RunOutcome::Complete(Box::new(RunReport {
        scorecard,
        results,
        complete: !incomplete,
        unported,
        target_locality,
        discovery,
    }))
}
