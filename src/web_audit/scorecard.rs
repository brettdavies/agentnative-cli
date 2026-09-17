//! The web scorecard, a Rust mirror of the site's `WebScorecard` pinned to
//! the wire JSON.
//!
//! The mirror is strict on purpose: an unknown field or status string fails
//! deserialization naming the value, so an addition on the site side breaks
//! a build here instead of being misread at runtime. Every number is an
//! integer type, because the site emits only half-up rounded integers and
//! `JSON.stringify` prints `85` where a float mirror would print `85.0`.
//! Field order is the site's emission order, `na_reason` and `unprobed` are
//! absent rather than null when unset, and `evidence` is null when empty,
//! so a scorecard serialized here is byte-comparable to one the site emits.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::registry::{WebCheck, WebCheckKeyword, WebCheckTier};
use super::score::{self, ScoreConfig};

/// Web scorecard schema version, independent of the CLI scorecard schema.
pub const WEB_SCHEMA_VERSION: &str = "0.4";

/// The seven final states a check can settle to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScorecardStatus {
    /// The surface is present and valid.
    Pass,
    /// The surface works while violating a spec detail.
    Noncompliant,
    /// The surface is present but invalid; an agent that finds it is misled.
    Broken,
    /// The surface is not there.
    Absent,
    /// The check does not apply, or an applicable MAY is not implemented.
    #[serde(rename = "n_a")]
    NA,
    /// The per-audit deadline passed before the check ran.
    Skip,
    /// The probe itself failed.
    Error,
}

impl ScorecardStatus {
    /// Every status, in the site's tally order.
    pub const ALL: [ScorecardStatus; 7] = [
        ScorecardStatus::Pass,
        ScorecardStatus::Noncompliant,
        ScorecardStatus::Broken,
        ScorecardStatus::Absent,
        ScorecardStatus::NA,
        ScorecardStatus::Skip,
        ScorecardStatus::Error,
    ];

    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ScorecardStatus::Pass => "pass",
            ScorecardStatus::Noncompliant => "noncompliant",
            ScorecardStatus::Broken => "broken",
            ScorecardStatus::Absent => "absent",
            ScorecardStatus::NA => "n_a",
            ScorecardStatus::Skip => "skip",
            ScorecardStatus::Error => "error",
        }
    }
}

