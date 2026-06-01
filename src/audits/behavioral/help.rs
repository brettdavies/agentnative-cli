use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct HelpAudit;

impl Audit for HelpAudit {
    fn id(&self) -> &str {
        "p3-help"
    }

    fn label(&self) -> &'static str {
        "Help flag produces useful output"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-top-level-examples"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok if result.exit_code == Some(0) => {
                let output = format!("{}{}", result.stdout, result.stderr);
                if output.trim().is_empty() {
                    AuditStatus::Fail("--help produced no output".into())
                } else if has_examples_section(&output) {
                    AuditStatus::Pass
                } else {
                    AuditStatus::Warn(
                        "--help output exists but no examples section detected".into(),
                    )
                }
            }
            RunStatus::Ok => AuditStatus::Fail(format!(
                "--help exited with code {}",
                result
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into())
            )),
            _ => AuditStatus::Fail(format!("--help failed: {:?}", result.status)),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P3,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
        })
    }
}

fn has_examples_section(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("example")
        || lower.contains("usage:")
        || lower.contains("usage\n")
        || lower.contains("examples:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::test_project_with_runner;

    #[test]
    fn help_audit_applicable_with_runner() {
        let project = test_project_with_runner("/bin/echo");
        assert!(HelpAudit.applicable(&project));
    }

    #[test]
    fn help_audit_not_applicable_without_runner() {
        let project = test_project_with_runner("/bin/echo");
        let mut project = project;
        project.runner = None;
        assert!(!HelpAudit.applicable(&project));
    }

    #[test]
    fn help_pass_with_examples() {
        let runner =
            crate::runner::BinaryRunner::new("/bin/sh".into(), std::time::Duration::from_secs(5))
                .expect("create test runner");
        let result = runner.run(&["-c", "echo 'Usage: foo\nExamples:\n  foo bar'"], &[]);
        assert!(has_examples_section(&result.stdout));
    }

    #[test]
    fn help_detects_examples_section() {
        assert!(has_examples_section("EXAMPLES\n  run foo"));
        assert!(has_examples_section("Usage: mycli [OPTIONS]"));
        assert!(has_examples_section("Examples:\n  mycli run"));
        assert!(!has_examples_section("This is just a description"));
    }

    #[test]
    fn help_handles_crash() {
        let project = crate::audits::behavioral::tests::test_project_with_sh_script("kill -11 $$");
        let result = HelpAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }
}
