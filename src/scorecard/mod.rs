pub mod audience;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::Serialize;

use crate::check::Check;
use crate::principles::registry::{Level, REQUIREMENTS, SPEC_VERSION};
use crate::types::{CheckGroup, CheckResult, CheckStatus};

/// Current scorecard JSON schema version. Consumers (site rendering,
/// leaderboard pipeline) pin against this to detect shape changes.
///
/// `0.x` is pre-launch — shape may still evolve. Will lock to `1.0` on
/// first public release of `anc`. During `0.x`, additive fields are the
/// norm; consumers feature-detect new keys rather than pinning exact
/// values. History: `0.1` (initial), `0.2` (audience, audit_profile,
/// coverage_summary), `0.3` (spec_version), `0.4` (tool / anc / run /
/// target metadata blocks — self-describing scoring run), `0.5` (`badge`
/// block — eligibility, embed snippet, and badge/scorecard URLs derived
/// from the run, so authors learn about the badge from the CLI itself
/// rather than a round-trip to the site), `0.6` (7-status taxonomy:
/// `opt_out` + `n_a` added to `status`; matching counters in `summary`;
/// `tier` field on each result; one result per requirement-row instead of
/// per-check_id; antecedent propagation for conditional rows).
pub const SCHEMA_VERSION: &str = "0.6";

/// Eligibility floor for the agent-native badge, expressed as an integer
/// percent. A score that meets or exceeds this floor qualifies a tool to
/// embed the badge.
///
/// Authority is the site's published badge convention at
/// <https://anc.dev/badge> (mirrors the `badgeColor` brightline in
/// `agentnative-site/src/build/badge.mjs`). The spec convention currently
/// lives on the `agentnative-spec` `feat/badge-claim-convention` branch
/// and will move into the vendored spec via `sync-spec` once the floor
/// lands as a published constant. Until then, this constant carries the
/// site's authoritative value.
pub const BADGE_ELIGIBILITY_FLOOR_PCT: u32 = 80;

/// Canonical base URL the badge convention publishes against. Per the
/// site convention, the URL is "always-latest" — `<base>/badge/<tool>.svg`
/// reflects the most recent score against the most recent published spec.
/// The constant is centralized here so the URL pattern is the single
/// source of truth across `text_hint`, JSON emission, and tests.
pub const BADGE_BASE_URL: &str = "https://anc.dev";

/// Pre-launch (`0.x`) scorecard shape emitted by `anc audit --output json`.
///
/// **Scorecard-level enum values are kebab-case.** Both `audience` and
/// `audit_profile` serialize their enum values as kebab-case strings
/// (`agent-optimized` / `mixed` / `human-primary` for `audience`;
/// `human-tui` / `file-traversal` / `posix-utility` / `diagnostic-only`
/// for `audit_profile`). `audit_profile` MUST be kebab-case because it
/// echoes the CLI flag value a caller types (`--audit-profile human-tui`);
/// `audience` uses the same convention so consumers don't have to juggle
/// two casing rules inside one JSON document.
///
/// Per-result enum values in `results[].group` / `layer` / `confidence`
/// stay snake_case via their `#[serde(rename_all = "snake_case")]`
/// derives — they are a different contract (one row per check) with
/// broader consumer history, and share spelling with the Rust
/// type-system identifiers they come from.
///
/// Consumers key on the exact string; never transform case.
#[derive(Serialize)]
pub struct Scorecard {
    pub schema_version: &'static str,
    pub results: Vec<CheckResultView>,
    pub summary: Summary,
    pub coverage_summary: CoverageSummary,
    /// Derived audience classification (`agent-optimized`, `mixed`,
    /// `human-primary`). Reserved in `anc` v0.1.1 / v0.1.2 (always `null`);
    /// populated in v0.1.3+. Pre-launch additive (schema `0.2`); older
    /// consumers feature-detect.
    pub audience: Option<String>,
    /// When `audience` is `null`, the reason the classifier declined to
    /// label: `suppressed` (signal check masked by `--audit-profile`) or
    /// `insufficient_signal` (signal check never produced, e.g. source-only
    /// run). Omitted from JSON when `audience` has a label. Pre-launch
    /// additive (schema `0.2`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience_reason: Option<String>,
    /// Registry-sourced exemption category (human-tui, file-traversal, etc.).
    /// Reserved for `anc` v0.1.3; emitted as `null` in v0.1.1 / v0.1.2.
    /// Pre-launch additive (schema `0.2`).
    pub audit_profile: Option<String>,
    /// agentnative-spec version this CLI was built against. Sourced at build
    /// time from `src/principles/spec/VERSION` by `build.rs`. Reads
    /// `"unknown"` if the vendored VERSION file was missing at build time
    /// (build still succeeds; warning emitted). Pre-launch additive
    /// (schema `0.3`); older consumers feature-detect.
    pub spec_version: &'static str,
    /// Identity of the scored target. Pre-launch additive (schema `0.4`).
    pub tool: ToolInfo,
    /// Identity of the `anc` build that produced this scorecard. Pre-launch
    /// additive (schema `0.4`).
    pub anc: AncInfo,
    /// Run-level facts: invocation, timestamp, duration, platform.
    /// Pre-launch additive (schema `0.4`).
    pub run: RunInfo,
    /// What `anc` was pointed at: project path, binary file, or PATH-resolved
    /// command. Pre-launch additive (schema `0.4`).
    pub target: TargetInfo,
    /// Agent-native badge eligibility + embed snippet for this run. Always
    /// emitted; below-floor runs leave `embed_markdown` `null` per the
    /// "do not nag" rule in the site's badge convention. Pre-launch
    /// additive (schema `0.5`).
    pub badge: BadgeInfo,
}

/// Agent-native badge metadata derived from the current run.
///
/// `score_pct` is the rounded integer percent of `pass / (pass + warn +
/// fail)` — the same denominator the site leaderboard uses. Skips and
/// errors do not count toward either side of the ratio. When the
/// denominator is zero (no scoring data — e.g., `--principle 99` filters
/// every check out) the score is reported as `0` and `eligible` is
/// `false`.
///
/// `eligible` is `true` iff `score_pct >= BADGE_ELIGIBILITY_FLOOR_PCT`
/// **and** a tool slug was derivable. Without a slug we cannot construct
/// the embed URL truthfully, and silently emitting a placeholder would
/// mislead authors.
///
/// `embed_markdown` is `Some` only when the tool is eligible — the field
/// drives the gating contract: a consumer that emits `embed_markdown` to
/// a README knows it's safe to show because the floor was checked here.
///
/// `scorecard_url` and `badge_url` are populated whenever a tool slug
/// exists, even below the floor. The site renders the SVG for every
/// scored tool regardless of score so a regression below the floor shows
/// the visual color shift instead of a 404.
#[derive(Serialize)]
pub struct BadgeInfo {
    pub eligible: bool,
    pub score_pct: u32,
    pub embed_markdown: Option<String>,
    pub scorecard_url: Option<String>,
    pub badge_url: Option<String>,
    pub convention_url: &'static str,
}

impl BadgeInfo {
    /// Render the post-summary text hint shown in `--output text` mode
    /// when the tool qualifies for the badge. Returns `None` below the
    /// eligibility floor so callers can append unconditionally without
    /// nagging authors who are not yet eligible.
    pub fn text_hint(&self) -> Option<String> {
        let embed = self.embed_markdown.as_deref()?;
        Some(format!(
            "\n🏆 Score: {}% — your tool qualifies for the agent-native badge.\n\
             \x20  Embed in your README:\n\
             \x20    {embed}\n\
             \x20  Convention: {}/badge\n",
            self.score_pct, BADGE_BASE_URL,
        ))
    }
}

/// Pure derivation of `BadgeInfo` from a result set and a tool slug. Used
/// by both `build_scorecard` (for JSON emission) and the runner's text
/// path (for the post-summary hint), so a single source of truth backs
/// both surfaces.
pub fn compute_badge(results: &[CheckResult], tool_name: &str) -> BadgeInfo {
    let pct = score_pct(results);
    let trimmed = tool_name.trim();
    let has_slug = !trimmed.is_empty();
    let eligible = has_slug && pct >= BADGE_ELIGIBILITY_FLOOR_PCT;

    let scorecard_url = has_slug.then(|| format!("{BADGE_BASE_URL}/score/{trimmed}"));
    let badge_url = has_slug.then(|| format!("{BADGE_BASE_URL}/badge/{trimmed}.svg"));
    let embed_markdown = if eligible {
        Some(format!(
            "[![agent-native]({BADGE_BASE_URL}/badge/{trimmed}.svg)]({BADGE_BASE_URL}/score/{trimmed})"
        ))
    } else {
        None
    };

    BadgeInfo {
        eligible,
        score_pct: pct,
        embed_markdown,
        scorecard_url,
        badge_url,
        convention_url: "https://anc.dev/badge",
    }
}

/// Compute the rounded integer percent score using the transitional
/// leaderboard denominator. Pass/Warn/Fail count in the denominator. Pass
/// counts in the numerator. Skip, Error, OptOut, and NotApplicable are
/// excluded from both sides of the ratio.
///
/// **Transitional formula choice.** The 7-status taxonomy semantically
/// treats `opt_out` as in-denominator (deliberate non-adoption is a real
/// signal), but plan U2 explicitly keeps the U2 formula conservative:
/// "leave the existing formula but exclude `opt_out` from the denominator
/// and exclude `n_a` from both — minimal change that respects the new
/// semantics without committing to a new formula." Whether `opt_out`
/// re-enters the denominator (and at what weight) is the U3 spec issue,
/// after the disambiguated input has been rescored.
///
/// Returns `0` when no checks contribute — pairs with `BadgeInfo::eligible
/// == false` so a zero score never qualifies.
fn score_pct(results: &[CheckResult]) -> u32 {
    let mut pass = 0u32;
    let mut denom = 0u32;
    for r in results {
        match &r.status {
            CheckStatus::Pass => {
                pass += 1;
                denom += 1;
            }
            CheckStatus::Warn(_) | CheckStatus::Fail(_) => {
                denom += 1;
            }
            CheckStatus::Skip(_)
            | CheckStatus::Error(_)
            | CheckStatus::OptOut(_)
            | CheckStatus::NotApplicable(_) => {}
        }
    }
    if denom == 0 {
        0
    } else {
        let ratio = f64::from(pass) / f64::from(denom);
        (ratio * 100.0).round() as u32
    }
}