impl fmt::Display for ScorecardStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a row is `n_a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NaReason {
    /// The check does not apply to this site (declared type or runtime antecedent).
    AntecedentUnmet,
    /// An applicable MAY that is simply not implemented.
    OptionalAbsent,
    /// The probed surface pair shows a deliberate, consistent opt-out.
    PostureConsistent,
}

/// The declared site type a run scopes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclaredSiteType {
    /// A documentation or content site.
    Content,
    /// An API.
    Api,
}

/// The one layer a web scorecard row can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// A web-audit row.
    Web,
}

/// A field the site always emits as `null`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Null;

impl Serialize for Null {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_none()
    }
}

impl<'de> Deserialize<'de> for Null {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NullVisitor;
        impl Visitor<'_> for NullVisitor {
            type Value = Null;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("null")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Null, E> {
                Ok(Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Null, E> {
                Ok(Null)
            }
        }
        deserializer.deserialize_option(NullVisitor)
    }
}

/// A handler's structured evidence row, kept open like the site's.
pub type EvidenceItem = serde_json::Map<String, serde_json::Value>;

/// Web identity of the audited target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    /// The audited host.
    pub name: String,
    /// The normalized audited URL.
    pub url: String,
}

/// Tally of every check by its final status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusTally {
    /// Passing rows.
    pub pass: u32,
    /// Noncompliant rows.
    pub noncompliant: u32,
    /// Broken rows.
    pub broken: u32,
    /// Absent rows.
    pub absent: u32,
    /// Not-applicable rows.
    pub n_a: u32,
    /// Skipped rows.
    pub skip: u32,
    /// Errored rows.
    pub error: u32,
}

impl StatusTally {
    /// Add one row to the tally.
    pub fn count(&mut self, status: ScorecardStatus) {
        let slot = match status {
            ScorecardStatus::Pass => &mut self.pass,
            ScorecardStatus::Noncompliant => &mut self.noncompliant,
            ScorecardStatus::Broken => &mut self.broken,
            ScorecardStatus::Absent => &mut self.absent,
            ScorecardStatus::NA => &mut self.n_a,
            ScorecardStatus::Skip => &mut self.skip,
            ScorecardStatus::Error => &mut self.error,
        };
        *slot += 1;
    }
}

/// How many rows applied at one keyword level and how many passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageLevel {
    /// Scored rows at this level.
    pub total: u32,
    /// Passing rows at this level.
    pub verified: u32,
}

/// Coverage at each keyword level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageSummary {
    /// MUST rows.
    pub must: CoverageLevel,
    /// SHOULD rows.
    pub should: CoverageLevel,
    /// MAY rows.
    pub may: CoverageLevel,
}

/// The two-score pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    /// Earned over this site's applicable maximum.
    pub relative: u32,
    /// Earned over the whole registry's maximum.
    pub global: u32,
}

/// A per-category rollup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryRollup {
    /// Category slug.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Passing rows in the category.
    pub passed: u32,
    /// Scored rows in the category.
    pub counted: u32,
}

/// One result row as the scorecard carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRow {
    /// Check id.
    pub id: String,
    /// Human-readable check title.
    pub label: String,
    /// Visible category slug.
    pub category: String,
    /// Mirrors `principle`.
    pub group: String,
    /// Always `web`.
    pub layer: Layer,
    /// Keyword derived from the tier.
    pub keyword: WebCheckKeyword,
    /// Registry tier.
    pub tier: WebCheckTier,
    /// Internal principle tag.
    pub principle: String,
    /// Final status.
    pub status: ScorecardStatus,
    /// Why the row is `n_a`; present only on rows that carry a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub na_reason: Option<NaReason>,
    /// The row settled from an antecedent rather than its own request; present only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unprobed: bool,
    /// Compact summary of what the probe observed; null when there is none.
    pub evidence: Option<String>,
}

/// The web scorecard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebScorecard {
    /// Envelope version.
    pub schema_version: String,
    /// The agentnative spec version the run scored against.
    pub spec_version: String,
    /// Normalized audited URL.
    pub target_url: String,
    /// Discovered MCP endpoint, if any.
    pub mcp_endpoint: Option<String>,
    /// The discovery trail.
    pub mcp_discovery: Vec<EvidenceItem>,
    /// Web identity.
    pub tool: ToolIdentity,
    /// Always null on a web scorecard.
    pub audience: Null,
    /// Always null on a web scorecard.
    pub audit_profile: Null,
    /// The declared site type, or null when everything ran.
    pub site_type: Option<DeclaredSiteType>,
    /// The submitter's opt-in to the public board listing.
    pub public_listing: bool,
    /// Tally by status.
    pub summary: StatusTally,
    /// Coverage by keyword level.
    pub coverage_summary: CoverageSummary,
    /// The headline relative score.
    pub score_pct: u32,
    /// The two-score pair.
    pub score: Score,
    /// Per-category rollups in display order.
    pub categories: Vec<CategoryRollup>,
    /// One row per check.
    pub results: Vec<ResultRow>,
}

/// A check's outcome as the engine finalizes it, before scorecard assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineResult {
    /// The check the row settles.
    pub check: &'static WebCheck,
    /// Final status.
    pub status: ScorecardStatus,
    /// Why the row is `n_a`.
    pub na_reason: Option<NaReason>,
    /// The row settled from an antecedent rather than its own request.
    pub unprobed: bool,
    /// Compact human-readable evidence line; empty when there is none.
    pub evidence: String,
    /// Full structured evidence.
    pub raw_evidence: Vec<EvidenceItem>,
}

/// Run-level facts the scorecard carries beside the rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScorecardMeta {
    /// Normalized audited URL.
    pub target_url: String,
    /// The audited host.
    pub domain: String,
    /// Discovered MCP endpoint, if any.
    pub mcp_endpoint: Option<String>,
    /// The discovery trail.
    pub discovery_evidence: Vec<EvidenceItem>,
    /// The spec version to record.
    pub spec_version: String,
    /// The declared site type.
    pub site_type: Option<DeclaredSiteType>,
    /// The public-listing opt-in.
    pub public_listing: bool,
}

/// Assemble the scorecard from finalized rows, the site's `buildWebScorecard`.
pub fn build_web_scorecard(
    results: &[EngineResult],
    meta: &ScorecardMeta,
    registry_keywords: impl IntoIterator<Item = WebCheckKeyword>,
    category_order: &[&str],
    category_names: &[(&str, &str)],
    config: &ScoreConfig,
) -> WebScorecard {
    let mut summary = StatusTally::default();
    let mut coverage = CoverageSummary::default();
    let mut rows = Vec::with_capacity(results.len());
    for r in results {
        summary.count(r.status);
        if score::is_scored(r.status) {
            let level = match r.check.keyword {
                WebCheckKeyword::Must => &mut coverage.must,
                WebCheckKeyword::Should => &mut coverage.should,
                WebCheckKeyword::May => &mut coverage.may,
            };
            level.total += 1;
            if r.status == ScorecardStatus::Pass {
                level.verified += 1;
            }
        }
        rows.push(ResultRow {
            id: r.check.id.to_string(),
            label: r.check.title.to_string(),
            category: r.check.category.to_string(),
            group: r.check.principle.to_string(),
            layer: Layer::Web,
            keyword: r.check.keyword,
            tier: r.check.tier,
            principle: r.check.principle.to_string(),
            status: r.status,
            na_reason: r.na_reason,
            unprobed: r.unprobed,
            evidence: if r.evidence.is_empty() {
                None
            } else {
                Some(r.evidence.clone())
            },
        });
    }
    let universe_max = score::universe_max(registry_keywords, config);
    let scored = score::score_web_audit(
        results.iter().map(|r| (r.check.keyword, r.status)),
        universe_max,
        config,
    );
    let categories = score::category_rollups(
        results.iter().map(|r| (r.check.category, r.status)),
        category_order,
        category_names,
    );
    WebScorecard {
        schema_version: WEB_SCHEMA_VERSION.to_string(),
        spec_version: meta.spec_version.clone(),
        target_url: meta.target_url.clone(),
        mcp_endpoint: meta.mcp_endpoint.clone(),
        mcp_discovery: meta.discovery_evidence.clone(),
        tool: ToolIdentity {
            name: meta.domain.clone(),
            url: meta.target_url.clone(),
        },
        audience: Null,
        audit_profile: Null,
        site_type: meta.site_type,
        public_listing: meta.public_listing,
        summary,
        coverage_summary: coverage,
        score_pct: scored.relative,
        score: Score {
            relative: scored.relative,
            global: scored.global,
        },
        categories,
        results: rows,
    }
}

/// Serialize a scorecard exactly as the site prints it: two-space pretty
/// JSON with a trailing newline.
pub fn to_wire_json(scorecard: &WebScorecard) -> String {
    let mut out =
        serde_json::to_string_pretty(scorecard).expect("a scorecard has no unserializable field");
    out.push('\n');
    out
}
