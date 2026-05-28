use serde::Serialize;

/// The result of running a single check.
///
/// The 7-status taxonomy splits the former `Skip` bucket into three distinct
/// outcomes so the scoring algorithm can tell "tool deliberately did not adopt
/// this" (`OptOut`) from "this check does not apply to this tool"
/// (`NotApplicable`) from "the linter could not measure" (`Skip`). See plan
/// `docs/plans/2026-05-21-001-feat-scorecard-fairness-taxonomy-plan.md` for
/// the taxonomy rationale and Decision 2a for antecedent propagation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", content = "evidence")]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn(String),
    Fail(String),
    /// Tool clearly has the capability surface but does not ship this feature
    /// (deliberate non-adoption). Excluded from the numerator; whether it
    /// counts in the denominator is the open formula choice deferred to U3.
    OptOut(String),
    /// Conditional antecedent unmet — the requirement does not apply to this
    /// tool. Excluded from both numerator and denominator. Set either by a
    /// verifier directly or by antecedent propagation in the scorecard module.
    NotApplicable(String),
    /// Linter probe limitation: the check could not measure. Excluded from
    /// both numerator and denominator (preserved for backward compatibility;
    /// pre-0.6 scorecards used this bucket for all of OptOut / NotApplicable
    /// / Skip).
    Skip(String),
    Error(String),
}

/// How confident a check is in its verdict. Direct probes (flag parsers,
/// exit-code observation) report `High`; heuristic text inference reports
/// `Medium`; soft cross-signal inference reports `Low`. Consumers use this
/// to weight conflicting signals and surface caveats on the scorecard.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    High,
    Medium,
    #[allow(dead_code)] // Reserved for future inferential checks.
    Low,
}

/// Groups checks by principle or category.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CheckGroup {
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
    P8,
    CodeQuality,
    ProjectStructure,
}

/// Which layer the check operates in.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum CheckLayer {
    Behavioral,
    Source,
    Project,
}

/// A single check result with metadata.
///
/// **Per-row emission (schema 0.6+).** A `Check::run()` produces one
/// probe-level `CheckResult` keyed by `id = check.id()`. The runner then
/// fans the probe out across every row in `Check::covers()`, producing one
/// scorecard row per requirement (with `id = row_id`). Antecedent
/// propagation runs after fan-out. The per-row result reuses this same
/// struct shape; `id` is then the requirement-row id, and the probe's
/// `check.id()` is recovered via the `check_id` field in
/// `scorecard::CheckResultView`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub id: String,
    pub label: String,
    pub group: CheckGroup,
    pub layer: CheckLayer,
    pub status: CheckStatus,
    /// How much the check trusts its own verdict. Defaults to `High`; only
    /// heuristic checks downgrade. Additive field; consumers feature-detect.
    #[serde(default)]
    pub confidence: Confidence,
}

/// A source location where a violation was found.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub text: String,
}
