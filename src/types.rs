use serde::Serialize;

/// The result of running a single check.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", content = "evidence")]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn(String),
    Fail(String),
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
