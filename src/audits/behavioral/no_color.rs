use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct NoColorBehavioralAudit;

impl Audit for NoColorBehavioralAudit {
    fn id(&self) -> &str {
        "p6-no-color-behavioral"
    }

    fn label(&self) -> &'static str {
        "Respects NO_COLOR"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-color"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        // Runner already sets NO_COLOR=1
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", result.stdout, result.stderr);
                if contains_ansi_escapes(&output) {
                    AuditStatus::Fail(
                        "output contains ANSI escape sequences despite NO_COLOR=1".into(),
                    )
                } else {
                    AuditStatus::Pass
                }
            }
            _ => AuditStatus::Skip("could not run --help to audit for ANSI escapes".into()),
        };

        Ok(AuditResult {
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
    use crate::audits::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::types::AuditStatus;

    #[test]
    fn no_color_pass_clean_output() {
        let project = test_project_with_runner("/bin/echo");
        let result = NoColorBehavioralAudit
            .run(&project)
            .expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn no_color_fail_with_ansi() {
        // Output ANSI escape sequence despite NO_COLOR
        let project = test_project_with_sh_script("printf '\\033[31mred text\\033[0m'");
        let result = NoColorBehavioralAudit
            .run(&project)
            .expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }

    #[test]
    fn contains_ansi_detection() {
        assert!(contains_ansi_escapes("\x1b[31mred\x1b[0m"));
        assert!(!contains_ansi_escapes("plain text"));
    }

    #[test]
    fn no_color_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = NoColorBehavioralAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Skip(_)));
    }
}
