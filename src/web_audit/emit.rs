//! The offline readers behind `anc emit web-checks` and
//! `anc emit web-remediation`: serializers over the registry tables
//! compiled into the binary, so `anc web --check <id>` has a discoverable
//! argument and an agent can read the whole fix catalog without a network.

use serde::Serialize;

use crate::principles::registry::SPEC_VERSION;
use crate::web_audit::registry::{
    CATEGORIES, CHECKS, MCP_DISCOVERY, REGISTRY_VERSION, REMEDIATION, SITE_SHA, WebCheck,
    WebRemediation,
};

/// One check, as `anc emit web-checks` prints it.
#[derive(Debug, Serialize)]
pub struct CheckEntry {
    /// The id `--check` accepts.
    pub id: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Category slug.
    pub category: &'static str,
    /// Category display name.
    pub category_name: &'static str,
    /// `required`, `recommended` or `optional`.
    pub tier: &'static str,
    /// `must`, `should` or `may`.
    pub keyword: &'static str,
    /// The spec principle the check serves.
    pub principle: &'static str,
    /// The site kinds the check applies to.
    pub site_types: Vec<&'static str>,
    /// The runtime condition that has to hold before the check is scored.
    pub antecedent: &'static str,
    /// The probe handler that decides the row.
    pub handler: &'static str,
    /// The eval rule that decides the row, when it has one.
    pub eval: Option<&'static str>,
    /// The check's weight in the score.
    pub weight: u32,
    /// The short breadcrumb the report uses.
    pub breadcrumb: &'static str,
    /// What to do about a miss, in one line.
    pub hint: &'static str,
}

impl From<&'static WebCheck> for CheckEntry {
    fn from(check: &'static WebCheck) -> Self {
        CheckEntry {
            id: check.id,
            label: check.title,
            category: check.category,
            category_name: CATEGORIES
                .iter()
                .find(|(slug, _)| *slug == check.category)
                .map_or("", |(_, name)| *name),
            tier: check.tier.as_str(),
            keyword: check.keyword.as_str(),
            principle: check.principle,
            site_types: check.site_types.iter().map(|t| t.as_str()).collect(),
            antecedent: check.antecedent.as_str(),
            handler: check.handler.as_str(),
            eval: check.eval.map(|e| e.as_str()),
            weight: check.weight,
            breadcrumb: check.breadcrumb,
            hint: check.hint,
        }
    }
}

/// The MCP discovery configuration a run probes with.
#[derive(Debug, Serialize)]
pub struct DiscoveryEntry {
    /// Well-known card paths, in probe order.
    pub well_known: &'static [&'static str],
    /// Endpoint paths the legacy and modern passes probe.
    pub common_paths: &'static [&'static str],
    /// The protocol version the legacy handshake claims.
    pub protocol_version: &'static str,
}

/// The whole check registry, as one document.
#[derive(Debug, Serialize)]
pub struct ChecksDocument {
    /// The vendored registry's version.
    pub registry_version: u64,
    /// The anc.dev commit the registry was vendored from.
    pub site_sha: &'static str,
    /// The vendored spec version this build scores against.
    pub spec_version: &'static str,
    /// Category slugs in report order.
    pub category_order: &'static [&'static str],
    /// The MCP discovery configuration.
    pub mcp_discovery: DiscoveryEntry,
    /// Every check, in registry order.
    pub checks: Vec<CheckEntry>,
}

/// One fix-catalog entry, as `anc emit web-remediation` prints it.
#[derive(Debug, Serialize)]
pub struct RemediationEntry {
    /// The check id this fixes.
    pub check_id: &'static str,
    /// The entry's title.
    pub title: &'static str,
    /// What the fix achieves, in one line.
    pub goal: &'static str,
    /// The fix itself, as markdown.
    pub fix: &'static str,
    /// Where to read more.
    pub resources: Vec<ResourceEntry>,
}

/// A doc link on a fix-catalog entry.
#[derive(Debug, Serialize)]
pub struct ResourceEntry {
    /// Link text.
    pub label: &'static str,
    /// Link target.
    pub url: &'static str,
}

