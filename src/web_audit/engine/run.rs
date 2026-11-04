//! The run: the root fetch beside discovery's first document reads, the rest of
//! discovery, the reachability gate, the schedule over every check, and
//! scorecard assembly. A mirror of the site's `runWebAudit`.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

use super::api_catalog::catalog_anchors;
use super::api_targets::api_targets;
use super::discovery::{DiscoveryResult, RootBeside, read_discovery_documents};
use super::discovery_documents::API_CATALOG_DOCUMENT;
use super::endpoint_of_record::{Settle, settle_endpoint_of_record};
use super::handlers::HandlerSet;
use super::schedule::{Plan, run_checks};
use super::sign_in::{DirectSource, SignInResolver};
use super::types::{FetchHandle, HandlerContext, McpLaneEvidence};
use crate::web_audit::fetch::launch::Launch;
use crate::web_audit::fetch::memo::RequestMemo;
use crate::web_audit::fetch::{
    FetchInit, FetchOptions, NO_PLAINTEXT_REQUEST, ProbeResponse, Refusal,
};
use crate::web_audit::handlers::api_hygiene::HostProbes;
use crate::web_audit::locality::Locality;
use crate::web_audit::registry::{ALTERNATIVES, CATEGORIES, CATEGORY_ORDER, CHECKS, MCP_DISCOVERY};
use crate::web_audit::score::ScoreConfig;
use crate::web_audit::scorecard::{
    DeclaredSiteType, EngineResult, EvidenceItem, ScorecardMeta, WebScorecard, WebVantage,
    build_web_scorecard,
};

/// Checks in flight at once.
pub use crate::web_audit::registry::DEFAULT_CONCURRENCY;
/// Wall-clock budget for the whole run.
pub use crate::web_audit::registry::DEFAULT_PER_AUDIT_DEADLINE;
/// Per-request timeout when the root answered.
pub use crate::web_audit::registry::DEFAULT_PER_CHECK_TIMEOUT;
/// Per-request timeout once the root fetch has failed at the network level.
pub use crate::web_audit::registry::DEGRADED_PER_CHECK_TIMEOUT;

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
    /// Checks in flight at once.
    pub concurrency: usize,
    /// Per-request timeout while the root answered.
    pub per_check_timeout: Duration,
    /// Wall-clock budget for the whole run.
    pub per_audit_deadline: Duration,
    /// Let checks that query external DNS resolvers run against a local target.
    pub external_dns: bool,
    /// Follow the off-origin hosts the site declares.
    pub follow_declarations: bool,
    /// The run's transport, resolver and identity.
    pub fetch: FetchHandle,
    /// The probe handlers.
    pub handlers: Arc<HandlerSet>,
    /// Where finalized rows go as they land.
    pub progress: Option<&'a mut dyn ProgressSink>,
}

impl std::fmt::Debug for RunInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunInput")
            .field("url", &self.url)
            .field("site_type", &self.site_type)
            .field("spec_version", &self.spec_version)
            .field("concurrency", &self.concurrency)
            .field("per_check_timeout", &self.per_check_timeout)
            .field("per_audit_deadline", &self.per_audit_deadline)
            .field("external_dns", &self.external_dns)
            .field("follow_declarations", &self.follow_declarations)
            .field("handlers", &self.handlers)
            .finish_non_exhaustive()
    }
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
            follow_declarations: true,
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
        /// Why the root fetch produced no response, in the transport's
        /// words: the line that tells a refused connection from a rejected
        /// certificate chain.
        root_error: Option<String>,
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
pub(crate) fn is_edge_error_status(status: Option<u16>) -> bool {
    matches!(status, Some(530) | Some(520..=527))
}

/// The root fetch, running beside discovery's first document reads. What the
/// root turns out to be takes effect before anything waiting on it resumes.
struct RootRead {
    answer: mpsc::Receiver<ProbeResponse>,
    response: OnceCell<ProbeResponse>,
}