/// Identity of the scored target. `version` is best-effort: when the binary
/// self-reports a parseable `--version` / `-V` first line we capture it,
/// otherwise the field is `null`. The site's `registry.yaml` continues to own
/// `version_extract` shell snippets as a fallback. Always-present keys
/// (`null` rather than missing) keep consumer code simple.
#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    /// Binary basename when an executable was located; `null` for
    /// project-mode runs without a built artifact.
    pub binary: Option<String>,
    /// Version string the tool self-reported. `null` when probing failed,
    /// produced no parseable output, or was declined (self-spawn guard).
    pub version: Option<String>,
}

/// Identity of the `anc` build that produced this scorecard. `version` is a
/// build-time constant generated by `build.rs`.
#[derive(Serialize)]
pub struct AncInfo {
    pub version: &'static str,
}

/// Run-level metadata. Captured by the runner immediately around the
/// `Commands::Audit` arm so the scorecard reflects this specific scoring run.
///
/// `invocation` is the user's argv joined with spaces, captured *before*
/// `inject_default_subcommand` rewrites bare paths into `audit <path>`.
/// `started_at` is RFC 3339 / ISO 8601 in UTC. `duration_ms` is wall-clock
/// milliseconds.
#[derive(Serialize)]
pub struct RunInfo {
    pub invocation: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub platform: PlatformInfo,
}

/// `os` / `arch` tuple sourced from `std::env::consts::{OS, ARCH}`.
#[derive(Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
}

/// What `anc audit` was pointed at. `kind` is one of `"project"`, `"binary"`,
/// or `"command"`. `path` carries the **basename** of the resolved target
/// (directory name in project mode, file name in binary mode) — never the
/// full filesystem path, which would leak operator PII (home-dir username,
/// org/employer dir structure) into committed scorecards, README badge URLs,
/// and any agent-posted artifact. `command` carries the user-supplied name
/// for `--command` mode. Always-present keys (the unused field is `null`,
/// not missing) keep consumer code simple.
#[derive(Serialize)]
pub struct TargetInfo {
    pub kind: String,
    pub path: Option<String>,
    pub command: Option<String>,
}

/// Per-level verification counts: how many requirements at this level had
/// at least one check in this run that declared `covers()` against them.
/// A requirement is "verified" regardless of pass/fail — the status tells
/// the consumer whether verification succeeded, this counter tells them
/// whether it was attempted at all.
#[derive(Serialize)]
pub struct LevelCounts {
    pub total: usize,
    pub verified: usize,
}

#[derive(Serialize)]
pub struct CoverageSummary {
    pub must: LevelCounts,
    pub should: LevelCounts,
    pub may: LevelCounts,
}

/// Run-level outcome counts. The 7-status taxonomy added `opt_out` and
/// `n_a` in schema 0.6; pre-0.6 consumers tolerate the new keys (additive
/// extension).
#[derive(Serialize, Debug)]
pub struct Summary {
    pub total: usize,
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub opt_out: usize,
    pub n_a: usize,
    pub skip: usize,
    pub error: usize,
}

/// One row of `results[]` in the scorecard JSON.
///
/// Schema 0.6 changed the unit of emission from "per check_id" to "per
/// requirement-row". `id` is now the requirement row id (matches
/// `coverage/matrix.json` row IDs). `tier` carries the row's RFC 2119
/// level (`must`/`should`/`may`) so downstream scoring consumers do not
/// need a matrix join. `check_id` is the probe that produced this row,
/// preserved for provenance and so the site renderer / audience classifier
/// can find the originating probe without a registry walk.
#[derive(Serialize)]
pub struct CheckResultView {
    pub id: String,
    pub label: String,
    pub group: String,
    pub layer: String,
    pub status: String,
    pub evidence: Option<String>,
    /// `high` for direct probes, `medium` for heuristics. Older consumers
    /// feature-detect and tolerate missing keys.
    pub confidence: String,
    /// Requirement tier (`must`/`should`/`may`). Pre-launch additive
    /// (schema `0.6`). `null` only for results whose row id is not in the
    /// registry — an internal inconsistency that should be loud.
    pub tier: Option<String>,
    /// Underlying probe that produced this row (e.g., `p3-version` covers
    /// both `p3-must-version` and `p3-should-version-short` — two rows
    /// share one `check_id`). Pre-launch additive (schema `0.6`). Falls
    /// back to the row `id` itself when no provenance was threaded in
    /// (legacy test fixtures that hand-build a `CheckResult` without the
    /// fan-out pipeline).
    pub check_id: String,
}

impl CheckResultView {
    /// Construct from a raw probe result (pre-fan-out callers and test
    /// fixtures). `check_id` defaults to `r.id` and `tier` is looked up
    /// from the registry (which fails to find anything for arbitrary test
    /// IDs — surfaces as JSON null). Production code uses `from_row`
    /// directly with the threaded probe provenance; this fallback exists
    /// for tests and any future caller that builds a per-check view
    /// without the fan-out pipeline.
    #[allow(dead_code)]
    pub fn from_result(r: &CheckResult) -> Self {
        Self::from_row(r, &r.id)
    }

    /// Construct from a fanned-out per-row result with explicit probe
    /// provenance. `check_id` is the probe's `Check::id()`; `r.id` is the
    /// requirement row id.
    pub fn from_row(r: &CheckResult, check_id: &str) -> Self {
        let (status, evidence) = match &r.status {
            CheckStatus::Pass => ("pass".to_string(), None),
            CheckStatus::Warn(e) => ("warn".to_string(), Some(e.clone())),
            CheckStatus::Fail(e) => ("fail".to_string(), Some(e.clone())),
            CheckStatus::OptOut(e) => ("opt_out".to_string(), Some(e.clone())),
            CheckStatus::NotApplicable(e) => ("n_a".to_string(), Some(e.clone())),
            CheckStatus::Skip(e) => ("skip".to_string(), Some(e.clone())),
            CheckStatus::Error(e) => ("error".to_string(), Some(e.clone())),
        };
        // Serialize CheckGroup / CheckLayer / Confidence via serde_json so
        // the JSON mirrors the canonical enum spelling (snake_case).
        let group = serde_json::to_value(r.group)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.group));
        let layer = serde_json::to_value(r.layer)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.layer));
        let confidence = serde_json::to_value(r.confidence)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.confidence));
        // Look up tier from the registry. The row ID is the result ID under
        // the per-row emission contract introduced in schema 0.6. Per-check
        // results fed in by older callers (or test fixtures) won't find a
        // match here — `tier` falls back to None, which surfaces as JSON
        // null and is a visible sign of inconsistency.
        let tier =
            crate::principles::registry::find(&r.id).map(|req| req.level.as_str().to_string());
        CheckResultView {
            id: r.id.clone(),
            label: r.label.clone(),
            group,
            layer,
            status,
            evidence,
            confidence,
            tier,
            check_id: check_id.to_string(),
        }
    }
}

fn build_summary(results: &[CheckResult]) -> Summary {
    Summary {
        total: results.len(),
        pass: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Pass))
            .count(),
        warn: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Warn(_)))
            .count(),
        fail: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Fail(_)))
            .count(),
        opt_out: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::OptOut(_)))
            .count(),
        n_a: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::NotApplicable(_)))
            .count(),
        skip: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Skip(_)))
            .count(),
        error: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Error(_)))
            .count(),
    }
}

fn group_display(group: &CheckGroup) -> &'static str {
    match group {
        CheckGroup::P1 => "P1 — Non-Interactive by Default",
        CheckGroup::P2 => "P2 — Structured Output",
        CheckGroup::P3 => "P3 — Progressive Help",
        CheckGroup::P4 => "P4 — Actionable Errors",
        CheckGroup::P5 => "P5 — Safe Retries",
        CheckGroup::P6 => "P6 — Composable Structure",
        CheckGroup::P7 => "P7 — Bounded Responses",
        CheckGroup::P8 => "P8 — Discoverable Skill Bundles",
        CheckGroup::CodeQuality => "Code Quality",
        CheckGroup::ProjectStructure => "Project Structure",
    }
}

/// Order groups for consistent display.
fn group_order(group: &CheckGroup) -> u8 {
    match group {
        CheckGroup::P1 => 1,
        CheckGroup::P2 => 2,
        CheckGroup::P3 => 3,
        CheckGroup::P4 => 4,
        CheckGroup::P5 => 5,
        CheckGroup::P6 => 6,
        CheckGroup::P7 => 7,
        CheckGroup::P8 => 8,
        CheckGroup::CodeQuality => 9,
        CheckGroup::ProjectStructure => 10,
    }
}

/// Format the scorecard as plain text. Pass `Some(badge)` to append the
/// post-summary embed hint when the tool qualifies for the agent-native
/// badge; below-floor runs see `text_hint()` return `None`, so nothing is
/// appended (the "do not nag" rule from the badge convention).
/// Rendering options for [`format_text`]. Grouped here so future flags
/// (raw mode, color choice, etc.) can be added without churning every
/// call site's argument list.
#[derive(Clone, Copy, Default)]
pub struct TextOptions {
    /// Suppress group headers, PASS/SKIP rows, summary, and badge hint —
    /// emit only `id<TAB>status` per check. Wired to `--raw`.
    pub raw: bool,
    /// Apply ANSI styling to status prefixes. Computed by the caller from
    /// `--color` plus TTY / `NO_COLOR` introspection so the renderer stays
    /// pure.
    pub color: bool,
}

