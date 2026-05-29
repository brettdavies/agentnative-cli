use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct BadArgsAudit;

impl Audit for BadArgsAudit {
    fn id(&self) -> &str {
        "p4-bad-args"
    }

    fn label(&self) -> &'static str {
        "Rejects invalid arguments"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-exit-code-mapping"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        let result = runner.run(&["--this-flag-does-not-exist-agentnative-probe"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                if result.exit_code.is_some_and(|c| c > 0) {
                    AuditStatus::Pass
                } else {
                    AuditStatus::Fail("binary silently accepted invalid flag (exit 0)".into())
                }
            }
            RunStatus::Crash { signal } => {
                AuditStatus::Fail(format!("binary crashed on bad args (signal {signal})"))
            }
            _ => AuditStatus::Fail(format!("unexpected status: {:?}", result.status)),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P4,
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
    fn bad_args_pass_when_rejected() {
        // sh -c 'exit 1' always exits non-zero
        let project = test_project_with_sh_script("exit 2");
        let result = BadArgsAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn bad_args_fail_when_accepted() {
        // echo silently accepts any args with exit 0
        let project = crate::audits::behavioral::tests::test_project_with_runner("/bin/echo");
        let result = BadArgsAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }

    #[test]
    fn bad_args_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = BadArgsAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }
}
