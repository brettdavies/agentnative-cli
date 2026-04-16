use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct VersionCheck;

impl Check for VersionCheck {
    fn id(&self) -> &str {
        "p3-version"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P3
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();
        let result = runner.run(&["--version"], &[]);

        let status = match result.status {
            RunStatus::Ok if result.exit_code == Some(0) && !result.stdout.trim().is_empty() => {
                CheckStatus::Pass
            }
            RunStatus::Ok if result.exit_code == Some(0) => {
                CheckStatus::Fail("--version produced no output".into())
            }
            RunStatus::Ok => CheckStatus::Fail(format!(
                "--version exited with code {}",
                result.exit_code.unwrap_or(-1)
            )),
            _ => CheckStatus::Fail(format!("--version failed: {:?}", result.status)),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Version flag works".into(),
            group: CheckGroup::P3,
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
    fn version_pass_with_output() {
        // echo always exits 0 and produces output for any args
        let project = test_project_with_runner("/bin/echo");
        let result = VersionCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn version_fail_non_zero_exit() {
        // /bin/false exits 1
        let project = test_project_with_runner("/bin/false");
        let result = VersionCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn version_handles_crash() {
        let project = crate::checks::behavioral::tests::test_project_with_sh_script("kill -11 $$");
        let result = VersionCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }
}
