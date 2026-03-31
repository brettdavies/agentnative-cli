use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct NonInteractiveCheck;

impl Check for NonInteractiveCheck {
    fn id(&self) -> &str {
        "p1-non-interactive"
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner.as_ref().unwrap();
        let result = runner.run(&[], &[]);

        let status = match result.status {
            RunStatus::Timeout => {
                CheckStatus::Warn("binary may be waiting for interactive input".into())
            }
            RunStatus::Ok | RunStatus::Crash { .. } => CheckStatus::Pass,
            _ => CheckStatus::Pass,
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Non-interactive by default".into(),
            group: CheckGroup::P1,
            layer: CheckLayer::Behavioral,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_runner;
    use crate::types::CheckStatus;

    #[test]
    fn non_interactive_pass_with_echo() {
        // echo with no args exits immediately
        let project = test_project_with_runner("/bin/echo");
        let result = NonInteractiveCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn non_interactive_pass_with_false() {
        // /bin/false exits immediately with non-zero
        let project = test_project_with_runner("/bin/false");
        let result = NonInteractiveCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }
}
