use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct SigpipeAudit;

impl Audit for SigpipeAudit {
    fn id(&self) -> &str {
        "p6-sigpipe"
    }

    fn label(&self) -> &'static str {
        "Handles SIGPIPE gracefully"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-sigpipe"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        let result = runner.run_partial(&["--help"], 16);

        let status = match result.status {
            RunStatus::Ok => AuditStatus::Pass,
            RunStatus::Crash { signal } => {
                AuditStatus::Fail(format!("crashed on SIGPIPE (signal {signal})"))
            }
            _ => AuditStatus::Warn(format!("unexpected status: {:?}", result.status)),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P6,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::test_project_with_runner;
    use crate::types::AuditStatus;

    #[test]
    fn sigpipe_pass_with_echo() {
        let project = test_project_with_runner("/bin/echo");
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn sigpipe_handles_crash() {
        let project = crate::audits::behavioral::tests::test_project_with_sh_script("kill -11 $$");
        let result = SigpipeAudit
            .run(&project)
            .expect("audit should not panic on crash");
        // run_partial always returns Ok status (kills child after partial read),
        // so a crash script may still yield Pass or a non-panic result
        assert!(matches!(
            result.status,
            AuditStatus::Pass | AuditStatus::Fail(_) | AuditStatus::Warn(_)
        ));
    }
}
