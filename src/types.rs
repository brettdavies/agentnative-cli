use serde::Serialize;

/// The result of running a single audit.
///
/// The 7-status taxonomy splits the former `Skip` bucket into three distinct
/// outcomes so the scoring algorithm can tell "tool deliberately did not adopt
/// this" (`OptOut`) from "this audit does not apply to this tool"
/// (`NotApplicable`) from "the linter could not measure" (`Skip`). See plan
/// `docs/plans/2026-05-21-001-feat-scorecard-fairness-taxonomy-plan.md` for
/// the taxonomy rationale and Decision 2a for antecedent propagation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", content = "evidence")]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
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
    /// Linter probe limitation: the audit could not measure. Excluded from
    /// both numerator and denominator (preserved for backward compatibility;
    /// pre-0.6 scorecards used this bucket for all of OptOut / NotApplicable
    /// / Skip).
    Skip(String),
    Error(String),
}

/// How confident an audit is in its verdict. Direct probes (flag parsers,
/// exit-code observation) report `High`; heuristic text inference reports
/// `Medium`; soft cross-signal inference reports `Low`. Consumers use this
/// to weight conflicting signals and surface caveats on the scorecard.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    High,
    Medium,
    #[allow(dead_code)] // Reserved for future inferential audits.
    Low,
}

/// Groups audits by principle or category.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AuditGroup {
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

/// Which layer the audit operates in.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum AuditLayer {
    Behavioral,
    Source,
    Project,
}

/// A single audit result with metadata.
///
/// **Per-row emission (schema 0.6+).** A `Audit::run()` produces one
/// probe-level `AuditResult` keyed by `id = audit.id()`. The runner then
/// fans the probe out across every row in `Audit::covers()`, producing one
/// scorecard row per requirement (with `id = row_id`). Antecedent
/// propagation runs after fan-out. The per-row result reuses this same
/// struct shape; `id` is then the requirement-row id, and the probe's
/// `audit.id()` is recovered via the `audit_id` field in
/// `scorecard::AuditResultView`.
#[derive(Debug, Clone, Serialize)]
pub struct AuditResult {
    pub id: String,
    pub label: String,
    pub group: AuditGroup,
    pub layer: AuditLayer,
    pub status: AuditStatus,
    /// How much the audit trusts its own verdict. Defaults to `High`; only
    /// heuristic audits downgrade. Additive field; consumers feature-detect.
    #[serde(default)]
    pub confidence: Confidence,
    /// Per-audit transparency carrier: when an audit's Pass depended on a
    /// per-CLI mitigation (today: `.anc.toml [p6] domain_verbs` for
    /// `p6-standard-names`), the audit populates this so the scorecard
    /// distinguishes a self-declared Pass from an unassisted one. `None` for
    /// every audit that has no mitigation to declare. Carrier-shaped rather
    /// than audit-specific so future audits with similar transparency needs
    /// reuse the slot instead of growing parallel fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mitigation: Option<MitigationInfo>,
}

/// Transparency metadata attached to an `AuditResult` when its verdict
/// depended on a documented opt-in (config-driven recognition, suppression
/// profile, etc.). Distinct from `evidence`, which is prose; `MitigationInfo`
/// is the structured signal a downstream consumer (scorecard renderer,
/// leaderboard) can dispatch on without parsing the evidence string.
///
/// Current uses:
/// - `p6-standard-names`: when one or more subcommands matched the audit
///   target's `.anc.toml [p6] domain_verbs` list (not the built-in
///   `STANDARD_VERBS`), the audit fills `domain_match_count` and
///   `domain_match_examples`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MitigationInfo {
    /// True iff the verdict was assisted by the named opt-in. Always present
    /// when `MitigationInfo` itself is present (the `Some` sentinel of the
    /// parent `Option` is the same fact, but consumers find the explicit
    /// flag easier to dispatch on).
    pub using_domain_verbs: bool,
    /// Count of subcommands recognized via `domain_verbs` (not via the
    /// built-in standard-verb list).
    pub domain_match_count: usize,
    /// Up to the first 5 domain-verb matches in encounter order, for
    /// display alongside the evidence string. Truncated set; consumers that
    /// need every match should re-derive from the CLI's `--help` output.
    pub domain_match_examples: Vec<String>,
    /// Count of subcommands recognized via the built-in `STANDARD_VERBS`
    /// list (not via `domain_verbs`). Combined with `domain_match_count`,
    /// this lets a consumer compute the recognized fraction without
    /// re-running the audit probe.
    pub builtin_match_count: usize,
    /// Total subcommand count (denominator of the "recognized" fraction).
    pub subcommand_total: usize,
}

/// A source location where a violation was found.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub text: String,
}