pub fn format_text(
    results: &[CheckResult],
    quiet: bool,
    badge: Option<&BadgeInfo>,
    opts: TextOptions,
) -> String {
    if opts.raw {
        return format_text_raw(results);
    }
    let mut out = String::new();

    // Group results by CheckGroup
    let mut grouped: BTreeMap<u8, (CheckGroup, Vec<&CheckResult>)> = BTreeMap::new();
    for r in results {
        let order = group_order(&r.group);
        grouped
            .entry(order)
            .or_insert_with(|| (r.group, Vec::new()))
            .1
            .push(r);
    }

    for (group, checks) in grouped.values() {
        if !quiet {
            let _ = writeln!(out, "\n{}", group_display(group));
        }
        for r in checks {
            let prefix = match &r.status {
                CheckStatus::Pass => {
                    if quiet {
                        continue;
                    }
                    "PASS"
                }
                CheckStatus::Warn(_) => "WARN",
                CheckStatus::Fail(_) => "FAIL",
                CheckStatus::OptOut(_) => {
                    if quiet {
                        continue;
                    }
                    "OPT "
                }
                CheckStatus::NotApplicable(_) => {
                    if quiet {
                        continue;
                    }
                    "N/A "
                }
                CheckStatus::Skip(_) => {
                    if quiet {
                        continue;
                    }
                    "SKIP"
                }
                CheckStatus::Error(_) => "ERR ",
            };
            let painted =
                crate::color::paint(crate::color::status_style(prefix, opts.color), prefix);
            // Tier comes from the requirement registry keyed on the row id.
            // Unregistered ids (legacy per-check rows, test fixtures) yield
            // no suffix rather than panicking — the same tolerance
            // `CheckResultView::from_row` applies for the JSON `tier` field.
            let tier_suffix = crate::principles::registry::find(&r.id)
                .map(|req| format!(" ({})", req.level.as_str()))
                .unwrap_or_default();
            let _ = writeln!(out, "  [{painted}] {} ({}){tier_suffix}", r.label, r.id);
            match &r.status {
                CheckStatus::Warn(e) | CheckStatus::Fail(e) | CheckStatus::Error(e) => {
                    for line in e.lines() {
                        let _ = writeln!(out, "         {line}");
                    }
                }
                CheckStatus::Skip(reason)
                | CheckStatus::OptOut(reason)
                | CheckStatus::NotApplicable(reason)
                    if !quiet =>
                {
                    let _ = writeln!(out, "         {reason}");
                }
                _ => {}
            }
        }
    }

    // Summary line
    let s = build_summary(results);
    let _ = writeln!(
        out,
        "\n{} checks: {} pass, {} warn, {} fail, {} opt_out, {} n_a, {} skip, {} error",
        s.total, s.pass, s.warn, s.fail, s.opt_out, s.n_a, s.skip, s.error
    );

    // Badge embed hint — appended only when eligible. Below the floor the
    // `text_hint()` returns None and nothing is added (the convention's
    // "do not nag" rule).
    if let Some(hint) = badge.and_then(BadgeInfo::text_hint) {
        out.push_str(&hint);
    }

    out
}

/// `--raw` rendering: one `id<TAB>status` line per result, nothing else.
/// Status maps to one of the seven tokens (`PASS`, `WARN`, `FAIL`,
/// `OPT_OUT`, `N_A`, `SKIP`, `ERR`) so downstream pipelines see the same
/// vocabulary as the JSON `status` field (uppercased).
fn format_text_raw(results: &[CheckResult]) -> String {
    let mut out = String::with_capacity(results.len() * 32);
    for r in results {
        let token = match &r.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn(_) => "WARN",
            CheckStatus::Fail(_) => "FAIL",
            CheckStatus::OptOut(_) => "OPT_OUT",
            CheckStatus::NotApplicable(_) => "N_A",
            CheckStatus::Skip(_) => "SKIP",
            CheckStatus::Error(_) => "ERR",
        };
        let _ = writeln!(out, "{}\t{token}", r.id);
    }
    out
}

/// Fan one probe-level result out into one entry per requirement-row in
/// the check's `Check::covers()` slice. The probe's status, label, group,
/// layer, and confidence propagate to every row; the `id` field is
/// replaced with the row id. Returns a pair `(row_result, check_id)` per
/// emitted row so downstream consumers (CheckResultView, propagation) know
/// the originating probe without a registry walk.
///
/// Checks that declare no `covers()` rows produce a single passthrough
/// entry keyed by their own id — preserves the legacy per-check_id shape
/// for checks not yet wired into the requirement registry.
pub fn fan_out_per_row(
    raw: &[CheckResult],
    catalog: &[Box<dyn Check>],
) -> Vec<(CheckResult, String)> {
    let covers_by_id: HashMap<&str, &'static [&'static str]> =
        catalog.iter().map(|c| (c.id(), c.covers())).collect();
    let mut out: Vec<(CheckResult, String)> = Vec::with_capacity(raw.len());
    for r in raw {
        let covers = covers_by_id.get(r.id.as_str()).copied().unwrap_or(&[]);
        if covers.is_empty() {
            out.push((r.clone(), r.id.clone()));
            continue;
        }
        for row_id in covers {
            let mut row = r.clone();
            row.id = (*row_id).to_string();
            out.push((row, r.id.clone()));
        }
    }
    out
}

/// Apply the antecedent-status propagation table from plan Decision 2a.
///
/// For each row whose registry entry has a conditional applicability with
/// an `antecedent.check_id`, look up the antecedent probe's raw status and
/// rewrite the row's status accordingly:
///
/// | Antecedent status | Consequent row becomes              |
/// | ----------------- | ----------------------------------- |
/// | `pass` / `warn` / `fail` | unchanged (evaluated normally) |
/// | `opt_out` / `n_a` | `n_a` (prerequisite absent)         |
/// | `skip`            | `skip` (inherited indeterminacy)    |
/// | `error`           | `error` (inherited indeterminacy)   |
///
/// Rows with no registry entry (legacy / unknown ids) are left untouched.
/// Rows whose antecedent did not produce a raw result (the antecedent
/// probe didn't run this invocation, e.g., source-only mode) are left
/// untouched — propagation needs an antecedent status to act on.
pub fn propagate_antecedents(rows: &mut [(CheckResult, String)], raw: &[CheckResult]) {
    use crate::principles::registry::{Applicability, find};
    let raw_by_id: HashMap<&str, &CheckStatus> =
        raw.iter().map(|r| (r.id.as_str(), &r.status)).collect();
    for (row, _check_id) in rows.iter_mut() {
        let Some(req) = find(&row.id) else { continue };
        let Applicability::Conditional { antecedent, .. } = req.applicability else {
            continue;
        };
        let Some(ante) = antecedent else { continue };
        let Some(ante_status) = raw_by_id.get(ante.check_id) else {
            continue;
        };
        let new_status = match ante_status {
            CheckStatus::Pass | CheckStatus::Warn(_) | CheckStatus::Fail(_) => continue,
            CheckStatus::OptOut(reason) | CheckStatus::NotApplicable(reason) => {
                CheckStatus::NotApplicable(format!(
                    "antecedent `{}` is {}: {reason}",
                    ante.check_id,
                    short_status_name(ante_status),
                ))
            }
            CheckStatus::Skip(reason) => CheckStatus::Skip(format!(
                "antecedent `{}` could not be measured: {reason}",
                ante.check_id,
            )),
            CheckStatus::Error(reason) => {
                CheckStatus::Error(format!("antecedent `{}` errored: {reason}", ante.check_id,))
            }
        };
        row.status = new_status;
    }
}

fn short_status_name(s: &CheckStatus) -> &'static str {
    match s {
        CheckStatus::Pass => "pass",
        CheckStatus::Warn(_) => "warn",
        CheckStatus::Fail(_) => "fail",
        CheckStatus::OptOut(_) => "opt_out",
        CheckStatus::NotApplicable(_) => "n_a",
        CheckStatus::Skip(_) => "skip",
        CheckStatus::Error(_) => "error",
    }
}

/// Fan raw probe results out to per-requirement rows and apply antecedent
/// propagation. The shared derivation every output surface routes through —
/// the JSON scorecard, the text renderer, the badge, and the process exit
/// code all build their per-row set with this one function, so they cannot
/// disagree on the row set, counts, score, or status of any requirement.
///
/// Each pair carries the row plus its originating probe `Check::id()` for
/// provenance; callers that only need the rows project the `String` away.
pub fn build_row_results(
    raw: &[CheckResult],
    catalog: &[Box<dyn Check>],
) -> Vec<(CheckResult, String)> {
    let mut rows = fan_out_per_row(raw, catalog);
    propagate_antecedents(&mut rows, raw);
    rows
}

/// Bundle of run-level metadata captured by the runner around `Commands::Audit`
/// and threaded into the scorecard. Grouped to keep `build_scorecard`'s
/// signature manageable as schema `0.x` continues to add fields. The runner
/// owns capture; this module owns serialization shape.
pub struct RunMetadata {
    pub tool: ToolInfo,
    pub anc: AncInfo,
    pub run: RunInfo,
    pub target: TargetInfo,
}

/// Build the scorecard. The `ran_checks` slice is the catalog of checks
/// that produced `raw_results`.
///
/// Pipeline (schema 0.6):
///   raw probe results → fan out per requirement-row → antecedent
///   propagation → JSON view. Audience and coverage_summary still consume
///   raw probe results (signal classification keys on check_ids; coverage
///   counts requirements covered by the underlying probes).
pub fn build_scorecard(
    raw_results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
    audience: Option<String>,
    audit_profile: Option<String>,
    metadata: RunMetadata,
) -> Scorecard {
    let row_results = build_row_results(raw_results, ran_checks);

    // `audience_reason` is derived from `raw_results` rather than threaded
    // through as a caller parameter — the reason is a property of the
    // probe-level result set, not a caller decision, and deriving it here
    // keeps the label and its explanation in lock-step. When audience has
    // a label the field is omitted from JSON.
    let audience_reason = if audience.is_some() {
        None
    } else {
        audience::classify_reason(raw_results).map(|s| s.to_string())
    };
    let RunMetadata {
        tool,
        anc,
        run,
        target,
    } = metadata;

    // Per-row results drive `summary` and `score_pct`. The badge uses the
    // same per-row vector so the embed URL the JSON emits agrees with the
    // post-summary text hint.
    let per_row_only: Vec<CheckResult> = row_results.iter().map(|(r, _)| r.clone()).collect();
    let badge = compute_badge(&per_row_only, &tool.name);

    Scorecard {
        schema_version: SCHEMA_VERSION,
        results: row_results
            .iter()
            .map(|(r, check_id)| CheckResultView::from_row(r, check_id))
            .collect(),
        summary: build_summary(&per_row_only),
        coverage_summary: build_coverage_summary(raw_results, ran_checks),
        audience,
        audience_reason,
        audit_profile,
        spec_version: SPEC_VERSION,
        tool,
        anc,
        run,
        target,
        badge,
    }
}

