//! The vendored web-audit check registry, compiled into the binary.
//!
//! `build.rs` reads the site's vendoring contract under
//! `src/web_audit/vendored/contract/` and emits every table and engine setting as a
//! `const` into `$OUT_DIR/generated_web_registry.rs`, which this module includes. The
//! tables are the check definitions (`CHECKS`, in registry order, which is
//! also scorecard row order), the display categories, the MCP protocol
//! lanes, MCP discovery configuration, the fix catalog, the probe User-Agent strings, and the
//! pinned site commit (`SITE_SHA`). Nothing here is parsed at runtime.
//!
//! A check whose handler kind or eval rule the engine has not ported binds to
//! `HandlerBinding::Unsupported` / `EvalBinding::Unsupported` carrying the
//! registry name, so the engine reports it as a skip instead of the vendor
//! pin waiting on a port.

/// MCP endpoint discovery configuration (the registry's `mcp_discovery` block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpDiscovery {
    /// The AI catalog path, read with the well-known cards.
    pub ai_catalog: &'static str,
    /// The path SEP-2127 appends to an endpoint to locate its card.
    pub card_suffix: &'static str,
    /// SEP-1649 well-known card paths, read in the first document round.
    pub well_known: &'static [&'static str],
    /// Common endpoint paths POSTed to when no card names an endpoint.
    pub common_paths: &'static [&'static str],
    /// The MCP protocol version the legacy `initialize` probe claims.
    pub protocol_version: &'static str,
}

/// A regex-bearing `with` value, translated for the `regex` crate at build
/// time. `source` and `flags` are the JavaScript form the site evaluates;
/// `rust` compiles under the same semantics (see `build_support/js_regex.rs`
/// for the divergences the translation cannot close).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegexPattern {
    /// `content_type`, `header_regex`, `body_regex` or `body_not_regex`.
    pub field: &'static str,
    /// The pattern as written in the registry.
    pub source: &'static str,
    /// The JavaScript flags `assert.ts` applies (`i` or `im`).
    pub flags: &'static str,
    /// The equivalent `regex` crate pattern, inline flags included.
    pub rust: &'static str,
}

/// One registry check, normalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebCheck {
    /// Stable slug; also the remediation key.
    pub id: &'static str,
    /// Display category slug (one of `CATEGORY_ORDER`).
    pub category: &'static str,
    /// Registry tier.
    pub tier: WebCheckTier,
    /// Keyword derived from `tier`.
    pub keyword: WebCheckKeyword,
    /// Internal principle tag (`P1`..`P8`).
    pub principle: &'static str,
    /// Declared-type filter.
    pub site_types: &'static [WebCheckSiteType],
    /// Runtime gate.
    pub antecedent: AntecedentToken,
    /// Optional non-standard evaluation rule.
    pub eval: Option<EvalBinding>,
    /// Registry weight.
    pub weight: u32,
    /// Human title.
    pub title: &'static str,
    /// Short breadcrumb label.
    pub breadcrumb: &'static str,
    /// One-line remediation summary.
    pub hint: &'static str,
    /// The `MCP_LANES` id a `LANE_CATEGORY` check groups under; `None`
    /// outside that category. Display only: no probe or score reads it.
    pub lane: Option<&'static str>,
    /// Probe handler.
    pub handler: HandlerBinding,
    /// The `with` handler parameters as a JSON object, `{ua:...}` tokens
    /// already expanded, for the handler to deserialize.
    pub with_json: &'static str,
    /// Every regex-bearing value in `with`, translated.
    pub patterns: &'static [RegexPattern],
}

/// One protocol lane the MCP category's rows group under in the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpLane {
    /// The id a check's `lane` names.
    pub id: &'static str,
    /// Display label.
    pub label: &'static str,
    /// One line saying what the lane covers.
    pub note: &'static str,
}

/// One site design in a group of alternatives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlternativeVariant {
    /// The variant's name within its group.
    pub name: &'static str,
    /// The checks gated on these tokens form the variant; empty for a
    /// design with no checks of its own.
    pub antecedents: &'static [AntecedentToken],
    /// The variant is presented when a check gated on one of these tokens applied.
    pub presented_by: &'static [AntecedentToken],
}

/// Site designs that cannot both be satisfied at full access, whatever the
/// audit's vantage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlternativeGroup {
    /// The group's name.
    pub group: &'static str,
    /// Its designs, in registry order.
    pub variants: &'static [AlternativeVariant],
}

/// A doc link attached to a fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemediationResource {
    /// Link text.
    pub label: &'static str,
    /// Absolute URL.
    pub url: &'static str,
}

/// The fix catalog entry for one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebRemediation {
    /// The check id.
    pub id: &'static str,
    /// Short human title.
    pub title: &'static str,
    /// One-line imperative goal.
    pub goal: &'static str,
    /// The canonical fix, markdown.
    pub fix: &'static str,
    /// Doc links.
    pub resources: &'static [RemediationResource],
}

include!(concat!(env!("OUT_DIR"), "/generated_web_registry.rs"));

impl WebCheck {
    /// The translated pattern for a `with` field, when the check carries one.
    pub fn pattern(&self, field: &str) -> Option<&'static RegexPattern> {
        self.patterns.iter().find(|p| p.field == field)
    }
}

