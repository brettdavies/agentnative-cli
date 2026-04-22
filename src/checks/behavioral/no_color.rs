use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct NoColorBehavioralCheck;

impl Check for NoColorBehavioralCheck {
    fn id(&self) -> &str {
        "p6-no-color-behavioral"
    }

    fn label(&self) -> &'static str {
        "Respects NO_COLOR"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-color"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();
        // Runner already sets NO_COLOR=1
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", result.stdout, result.stderr);
                if contains_ansi_escapes(&output) {
                    CheckStatus::Fail(
                        "output contains ANSI escape sequences despite NO_COLOR=1".into(),
                    )
                } else {
                    CheckStatus::Pass
                }
            }
            _ => CheckStatus::Skip("could not run --help to check for ANSI escapes".into()),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

fn contains_ansi_escapes(text: &str) -> bool {
    text.contains("\x1b[")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::types::CheckStatus;

    #[test]
    fn no_color_pass_clean_output() {
        let project = test_project_with_runner("/bin/echo");
        let result = NoColorBehavioralCheck
            .run(&project)
            .expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn no_color_fail_with_ansi() {
        // Output ANSI escape sequence despite NO_COLOR
        let project = test_project_with_sh_script("printf '\\033[31mred text\\033[0m'");
        let result = NoColorBehavioralCheck
            .run(&project)
            .expect("check should run");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn contains_ansi_detection() {
        assert!(contains_ansi_escapes("\x1b[31mred\x1b[0m"));
        assert!(!contains_ansi_escapes("plain text"));
    }

    #[test]
    fn no_color_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = NoColorBehavioralCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }
}