pub fn format_json(
    raw_results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
    audience: Option<String>,
    audit_profile: Option<String>,
    metadata: RunMetadata,
) -> String {
    let scorecard = build_scorecard(raw_results, ran_checks, audience, audit_profile, metadata);
    serde_json::to_string_pretty(&scorecard).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn build_coverage_summary(
    results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
) -> CoverageSummary {
    // Map each ran check to its covers() so we can turn the set of ran
    // check IDs into a set of covered requirement IDs.
    let covers_by_id: HashMap<&str, &'static [&'static str]> =
        ran_checks.iter().map(|c| (c.id(), c.covers())).collect();

    // Verified = requirements covered by a check that actually executed.
    // A check suppressed by --audit-profile did NOT verify its
    // requirement — it emitted Skip with the `SUPPRESSION_EVIDENCE_PREFIX`
    // sentinel. Counting it toward `verified` would overstate coverage on
    // any --audit-profile run (a misleading public metric for the site
    // leaderboard). Filter those out here and mirror the exclusion in the
    // regression test below.
    let mut verified: HashSet<&'static str> = HashSet::new();
    for r in results {
        if audience::is_audit_profile_suppression(&r.status) {
            continue;
        }
        if let Some(ids) = covers_by_id.get(r.id.as_str()) {
            verified.extend(ids.iter().copied());
        }
    }

    let mut must = LevelCounts {
        total: 0,
        verified: 0,
    };
    let mut should = LevelCounts {
        total: 0,
        verified: 0,
    };
    let mut may = LevelCounts {
        total: 0,
        verified: 0,
    };

    for req in REQUIREMENTS {
        let bucket = match req.level {
            Level::Must => &mut must,
            Level::Should => &mut should,
            Level::May => &mut may,
        };
        bucket.total += 1;
        if verified.contains(req.id) {
            bucket.verified += 1;
        }
    }

    CoverageSummary { must, should, may }
}

