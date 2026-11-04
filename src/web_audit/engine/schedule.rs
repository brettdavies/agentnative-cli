//! Every check under one concurrency cap, each starting as soon as what it
//! reads has settled, a mirror of the site's `engine.ts`.
//!
//! The source checks probe first and unconditionally. Every other check gates
//! once its prerequisites settle, and probes if it applies. One thread owns the
//! run's state and decides what starts next; handlers run on the audit's slots
//! and report back over a channel, so no two of them share mutable state, and
//! the order they finish in cannot reach a row.
//!
//! ```text
//! wire probes settled -> [re-asked without the token] -> MCP settled -> notified
//!   source settled ----.                                     |            |
//!   MCP settled -------+--> row gates -> n_a or error, no slot taken      |
//!   notified ----------'             '-> applies: waits for a slot, probes
//! ```

mod mcp;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use self::mcp::McpSteps;
use super::antecedents::{self, AntecedentContext, Resolution, is_source_check};
use super::follow_trail::{DeclaredHostReason, host_of};
use super::handlers::HandlerSet;
use super::prerequisites::{Prerequisite, row_prerequisites};
use super::probe::{
    Probed, deadline_skip, error_result, finalize_optional, na_at_host, na_result, probe_one,
};
use super::retained::enumerate_scoped_dirs;
use super::sign_in::{McpAuthRequired, SignInChallenge, SignInResolver};
use super::slots::{AuditSlots, Rank};
use super::types::{HandlerContext, ProbeOutcome, ProgressSink, Unported};
use crate::web_audit::fetch::ProbeResponse;
use crate::web_audit::handlers::api_description::api_description_bodies;
use crate::web_audit::handlers::mcp::ops::McpOp;
use crate::web_audit::registry::{CHECKS, WebCheck};
use crate::web_audit::scorecard::{DeclaredSiteType, EngineResult, EvidenceItem, NaReason};

/// How long past the deadline the schedule waits for rows still out. A request
/// takes its timeout from what the deadline leaves, so a row on one answers for
/// itself within moments, and that answer is its row. One past this is blocked on
/// a name lookup or a connection, which no timeout reaches, and reads as skipped.
const DEADLINE_GRACE: Duration = Duration::from_millis(250);

/// A request's timeout, as of now: taken when its job starts rather than when it
/// queued, or a row that waited for a slot could outlast the deadline.
fn request_timeout(per_check: Duration, deadline: Instant) -> Duration {
    per_check.min(
        deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1)),
    )
}

/// What the schedule runs from: what discovery settled, and the context every
/// handler shares, with the fields the schedule fills in left empty.
pub(super) struct Plan<'a> {
    pub(super) template: HandlerContext,
    pub(super) handlers: Arc<HandlerSet>,
    pub(super) slots: AuditSlots,
    pub(super) per_check_timeout: Duration,
    pub(super) site_type: Option<DeclaredSiteType>,
    pub(super) discovery_evidence: &'a [EvidenceItem],
    pub(super) follow_unmet: Option<&'a DeclaredHostReason>,
    pub(super) root: Option<&'a ProbeResponse>,
    pub(super) api_anchors: bool,
    pub(super) challenge: Option<&'a SignInChallenge>,
    pub(super) resolver: Arc<SignInResolver>,
}

/// What the schedule leaves for the scorecard.
pub(super) struct Scheduled {
    /// Every row, in registry order.
    pub(super) results: Vec<EngineResult>,
    /// Rows skipped because their handler is not ported, in registry order.
    pub(super) unported: Vec<Unported>,
    /// The run ran out of budget, or a handler was cut short.
    pub(super) incomplete: bool,
}

/// A step a job finished on a slot.
enum Done {
    Source(Probed),
    Row(Probed),
    TokenLess(Probed),
    McpSettled(Option<Arc<McpAuthRequired>>),
    McpNotified,
}

struct WaitingRow {
    index: usize,
    check: &'static WebCheck,
    edges: Vec<Prerequisite>,
}

