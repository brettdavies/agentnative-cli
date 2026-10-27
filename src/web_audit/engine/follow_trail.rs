//! The declared-hosts trail, a mirror of the site's `follow-trail.ts` and of
//! `declaredHostReason` in `endpoint-of-record.ts`: one entry per URL the
//! audited site declares off its origin, the closed vocabulary of how the
//! audit treated it, and the reason a row reads when a declared host kept
//! it from being evaluated. The refusals settled before any request live
//! in `follow_refusals`.

use serde_json::json;
use url::Url;

use super::discovery_documents::same_origin;
use crate::web_audit::scorecard::{EvidenceItem, NaReason};

/// What a declaration names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An MCP endpoint a card names, or where a POST was redirected.
    McpEndpoint,
    /// A server card the AI catalog names by URL.
    CardDocument,
    /// A linkset anchor: an API host.
    ApiAnchor,
    /// The description an API anchor declares.
    ApiDescription,
}

impl Kind {
    /// The trail spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::McpEndpoint => "mcp-endpoint",
            Kind::CardDocument => "card-document",
            Kind::ApiAnchor => "api-anchor",
            Kind::ApiDescription => "api-description",
        }
    }

    /// An MCP declaration rather than an API one.
    pub fn is_mcp(self) -> bool {
        matches!(self, Kind::McpEndpoint | Kind::CardDocument)
    }
}

/// A URL a discovery document names, which the follow slice settles when
/// it is off the audited origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    /// What it names.
    pub kind: Kind,
    /// The declared URL, resolved, or as written when templated.
    pub url: String,
    /// Where it was declared: a path on the audited origin, or a document
    /// entry as a JSON Pointer.
    pub source: String,
    /// Settled not followed for this reason before the slice runs.
    pub not_followed: Option<&'static str>,
}

impl Declaration {
    /// A declaration the slice may request.
    pub fn new(kind: Kind, url: String, source: String) -> Self {
        Declaration {
            kind,
            url,
            source,
            not_followed: None,
        }
    }
}

/// How the audit treated a declared URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Admitted, and read when it is a document.
    Followed,
    /// Nothing the audit can request, or nothing on the host confirmed it.
    ReciprocityRefused,
    /// Left alone, for the entry's reason.
    NotFollowed,
    /// The guard refuses the host.
    Blocked,
    /// No response, or none that could be read.
    Unreachable,
    /// A cap or the slice clock stopped it.
    BudgetExceeded,
}

impl Outcome {
    /// The trail spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Followed => "followed",
            Outcome::ReciprocityRefused => "reciprocity-refused",
            Outcome::NotFollowed => "not-followed",
            Outcome::Blocked => "blocked",
            Outcome::Unreachable => "unreachable",
            Outcome::BudgetExceeded => "budget-exceeded",
        }
    }
}

/// How one declaration settled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    /// Where its redirect led, when it led somewhere else.
    pub final_url: Option<String>,
    /// The outcome.
    pub outcome: Outcome,
    /// The artifact that admitted an endpoint, on `followed`.
    pub admitted_by: Option<&'static str>,
    /// What stopped it, on `budget-exceeded`.
    pub cause: Option<&'static str>,
    /// Why, on `not-followed`.
    pub reason: Option<&'static str>,
}

impl Settled {
    pub(super) fn of(outcome: Outcome) -> Self {
        Settled {
            final_url: None,
            outcome,
            admitted_by: None,
            cause: None,
            reason: None,
        }
    }

    pub(super) fn not_followed(reason: &'static str) -> Self {
        Settled {
            reason: Some(reason),
            ..Settled::of(Outcome::NotFollowed)
        }
    }

    pub(super) fn budget(cause: &'static str) -> Self {
        Settled {
            cause: Some(cause),
            ..Settled::of(Outcome::BudgetExceeded)
        }
    }

    pub(super) fn landed(final_url: Option<String>, outcome: Outcome) -> Self {
        Settled {
            final_url,
            ..Settled::of(outcome)
        }
    }
}

