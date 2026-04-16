use crate::project::Project;
use crate::types::CheckResult;

/// Trait implemented by all checks (behavioral, source, project).
pub trait Check {
    /// Unique identifier for this check (e.g., "code-unwrap", "p3-help").
    fn id(&self) -> &str;

    /// Whether this check is applicable to the given project.
    fn applicable(&self, project: &Project) -> bool;

    /// Run the check against the project.
    fn run(&self, project: &Project) -> anyhow::Result<CheckResult>;
}