/// Derive the process exit code from the full result set.
///
/// - `0` — every check Pass or Skip.
/// - `1` — at least one Warn.
/// - `2` — at least one Fail or Error.
///
/// **`--audit-profile` affects the exit code by masking Fails to Skips.**
/// A check that would otherwise Fail but is suppressed by the applied
/// profile contributes nothing to `has_fail_or_error` and cannot lift the
/// code above `0`/`1`. This is intentional per plan R4: the caller is
/// declaring "this category of check doesn't apply to this tool", so
/// scoring against that requirement would produce a misleading non-zero
/// exit. The tradeoff is that callers passing the wrong profile can
/// silently bless a broken tool — guarding against that lives upstream
/// (site's regen script, CI policy), not here.
pub fn exit_code(results: &[CheckResult]) -> i32 {
    let has_fail_or_error = results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Fail(_) | CheckStatus::Error(_)));
    let has_warn = results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Warn(_)));

    if has_fail_or_error {
        2
    } else if has_warn {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

    fn make_result(id: &str, status: CheckStatus, group: CheckGroup) -> CheckResult {
        CheckResult {
            id: id.to_string(),
            label: format!("Test {id}"),
            group,
            layer: CheckLayer::Behavioral,
            status,
            confidence: Confidence::High,
        }
    }

    /// Synthesize the metadata bundle existing tests need but don't care
    /// about. Tests that exercise metadata behavior build their own.
    fn fixture_metadata() -> RunMetadata {
        RunMetadata {
            tool: ToolInfo {
                name: "fixture-tool".into(),
                binary: None,
                version: None,
            },
            anc: AncInfo {
                version: "0.0.0-test",
            },
            run: RunInfo {
                invocation: "anc audit .".into(),
                started_at: "1970-01-01T00:00:00Z".into(),
                duration_ms: 0,
                platform: PlatformInfo {
                    os: "test-os",
                    arch: "test-arch",
                },
            },
            target: TargetInfo {
                kind: "project".into(),
                path: Some(".".into()),
                command: None,
            },
        }
    }

    #[test]
    fn test_format_json_valid() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Fail("bad".into()), CheckGroup::P2),
        ];
        let json = format_json(&results, &[], None, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema_version"], "0.6");
        assert_eq!(parsed["summary"]["total"], 2);
        assert_eq!(parsed["summary"]["pass"], 1);
        assert_eq!(parsed["summary"]["fail"], 1);
        assert_eq!(parsed["results"][0]["status"], "pass");
        assert!(parsed["results"][0]["evidence"].is_null());
        assert_eq!(parsed["results"][0]["confidence"], "high");
        assert_eq!(parsed["results"][1]["status"], "fail");
        assert_eq!(parsed["results"][1]["evidence"], "bad");
        assert_eq!(parsed["results"][1]["confidence"], "high");
        // 0.2 additions: coverage_summary present with three levels, audience + audit_profile null.
        assert!(parsed["coverage_summary"]["must"]["total"].is_number());
        assert!(parsed["coverage_summary"]["should"]["total"].is_number());
        assert!(parsed["coverage_summary"]["may"]["total"].is_number());
        assert!(parsed["audience"].is_null());
        assert!(parsed["audit_profile"].is_null());
        // 0.3 addition: spec_version is always present and non-empty.
        let spec = parsed["spec_version"]
            .as_str()
            .expect("spec_version is a string");
        assert!(!spec.is_empty(), "spec_version must not be empty");
    }

    #[test]
    fn medium_confidence_serializes_as_medium() {
        let mut r = make_result("c3", CheckStatus::Warn("soft".into()), CheckGroup::P6);
        r.confidence = Confidence::Medium;
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.confidence, "medium");
    }

    #[test]
    fn coverage_summary_counts_verified_requirements() {
        use crate::check::Check;
        use crate::project::Project;
        use crate::types::CheckLayer;

        struct FakeCheck {
            id: &'static str,
            covers: &'static [&'static str],
        }

        impl Check for FakeCheck {
            fn id(&self) -> &str {
                self.id
            }
            fn label(&self) -> &'static str {
                "fake"
            }
            fn group(&self) -> CheckGroup {
                CheckGroup::P1
            }
            fn layer(&self) -> CheckLayer {
                CheckLayer::Behavioral
            }
            fn applicable(&self, _p: &Project) -> bool {
                true
            }
            fn run(&self, _p: &Project) -> anyhow::Result<CheckResult> {
                unreachable!()
            }
            fn covers(&self) -> &'static [&'static str] {
                self.covers
            }
        }

        let results = vec![make_result("verifier-a", CheckStatus::Pass, CheckGroup::P1)];
        let checks: Vec<Box<dyn Check>> = vec![Box::new(FakeCheck {
            id: "verifier-a",
            covers: &["p1-must-no-interactive"],
        })];

        let summary = build_coverage_summary(&results, &checks);
        assert_eq!(summary.must.verified, 1);
        assert_eq!(summary.should.verified, 0);
        assert_eq!(summary.may.verified, 0);
        // Totals match the registry snapshot baked into registry.rs tests.
        assert!(summary.must.total >= 1);
    }

    #[test]
    fn coverage_summary_excludes_audit_profile_suppressed_checks() {
        use crate::check::Check;
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;
        use crate::project::Project;
        use crate::types::CheckLayer;

        struct FakeCheck {
            id: &'static str,
            covers: &'static [&'static str],
        }

        impl Check for FakeCheck {
            fn id(&self) -> &str {
                self.id
            }
            fn label(&self) -> &'static str {
                "fake"
            }
            fn group(&self) -> CheckGroup {
                CheckGroup::P1
            }
            fn layer(&self) -> CheckLayer {
                CheckLayer::Behavioral
            }
            fn applicable(&self, _p: &Project) -> bool {
                true
            }
            fn run(&self, _p: &Project) -> anyhow::Result<CheckResult> {
                unreachable!()
            }
            fn covers(&self) -> &'static [&'static str] {
                self.covers
            }
        }

        // Two checks: one ran (Pass → counts as verified), one was
        // suppressed by --audit-profile (Skip with the sentinel prefix →
        // MUST NOT count as verified).
        let results = vec![
            make_result("verifier-ran", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "verifier-suppressed",
                CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui")),
                CheckGroup::P1,
            ),
        ];
        let checks: Vec<Box<dyn Check>> = vec![
            Box::new(FakeCheck {
                id: "verifier-ran",
                covers: &["p1-must-no-interactive"],
            }),
            Box::new(FakeCheck {
                id: "verifier-suppressed",
                covers: &["p1-should-tty-detection"],
            }),
        ];

        let summary = build_coverage_summary(&results, &checks);
        assert_eq!(
            summary.must.verified, 1,
            "only the non-suppressed verifier's requirement should count; \
             suppressed Skips MUST NOT inflate coverage_summary.verified",
        );
        assert_eq!(summary.should.verified, 0);
    }

    #[test]
    fn test_exit_code_all_pass() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Skip("n/a".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 0);
    }

    #[test]
    fn test_exit_code_warn() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Warn("meh".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 1);
    }

    #[test]
    fn test_exit_code_fail() {
        let results = vec![
            make_result("c1", CheckStatus::Fail("bad".into()), CheckGroup::P1),
            make_result("c2", CheckStatus::Warn("meh".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 2);
    }

    #[test]
    fn test_exit_code_error() {
        let results = vec![make_result(
            "c1",
            CheckStatus::Error("boom".into()),
            CheckGroup::P1,
        )];
        assert_eq!(exit_code(&results), 2);
    }

    #[test]
    fn test_check_result_view_conversion() {
        let r = make_result(
            "test-id",
            CheckStatus::Warn("warning msg".into()),
            CheckGroup::P3,
        );
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.id, "test-id");
        assert_eq!(view.status, "warn");
        assert_eq!(view.evidence.as_deref(), Some("warning msg"));
        assert_eq!(view.layer, "behavioral");
    }

    #[test]
    fn test_check_result_view_pass_has_no_evidence() {
        let r = make_result("pass-id", CheckStatus::Pass, CheckGroup::P1);
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.status, "pass");
        assert!(view.evidence.is_none());
    }

    #[test]
    fn format_json_emits_audience_when_all_signals_present() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audience"], "agent-optimized");
        assert!(parsed["audit_profile"].is_null());
        assert_eq!(parsed["schema_version"], "0.6");
    }

    #[test]
    fn format_json_emits_human_primary_when_signals_warn() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let status = if i < 3 {
                    CheckStatus::Warn(format!("missing {id}"))
                } else {
                    CheckStatus::Pass
                };
                make_result(id, status, CheckGroup::P1)
            })
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audience"], "human-primary");
    }

    #[test]
    fn format_json_audience_null_when_signals_missing() {
        use crate::scorecard::audience::classify;

        // Source-only-style run: no behavioral checks, so no signal IDs.
        let results = vec![make_result(
            "p1-env-flags-source",
            CheckStatus::Pass,
            CheckGroup::P1,
        )];
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
    }

    #[test]
    fn format_json_echoes_audit_profile() {
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let json = format_json(
            &results,
            &[],
            None,
            Some("human-tui".into()),
            fixture_metadata(),
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audit_profile"], "human-tui");
    }

    #[test]
    fn format_json_audience_reason_insufficient_signal() {
        // Source-only-style run: no signal checks → audience null and
        // audience_reason must explain why.
        let results = vec![make_result(
            "p1-env-flags-source",
            CheckStatus::Pass,
            CheckGroup::P1,
        )];
        let json = format_json(&results, &[], None, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
        assert_eq!(parsed["audience_reason"], "insufficient_signal");
    }

    #[test]
    fn format_json_audience_reason_omitted_when_audience_labeled() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None, fixture_metadata());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        // audience has a label, so audience_reason must be omitted — not
        // merely null. `#[serde(skip_serializing_if = "Option::is_none")]`
        // on the field makes this verifiable by key presence.
        assert_eq!(parsed["audience"], "agent-optimized");
        assert!(
            parsed.get("audience_reason").is_none(),
            "audience_reason key should be absent when audience is labeled, got {}",
            parsed["audience_reason"],
        );
    }

    #[test]
    fn format_json_audience_reason_suppressed() {
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        // One signal suppressed → audience null and reason "suppressed".
        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let status = if i == 0 {
                    CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui"))
                } else {
                    CheckStatus::Pass
                };
                make_result(id, status, CheckGroup::P1)
            })
            .collect();
        let audience = classify(&results);
        let json = format_json(
            &results,
            &[],
            audience,
            Some("human-tui".into()),
            fixture_metadata(),
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
        assert_eq!(parsed["audience_reason"], "suppressed");
    }

    #[test]
    fn exit_code_drops_when_audit_profile_suppresses_a_would_have_failed_check() {
        // Intentional behavior per plan R4: when --audit-profile suppresses
        // a check that would otherwise Fail, the check emits Skip with the
        // suppression prefix and the overall exit code reflects the
        // masked state. This is a trust-boundary choice — the caller
        // declared the requirement doesn't apply, so failing on it would
        // be misleading.
        //
        // This test pins the behavior against a future well-meaning
        // change that tries to "refuse to exit 0 if any check was
        // suppressed." Such a change must update this test deliberately
        // and resolve the conflict with plan R4, not sneak through.
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;

        let baseline = vec![
            make_result("c-pass", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "c-would-fail",
                CheckStatus::Fail("violates MUST".into()),
                CheckGroup::P1,
            ),
        ];
        assert_eq!(exit_code(&baseline), 2, "baseline: a Fail → exit 2");

        let suppressed = vec![
            make_result("c-pass", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "c-would-fail",
                CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui")),
                CheckGroup::P1,
            ),
        ];
        assert_eq!(
            exit_code(&suppressed),
            0,
            "suppression by audit_profile must lower the exit code — \
             Fail → Skip is intentional masking per plan R4",
        );
    }

    #[test]
    fn scorecard_level_enum_values_are_kebab_case() {
        // Both `audience` and `audit_profile` enum values MUST serialize
        // as kebab-case inside the scorecard JSON. `audit_profile`
        // echoes the CLI flag value (`--audit-profile human-tui`) and
        // cannot change casing; `audience` adopts the same convention so
        // consumers don't juggle two rules inside one document.
        //
        // A future serde `rename_all` edit, field reorder, or enum
        // migration that silently flips either convention must fail here
        // loudly. The snake_case negative assertions below guard against
        // the most likely regression direction (adopting the per-result
        // enum convention from `CheckGroup` / `CheckLayer` / `Confidence`).
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(
            &results,
            &[],
            audience,
            Some("human-tui".into()),
            fixture_metadata(),
        );

        // audience: kebab-case.
        assert!(
            json.contains("\"audience\": \"agent-optimized\""),
            "audience must serialize as kebab-case 'agent-optimized', got:\n{json}",
        );
        assert!(
            !json.contains("\"agent_optimized\""),
            "audience must NOT render as snake_case 'agent_optimized' — \
             kebab-case unified with audit_profile in v0.1.3",
        );
        assert!(
            !json.contains("\"human_primary\""),
            "audience must NOT render as snake_case 'human_primary'",
        );

        // audit_profile: kebab-case (echo of the CLI flag value).
        assert!(
            json.contains("\"audit_profile\": \"human-tui\""),
            "audit_profile must serialize as kebab-case 'human-tui', got:\n{json}",
        );
        assert!(
            !json.contains("\"human_tui\""),
            "audit_profile must NOT render as snake_case 'human_tui' — \
             would desync from the --audit-profile flag value shape",
        );
    }

    #[test]
    fn schema_v05_emits_every_documented_key() {
        // Drift guard for schema 0.5. Builds a synthetic Scorecard, parses
        // the JSON, and asserts every documented key path resolves —
        // including keys that hold `null` values (those are part of the
        // contract: consumer code should treat null and missing differently
        // only for `audience_reason`, which uses `skip_serializing_if`).
        //
        // A field rename, deletion, or accidental top-level relocation is
        // caught here loudly with a named-field assertion. New fields land
        // alongside this test, not at the expense of it.
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let metadata = RunMetadata {
            tool: ToolInfo {
                name: "demo".into(),
                binary: None,
                version: None,
            },
            anc: AncInfo {
                version: "0.0.1-test",
            },
            run: RunInfo {
                invocation: "anc audit .".into(),
                started_at: "2026-04-29T16:00:00Z".into(),
                duration_ms: 42,
                platform: PlatformInfo {
                    os: "linux",
                    arch: "x86_64",
                },
            },
            target: TargetInfo {
                kind: "project".into(),
                path: Some("/tmp/x".into()),
                command: None,
            },
        };
        let json = format_json(&results, &[], None, None, metadata);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        // 0.1 / 0.2 / 0.3 keys remain — defends against accidental removal
        // during schema work.
        for path in [
            "schema_version",
            "results",
            "summary",
            "coverage_summary",
            "audience",
            "audit_profile",
            "spec_version",
        ] {
            assert!(
                parsed.get(path).is_some(),
                "0.1-0.3 key `{path}` must remain present in 0.5",
            );
        }

        // 0.4 + 0.5 additions — every documented sub-key resolves.
        assert_eq!(parsed["schema_version"], "0.6");
        for path in [
            // 0.4
            "tool.name",
            "tool.binary",
            "tool.version",
            "anc.version",
            "run.invocation",
            "run.started_at",
            "run.duration_ms",
            "run.platform.os",
            "run.platform.arch",
            "target.kind",
            "target.path",
            "target.command",
            // 0.5 — badge block
            "badge.eligible",
            "badge.score_pct",
            "badge.embed_markdown",
            "badge.scorecard_url",
            "badge.badge_url",
            "badge.convention_url",
        ] {
            let mut node = &parsed;
            for segment in path.split('.') {
                node = node
                    .get(segment)
                    .unwrap_or_else(|| panic!("0.5 key `{path}` missing — segment `{segment}`"));
            }
        }

        // Emitted values match the synthetic input.
        assert_eq!(parsed["tool"]["name"], "demo");
        assert_eq!(parsed["anc"]["version"], "0.0.1-test");
        assert_eq!(parsed["run"]["invocation"], "anc audit .");
        assert_eq!(parsed["run"]["duration_ms"], 42);
        assert_eq!(parsed["run"]["platform"]["os"], "linux");
        assert_eq!(parsed["target"]["kind"], "project");
        assert_eq!(parsed["target"]["path"], "/tmp/x");

        // Always-present-null contract: `tool.version`, `target.command`
        // serialize as JSON null, not as missing keys. Consumer code should
        // be able to access these paths unconditionally.
        assert!(parsed["tool"]["version"].is_null());
        assert!(parsed["tool"]["binary"].is_null());
        assert!(parsed["target"]["command"].is_null());
    }

    #[test]
    fn compute_badge_eligible_when_all_pass_and_slug_present() {
        // Three Pass and zero failures → 100% → above the 80% floor.
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Pass, CheckGroup::P2),
            make_result("c3", CheckStatus::Pass, CheckGroup::P3),
        ];
        let badge = compute_badge(&results, "navi");
        assert!(badge.eligible);
        assert_eq!(badge.score_pct, 100);
        assert_eq!(
            badge.embed_markdown.as_deref(),
            Some("[![agent-native](https://anc.dev/badge/navi.svg)](https://anc.dev/score/navi)"),
        );
        assert_eq!(
            badge.scorecard_url.as_deref(),
            Some("https://anc.dev/score/navi"),
        );
        assert_eq!(
            badge.badge_url.as_deref(),
            Some("https://anc.dev/badge/navi.svg"),
        );
        assert_eq!(badge.convention_url, "https://anc.dev/badge");
    }

    #[test]
    fn compute_badge_below_floor_emits_urls_but_no_embed() {
        // 4 of 5 fail → 1 pass / 5 denom = 20% → below floor.
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Fail("a".into()), CheckGroup::P2),
            make_result("c3", CheckStatus::Fail("b".into()), CheckGroup::P3),
            make_result("c4", CheckStatus::Fail("c".into()), CheckGroup::P4),
            make_result("c5", CheckStatus::Fail("d".into()), CheckGroup::P5),
        ];
        let badge = compute_badge(&results, "needs-work");
        assert!(!badge.eligible);
        assert_eq!(badge.score_pct, 20);
        assert!(
            badge.embed_markdown.is_none(),
            "below the floor: embed_markdown must be None per the do-not-nag rule",
        );
        // The site renders an SVG for every scored tool regardless of
        // score, so the URL is still useful below the floor.
        assert!(badge.scorecard_url.is_some());
        assert!(badge.badge_url.is_some());
    }

    #[test]
    fn compute_badge_at_floor_is_eligible() {
        // 4 pass / 5 denom = 80% — exactly at the floor must qualify.
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Pass, CheckGroup::P2),
            make_result("c3", CheckStatus::Pass, CheckGroup::P3),
            make_result("c4", CheckStatus::Pass, CheckGroup::P4),
            make_result("c5", CheckStatus::Fail("one fail".into()), CheckGroup::P5),
        ];
        let badge = compute_badge(&results, "edge-case");
        assert!(badge.eligible, "score == floor must qualify");
        assert_eq!(badge.score_pct, 80);
        assert!(badge.embed_markdown.is_some());
    }

    #[test]
    fn compute_badge_skips_excluded_from_denominator() {
        // 1 Pass + 1 Skip + 1 Error → denom is 1 (only Pass), score 100%.
        // Skips and Errors must not pull the score down — they're not
        // verdicts, so the leaderboard formula excludes them.
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "c2",
                CheckStatus::Skip("not applicable".into()),
                CheckGroup::P2,
            ),
            make_result("c3", CheckStatus::Error("boom".into()), CheckGroup::P3),
        ];
        let badge = compute_badge(&results, "skipper");
        assert_eq!(badge.score_pct, 100);
        assert!(badge.eligible);
    }

    #[test]
    fn compute_badge_no_scoring_data_is_ineligible() {
        // Every result is a Skip → denominator is zero. Score 0% and not
        // eligible — guard against division-by-zero turning into NaN or a
        // misleading 100%.
        let results = vec![
            make_result("c1", CheckStatus::Skip("filtered".into()), CheckGroup::P1),
            make_result("c2", CheckStatus::Skip("filtered".into()), CheckGroup::P2),
        ];
        let badge = compute_badge(&results, "ghost");
        assert_eq!(badge.score_pct, 0);
        assert!(!badge.eligible);
        assert!(badge.embed_markdown.is_none());
    }

    #[test]
    fn compute_badge_empty_slug_is_ineligible_even_at_perfect_score() {
        // Without a tool slug the embed URL would be malformed
        // (`/badge/.svg`); ineligible is the safe default — better to omit
        // the hint than to emit a broken URL.
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let badge = compute_badge(&results, "");
        assert_eq!(badge.score_pct, 100);
        assert!(!badge.eligible);
        assert!(badge.embed_markdown.is_none());
        assert!(badge.scorecard_url.is_none());
        assert!(badge.badge_url.is_none());
        // Convention URL is always emitted — it's the same for every tool.
        assert_eq!(badge.convention_url, "https://anc.dev/badge");
    }

    #[test]
    fn badge_text_hint_present_when_eligible() {
        let badge = compute_badge(
            &[make_result("c1", CheckStatus::Pass, CheckGroup::P1)],
            "demo",
        );
        let hint = badge.text_hint().expect("eligible run must produce hint");
        assert!(
            hint.contains("Score: 100%"),
            "hint should announce the score, got: {hint}",
        );
        assert!(
            hint.contains("https://anc.dev/badge/demo.svg"),
            "hint should embed the canonical badge URL, got: {hint}",
        );
        assert!(
            hint.contains("https://anc.dev/score/demo"),
            "hint should link to the per-tool scorecard page, got: {hint}",
        );
        assert!(
            hint.contains("https://anc.dev/badge"),
            "hint should reference the convention page, got: {hint}",
        );
    }

    #[test]
    fn badge_text_hint_absent_when_below_floor() {
        // The "do not nag" rule from the badge convention: below the floor
        // we print nothing badge-related.
        let badge = compute_badge(
            &[
                make_result("c1", CheckStatus::Fail("a".into()), CheckGroup::P1),
                make_result("c2", CheckStatus::Fail("b".into()), CheckGroup::P2),
            ],
            "needs-work",
        );
        assert!(badge.text_hint().is_none());
    }

    #[test]
    fn format_text_appends_hint_when_badge_eligible() {
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let badge = compute_badge(&results, "demo");
        let text = format_text(&results, false, Some(&badge), TextOptions::default());
        assert!(
            text.contains("qualifies for the agent-native badge"),
            "format_text must append the badge hint when eligible:\n{text}",
        );
        assert!(
            text.contains("https://anc.dev/badge/demo.svg"),
            "embedded URL must use the tool slug:\n{text}",
        );
    }

    #[test]
    fn format_text_omits_hint_when_below_floor() {
        let results = vec![
            make_result("c1", CheckStatus::Fail("a".into()), CheckGroup::P1),
            make_result("c2", CheckStatus::Fail("b".into()), CheckGroup::P2),
        ];
        let badge = compute_badge(&results, "needs-work");
        let text = format_text(&results, false, Some(&badge), TextOptions::default());
        assert!(
            !text.contains("agent-native badge"),
            "below-floor runs must not nag:\n{text}",
        );
    }

    #[test]
    fn format_text_without_badge_arg_is_unchanged() {
        // Callers that pass `None` (e.g., legacy plumbing or tests
        // exercising the formatter alone) get the historical output with
        // no badge tail.
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let text = format_text(&results, false, None, TextOptions::default());
        assert!(!text.contains("agent-native badge"));
    }

    #[test]
    fn format_text_raw_emits_id_tab_status_per_line() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Warn("watch this".into()), CheckGroup::P2),
            make_result("c3", CheckStatus::Fail("broken".into()), CheckGroup::P3),
            make_result("c4", CheckStatus::Skip("n/a".into()), CheckGroup::P4),
        ];
        let opts = TextOptions {
            raw: true,
            color: false,
        };
        let text = format_text(&results, false, None, opts);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, vec!["c1\tPASS", "c2\tWARN", "c3\tFAIL", "c4\tSKIP"]);
    }

    #[test]
    fn format_text_color_wraps_status_prefix() {
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let opts = TextOptions {
            raw: false,
            color: true,
        };
        let text = format_text(&results, false, None, opts);
        // Color enabled means the PASS prefix carries ANSI escapes.
        assert!(
            text.contains('\u{1b}'),
            "color=true should embed ANSI escapes around the PASS prefix:\n{text}",
        );
    }

    #[test]
    fn scorecard_emits_badge_block() {
        // End-to-end: a synthetic perfect run produces a JSON scorecard
        // whose `badge` block reflects eligibility and the slug echoed in
        // `tool.name`. Pins the contract that JSON consumers (notably the
        // site's `/score/<tool>` renderer) can rely on without re-running
        // `compute_badge` themselves.
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let metadata = RunMetadata {
            tool: ToolInfo {
                name: "navi".into(),
                binary: Some("navi".into()),
                version: Some("0.1.0".into()),
            },
            anc: AncInfo {
                version: "0.0.0-test",
            },
            run: RunInfo {
                invocation: "anc audit .".into(),
                started_at: "1970-01-01T00:00:00Z".into(),
                duration_ms: 0,
                platform: PlatformInfo {
                    os: "test-os",
                    arch: "test-arch",
                },
            },
            target: TargetInfo {
                kind: "project".into(),
                path: Some(".".into()),
                command: None,
            },
        };
        let json = format_json(&results, &[], None, None, metadata);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["badge"]["eligible"], true);
        assert_eq!(parsed["badge"]["score_pct"], 100);
        assert_eq!(
            parsed["badge"]["embed_markdown"],
            "[![agent-native](https://anc.dev/badge/navi.svg)](https://anc.dev/score/navi)"
        );
        assert_eq!(
            parsed["badge"]["scorecard_url"],
            "https://anc.dev/score/navi"
        );
        assert_eq!(
            parsed["badge"]["badge_url"],
            "https://anc.dev/badge/navi.svg"
        );
        assert_eq!(parsed["badge"]["convention_url"], "https://anc.dev/badge");
    }

    // ──────────────────────────────────────────────────────────────────
    // U2 (schema 0.6): per-row emission, tier, 7-status taxonomy,
    // antecedent propagation. Plan reference:
    // docs/plans/2026-05-21-001-feat-scorecard-fairness-taxonomy-plan.md
    // in agentnative-site.
    // ──────────────────────────────────────────────────────────────────

    fn make_raw(id: &str, status: CheckStatus) -> CheckResult {
        make_result(id, status, CheckGroup::P2)
    }

    /// Minimal `Check` impl that lets per-row fan-out tests express a
    /// `covers()` slice without spinning up a real probe.
    struct FakeCheck {
        id: &'static str,
        covers: &'static [&'static str],
    }

    impl crate::check::Check for FakeCheck {
        fn id(&self) -> &str {
            self.id
        }
        fn label(&self) -> &'static str {
            "fake"
        }
        fn group(&self) -> CheckGroup {
            CheckGroup::P2
        }
        fn layer(&self) -> CheckLayer {
            CheckLayer::Behavioral
        }
        fn applicable(&self, _p: &crate::project::Project) -> bool {
            true
        }
        fn run(&self, _p: &crate::project::Project) -> anyhow::Result<CheckResult> {
            unreachable!()
        }
        fn covers(&self) -> &'static [&'static str] {
            self.covers
        }
    }

    #[test]
    fn fan_out_emits_one_row_per_covered_requirement() {
        // Single probe (`p3-version`) covers two requirement rows. Fan-out
        // produces two entries with id = row_id and check_id = probe id.
        let raw = vec![make_raw(
            "p3-version",
            CheckStatus::Warn("short alias missing".into()),
        )];
        let catalog: Vec<Box<dyn crate::check::Check>> = vec![Box::new(FakeCheck {
            id: "p3-version",
            covers: &["p3-must-version", "p3-should-version-short"],
        })];
        let rows = fan_out_per_row(&raw, &catalog);
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows.iter().map(|(r, _)| r.id.as_str()).collect();
        assert!(ids.contains(&"p3-must-version"));
        assert!(ids.contains(&"p3-should-version-short"));
        for (r, check_id) in &rows {
            assert_eq!(
                check_id, "p3-version",
                "check_id provenance lost on row {}",
                r.id
            );
            assert!(
                matches!(r.status, CheckStatus::Warn(_)),
                "probe status must propagate to every covered row pre-propagation",
            );
        }
    }

    #[test]
    fn fan_out_emits_passthrough_for_checks_without_covers() {
        // Checks that don't declare any covers() pass through as a single
        // row keyed by check.id() — preserves the legacy shape for any
        // future check not yet wired into the registry.
        let raw = vec![make_raw("orphan-check", CheckStatus::Pass)];
        let catalog: Vec<Box<dyn crate::check::Check>> = vec![Box::new(FakeCheck {
            id: "orphan-check",
            covers: &[],
        })];
        let rows = fan_out_per_row(&raw, &catalog);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id, "orphan-check");
        assert_eq!(rows[0].1, "orphan-check");
    }

    #[test]
    fn propagation_passes_through_when_antecedent_is_pass_warn_fail() {
        // Antecedent statuses that mean "feature present" (pass / warn /
        // fail) leave the consequent row untouched.
        let raw = vec![
            make_raw("p2-json-output", CheckStatus::Pass),
            make_raw("p2-schema-print", CheckStatus::Fail("missing".into())),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Fail("missing".into())),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(matches!(rows[0].0.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn propagation_collapses_consequent_when_antecedent_is_opt_out() {
        // Antecedent OptOut → consequent becomes NotApplicable, regardless
        // of what the consequent's own probe emitted.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::OptOut("no --output flag".into()),
            ),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        match &rows[0].0.status {
            CheckStatus::NotApplicable(reason) => {
                assert!(
                    reason.contains("p2-json-output") && reason.contains("opt_out"),
                    "evidence should cite the antecedent + its status, got: {reason}",
                );
            }
            other => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn propagation_collapses_consequent_when_antecedent_is_n_a() {
        // n_a antecedent (e.g., a chained conditional) propagates the same
        // way as opt_out.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::NotApplicable("upstream n/a".into()),
            ),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(matches!(rows[0].0.status, CheckStatus::NotApplicable(_)));
    }

    #[test]
    fn propagation_inherits_skip_from_antecedent() {
        // Skip antecedent → consequent inherits Skip (couldn't measure
        // upstream means can't meaningfully evaluate downstream).
        let raw = vec![
            make_raw("p2-json-output", CheckStatus::Skip("probe limit".into())),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(matches!(rows[0].0.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn propagation_inherits_error_from_antecedent() {
        let raw = vec![
            make_raw("p2-json-output", CheckStatus::Error("probe crashed".into())),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(matches!(rows[0].0.status, CheckStatus::Error(_)));
    }

    #[test]
    fn propagation_leaves_universal_rows_untouched() {
        // A row with applicability: universal must not be touched by
        // propagation even if a check with the same id exists in `raw`.
        let raw = vec![make_raw("p1-non-interactive", CheckStatus::Pass)];
        let mut rows = vec![(
            make_raw("p1-must-no-interactive", CheckStatus::Pass),
            "p1-non-interactive".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(matches!(rows[0].0.status, CheckStatus::Pass));
    }

    #[test]
    fn build_row_results_fans_out_then_propagates() {
        // End-to-end through the shared entry point both output surfaces
        // use: fan-out keys rows to requirement ids, then propagation
        // collapses the conditional consequent because its antecedent
        // opted out. A refactor that drops either step is caught here.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::OptOut("no --output flag".into()),
            ),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let catalog: Vec<Box<dyn crate::check::Check>> = vec![
            Box::new(FakeCheck {
                id: "p2-json-output",
                covers: &["p2-must-output-flag"],
            }),
            Box::new(FakeCheck {
                id: "p2-schema-print",
                covers: &["p2-must-schema-print"],
            }),
        ];
        let rows = build_row_results(&raw, &catalog);
        let schema_row = rows
            .iter()
            .find(|(r, _)| r.id == "p2-must-schema-print")
            .expect("schema-print requirement row present");
        match &schema_row.0.status {
            CheckStatus::NotApplicable(reason) => assert!(
                reason.contains("p2-json-output") && reason.contains("opt_out"),
                "consequent must collapse to n_a citing the antecedent, got: {reason}",
            ),
            other => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn exit_code_drops_to_zero_when_consequent_propagates_to_n_a() {
        // Key Technical Decision §4: exit_code reads the per-row set, not
        // raw probes. A probe that raw-Fails a requirement whose row
        // collapses to n_a (its antecedent opted out) must not lift the
        // exit code — the requirement does not apply. Raw results would
        // exit 2; the per-row set exits 0.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::OptOut("no --output flag".into()),
            ),
            make_raw("p2-schema-print", CheckStatus::Fail("no schema".into())),
        ];
        assert_eq!(exit_code(&raw), 2, "raw probe Fail would exit 2");

        let catalog: Vec<Box<dyn crate::check::Check>> = vec![
            Box::new(FakeCheck {
                id: "p2-json-output",
                covers: &["p2-must-output-flag"],
            }),
            Box::new(FakeCheck {
                id: "p2-schema-print",
                covers: &["p2-must-schema-print"],
            }),
        ];
        let per_row: Vec<CheckResult> = build_row_results(&raw, &catalog)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        assert_eq!(
            exit_code(&per_row),
            0,
            "consequent propagated to n_a must not lift the exit code",
        );
    }

    #[test]
    fn text_and_json_agree_on_a_propagated_conditional_row() {
        // $100 guard for the text/JSON data-flow gap this plan closed: both
        // surfaces must derive the same row set from the same raw results +
        // catalog. A bat-shaped fixture — an opt_out antecedent
        // (p2-json-output) plus a raw-Fail consequent (p2-schema-print) —
        // exercises the n_a propagation both surfaces share. If the text
        // path ever stops routing through build_row_results, the row id,
        // count, status, and badge score diverge and this fails.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::OptOut("no --output flag".into()),
            ),
            make_raw(
                "p2-schema-print",
                CheckStatus::Fail("no schema surface".into()),
            ),
        ];
        let catalog: Vec<Box<dyn crate::check::Check>> = vec![
            Box::new(FakeCheck {
                id: "p2-json-output",
                covers: &["p2-must-output-flag"],
            }),
            Box::new(FakeCheck {
                id: "p2-schema-print",
                covers: &["p2-must-schema-print"],
            }),
        ];

        // Text path: exactly the projection main::run feeds the renderer.
        let per_row: Vec<CheckResult> = build_row_results(&raw, &catalog)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        let text_badge = compute_badge(&per_row, "fixture-tool");
        let text = format_text(&per_row, false, Some(&text_badge), TextOptions::default());

        // JSON path.
        let scorecard = build_scorecard(&raw, &catalog, None, None, fixture_metadata());

        // (a) The consequent renders n_a, never fail, on both surfaces.
        let json_consequent = scorecard
            .results
            .iter()
            .find(|v| v.id == "p2-must-schema-print")
            .expect("consequent row present in JSON");
        assert_eq!(json_consequent.status, "n_a");
        assert!(
            text.contains("[N/A ]") && text.contains("p2-must-schema-print"),
            "text must render the consequent row as N/A:\n{text}",
        );
        assert!(
            !text.contains("[FAIL]"),
            "the only raw-Fail probe propagated to n_a; no row may render FAIL:\n{text}",
        );

        // (b) Row counts agree.
        assert_eq!(
            per_row.len(),
            scorecard.results.len(),
            "text row count must equal JSON results.len()",
        );

        // (c) Badge scores agree.
        assert_eq!(
            text_badge.score_pct, scorecard.badge.score_pct,
            "text badge score must equal JSON badge.score_pct",
        );
    }

    #[test]
    fn score_pct_excludes_opt_out_and_n_a_from_denominator() {
        // Transitional formula (U2): pass / (pass + warn + fail). opt_out
        // and n_a do not count in either side of the ratio. A run of
        // 1 Pass + 1 OptOut + 1 NotApplicable scores 100% (1/1), not 33%.
        let results = vec![
            make_raw("c1", CheckStatus::Pass),
            make_raw("c2", CheckStatus::OptOut("deliberate".into())),
            make_raw("c3", CheckStatus::NotApplicable("conditional unmet".into())),
        ];
        assert_eq!(score_pct(&results), 100);

        // Adding one Fail pulls the score down to 50%; opt_out / n_a still
        // do not contribute to the denominator.
        let mixed = vec![
            make_raw("c1", CheckStatus::Pass),
            make_raw("c2", CheckStatus::Fail("violates".into())),
            make_raw("c3", CheckStatus::OptOut("deliberate".into())),
            make_raw("c4", CheckStatus::NotApplicable("conditional unmet".into())),
        ];
        assert_eq!(score_pct(&mixed), 50);
    }

    #[test]
    fn summary_counts_seven_statuses_independently() {
        // build_summary surfaces opt_out and n_a alongside the historical
        // five counters; total covers all seven.
        let results = vec![
            make_raw("a", CheckStatus::Pass),
            make_raw("b", CheckStatus::Warn("w".into())),
            make_raw("c", CheckStatus::Fail("f".into())),
            make_raw("d", CheckStatus::OptOut("o".into())),
            make_raw("e", CheckStatus::NotApplicable("n".into())),
            make_raw("f", CheckStatus::Skip("s".into())),
            make_raw("g", CheckStatus::Error("e".into())),
        ];
        let s = build_summary(&results);
        assert_eq!(s.total, 7);
        assert_eq!(s.pass, 1);
        assert_eq!(s.warn, 1);
        assert_eq!(s.fail, 1);
        assert_eq!(s.opt_out, 1);
        assert_eq!(s.n_a, 1);
        assert_eq!(s.skip, 1);
        assert_eq!(s.error, 1);
    }

    #[test]
    fn check_result_view_carries_tier_and_check_id() {
        // Per-row CheckResultView built via from_row exposes the requirement
        // tier (looked up from the registry) and the originating probe.
        let r = make_raw("p3-must-version", CheckStatus::Pass);
        let view = CheckResultView::from_row(&r, "p3-version");
        assert_eq!(view.id, "p3-must-version");
        assert_eq!(view.check_id, "p3-version");
        assert_eq!(view.tier.as_deref(), Some("must"));
    }

    #[test]
    fn check_result_view_tier_is_null_for_unknown_row_id() {
        // Test fixtures with synthetic ids that don't exist in the registry
        // surface as JSON null for tier — visible signal of inconsistency.
        let r = make_raw("not-a-real-row-id", CheckStatus::Pass);
        let view = CheckResultView::from_row(&r, "some-check");
        assert!(view.tier.is_none(), "got: {:?}", view.tier);
    }

    #[test]
    fn opt_out_status_serializes_as_opt_out_in_json() {
        let r = make_raw("c1", CheckStatus::OptOut("test reason".into()));
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.status, "opt_out");
        assert_eq!(view.evidence.as_deref(), Some("test reason"));
    }

    #[test]
    fn n_a_status_serializes_as_n_a_in_json() {
        let r = make_raw("c1", CheckStatus::NotApplicable("antecedent unmet".into()));
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.status, "n_a");
        assert_eq!(view.evidence.as_deref(), Some("antecedent unmet"));
    }

    // ──────────────────────────────────────────────────────────────────
    // U2 red team: adversarial inputs that try to break the per-row +
    // propagation pipeline or the score formula.
    // ──────────────────────────────────────────────────────────────────

    #[test]
    fn rt_propagation_is_idempotent() {
        // Propagation reads raw probe statuses (not row statuses), so a
        // second pass over an already-propagated row vector must produce
        // an identical result. Pins the no-feedback contract — a future
        // refactor that reads `rows` in place would break this and
        // potentially loop or oscillate on chained conditionals.
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::OptOut("no --output flag".into()),
            ),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        let after_first: Vec<(String, String)> = rows
            .iter()
            .map(|(r, c)| {
                (
                    serde_json::to_string(&r.status).expect("status serializes"),
                    c.clone(),
                )
            })
            .collect();
        propagate_antecedents(&mut rows, &raw);
        let after_second: Vec<(String, String)> = rows
            .iter()
            .map(|(r, c)| {
                (
                    serde_json::to_string(&r.status).expect("status serializes"),
                    c.clone(),
                )
            })
            .collect();
        assert_eq!(after_first, after_second, "propagation must be idempotent");
    }

    #[test]
    fn rt_propagation_no_op_when_antecedent_did_not_run() {
        // Source-only or filtered run: the antecedent probe didn't produce
        // a raw result. The row keeps its own status — propagation can't
        // override what it can't read. This is the exact path tools using
        // `--source` or `--principle <N>` exercise in production.
        let raw = vec![make_raw("p2-schema-print", CheckStatus::Pass)];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        assert!(
            matches!(rows[0].0.status, CheckStatus::Pass),
            "no antecedent in raw → row untouched, got: {:?}",
            rows[0].0.status,
        );
    }

    #[test]
    fn rt_propagation_inherits_audit_profile_suppression_as_skip() {
        // Adversarial case: a CLI runs with `--audit-profile <X>` that
        // suppresses the antecedent probe. The suppressed Skip carries the
        // SUPPRESSION_EVIDENCE_PREFIX sentinel. The consequent row should
        // inherit Skip (cannot meaningfully evaluate). The new evidence
        // string cites the antecedent so a reader can still trace the
        // root cause back to the audit profile.
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;
        let raw = vec![
            make_raw(
                "p2-json-output",
                CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui")),
            ),
            make_raw("p2-schema-print", CheckStatus::Pass),
        ];
        let mut rows = vec![(
            make_raw("p2-must-schema-print", CheckStatus::Pass),
            "p2-schema-print".to_string(),
        )];
        propagate_antecedents(&mut rows, &raw);
        match &rows[0].0.status {
            CheckStatus::Skip(reason) => {
                assert!(
                    reason.contains("p2-json-output"),
                    "propagated Skip must cite the antecedent, got: {reason}",
                );
                assert!(
                    reason.contains("human-tui"),
                    "propagated Skip must preserve the suppression reason, got: {reason}",
                );
            }
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn rt_score_pct_only_n_a_returns_zero_without_panic() {
        // Pathological: every result is NotApplicable. Denominator is zero;
        // score must surface as 0 with no division-by-zero or NaN.
        let results: Vec<CheckResult> = (0..100)
            .map(|i| {
                make_raw(
                    &format!("row-{i}"),
                    CheckStatus::NotApplicable("conditional unmet".into()),
                )
            })
            .collect();
        assert_eq!(score_pct(&results), 0);
    }

    #[test]
    fn rt_score_pct_only_opt_out_returns_zero_without_panic() {
        // Mirror of the n_a case: opt_out also excluded from denominator.
        let results: Vec<CheckResult> = (0..50)
            .map(|i| {
                make_raw(
                    &format!("row-{i}"),
                    CheckStatus::OptOut("deliberate".into()),
                )
            })
            .collect();
        assert_eq!(score_pct(&results), 0);
    }

    #[test]
    fn rt_score_pct_one_pass_amid_999_n_a_returns_100() {
        // n_a must not dilute. One genuine pass against a thousand
        // inapplicable rows is still 100%.
        let mut results: Vec<CheckResult> = (0..999)
            .map(|i| {
                make_raw(
                    &format!("row-{i}"),
                    CheckStatus::NotApplicable("conditional unmet".into()),
                )
            })
            .collect();
        results.push(make_raw("row-last", CheckStatus::Pass));
        assert_eq!(score_pct(&results), 100);
    }

    #[test]
    fn rt_score_pct_skip_and_error_still_excluded() {
        // Carry the legacy contract forward: Skip and Error contribute to
        // neither side. A run of (1 Pass + 100 Skip + 100 Error) is 100%.
        let mut results = vec![make_raw("good", CheckStatus::Pass)];
        for i in 0..100 {
            results.push(make_raw(
                &format!("s-{i}"),
                CheckStatus::Skip("limit".into()),
            ));
            results.push(make_raw(
                &format!("e-{i}"),
                CheckStatus::Error("boom".into()),
            ));
        }
        assert_eq!(score_pct(&results), 100);
    }

    #[test]
    fn rt_evidence_with_control_chars_roundtrips_through_json() {
        // Evidence strings come from probe output and may contain quotes,
        // backslashes, newlines, tabs. serde_json must escape them. The
        // roundtrip parse must recover the exact byte sequence.
        let hostile: &str = "line1\nline2\t\"quoted\"\\backslash\u{0007}bell";
        let r = make_raw("c1", CheckStatus::Warn(hostile.to_string()));
        let view = CheckResultView::from_result(&r);
        let json = serde_json::to_string(&view).expect("view serializes");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("re-parses");
        assert_eq!(
            parsed["evidence"].as_str(),
            Some(hostile),
            "evidence must roundtrip through JSON without loss",
        );
    }

    #[test]
    fn rt_evidence_with_unicode_zero_width_and_rtl_roundtrips() {
        // Zero-width joiner and RTL override are common smuggling vectors
        // in display contexts. They must roundtrip through JSON unchanged;
        // any sanitization belongs at the render layer (site), not here.
        let hostile = "left\u{202e}right\u{200b}invisible";
        let r = make_raw("c1", CheckStatus::OptOut(hostile.to_string()));
        let view = CheckResultView::from_result(&r);
        let json = serde_json::to_string(&view).expect("view serializes");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("re-parses");
        assert_eq!(parsed["evidence"].as_str(), Some(hostile));
        assert_eq!(parsed["status"], "opt_out");
    }

    #[test]
    fn rt_summary_total_equals_sum_of_per_status_counts() {
        // Invariant: total == pass + warn + fail + opt_out + n_a + skip + error.
        // A new variant added without updating build_summary would break this.
        let statuses = vec![
            CheckStatus::Pass,
            CheckStatus::Pass,
            CheckStatus::Warn("w".into()),
            CheckStatus::Fail("f".into()),
            CheckStatus::OptOut("o".into()),
            CheckStatus::OptOut("o".into()),
            CheckStatus::OptOut("o".into()),
            CheckStatus::NotApplicable("n".into()),
            CheckStatus::Skip("s".into()),
            CheckStatus::Error("e".into()),
        ];
        let results: Vec<CheckResult> = statuses
            .into_iter()
            .enumerate()
            .map(|(i, s)| make_raw(&format!("c{i}"), s))
            .collect();
        let s = build_summary(&results);
        assert_eq!(
            s.total,
            s.pass + s.warn + s.fail + s.opt_out + s.n_a + s.skip + s.error,
            "summary.total must equal the sum of every per-status counter",
        );
    }

    #[test]
    fn rt_full_pipeline_n_a_excluded_from_summary_n_a_and_score() {
        // End-to-end: a probe emits OptOut for the antecedent. After
        // fan-out + propagation, the consequent row carries n_a. The
        // summary counts both. The score reflects the non-conditional
        // pass-rate, untouched by the opt_out / n_a pair.
        let raw = vec![
            make_raw("p2-json-output", CheckStatus::OptOut("no flag".into())),
            make_raw("p2-schema-print", CheckStatus::Pass),
            make_raw("p1-non-interactive", CheckStatus::Pass),
        ];
        let catalog: Vec<Box<dyn crate::check::Check>> = vec![
            Box::new(FakeCheck {
                id: "p2-json-output",
                covers: &["p2-must-output-flag"],
            }),
            Box::new(FakeCheck {
                id: "p2-schema-print",
                covers: &["p2-must-schema-print"],
            }),
            Box::new(FakeCheck {
                id: "p1-non-interactive",
                covers: &["p1-must-no-interactive"],
            }),
        ];
        let mut rows = fan_out_per_row(&raw, &catalog);
        propagate_antecedents(&mut rows, &raw);
        let per_row: Vec<CheckResult> = rows.into_iter().map(|(r, _)| r).collect();

        let s = build_summary(&per_row);
        assert_eq!(s.opt_out, 1, "p2-must-output-flag → opt_out: got {s:?}");
        assert_eq!(
            s.n_a, 1,
            "p2-must-schema-print → n_a via propagation: got {s:?}",
        );
        // Score: 2 passes (p2-must-output-flag is opt_out, p2-must-schema-print
        // is n_a; only 2 pass rows remain). Denominator = 2 (both passes), so
        // 100%. The opt_out + n_a do not pull the score down.
        assert_eq!(score_pct(&per_row), 100);
    }
}
