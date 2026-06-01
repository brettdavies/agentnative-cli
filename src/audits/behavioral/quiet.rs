use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct QuietAudit;

impl Audit for QuietAudit {
    fn id(&self) -> &str {
        "p7-quiet"
    }

    fn label(&self) -> &'static str {
        "Quiet mode available"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-must-quiet"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", result.stdout, result.stderr);
                if output.contains("--quiet") || output.contains("-q") {
                    AuditStatus::Pass
                } else {
                    AuditStatus::Warn("no --quiet/-q flag detected in --help output".into())
                }
            }
            _ => AuditStatus::Warn("could not run --help to detect quiet flag".into()),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P7,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::test_project_with_sh_script;
    use crate::types::AuditStatus;

    #[test]
    fn quiet_pass_when_flag_present() {
        let project = test_project_with_sh_script("echo '  --quiet  Suppress output'");
        let result = QuietAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn quiet_warn_when_flag_absent() {
        let project = test_project_with_sh_script("echo 'no quiet here'");
        let result = QuietAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
    }

    #[test]
    fn quiet_not_applicable_without_runner() {
        let mut project = test_project_with_sh_script("echo hi");
        project.runner = None;
        assert!(!QuietAudit.applicable(&project));
    }
}