impl RootRead {
    fn start(
        fetch: &FetchHandle,
        base: &str,
        timeout: Duration,
        memo: RequestMemo,
        launch: Launch,
    ) -> Self {
        let (fetch, base) = (fetch.clone(), base.to_string());
        let (tx, answer) = mpsc::channel();
        thread::spawn(move || {
            let _settled = RootSettled(launch.clone());
            let opts = FetchOptions {
                timeout,
                ..FetchOptions::default()
            };
            let response = fetch.fetch(&base, &FetchInit::default(), &opts);
            if response.refused == Some(Refusal::InsecureScheme) {
                // The first reads go out before the root's answer is read on
                // every https target, however fast it comes.
                launch.wait(timeout);
                memo.close();
            } else if response.status.is_none() {
                memo.cut();
            }
            let _ = tx.send(response);
        });
        RootRead {
            answer,
            response: OnceCell::new(),
        }
    }

    fn wait(&self) -> &ProbeResponse {
        self.response.get_or_init(|| {
            self.answer
                .recv()
                .unwrap_or_else(|_| ProbeResponse::failed("root fetch panicked".to_string(), 0))
        })
    }
}

/// However the root fetch ends, a redirect hop waiting on it goes on.
struct RootSettled(Launch);

impl Drop for RootSettled {
    fn drop(&mut self) {
        self.0.root_settled();
    }
}

