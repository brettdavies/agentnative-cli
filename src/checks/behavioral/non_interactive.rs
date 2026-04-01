use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct NonInteractiveCheck;

impl Check for NonInteractiveCheck {
    fn id(&self) -> &str {
        "p1-non-interactive"
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner.as_ref().unwrap();

        // Test P1: binary must not block waiting for interactive input.
        //
        // Ideally we'd run with zero args and stdin null. But that triggers the
        // binary's full default action, which causes infinite recursion when the
        // target is agentnative itself (or any tool whose default action is
        // expensive). The AGENTNATIVE_CHECK env var prevents recursion, but if
        // we're in a child process, we use --help as a safe proxy instead.
        //
        // --help is an imperfect proxy: `cat --help` exits 0 but bare `cat`
        // blocks on stdin. This is a known gap — we accept it because the
        // alternative (fork bombs) is worse. A future version could use ptrace
        // or /proc to detect stdin reads without full execution.
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
        let result = NonInteractiveCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn non_interactive_pass_with_false() {
        let project = test_project_with_runner("/bin/false");
        let result = NonInteractiveCheck.run(&project).unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }
}
