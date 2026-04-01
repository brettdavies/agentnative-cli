use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct NonInteractiveCheck;

impl Check for NonInteractiveCheck {
    fn id(&self) -> &str {
        "p1-non-interactive"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();

        // Test P1: binary must not block waiting for interactive input.
        //
        // BinaryRunner sets stdin to /dev/null, so binaries that read stdin
        // (like cat) get EOF immediately and exit — no blocking. The real
        // danger is recursive self-invocation: if the target binary is
        // agentnative itself, zero-args triggers `check .` which spawns
        // more children. AGENTNATIVE_CHECK (set by BinaryRunner on all
        // children, like make's MAKELEVEL) detects this. In child processes
        // we fall back to --help as a proxy.
        let is_child = std::env::var("AGENTNATIVE_CHECK").is_ok();
        let result = if is_child {
            runner.run(&["--help"], &[])
        } else {
            runner.run(&[], &[])
        };

        let status = match result.status {
            RunStatus::Timeout => {
                CheckStatus::Warn("binary may be waiting for interactive input".into())
            }
            RunStatus::Ok => CheckStatus::Pass,
            RunStatus::Crash { signal } => CheckStatus::Warn(format!(
                "binary crashed on bare invocation (signal {signal})"
            )),
            _ => CheckStatus::Pass,
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Non-interactive by default".into(),
            group: CheckGroup::P1,
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
    fn non_interactive_pass_with_echo() {
        let project = test_project_with_runner("/bin/echo");
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn non_interactive_pass_with_false() {
        let project = test_project_with_runner("/bin/false");
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }
}