struct Schedule<'a> {
    plan: Plan<'a>,
    tx: mpsc::Sender<Done>,
    settled: HashSet<Prerequisite>,
    waiting: Vec<WaitingRow>,
    probing: HashMap<&'static str, &'static WebCheck>,
    finalized: HashMap<&'static str, EngineResult>,
    /// Rows finalized since the consumer last heard, in the order they were.
    fresh: Vec<&'static str>,
    unported: Vec<Unported>,
    incomplete: bool,
    sources: HashMap<String, ProbeOutcome>,
    source_rows: HashMap<&'static str, (EngineResult, Option<&'static str>)>,
    mcp: McpSteps,
    retained: Arc<HashMap<String, String>>,
    scoped_dirs: Arc<Vec<String>>,
    descriptions: Arc<HashMap<String, String>>,
}

/// Run every check. Rows reach `progress` as they finalize. Once the deadline and
/// its grace have passed, whatever has not finished is settled from what the run
/// knows, and the run's end stops every request still out.
pub(super) fn run_checks<'p>(
    plan: Plan<'_>,
    mut progress: Option<&mut (dyn ProgressSink + 'p)>,
) -> Scheduled {
    let give_up = plan.template.deadline + DEADLINE_GRACE;
    let (tx, rx) = mpsc::channel();
    let mut schedule = Schedule::new(plan, tx);
    schedule.start();
    schedule.report(&mut progress);
    while !schedule.finished() {
        let left = give_up.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(done) => schedule.on(done),
            // The schedule holds a sender, so this is the grace running out.
            Err(_) => schedule.finish_at_deadline(),
        }
        schedule.report(&mut progress);
    }
    schedule.into_scheduled()
}

impl<'a> Schedule<'a> {
    fn new(plan: Plan<'a>, tx: mpsc::Sender<Done>) -> Self {
        let waiting = CHECKS
            .iter()
            .enumerate()
            .map(|(index, check)| {
                let mut edges = row_prerequisites(check);
                let own = Prerequisite::Source(check.id);
                if is_source_check(check.id) && !edges.contains(&own) {
                    edges.push(own);
                }
                WaitingRow {
                    index,
                    check,
                    edges,
                }
            })
            .collect();
        let mcp = McpSteps::new(plan.template.fetch.credentialed());
        Schedule {
            plan,
            tx,
            settled: HashSet::new(),
            waiting,
            probing: HashMap::new(),
            finalized: HashMap::new(),
            fresh: Vec::new(),
            unported: Vec::new(),
            incomplete: false,
            sources: HashMap::new(),
            source_rows: HashMap::new(),
            mcp,
            retained: Arc::new(HashMap::new()),
            scoped_dirs: Arc::new(Vec::new()),
            descriptions: Arc::new(HashMap::new()),
        }
    }

    fn finished(&self) -> bool {
        self.waiting.is_empty() && self.probing.is_empty()
    }

