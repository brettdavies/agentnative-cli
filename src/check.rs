use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult};

/// Trait implemented by all checks (behavioral, source, project).
pub trait Check {
    /// Unique identifier for this check (e.g., "code-unwrap", "p3-help").
    fn id(&self) -> &str;

    /// Which principle or category this check belongs to.
    fn group(&self) -> CheckGroup;

    /// Which layer this check operates in.
    fn layer(&self) -> CheckLayer;

    /// Whether this check is applicable to the given project.
    fn applicable(&self, project: &Project) -> bool;

    /// Run the check against the project.
    fn run(&self, project: &Project) -> anyhow::Result<CheckResult>;

    /// Requirement IDs (from `crate::principles::REQUIREMENTS`) that this
    /// check verifies. Empty by default so checks opt in explicitly.
    /// The registry validator fails if an ID here is not registered.
    fn covers(&self) -> &'static [&'static str] {
        &[]
    }
}
