//! Audit: `p2-must-exit-codes`.
//!
//! Structured exit codes (0/1/2/77/78) are the agent-facing dialect for
//! distinguishing success, generic error, usage error, auth failure, and
//! configuration error. We can only observe a slice of the convention
//! behaviorally — code 2 (usage error) is the one a bad-flag probe surfaces.
//!
//! Pass on exit code 2 (the canonical usage-error code). Warn on any other
//! non-zero code (1 is common in Cobra-based CLIs; the structured convention
//! is still violated but the binary at least refuses). Fail on exit 0 (the
//! invocation was silently accepted, no exit-code structure to speak of) or
//! crash.

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::probe_bad_invocation;
use crate::project::Project;
use crate::runner::{RunResult, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct StructuredExitCodesAudit;

impl Audit for StructuredExitCodesAudit {
    fn id(&self) -> &str {
        "p2-structured-exit-codes"
    }

    fn label(&self) -> &'static str {
        "Bad invocation exits with structured usage-error code (2)"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-exit-codes"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let result = probe_bad_invocation(project.runner_ref());
        let status = classify_exit(&result);
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

pub(crate) fn classify_exit(result: &RunResult) -> AuditStatus {
    match (&result.status, result.exit_code) {
        (RunStatus::Ok, Some(2)) => AuditStatus::Pass,
        (RunStatus::Ok, Some(0)) => AuditStatus::Fail(
            "binary silently accepted invalid flag (exit 0). \
             Bad invocation must signal usage error via a non-zero exit code."
                .into(),
        ),
        (RunStatus::Ok, Some(code)) if code > 0 => AuditStatus::Warn(format!(
            "bad invocation exited with code {code}. The 0/1/2/77/78 \
             convention reserves code 2 for usage errors; using a different \
             non-zero code (often 1) blurs the distinction between usage \
             errors and general failure."
        )),
        (RunStatus::Crash { signal }, _) => AuditStatus::Fail(format!(
            "binary crashed on bad invocation (signal {signal}). \
             Argument parsers must reject unknown flags without crashing."
        )),
        (status, _) => AuditStatus::Fail(format!(
            "unexpected run status on bad invocation: {status:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::test_project_with_sh_script;

    #[test]
    fn pass_on_exit_code_2() {
        let project = test_project_with_sh_script("exit 2");
        let result = StructuredExitCodesAudit
            .run(&project)
            .expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn warn_on_exit_code_1() {
        let project = test_project_with_sh_script("exit 1");
        let result = StructuredExitCodesAudit
            .run(&project)
            .expect("audit should run");
        match result.status {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("code 1"));
                assert!(msg.contains("2"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn fail_on_exit_code_0() {
        let project = test_project_with_sh_script("exit 0");
        let result = StructuredExitCodesAudit
            .run(&project)
            .expect("audit should run");
        match result.status {
            AuditStatus::Fail(msg) => assert!(msg.contains("silently accepted")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_on_crash() {
        // Some kernels surface the segfault as `RunStatus::Crash`, others
        // as exit code 139 (128 + 11). Both shapes legitimately route to
        // Fail; the test pins on the verdict rather than the message text
        // so it doesn't flap on platform variance.
        let project = test_project_with_sh_script("kill -11 $$");
        let result = StructuredExitCodesAudit
            .run(&project)
            .expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }
}
