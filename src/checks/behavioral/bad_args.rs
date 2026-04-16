use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct BadArgsCheck;

impl Check for BadArgsCheck {
    fn id(&self) -> &str {
        "p4-bad-args"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();
        let result = runner.run(&["--this-flag-does-not-exist-agentnative-probe"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                if result.exit_code.is_some_and(|c| c > 0) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail("binary silently accepted invalid flag (exit 0)".into())
                }
            }
            RunStatus::Crash { signal } => {
                CheckStatus::Fail(format!("binary crashed on bad args (signal {signal})"))
            }
            _ => CheckStatus::Fail(format!("unexpected status: {:?}", result.status)),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Rejects invalid arguments".into(),
            group: CheckGroup::P4,
            layer: CheckLayer::Behavioral,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_sh_script;
    use crate::types::CheckStatus;

    #[test]
    fn bad_args_pass_when_rejected() {
        // sh -c 'exit 1' always exits non-zero
        let project = test_project_with_sh_script("exit 2");
        let result = BadArgsCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn bad_args_fail_when_accepted() {
        // echo silently accepts any args with exit 0
        let project = crate::checks::behavioral::tests::test_project_with_runner("/bin/echo");
        let result = BadArgsCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn bad_args_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = BadArgsCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }
}
