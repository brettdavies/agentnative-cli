//! The MCP steps a run takes once its wire probes have settled: a credentialed
//! run's re-ask without its token, whether the endpoint requires sign-in, and the
//! session's initialized notification, which every session-carrying row waits on.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use super::{Done, Schedule, request_timeout};
use crate::web_audit::engine::antecedents::is_source_check;
use crate::web_audit::engine::mcp_wire::{
    advertised_capabilities, modern_lane_from, notify_mcp_initialized, session_id_from,
};
use crate::web_audit::engine::prerequisites::Prerequisite;
use crate::web_audit::engine::probe::{Probed, to_result};
use crate::web_audit::engine::sign_in::{
    McpAuthRequired, SignInResolver, is_wire_probe, settle_mcp_auth,
    settle_mcp_auth_without_reading,
};
use crate::web_audit::engine::slots::Rank;
use crate::web_audit::engine::types::{HandlerContext, McpLaneEvidence, ProbeOutcome};
use crate::web_audit::handlers::mcp::sign_in::sign_in_required_outcome;
use crate::web_audit::registry::{CHECKS, HandlerBinding};

/// What the steps have learned, read by the rows that wait on them.
pub(super) struct McpSteps {
    started: bool,
    /// The wire probes as a client without a token saw them, in a run that
    /// presents one.
    pub(super) token_less: Option<HashMap<String, ProbeOutcome>>,
    token_less_left: usize,
    pub(super) session: Option<String>,
    pub(super) lanes: McpLaneEvidence,
    pub(super) auth: Option<Arc<McpAuthRequired>>,
    pub(super) sign_in: Option<Arc<SignInResolver>>,
}

impl McpSteps {
    pub(super) fn new(credentialed: bool) -> Self {
        McpSteps {
            started: false,
            token_less: credentialed.then(HashMap::new),
            token_less_left: 0,
            session: None,
            lanes: McpLaneEvidence::default(),
            auth: None,
            sign_in: None,
        }
    }
}

impl Schedule<'_> {
    /// Whether the endpoint requires sign-in is settled once both wire probes
    /// have answered. A credentialed run's own handshakes say only what its
    /// token opened, so that reading comes from the same probes asked again by
    /// a client without the token, as the public engine reads its own.
    pub(super) fn start_mcp_when_wire_probes_settled(&mut self) {
        if self.mcp.started {
            return;
        }
        let mut wire = CHECKS.iter().filter(|check| is_wire_probe(check.id));
        if !wire.all(|check| self.settled.contains(&Prerequisite::Source(check.id))) {
            return;
        }
        self.mcp.started = true;
        if self.mcp.token_less.is_none() {
            self.submit_settle();
            return;
        }
        for check in CHECKS.iter().filter(|check| is_wire_probe(check.id)) {
            let ctx = self.context();
            let ctx = HandlerContext {
                fetch: ctx.fetch.without_credential(),
                ..ctx
            };
            self.mcp.token_less_left += 1;
            self.submit_probe(check, ctx, Rank::McpStep, Done::TokenLess);
        }
        if self.mcp.token_less_left == 0 {
            self.submit_settle();
        }
    }

    pub(super) fn on_token_less(&mut self, probed: Probed) {
        self.incomplete |= probed.incomplete;
        if let (Some(asked), Some(outcome)) = (self.mcp.token_less.as_mut(), probed.outcome) {
            asked.insert(probed.check.id.to_string(), outcome);
        }
        self.mcp.token_less_left -= 1;
        if self.mcp.token_less_left == 0 {
            self.submit_settle();
        }
    }

    /// The wire probes' outcomes as a client without a token saw them.
    fn wire_outcomes(&self) -> HashMap<String, ProbeOutcome> {
        self.mcp.token_less.clone().unwrap_or_else(|| {
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
        self.plan.slots.submit(Rank::McpStep, move || {
            let settled = catch_unwind(AssertUnwindSafe(|| {
                settle_mcp_auth(challenge.as_ref(), &token_less, &resolver)
            }));
            let _ = tx.send(Done::McpSettled(settled.unwrap_or(None)));
        });
    }

    /// Sign-in settled: the wire probes' own rows are read again the way every
    /// later MCP row is, and the session and lane facts are taken. When it
    /// settled nothing, a later row's 401 is read against the same metadata.
    pub(super) fn on_mcp_settled(&mut self, auth: Option<Arc<McpAuthRequired>>) {
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
        self.mcp.sign_in = auth.is_none().then(|| Arc::clone(&self.plan.resolver));
        self.mcp.auth = auth;
        let evidence = |id: &str| self.sources.get(id).map_or(&[][..], |o| &o.evidence[..]);
        self.mcp.lanes = McpLaneEvidence {
            modern: modern_lane_from(self.sources.get("mcp-server-discover")),
            legacy_advertised: advertised_capabilities(evidence("mcp-initialize")),
            modern_advertised: advertised_capabilities(evidence("mcp-server-discover")),
        };
        self.mcp.session = session_id_from(self.sources.get("mcp-initialize"));
        self.settled.insert(Prerequisite::McpSettled);
        match (&self.mcp.session, &self.plan.template.mcp_endpoint) {
            (Some(session), Some(endpoint)) => {
                self.submit_notify(endpoint.clone(), session.clone());
            }
            _ => {
                self.settled.insert(Prerequisite::McpNotified);
            }
        }
    }

    pub(super) fn on_mcp_notified(&mut self) {
        if Instant::now() >= self.plan.template.deadline {
            self.incomplete = true;
        }
        self.settled.insert(Prerequisite::McpNotified);
    }

    fn submit_notify(&self, endpoint: String, session: String) {
        let fetch = self.plan.template.fetch.clone();
        let followed = self.plan.template.mcp_endpoint_followed;
        let per_check = self.plan.per_check_timeout;
        let deadline = self.plan.template.deadline;
        let tx = self.tx.clone();
        self.plan.slots.submit(Rank::McpStep, move || {
            let timeout = request_timeout(per_check, deadline);
            let _ = catch_unwind(AssertUnwindSafe(|| {
                notify_mcp_initialized(&fetch, &endpoint, &session, timeout, followed);
            }));
            let _ = tx.send(Done::McpNotified);
        });
    }

    /// Sign-in settles even with a step still out: a 401 already on record must
    /// still read as sign-in, or a row it would rule out reads as skipped. This
    /// runs on the thread that ends the run, and a metadata read past the deadline
    /// declines to send anyway, so it neither reads nor waits on a read still open.
    pub(super) fn settle_mcp_at_deadline(&mut self) {
        if self.settled.contains(&Prerequisite::McpSettled) {
            return;
        }
        let settled = settle_mcp_auth_without_reading(
            self.plan.challenge,
            &self.wire_outcomes(),
            &self.plan.resolver,
        );
        self.on_mcp_settled(settled);
    }
}
