//! The seam between the engine and the probe handlers.
//!
//! A check dispatches by its handler kind, unless it carries the
//! `legacy-alias-redirects` eval rule, which has its own entry point in
//! the site engine. A kind or rule nothing has registered is unported: the
//! engine reports the row as a skip naming the kind, and the run's score is
//! flagged as not comparable with anc.dev.

use std::collections::HashMap;
use std::sync::Arc;

use super::types::{HandlerContext, ProbeOutcome};
use crate::web_audit::registry::{EvalBinding, HandlerBinding, WebCheck};

/// A probe handler: check plus context in, outcome out.
pub type HandlerFn = Arc<dyn Fn(&WebCheck, &HandlerContext) -> ProbeOutcome + Send + Sync>;

/// The handlers a run dispatches to.
#[derive(Clone, Default)]
pub struct HandlerSet {
    kinds: HashMap<&'static str, HandlerFn>,
    evals: HashMap<&'static str, HandlerFn>,
}

impl std::fmt::Debug for HandlerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut kinds: Vec<&&str> = self.kinds.keys().collect();
        kinds.sort();
        let mut evals: Vec<&&str> = self.evals.keys().collect();
        evals.sort();
        f.debug_struct("HandlerSet")
            .field("kinds", &kinds)
            .field("evals", &evals)
            .finish()
    }
}

/// What dispatch found for a check.
#[derive(Debug)]
pub enum Dispatch {
    /// The handler ran.
    Outcome(ProbeOutcome),
    /// No handler is registered for this kind or eval rule.
    Unported(&'static str),
}

impl HandlerSet {
    /// A set with nothing registered; every check is unported.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the handler for a kind, by its registry spelling.
    pub fn register<F>(&mut self, kind: &'static str, handler: F) -> &mut Self
    where
        F: Fn(&WebCheck, &HandlerContext) -> ProbeOutcome + Send + Sync + 'static,
    {
        self.kinds.insert(kind, Arc::new(handler));
        self
    }

    /// Register the entry point for an eval rule, by its registry spelling.
    pub fn register_eval<F>(&mut self, rule: &'static str, handler: F) -> &mut Self
    where
        F: Fn(&WebCheck, &HandlerContext) -> ProbeOutcome + Send + Sync + 'static,
    {
        self.evals.insert(rule, Arc::new(handler));
        self
    }

    /// Whether a kind has a handler.
    pub fn supports_kind(&self, kind: &str) -> bool {
        self.kinds.contains_key(kind)
    }

    /// Run the handler a check binds to.
    pub fn dispatch(&self, check: &WebCheck, ctx: &HandlerContext) -> Dispatch {
        match check.eval {
            Some(EvalBinding::LegacyAliasRedirects) => {
                return match self.evals.get("legacy-alias-redirects") {
                    Some(handler) => Dispatch::Outcome(handler(check, ctx)),
                    None => Dispatch::Unported("legacy-alias-redirects"),
                };
            }
            Some(EvalBinding::Unsupported(rule)) => return Dispatch::Unported(rule),
            Some(EvalBinding::ScopedDiscovery) | None => {}
        }
        let kind = match check.handler {
            HandlerBinding::Unsupported(kind) => return Dispatch::Unported(kind),
            supported => supported.as_str(),
        };
        match self.kinds.get(kind) {
            Some(handler) => Dispatch::Outcome(handler(check, ctx)),
            None => Dispatch::Unported(kind),
        }
    }
}
