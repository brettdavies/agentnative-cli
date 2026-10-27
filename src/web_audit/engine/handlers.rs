//! The seam between the engine and the probe handlers.
//!
//! A check dispatches by its handler kind, unless it carries the
//! `api-description` or `legacy-alias-redirects` eval rule or the
//! `retained-document` rule on an `http` check, which have their own entry
//! points in the site engine;
//! a `retained-document` check on any other handler scores the document
//! with that handler. A kind or rule nothing has registered is unported: the
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
        let rule = match check.eval {
            Some(EvalBinding::ApiDescription) => Some("api-description"),
            Some(EvalBinding::LegacyAliasRedirects) => Some("legacy-alias-redirects"),
            Some(EvalBinding::RetainedDocument) if check.handler == HandlerBinding::Http => {
                Some("retained-document")
            }
            Some(EvalBinding::Unsupported(rule)) => return Dispatch::Unported(rule),
            Some(EvalBinding::RetainedDocument | EvalBinding::ScopedDiscovery) | None => None,
        };
        if let Some(rule) = rule {
            return match self.evals.get(rule) {
                Some(handler) => Dispatch::Outcome(handler(check, ctx)),
                None => Dispatch::Unported(rule),
            };
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
