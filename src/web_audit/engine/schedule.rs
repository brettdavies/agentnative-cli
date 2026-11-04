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

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::antecedents::{self, AntecedentContext, Resolution, is_source_check};
use super::follow_trail::{DeclaredHostReason, host_of};
use super::handlers::HandlerSet;
use super::mcp_wire::{
    advertised_capabilities, modern_lane_from, notify_mcp_initialized, session_id_from,
};
use super::prerequisites::{Prerequisite, row_prerequisites};
use super::probe::{
    Probed, deadline_skip, error_result, finalize_optional, na_at_host, na_result, probe_one,
    to_result,
};
use super::retained::enumerate_scoped_dirs;
use super::run::{ProgressSink, Unported};
use super::sign_in::{
    McpAuthRequired, SignInChallenge, SignInResolver, is_wire_probe, settle_mcp_auth,
};
use super::slots::{AuditSlots, Rank};
use super::types::{HandlerContext, McpLaneEvidence, ProbeOutcome};
use crate::web_audit::fetch::ProbeResponse;
use crate::web_audit::handlers::api_description::api_description_bodies;
use crate::web_audit::handlers::mcp::ops::McpOp;
use crate::web_audit::handlers::mcp::sign_in::sign_in_required_outcome;
use crate::web_audit::registry::{CHECKS, HandlerBinding, WebCheck};
use crate::web_audit::scorecard::{DeclaredSiteType, EngineResult, EvidenceItem, NaReason};

/// What the schedule runs from: what discovery settled, and the context every
/// handler shares, with the fields the schedule fills in left empty.
pub(super) struct Plan<'a> {
    pub(super) template: HandlerContext,
    pub(super) handlers: Arc<HandlerSet>,
    pub(super) concurrency: usize,
    pub(super) per_check_timeout: Duration,
    pub(super) local_target: bool,
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
    slots: AuditSlots,
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
    /// The wire probes as a client without a token saw them, in a run that
    /// presents one.
    token_less: Option<HashMap<String, ProbeOutcome>>,
    token_less_left: usize,
    mcp_started: bool,
    retained: Arc<HashMap<String, String>>,
    scoped_dirs: Arc<Vec<String>>,
    descriptions: Arc<HashMap<String, String>>,
    session: Option<String>,
    lanes: McpLaneEvidence,
    mcp_auth: Option<Arc<McpAuthRequired>>,
    mcp_sign_in: Option<Arc<SignInResolver>>,
}

struct CloseOnDrop(AuditSlots);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Run every check. Rows reach `progress` as they finalize; once the deadline
/// passes, whatever has not finished is settled from what the run knows and
/// every worker still on a request is abandoned to its own timeout.
pub(super) fn run_checks<'p>(
    mut plan: Plan<'_>,
    mut progress: Option<&mut (dyn ProgressSink + 'p)>,
) -> Scheduled {
    let slots = AuditSlots::new(plan.concurrency);
    let _close = CloseOnDrop(slots.clone());
    plan.template.slots = Some(slots.clone());
    let deadline = plan.template.deadline;
    let (tx, rx) = mpsc::channel();
    let mut schedule = Schedule::new(plan, slots, tx);
    schedule.start();
    schedule.report(&mut progress);
    while !schedule.finished() {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(done) => schedule.on(done),
            // The schedule holds a sender, so this is the deadline.
            Err(_) => schedule.finish_at_deadline(),
        }
        schedule.report(&mut progress);
    }
    schedule.into_scheduled()
}

