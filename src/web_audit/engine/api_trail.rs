//! The declared-hosts trail for the API catalog, a mirror of the site's
//! `follow-trail.ts` and of `declaredHostReason` in `endpoint-of-record.ts`:
//! the closed vocabulary of how the audit treated a declared URL, the
//! refusals settled before any request, and the reason a row reads when a
//! declared host kept it from being evaluated.

use serde_json::json;
use url::Url;

use super::api_catalog::{ApiDeclaration, ApiKind};
use super::discovery_documents::is_templated_url;
use super::types::FetchHandle;
use crate::web_audit::locality::Locality;
use crate::web_audit::scorecard::{EvidenceItem, NaReason};

// The auditor's own zone, which a declaration never reaches.
const SELF_ZONE: &str = "anc.dev";

/// How the audit treated a declared URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Admitted, and read when it is a description.
    Followed,
    /// Nothing the audit can request.
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
}

/// One declared URL and how the audit treated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrailEntry {
    /// What the catalog declares there.
    pub kind: ApiKind,
    /// The declared URL.
    pub url: String,
    /// Where the catalog declares it.
    pub source: String,
    /// How it settled.
    pub settled: Settled,
}

impl TrailEntry {
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

/// One trail entry per declared URL: the key a declaration and its entry share.
pub fn declaration_key(kind: ApiKind, url: &str) -> String {
    let normalized = Url::parse(url).map(|mut u| {
        u.set_fragment(None);
        u.to_string()
    });
    format!("{} {}", kind.as_str(), normalized.as_deref().unwrap_or(url))
}

/// Where the URL may not be requested at all, or `None` when it may: only
/// https, never an IP literal or a host the guard refuses, never the
/// auditor's own zone.
pub(super) fn refusal(
    url: &str,
    fetch: &FetchHandle,
    target_locality: Option<Locality>,
) -> Option<Settled> {
    let parsed = match Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => return Some(Settled::of(Outcome::ReciprocityRefused)),
    };
    match fetch.validate(url) {
        Err(reason) if reason.starts_with("blocked") => return Some(Settled::of(Outcome::Blocked)),
        Err(_) => return Some(Settled::of(Outcome::ReciprocityRefused)),
        Ok(class) if target_locality.is_some_and(|t| t != class) => {
            return Some(Settled::of(Outcome::Blocked));
        }
        Ok(_) => {}
    }
    if matches!(parsed.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
        return Some(Settled::of(Outcome::Blocked));
    }
    if parsed.scheme() != "https" {
        return Some(Settled::not_followed("insecure-scheme"));
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    if host == SELF_ZONE || host.ends_with(&format!(".{SELF_ZONE}")) {
        return Some(Settled::not_followed("self-path"));
    }
    None
}

/// How a declaration settles before any request, or `None` when it must be requested.
pub(super) fn upfront(
    d: &ApiDeclaration,
    fetch: &FetchHandle,
    target_locality: Option<Locality>,
) -> Option<Settled> {
    if d.no_service_desc {
        return Some(Settled::not_followed("no-service-desc"));
    }
    if is_templated_url(&d.url) {
        return Some(Settled::not_followed("templated-url"));
    }
    refusal(&d.url, fetch, target_locality)
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
