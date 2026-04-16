use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct QuietCheck;

impl Check for QuietCheck {
    fn id(&self) -> &str {
        "p7-quiet"
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner.as_ref().unwrap();
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", result.stdout, result.stderr);
                if output.contains("--quiet") || output.contains("-q") {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warn("no --quiet/-q flag detected in --help output".into())
                }
            }
            _ => CheckStatus::Warn("could not run --help to detect quiet flag".into()),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Quiet mode available".into(),
            group: CheckGroup::P7,
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
    fn quiet_pass_when_flag_present() {
        let project = test_project_with_sh_script("echo '  --quiet  Suppress output'");
        let result = QuietCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn quiet_warn_when_flag_absent() {
        let project = test_project_with_sh_script("echo 'no quiet here'");
        let result = QuietCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }
}