impl<'a> Schedule<'a> {
    fn new(plan: Plan<'a>, slots: AuditSlots, tx: mpsc::Sender<Done>) -> Self {
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
        let token_less = plan.template.fetch.credentialed().then(HashMap::new);
        Schedule {
            plan,
            slots,
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
            token_less,
            token_less_left: 0,
            mcp_started: false,
            retained: Arc::new(HashMap::new()),
            scoped_dirs: Arc::new(Vec::new()),
            descriptions: Arc::new(HashMap::new()),
            session: None,
            lanes: McpLaneEvidence::default(),
            mcp_auth: None,
            mcp_sign_in: None,
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
            Done::McpNotified => {
                if Instant::now() >= self.plan.template.deadline {
                    self.incomplete = true;
                }
                self.settled.insert(Prerequisite::McpNotified);
            }
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

    /// A source that measured nothing, because its handler threw, it has no
    /// handler in this build, or the deadline came first, keeps its own row: the
    /// gate would read that silence as the surface not being there, which can lift
    /// the score of a run that never looked. Otherwise a row its gate rules out
    /// finalizes here and takes no slot, a source row reads the probe it already
    /// made, and any other row queues for a slot.
    fn finish_row(&mut self, check: &'static WebCheck, index: usize) {
        if self.measured_nothing(check) {
            let result = self.source_row(check);
            self.finalize(check, result);
            return;
        }
        if let Some(gated) = self.gate(check) {
            self.finalize(check, gated);
            return;
        }
        if is_source_check(check.id) {
            let result = self.source_row(check);
            self.finalize(check, result);
            return;
        }
        self.probing.insert(check.id, check);
        self.submit_probe(check, self.context(), Rank::Row(index), Done::Row);
    }

    fn measured_nothing(&self, check: &WebCheck) -> bool {
        is_source_check(check.id) && !self.sources.contains_key(check.id)
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

    /// Whether the endpoint requires sign-in is settled once both wire probes
    /// have answered. A credentialed run's own handshakes say only what its
    /// token opened, so that reading comes from the same probes asked again by
    /// a client without the token, as the public engine reads its own.
    fn start_mcp_when_wire_probes_settled(&mut self) {
        if self.mcp_started {
            return;
        }
        let mut wire = CHECKS.iter().filter(|check| is_wire_probe(check.id));
        if !wire.all(|check| self.settled.contains(&Prerequisite::Source(check.id))) {
            return;
        }
        self.mcp_started = true;
        if self.token_less.is_none() {
            self.submit_settle();
            return;
        }
        for check in CHECKS.iter().filter(|check| is_wire_probe(check.id)) {
            let ctx = self.context();
            let ctx = HandlerContext {
                fetch: ctx.fetch.without_credential(),
                ..ctx
            };
            self.token_less_left += 1;
            self.submit_probe(check, ctx, Rank::McpStep, Done::TokenLess);
        }
        if self.token_less_left == 0 {
            self.submit_settle();
        }
    }

    fn on_token_less(&mut self, probed: Probed) {
        self.incomplete |= probed.incomplete;
        if let (Some(asked), Some(outcome)) = (self.token_less.as_mut(), probed.outcome) {
            asked.insert(probed.check.id.to_string(), outcome);
        }
        self.token_less_left -= 1;
        if self.token_less_left == 0 {
            self.submit_settle();
        }
    }

    /// The wire probes' outcomes as a client without a token saw them.
    fn wire_outcomes(&self) -> HashMap<String, ProbeOutcome> {
        self.token_less.clone().unwrap_or_else(|| {
            self.sources
                .iter()
                .filter(|(id, _)| is_wire_probe(id))
                .map(|(id, outcome)| (id.clone(), outcome.clone()))
                .collect()
        })
    }

    // A slot, because settling can read the endpoint's metadata.
    fn submit_settle(&self) {
        let token_less = self.wire_outcomes();
        let challenge = self.plan.challenge.cloned();
        let resolver = Arc::clone(&self.plan.resolver);
        let tx = self.tx.clone();
        self.slots.submit(Rank::McpStep, move || {
            let settled = catch_unwind(AssertUnwindSafe(|| {
                settle_mcp_auth(challenge.as_ref(), &token_less, &resolver)
            }));
            let _ = tx.send(Done::McpSettled(settled.unwrap_or(None)));
        });
    }

    /// Sign-in settled: the wire probes' own rows are read again the way every
    /// later MCP row is, and the session and lane facts are taken. When it
    /// settled nothing, a later row's 401 is read against the same metadata.
    fn on_mcp_settled(&mut self, auth: Option<Arc<McpAuthRequired>>) {
        if auth.is_some() {
            let mcp_sources = CHECKS
                .iter()
                .filter(|c| is_source_check(c.id) && c.handler == HandlerBinding::Mcp);
            for check in mcp_sources {
                let reread = self
                    .sources
                    .get(check.id)
                    .and_then(sign_in_required_outcome);
                if let Some(reread) = reread {
                    self.source_rows
                        .insert(check.id, (to_result(check, reread.clone()), None));
                    self.sources.insert(check.id.to_string(), reread);
                }
            }
        }
        self.mcp_sign_in = auth.is_none().then(|| Arc::clone(&self.plan.resolver));
        self.mcp_auth = auth;
        let evidence = |id: &str| self.sources.get(id).map_or(&[][..], |o| &o.evidence[..]);
        self.lanes = McpLaneEvidence {
            modern: modern_lane_from(self.sources.get("mcp-server-discover")),
            legacy_advertised: advertised_capabilities(evidence("mcp-initialize")),
            modern_advertised: advertised_capabilities(evidence("mcp-server-discover")),
        };
        self.session = session_id_from(self.sources.get("mcp-initialize"));
        self.settled.insert(Prerequisite::McpSettled);
        match (&self.session, &self.plan.template.mcp_endpoint) {
            (Some(session), Some(endpoint)) => {
                self.submit_notify(endpoint.clone(), session.clone());
            }
            _ => {
                self.settled.insert(Prerequisite::McpNotified);
            }
        }
    }

    fn submit_notify(&self, endpoint: String, session: String) {
        let fetch = self.plan.template.fetch.clone();
        let followed = self.plan.template.mcp_endpoint_followed;
        let per_check = self.plan.per_check_timeout;
        let deadline = self.plan.template.deadline;
        let tx = self.tx.clone();
        self.slots.submit(Rank::McpStep, move || {
            let timeout = per_check.min(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            );
            let _ = catch_unwind(AssertUnwindSafe(|| {
                notify_mcp_initialized(&fetch, &endpoint, &session, timeout, followed);
            }));
            let _ = tx.send(Done::McpNotified);
        });
    }

    fn submit_probe(
        &self,
        check: &'static WebCheck,
        ctx: HandlerContext,
        rank: Rank,
        done: fn(Probed) -> Done,
    ) {
        let handlers = Arc::clone(&self.plan.handlers);
        let deadline = self.plan.template.deadline;
        let local_target = self.plan.local_target;
        let tx = self.tx.clone();
        self.slots.submit(rank, move || {
            let probed = probe_one(check, &ctx, &handlers, deadline, local_target);
            let _ = tx.send(done(probed));
        });
    }

    /// Each field that reads a source holds what that source returned by the
    /// time a row whose prerequisites name it reads the field.
    fn context(&self) -> HandlerContext {
        let deadline = self.plan.template.deadline;
        HandlerContext {
            default_timeout: self.plan.per_check_timeout.min(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            ),
            scoped_dirs: Arc::clone(&self.scoped_dirs),
            retained_bodies: Arc::clone(&self.retained),
            api_description_bodies: Arc::clone(&self.descriptions),
            mcp_session_id: self.session.clone(),
            mcp_lanes: self.lanes.clone(),
            mcp_auth: self.mcp_auth.clone(),
            mcp_sign_in: self.mcp_sign_in.clone(),
            ..self.plan.template.clone()
        }
    }

    /// Gate: declared-type filter first, then the antecedent token. The row a
    /// check settles to, or `None` when it must be probed.
    fn gate(&self, check: &'static WebCheck) -> Option<EngineResult> {
        let actx = AntecedentContext {
            site_type: self.plan.site_type,
            mcp_endpoint: self.plan.template.mcp_endpoint.as_deref(),
            discovery_evidence: self.plan.discovery_evidence,
            follow_unmet: self.plan.follow_unmet,
            root: self.plan.root,
            sources: &self.sources,
            token_less: self.token_less.as_ref().unwrap_or(&self.sources),
            api_anchors: self.plan.api_anchors,
            mcp_auth: self.mcp_auth.as_deref(),
            mcp_lane: McpOp::request_era(check),
        };
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

    /// The budget is gone. Nothing more goes out, and every row settles from
    /// what the run knows: a source that measured nothing keeps its own row, a
    /// row its gate rules out keeps that verdict, and any other with no answer by
    /// now reads as skipped. Sign-in still settles if nothing has started it, from
    /// a 401 already on record; a metadata read past the deadline declines to send.
    fn finish_at_deadline(&mut self) {
        self.incomplete = true;
        self.slots.close();
        if !self.settled.contains(&Prerequisite::McpSettled) && self.token_less_left == 0 {
            let settled = settle_mcp_auth(
                self.plan.challenge,
                &self.wire_outcomes(),
                &self.plan.resolver,
            );
            self.on_mcp_settled(settled);
        }
        for row in std::mem::take(&mut self.waiting) {
            let check = row.check;
            let result = if self.measured_nothing(check) {
                self.source_row(check)
            } else {
                match self.gate(check) {
                    Some(gated) => gated,
                    None if is_source_check(check.id) => self.source_row(check),
                    None => deadline_skip(check),
                }
            };
            self.finalize(check, result);
        }
        for (_, check) in std::mem::take(&mut self.probing) {
            self.finalize(check, deadline_skip(check));
        }
    }
}
