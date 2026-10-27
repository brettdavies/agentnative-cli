//! Where the API rows evaluate when the API catalog lists API anchors, a
//! mirror of the site's `api-targets.ts`: the OpenAPI row at each anchor's
//! description, and the hygiene rows at each anchor host, in declaration
//! order. A target off the audited origin is evaluated only when the follow
//! settled it followed; otherwise it carries the reason its rows read, or
//! is left out when the trail names none (a templated URL, a path in the
//! auditor's own zone). A target declared only over plaintext is left out
//! beside any other target, and stands alone as a miss when there is none,
//! so declaring an API over http never scores above declaring nothing.

use std::collections::HashMap;

use url::Url;

use super::api_catalog::{ApiKind, CatalogAnchor};
use super::api_follow::{ApiFollowResult, Fetched};
use super::api_trail::{
    DeclaredHostReason, Outcome, TrailEntry, declaration_key, declared_host_reason,
};
use super::discovery_documents::same_origin;
use crate::web_audit::handlers::shared::PlaintextReason;

/// How a description the catalog declares is evaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DescriptionReach {
    /// The follow read it off the audited origin.
    Fetched(Fetched),
    /// It is on the audited origin; the row reads it.
    OnOrigin,
    /// Not evaluated, and why.
    Unmet(DeclaredHostReason),
    /// Declared only over plaintext, so never requested.
    Plaintext(PlaintextReason),
}

/// An OpenAPI description the catalog declares, by its declared URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiDescriptionTarget {
    /// The declared URL.
    pub url: String,
    /// How it is evaluated.
    pub reach: DescriptionReach,
}

/// One anchor on a host: its URL and the description it declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostAnchor {
    /// The anchor URL.
    pub url: String,
    /// The description it declares.
    pub description: String,
}

/// One API anchor host, where the hygiene probes go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiHostTarget {
    /// The host's origin.
    pub origin: String,
    /// The anchors on this host, in declaration order.
    pub anchors: Vec<HostAnchor>,
    /// Off the audited origin, so a probe of it takes no redirect to another origin.
    pub declared: bool,
    /// Why the host was not evaluated.
    pub unmet: Option<DeclaredHostReason>,
    /// Declared only over plaintext, so nothing was requested from it.
    pub plaintext: Option<PlaintextReason>,
}

/// The API rows' targets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiTargets {
    /// The descriptions the OpenAPI row scores.
    pub descriptions: Vec<ApiDescriptionTarget>,
    /// The hosts the hygiene rows probe.
    pub hosts: Vec<ApiHostTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Reach {
    AuditedOrigin,
    Evaluate,
    Omit,
    Plaintext(PlaintextReason),
    Unmet(DeclaredHostReason),
}

fn plaintext_reason(entry: &TrailEntry) -> Option<PlaintextReason> {
    if entry.settled.outcome != Outcome::NotFollowed
        || entry.settled.reason != Some("insecure-scheme")
    {
        return None;
    }
    Some(match entry.settled.final_url {
        None => PlaintextReason::NotHttps,
        Some(_) => PlaintextReason::RedirectsToHttp,
    })
}

/// Plaintext targets only when there is nothing else to evaluate.
fn plaintext_alone<T>(targets: Vec<T>, plaintext: impl Fn(&T) -> bool) -> Vec<T> {
    if targets.iter().all(&plaintext) {
        targets
    } else {
        targets.into_iter().filter(|t| !plaintext(t)).collect()
    }
}

fn origin_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

/// The API rows' targets, or `None` when the catalog lists no API anchor
/// and the rows evaluate the audited origin.
pub fn api_targets(
    base: &str,
    anchors: &[CatalogAnchor],
    follow: &ApiFollowResult,
) -> Option<ApiTargets> {
    let api: Vec<&CatalogAnchor> = anchors.iter().filter(|a| a.is_api()).collect();
    if api.is_empty() {
        return None;
    }
    let settled: HashMap<String, &TrailEntry> = follow
        .entries
        .iter()
        .map(|e| (declaration_key(e.kind, &e.url), e))
        .collect();
    let reach = |kind: ApiKind, url: &str| -> Reach {
        if same_origin(url, base) {
            return Reach::AuditedOrigin;
        }
        let Some(entry) = settled.get(&declaration_key(kind, url)) else {
            return Reach::Omit;
        };
        if entry.settled.outcome == Outcome::Followed {
            return Reach::Evaluate;
        }
        plaintext_reason(entry)
            .map(Reach::Plaintext)
            .or_else(|| declared_host_reason(entry).map(Reach::Unmet))
            .unwrap_or(Reach::Omit)
    };

    let mut descriptions: Vec<(String, ApiDescriptionTarget)> = Vec::new();
    let mut origins: Vec<(String, bool, Vec<Reach>, Vec<HostAnchor>)> = Vec::new();
    for anchor in api {
        let declared = anchor.description.as_ref().map_or("", |d| d.url.as_str());
        let key = declaration_key(ApiKind::Description, declared);
        if !descriptions.iter().any(|(k, _)| *k == key) {
            let target = match reach(ApiKind::Description, declared) {
                Reach::Omit => None,
                Reach::AuditedOrigin => Some(DescriptionReach::OnOrigin),
                Reach::Evaluate => follow
                    .descriptions
                    .get(&key)
                    .cloned()
                    .map(DescriptionReach::Fetched),
                Reach::Plaintext(r) => Some(DescriptionReach::Plaintext(r)),
                Reach::Unmet(r) => Some(DescriptionReach::Unmet(r)),
            };
            if let Some(reach) = target {
                let url = declared.to_string();
                descriptions.push((key, ApiDescriptionTarget { url, reach }));
            }
        }
        let Some(origin) = origin_of(&anchor.link.url) else {
            continue;
        };
        let index = match origins.iter().position(|(o, ..)| *o == origin) {
            Some(index) => index,
            None => {
                let declared = !same_origin(&anchor.link.url, base);
                origins.push((origin, declared, Vec::new(), Vec::new()));
                origins.len() - 1
            }
        };
        let group = &mut origins[index];
        group.2.push(reach(ApiKind::Anchor, &anchor.link.url));
        group.3.push(HostAnchor {
            url: anchor.link.url.clone(),
            description: declared.to_string(),
        });
    }

    let hosts = origins
        .into_iter()
        .filter_map(|(origin, declared, reaches, anchors)| {
            let mut target = ApiHostTarget {
                origin,
                anchors,
                declared,
                unmet: None,
                plaintext: None,
            };
            if reaches
                .iter()
                .any(|r| matches!(r, Reach::Evaluate | Reach::AuditedOrigin))
            {
                return Some(target);
            }
            target.unmet = reaches.iter().find_map(|r| match r {
                Reach::Unmet(reason) => Some(reason.clone()),
                _ => None,
            });
            if target.unmet.is_some() {
                return Some(target);
            }
            target.plaintext = reaches.iter().find_map(|r| match r {
                Reach::Plaintext(reason) => Some(*reason),
                _ => None,
            });
            target.plaintext.is_some().then_some(target)
        })
        .collect();
    Some(ApiTargets {
        descriptions: plaintext_alone(descriptions.into_iter().map(|(_, t)| t).collect(), |t| {
            matches!(t.reach, DescriptionReach::Plaintext(_))
        }),
        hosts: plaintext_alone(hosts, |t| t.plaintext.is_some()),
    })
}