impl From<&'static WebRemediation> for RemediationEntry {
    fn from(entry: &'static WebRemediation) -> Self {
        RemediationEntry {
            check_id: entry.id,
            title: entry.title,
            goal: entry.goal,
            fix: entry.fix,
            resources: entry
                .resources
                .iter()
                .map(|r| ResourceEntry {
                    label: r.label,
                    url: r.url,
                })
                .collect(),
        }
    }
}

/// The whole fix catalog, as one document.
#[derive(Debug, Serialize)]
pub struct RemediationDocument {
    /// The vendored registry's version.
    pub registry_version: u64,
    /// The anc.dev commit the catalog was vendored from.
    pub site_sha: &'static str,
    /// Every entry, in check-id order.
    pub remediation: Vec<RemediationEntry>,
}

fn to_json<T: Serialize>(value: &T) -> String {
    let mut out = serde_json::to_string_pretty(value).expect("a registry projection serializes");
    out.push('\n');
    out
}

/// The `anc emit web-checks` document.
pub fn checks_document() -> ChecksDocument {
    ChecksDocument {
        registry_version: REGISTRY_VERSION,
        site_sha: SITE_SHA,
        spec_version: SPEC_VERSION,
        category_order: crate::web_audit::registry::CATEGORY_ORDER,
        mcp_discovery: DiscoveryEntry {
            well_known: MCP_DISCOVERY.well_known,
            common_paths: MCP_DISCOVERY.common_paths,
            protocol_version: MCP_DISCOVERY.protocol_version,
        },
        checks: CHECKS.iter().map(CheckEntry::from).collect(),
    }
}

/// `anc emit web-checks` output.
pub fn render_checks() -> String {
    to_json(&checks_document())
}

/// `anc emit web-remediation` output.
pub fn render_remediation() -> String {
    to_json(&RemediationDocument {
        registry_version: REGISTRY_VERSION,
        site_sha: SITE_SHA,
        remediation: REMEDIATION.iter().map(RemediationEntry::from).collect(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn the_checks_document_lists_every_registry_id_in_order() {
        let rendered = render_checks();
        assert!(rendered.ends_with('\n'));
        let parsed: Value = serde_json::from_str(&rendered).unwrap();
        let ids: Vec<&str> = parsed["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        let registry: Vec<&str> = CHECKS.iter().map(|c| c.id).collect();
        assert_eq!(ids, registry);
        assert_eq!(ids.len(), 65);
        assert_eq!(parsed["registry_version"], Value::from(REGISTRY_VERSION));
        assert_eq!(parsed["site_sha"], Value::from(SITE_SHA));
        assert_eq!(
            parsed["mcp_discovery"]["protocol_version"].as_str(),
            Some("2025-06-18")
        );
        let robots = parsed["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "robots")
            .unwrap();
        assert_eq!(robots["keyword"], Value::from("should"));
        assert_eq!(robots["tier"], Value::from("recommended"));
        assert_eq!(robots["handler"], Value::from("http"));
        assert_eq!(robots["eval"], Value::Null);
        assert_eq!(robots["site_types"], Value::from(vec!["all"]));
        assert!(!robots["hint"].as_str().unwrap().is_empty());
        assert!(!robots["category_name"].as_str().unwrap().is_empty());
    }

    #[test]
    fn the_fix_catalog_has_one_entry_per_check() {
        let rendered = render_remediation();
        let parsed: Value = serde_json::from_str(&rendered).unwrap();
        let entries = parsed["remediation"].as_array().unwrap();
        assert_eq!(entries.len(), CHECKS.len());
        let mut ids: Vec<&str> = entries
            .iter()
            .map(|e| e["check_id"].as_str().unwrap())
            .collect();
        let mut registry: Vec<&str> = CHECKS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        registry.sort_unstable();
        assert_eq!(ids, registry);
        for entry in entries {
            assert!(!entry["goal"].as_str().unwrap().is_empty());
            assert!(!entry["fix"].as_str().unwrap().is_empty());
            assert!(entry["resources"].is_array());
        }
    }
}