/// The check with this id, if any.
pub fn check_by_id(id: &str) -> Option<&'static WebCheck> {
    CHECKS.iter().find(|c| c.id == id)
}

/// The fix catalog entry for a check id, if any.
pub fn remediation_for(id: &str) -> Option<&'static WebRemediation> {
    REMEDIATION
        .binary_search_by(|r| r.id.cmp(id))
        .ok()
        .map(|i| &REMEDIATION[i])
}

/// The protocol lane a check groups under, if any.
pub fn lane_of(id: &str) -> Option<&'static McpLane> {
    let lane = check_by_id(id)?.lane?;
    MCP_LANES.iter().find(|l| l.id == lane)
}

/// The display name of a category slug, if any.
pub fn category_name(slug: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find(|(s, _)| *s == slug)
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn the_engine_runs_a_fixed_set_of_kinds_over_a_vendored_table_of_checks() {
        assert!(!CHECKS.is_empty());
        assert_eq!(CATEGORIES.len(), CATEGORY_ORDER.len());
        assert_eq!(HANDLER_KINDS.len(), 13);
        assert_eq!(EVAL_RULES.len(), 4);
    }

    #[test]
    fn every_lane_in_the_map_is_distinct_and_says_what_it_covers() {
        let ids: BTreeSet<&str> = MCP_LANES.iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), MCP_LANES.len());
        for lane in MCP_LANES {
            assert!(!lane.label.is_empty() && !lane.note.is_empty(), "{lane:?}");
        }
    }

    #[test]
    fn every_mcp_check_and_no_other_resolves_to_a_lane() {
        for c in CHECKS {
            assert_eq!(
                lane_of(c.id).is_some(),
                c.category == LANE_CATEGORY,
                "{}: lane {:?}",
                c.id,
                c.lane
            );
        }
        assert_eq!(lane_of("no-such-check"), None);
    }

    #[test]
    fn every_ua_token_is_expanded_to_a_vendored_literal() {
        let literals: Vec<&str> = PROBE_UA_TOKENS.iter().map(|(_, v)| *v).collect();
        for c in CHECKS {
            assert!(
                !c.with_json.contains("{ua:"),
                "{}: unexpanded token in {}",
                c.id,
                c.with_json
            );
            let with: serde_json::Value =
                serde_json::from_str(c.with_json).expect("with_json parses");
            if let Some(ua) = with
                .get("headers")
                .and_then(|h| h.get("User-Agent"))
                .and_then(|v| v.as_str())
            {
                assert!(
                    literals.contains(&ua),
                    "{}: User-Agent {ua:?} is not a vendored literal",
                    c.id
                );
            }
        }
        assert!(AUDIT_USER_AGENT.starts_with("anc-web-audit/"));
    }

    #[test]
    fn remediation_is_one_to_one_and_sorted() {
        assert_eq!(REMEDIATION.len(), CHECKS.len() + RETIRED_CHECK_IDS.len());
        for id in RETIRED_CHECK_IDS {
            assert!(remediation_for(id).is_some(), "{id}: no remediation");
        }
        for c in CHECKS {
            let r = remediation_for(c.id).unwrap_or_else(|| panic!("{}: no remediation", c.id));
            assert!(!r.goal.is_empty() && !r.fix.is_empty());
        }
        assert!(REMEDIATION.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn site_sha_is_a_full_commit_sha() {
        assert_eq!(SITE_SHA.len(), 40);
        assert!(SITE_SHA.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn keyword_follows_tier() {
        for c in CHECKS {
            let expected = match c.tier {
                WebCheckTier::Required => WebCheckKeyword::Must,
                WebCheckTier::Recommended => WebCheckKeyword::Should,
                WebCheckTier::Optional => WebCheckKeyword::May,
            };
            assert_eq!(c.keyword, expected, "{}", c.id);
        }
    }

    #[test]
    fn category_lookups_resolve() {
        for slug in CATEGORY_ORDER {
            assert!(category_name(slug).is_some(), "{slug}");
        }
        for c in CHECKS {
            assert!(
                CATEGORY_ORDER.contains(&c.category),
                "{}: {}",
                c.id,
                c.category
            );
        }
        assert!(check_by_id("robots").is_some());
        assert!(check_by_id("no-such-check").is_none());
    }

    #[test]
    fn every_pattern_compiles_at_runtime() {
        let mut count = 0;
        for c in CHECKS {
            for p in c.patterns {
                assert!(matches!(
                    p.field,
                    "content_type" | "header_regex" | "body_regex" | "body_not_regex"
                ));
                assert!(
                    p.rust.starts_with("(?i)") || p.rust.starts_with("(?imR)"),
                    "{}/{}",
                    c.id,
                    p.field
                );
                count += 1;
            }
        }
        assert!(
            count > 20,
            "expected the vendored registry to carry patterns, got {count}"
        );
        let robots = check_by_id("robots-ai-rules").expect("robots-ai-rules");
        assert!(robots.pattern("body_regex").is_some());
        assert!(robots.pattern("content_type").is_none());
    }
}
