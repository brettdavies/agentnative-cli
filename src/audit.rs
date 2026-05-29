use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult};

/// Trait implemented by all audits (behavioral, source, project).
pub trait Audit {
    /// Unique identifier for this audit (e.g., "code-unwrap", "p3-help").
    fn id(&self) -> &str;

    /// Human-readable one-line label. Surfaces in scorecard JSON
    /// (`results[].label`) and text output. Every `run()` implementation
    /// must hand this same string to `AuditResult`, and the suppression
    /// and error branches in `main::run` use it so that an audit
    /// short-circuited by `--audit-profile` or an internal error
    /// produces the same label the user would see on a successful run —
    /// rather than falling back to the opaque `id`.
    fn label(&self) -> &'static str;

    /// Which principle or category this audit belongs to.
    fn group(&self) -> AuditGroup;

    /// Which layer this audit operates in.
    fn layer(&self) -> AuditLayer;

    /// Whether this audit is applicable to the given project.
    fn applicable(&self, project: &Project) -> bool;

    /// Run the audit against the project.
    fn run(&self, project: &Project) -> anyhow::Result<AuditResult>;

    /// Requirement IDs (from `crate::principles::REQUIREMENTS`) that this
    /// audit verifies. Empty by default so audits opt in explicitly.
    /// The registry validator fails if an ID here is not registered.
    fn covers(&self) -> &'static [&'static str] {
        &[]
    }
}
