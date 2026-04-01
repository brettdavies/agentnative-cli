use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct SigpipeCheck;

impl Check for SigpipeCheck {
    fn id(&self) -> &str {
        "p6-sigpipe"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();
        let result = runner.run_partial(&["--help"], 16);

        let status = match result.status {
            RunStatus::Ok => CheckStatus::Pass,
            RunStatus::Crash { signal } => {
                CheckStatus::Fail(format!("crashed on SIGPIPE (signal {signal})"))
            }
            _ => CheckStatus::Warn(format!("unexpected status: {:?}", result.status)),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Handles SIGPIPE gracefully".into(),
            group: CheckGroup::P6,
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
    fn sigpipe_pass_with_echo() {
        let project = test_project_with_runner("/bin/echo");
        let result = SigpipeCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }
}