struct CloseOnDrop(RequestMemo);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Run the audit.
pub fn run_web_audit(mut input: RunInput<'_>) -> RunOutcome {
    let Some(Base { base, host, domain }) = normalize_base(&input.url) else {
        return RunOutcome::Unreachable {
            reason: format!("blocked: unparseable url: {}", input.url),
            discovery: Vec::new(),
            root_error: None,
        };
    };
    let deadline = Instant::now() + input.per_audit_deadline;
    // One memo per audit, closed on every way out of the run, so each
    // distinct request goes out once and nothing it holds outlives the run.
    let memo = RequestMemo::new();
    let _close_memo = CloseOnDrop(memo.clone());
    input.fetch = input.fetch.with_memo(memo.clone());
    let target_locality = input.fetch.classify(&host).ok();
    let local_target = target_locality == Some(Locality::Local);

    // The single canonical root fetch every root-HTML check and several
    // antecedents read, with discovery's first document reads beside it. It
    // doubles as the reachability probe: a network-dead root drops every later
    // probe to the degraded timeout and cuts those reads to that same budget
    // from their start, so a tarpitting target cannot spend the whole deadline
    // on a handful of fetches. A root that is not https, or that redirects to
    // http, ends the run and stops them.
    let launch = Launch::new();
    let root_read = RootRead::start(
        &input.fetch,
        &base,
        input.per_check_timeout,
        memo.clone(),
        launch.clone(),
    );
    let settled = || {
        if root_read.wait().status.is_none() {
            input.per_check_timeout.min(DEGRADED_PER_CHECK_TIMEOUT)
        } else {
            input.per_check_timeout
        }
    };
    let documents = read_discovery_documents(
        &input.fetch,
        &base,
        &MCP_DISCOVERY,
        input.per_check_timeout,
        deadline,
        Some(&RootBeside {
            cut_to: DEGRADED_PER_CHECK_TIMEOUT,
            launch,
            settled: &settled,
        }),
    );
    let root_resp = root_read.wait();
    if root_resp.refused == Some(Refusal::InsecureScheme) {
        let how = if root_resp.status.is_none() {
            "is not https"
        } else {
            "redirects to http"
        };
        return RunOutcome::Unreachable {
            reason: format!("{base} {how}, and {NO_PLAINTEXT_REQUEST}."),
            discovery: Vec::new(),
            root_error: root_resp.error.clone(),
        };
    }
    let root: Option<Arc<ProbeResponse>> = root_resp.status.map(|_| Arc::new(root_resp.clone()));
    let per_check_timeout = settled();

    let root_from_target = root
        .as_ref()
        .is_some_and(|r| !is_edge_error_status(r.status));
    let answered = |status: &Option<u16>| status.is_some_and(|s| !is_edge_error_status(Some(s)));
    let site_answered = root_from_target || documents.statuses().iter().any(answered);
    // The API catalog's anchors decide where the API rows evaluate. Its
    // off-origin declarations are followed only when an API row can apply
    // to the declared site type.
    let anchors = catalog_anchors(documents.document(API_CATALOG_DOCUMENT));
    let (discovery, declared) = {
        let settle = Settle {
            base: &base,
            enabled: input.follow_declarations,
            site_answered,
            api_rows_apply: matches!(input.site_type, None | Some(DeclaredSiteType::Api)),
            discovery: &MCP_DISCOVERY,
            fetch: &input.fetch,
            target_locality,
            timeout: per_check_timeout,
            deadline,
        };
        settle_endpoint_of_record(documents, &anchors, &settle)
    };
    if let Some(sink) = input.progress.as_deref_mut() {
        sink.discovery(declared.endpoint.as_deref());
    }

    // Nothing from the target itself answered: unreachable from this vantage
    // point. Any real response, even a 401 or 404, is auditable evidence. A
    // declared host's answer is not the site's, so only discovery's
    // evidence counts.
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
            root_error: root_resp.error.clone(),
        };
    }

    let api = api_targets(&base, &anchors, &declared.api).map(Arc::new);
    let resolver = Arc::new(SignInResolver::new(
        declared.endpoint.clone(),
        declared.metadata.clone(),
        DirectSource {
            fetch: input.fetch.clone(),
            timeout: per_check_timeout,
            deadline_at: deadline,
        },
    ));
    let template = HandlerContext {
        base: base.clone(),
        host: host.clone(),
        mcp_endpoint: declared.endpoint.clone(),
        mcp_endpoint_followed: declared.followed,
        protocol_version: MCP_DISCOVERY.protocol_version,
        default_timeout: per_check_timeout,
        root: root.clone(),
        scoped_dirs: Arc::new(Vec::new()),
        retained_bodies: Arc::new(HashMap::new()),
        retained_documents: Arc::new(discovery.documents.clone()),
        api_targets: api,
        api_description_bodies: Arc::new(HashMap::new()),
        api_host_probes: Arc::new(HostProbes::default()),
        fetch: input.fetch.clone(),
        mcp_session_id: None,
        mcp_lanes: McpLaneEvidence::default(),
        mcp_auth: None,
        mcp_sign_in: None,
        deadline,
        external_dns: input.external_dns,
        target_locality,
        slots: None,
    };
    let scheduled = run_checks(
        Plan {
            template,
            handlers: Arc::clone(&input.handlers),
            concurrency: input.concurrency,
            per_check_timeout,
            local_target,
            site_type: input.site_type,
            discovery_evidence: &discovery.evidence,
            follow_unmet: declared.unmet.as_ref(),
            root: root.as_deref(),
            api_anchors: anchors.iter().any(|a| a.is_api()),
            challenge: declared.challenge.as_ref(),
            resolver,
        },
        input.progress.as_deref_mut(),
    );
    let results = scheduled.results;
    let meta = ScorecardMeta {
        target_url: base.clone(),
        domain,
        mcp_endpoint: declared.endpoint.clone(),
        discovery_evidence: discovery.evidence.clone(),
        spec_version: input.spec_version.clone(),
        site_type: input.site_type,
        public_listing: false,
        follow_declarations: input.follow_declarations,
        declared_hosts: declared.trail.iter().map(|e| e.to_item()).collect(),
        vantage: WebVantage {
            network: if local_target { "local" } else { "public" }.to_string(),
            credentialed: input.fetch.credentialed(),
        },
    };
    let scorecard = build_web_scorecard(
        &results,
        &meta,
        (CHECKS, ALTERNATIVES),
        CATEGORY_ORDER,
        CATEGORIES,
        &ScoreConfig::default(),
    );
    RunOutcome::Complete(Box::new(RunReport {
        scorecard,
        results,
        complete: !scheduled.incomplete,
        unported: scheduled.unported,
        target_locality,
        discovery,
    }))
}