/// One declared URL and how the audit treated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrailEntry {
    /// What the document declares there.
    pub kind: Kind,
    /// The declared URL.
    pub url: String,
    /// Where it is declared.
    pub source: String,
    /// How it settled.
    pub settled: Settled,
}

impl TrailEntry {
    /// The entry for a declaration.
    pub fn of(declaration: &Declaration, settled: Settled) -> Self {
        TrailEntry {
            kind: declaration.kind,
            url: declaration.url.clone(),
            source: declaration.source.clone(),
            settled,
        }
    }

    /// The `declared_hosts` item, in the site's key order.
    pub fn to_item(&self) -> EvidenceItem {
        let mut item = EvidenceItem::new();
        item.insert("surface".into(), json!(self.source));
        item.insert("kind".into(), json!(self.kind.as_str()));
        item.insert("url".into(), json!(self.url));
        if let Some(host) = host_of(&self.url) {
            item.insert("host".into(), json!(host));
        }
        if let Some(final_url) = &self.settled.final_url {
            item.insert("final_url".into(), json!(final_url));
        }
        item.insert("outcome".into(), json!(self.settled.outcome.as_str()));
        if let Some(by) = self.settled.admitted_by {
            item.insert("admitted_by".into(), json!(by));
        }
        if let Some(cause) = self.settled.cause {
            item.insert("cause".into(), json!(cause));
        }
        if let Some(reason) = self.settled.reason {
            item.insert("reason".into(), json!(reason));
        }
        item
    }
}

/// The WHATWG host, port kept, or `None` when the URL does not parse.
pub fn host_of(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// RFC 3986 syntax normalization as the WHATWG parser applies it, fragment
/// dropped; `None` when the value is not an absolute URL.
pub fn normalize_endpoint_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    url.set_fragment(None);
    Some(url.to_string())
}

/// One trail entry per declared URL: the key a declaration and its entry share.
pub fn declaration_key(kind: Kind, url: &str) -> String {
    let normalized = normalize_endpoint_url(url);
    format!("{} {}", kind.as_str(), normalized.as_deref().unwrap_or(url))
}

/// A declaration names a host other than the audited one.
pub fn declares_host(declaration: &Declaration, base: &str) -> bool {
    !same_origin(&declaration.url, base)
}

/// The declarations in order, each URL kept at its first place.
pub fn unique(declarations: &[Declaration]) -> Vec<Declaration> {
    let mut keys: Vec<String> = Vec::new();
    declarations
        .iter()
        .filter(|d| {
            let key = declaration_key(d.kind, &d.url);
            let first = !keys.contains(&key);
            keys.push(key);
            first
        })
        .cloned()
        .collect()
}

/// A declared host a row could not be evaluated at, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredHostReason {
    /// The row's `na_reason`.
    pub reason: NaReason,
    /// The host the row reads as.
    pub host: String,
    /// The URL the reason is about: the redirect's target, else the declared URL.
    pub url: String,
}

/// Why a row could not be evaluated at an entry's host, or `None` when the
/// entry names no such reason.
pub fn declared_host_reason(entry: &TrailEntry) -> Option<DeclaredHostReason> {
    let reason = match entry.settled.outcome {
        Outcome::NotFollowed if entry.settled.reason == Some("follow-disabled") => {
            NaReason::FollowDisabled
        }
        Outcome::ReciprocityRefused => NaReason::ReciprocityRefused,
        Outcome::Blocked => NaReason::DeclaredHostBlocked,
        Outcome::Unreachable => NaReason::DeclaredHostUnreachable,
        Outcome::BudgetExceeded => NaReason::DeclaredHostBudgetExceeded,
        Outcome::Followed | Outcome::NotFollowed => return None,
    };
    let url = entry
        .settled
        .final_url
        .clone()
        .unwrap_or_else(|| entry.url.clone());
    let host = host_of(&url).filter(|h| !h.is_empty())?;
    Some(DeclaredHostReason { reason, host, url })
}