    fn report<'p>(&mut self, progress: &mut Option<&mut (dyn ProgressSink + 'p)>) {
        for id in self.fresh.drain(..) {
            if let (Some(sink), Some(result)) = (progress.as_deref_mut(), self.finalized.get(id)) {
                sink.result(result);
            }
        }
    }

    fn into_scheduled(mut self) -> Scheduled {
        let results = CHECKS
            .iter()
            .filter_map(|check| self.finalized.remove(check.id))
            .collect();
        let position = |id: &str| CHECKS.iter().position(|check| check.id == id);
        self.unported.sort_by_key(|row| position(row.check_id));
        Scheduled {
            results,
            unported: self.unported,
            incomplete: self.incomplete,
        }
    }

    /// The source checks first, in registry order, so a check that reads
    /// nothing queues behind them as it does on the site.
    fn start(&mut self) {
        for check in CHECKS.iter().filter(|check| is_source_check(check.id)) {
            self.submit_probe(check, self.context(), Rank::Source, Done::Source);
        }
        self.advance();
    }

    fn on(&mut self, done: Done) {
        match done {
            Done::Source(probed) => self.on_source(probed),
            Done::Row(probed) => self.on_row(probed),
            Done::TokenLess(probed) => self.on_token_less(probed),
            Done::McpSettled(auth) => self.on_mcp_settled(auth),
            Done::McpNotified => self.on_mcp_notified(),
        }
        self.advance();
    }

    /// Start whatever the last step made ready.
    fn advance(&mut self) {
        self.start_mcp_when_wire_probes_settled();
        let settled = &self.settled;
        let (ready, waiting): (Vec<WaitingRow>, Vec<WaitingRow>) =
            std::mem::take(&mut self.waiting)
                .into_iter()
                .partition(|row| row.edges.iter().all(|edge| settled.contains(edge)));
        self.waiting = waiting;
        for row in ready {
            self.finish_row(row.check, row.index);
        }
    }

    /// A row its gate rules out finalizes here and takes no slot, a source row
    /// reads the probe it already made, and any other row queues for a slot.
    fn finish_row(&mut self, check: &'static WebCheck, index: usize) {
        if let Some(result) = self.settled_without_probe(check) {
            self.finalize(check, result);
            return;
        }
        self.probing.insert(check.id, check);
        self.submit_probe(check, self.context(), Rank::Row(index), Done::Row);
    }

    /// The row a check reads from what the run knows, or `None` when only a probe
    /// can say. A source that measured nothing, because its handler threw, it has
    /// no handler in this build, or the deadline came first, keeps its own row: the
    /// gate would read that silence as the surface not being there, which can lift
    /// the score of a run that never looked. One outside the declared site type
    /// still reads not applicable, which reads no source.
    fn settled_without_probe(&mut self, check: &'static WebCheck) -> Option<EngineResult> {
        if self.measured_nothing(check) {
            return Some(self.source_row(check));
        }
        if let Some(gated) = self.gate(check) {
            return Some(gated);
        }
        is_source_check(check.id).then(|| self.source_row(check))
    }

    fn measured_nothing(&self, check: &'static WebCheck) -> bool {
        is_source_check(check.id)
            && !self.sources.contains_key(check.id)
            && antecedents::site_type_applies(check.site_types, &self.antecedent_context(check))
    }

    fn source_row(&mut self, check: &'static WebCheck) -> EngineResult {
        let Some((result, unported)) = self.source_rows.remove(check.id) else {
            return deadline_skip(check);
        };
        if let Some(kind) = unported {
            self.unported.push(Unported {
                check_id: check.id,
                kind,
            });
        }
        result
    }

    fn finalize(&mut self, check: &'static WebCheck, result: EngineResult) {
        self.finalized
            .insert(check.id, finalize_optional(check, result));
        self.fresh.push(check.id);
    }

    fn on_source(&mut self, probed: Probed) {
        let Probed {
            check,
            outcome,
            result,
            incomplete,
            unported,
        } = probed;
        self.incomplete |= incomplete;
        self.source_rows.insert(check.id, (result, unported));
        if let Some(outcome) = outcome {
            let body = outcome
                .evidence
                .iter()
                .find_map(|item| item.get("body").and_then(|b| b.as_str()))
                .filter(|body| !body.is_empty());
            if let Some(body) = body {
                Arc::make_mut(&mut self.retained).insert(check.id.to_string(), body.to_string());
            }
            if check.id == "openapi" {
                self.descriptions = Arc::new(api_description_bodies(&outcome.evidence));
            }
            self.sources.insert(check.id.to_string(), outcome);
        }
        self.settled.insert(Prerequisite::Source(check.id));
        if matches!(check.id, "llms-txt" | "sitemap")
            && self.settled.contains(&Prerequisite::Source("llms-txt"))
            && self.settled.contains(&Prerequisite::Source("sitemap"))
        {
            let body = |id: &str| self.retained.get(id).map_or("", String::as_str);
            self.scoped_dirs = Arc::new(enumerate_scoped_dirs(
                body("llms-txt"),
                body("sitemap"),
                &self.plan.template.base,
            ));
        }
    }

    fn on_row(&mut self, probed: Probed) {
        let Probed {
            check,
            result,
            incomplete,
            unported,
            ..
        } = probed;
        self.incomplete |= incomplete;
        self.probing.remove(check.id);
        if let Some(kind) = unported {
            self.unported.push(Unported {
                check_id: check.id,
                kind,
            });
        }
        self.finalize(check, result);
    }

    fn submit_probe(
        &self,
        check: &'static WebCheck,
        mut ctx: HandlerContext,
        rank: Rank,
        done: fn(Probed) -> Done,
    ) {
        let handlers = Arc::clone(&self.plan.handlers);
        let per_check = self.plan.per_check_timeout;
        let tx = self.tx.clone();
        self.plan.slots.submit(rank, move || {
            ctx.default_timeout = request_timeout(per_check, ctx.deadline);
            let probed = probe_one(check, &ctx, &handlers);
            let _ = tx.send(done(probed));
        });
    }

    /// Each field that reads a source holds what that source returned by the
    /// time a row whose prerequisites name it reads the field.
    fn context(&self) -> HandlerContext {
        HandlerContext {
            scoped_dirs: Arc::clone(&self.scoped_dirs),
            retained_bodies: Arc::clone(&self.retained),
            api_description_bodies: Arc::clone(&self.descriptions),
            mcp_session_id: self.mcp.session.clone(),
            mcp_lanes: self.mcp.lanes.clone(),
            mcp_auth: self.mcp.auth.clone(),
            mcp_sign_in: self.mcp.sign_in.clone(),
            ..self.plan.template.clone()
        }
    }

    fn antecedent_context(&self, check: &'static WebCheck) -> AntecedentContext<'_> {
        AntecedentContext {
            site_type: self.plan.site_type,
            mcp_endpoint: self.plan.template.mcp_endpoint.as_deref(),
            discovery_evidence: self.plan.discovery_evidence,
            follow_unmet: self.plan.follow_unmet,
            root: self.plan.root,
            sources: &self.sources,
            token_less: self.mcp.token_less.as_ref().unwrap_or(&self.sources),
            api_anchors: self.plan.api_anchors,
            mcp_auth: self.mcp.auth.as_deref(),
            mcp_lane: McpOp::request_era(check),
        }
    }

    /// Gate: declared-type filter first, then the antecedent token. The row a
    /// check settles to, or `None` when it must be probed.
    fn gate(&self, check: &'static WebCheck) -> Option<EngineResult> {
        let actx = self.antecedent_context(check);
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
            Resolution::NotApplicableBecause(evidence) => {
                Some(na_result(check, NaReason::AntecedentUnmet, evidence))
            }
            Resolution::DeclaredHost(unmet) => Some(na_at_host(
                na_result(check, unmet.reason, &unmet.url),
                Some(unmet.host),
            )),
            Resolution::AuthRequired(endpoint) => Some(na_at_host(
                na_result(check, NaReason::AuthRequired, &endpoint),
                host_of(&endpoint),
            )),
            Resolution::Error => Some(error_result(
                check,
                "antecedent unresolvable: root fetch failed",
            )),
            Resolution::Apply => None,
        }
    }

    /// The budget and its grace are gone. Nothing more goes out, and every row
    /// settles from what the run knows, or reads as skipped with no answer by now.
    fn finish_at_deadline(&mut self) {
        self.incomplete = true;
        self.plan.slots.close();
        self.settle_mcp_at_deadline();
        for row in std::mem::take(&mut self.waiting) {
            let check = row.check;
            let result = self
                .settled_without_probe(check)
                .unwrap_or_else(|| deadline_skip(check));
            self.finalize(check, result);
        }
        for (_, check) in std::mem::take(&mut self.probing) {
            self.finalize(check, deadline_skip(check));
        }
    }
}
