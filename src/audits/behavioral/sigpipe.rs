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
            RunStatus::Ok => match result.exit_code {
                // A shell reports a child that died of a signal as 128 plus
                // the signal number, so 141 is the tool's SIGPIPE death.
                #[cfg(unix)]
                Some(code) if code == 128 + libc::SIGPIPE => AuditStatus::Pass,
                // A crash reports itself on stderr. A tool that ends quietly on a
                // closed pipe, as Python's documented recipe does with exit 1, has
                // not crashed.
                Some(code)
                    if code != 0
                        && !result.stderr.is_empty()
                        && runner.run(&["--help"], &[]).exit_code == Some(0) =>
                {
                    AuditStatus::Fail(format!(
                        "`--help` exited {code} with output on stderr once its reader closed the \
                         pipe, where the full `--help` exits 0. Restore the default SIGPIPE \
                         action, or end without writing to stderr when a write finds the reader \
                         gone."
                    ))
                }
                _ => AuditStatus::Pass,
            },
            #[cfg(unix)]
            RunStatus::Crash { signal } if signal == libc::SIGPIPE => AuditStatus::Pass,
            RunStatus::Crash { signal } => AuditStatus::Fail(format!(
                "`--help` crashed (signal {signal}) during a partial read of its output"
            )),
            RunStatus::Timeout => AuditStatus::Warn(
                "`--help` was still running at the probe timeout, so how it ends once its \
                 reader closes the pipe could not be observed"
                    .into(),
            ),
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
            config_hint: None,
            row_statuses: Vec::new(),
            pass_evidence: None,
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
        assert!(
            matches!(&result.status, AuditStatus::Fail(evidence) if evidence.contains("signal 11")),
            "{:?}",
            result.status
        );
    }

    #[test]
    fn sigpipe_warns_a_help_that_outlasts_the_probe() {
        let mut project = crate::audits::behavioral::tests::test_project_with_sh_script(
            "echo usage; exec sleep 20",
        );
        project.runner = Some(
            crate::runner::BinaryRunner::new(
                project.binary_paths[0].clone(),
                std::time::Duration::from_secs(1),
            )
            .expect("create test runner"),
        );
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(&result.status, AuditStatus::Warn(evidence)
                if evidence == "`--help` was still running at the probe timeout, so how it ends once its reader closes the pipe could not be observed"),
            "{:?}",
            result.status
        );
    }

    /// A tool with the default SIGPIPE disposition dies of it once its reader
    /// closes the pipe, which is the graceful end P6 asks for.
    #[cfg(unix)]
    #[test]
    fn sigpipe_passes_a_tool_that_dies_of_sigpipe() {
        let project = crate::audits::behavioral::tests::test_project_with_sh_script("exec yes");
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::Pass),
            "{:?}",
            result.status
        );
    }

    /// The stand-in ignores SIGPIPE, as Rust does by default, and when a
    /// write fails it reports on stderr and exits 101, as a panicking
    /// `println!` does. Its help is longer than a pipe buffer, so the partial
    /// read's early close reaches it.
    #[cfg(unix)]
    #[test]
    fn sigpipe_fails_a_help_that_exits_non_zero_once_its_reader_closes_the_pipe() {
        let project = crate::audits::behavioral::tests::test_project_with_sh_script(
            "trap '' PIPE\n\
             i=0\n\
             while [ $i -lt 20000 ]; do\n\
             echo \"usage line $i of a long help text\" || { echo panicked >&2; exit 101; }\n\
             i=$((i + 1))\n\
             done",
        );
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(&result.status, AuditStatus::Fail(evidence)
                if evidence == "`--help` exited 101 with output on stderr once its reader closed the pipe, where the full `--help` exits 0. Restore the default SIGPIPE action, or end without writing to stderr when a write finds the reader gone."),
            "{:?}",
            result.status
        );
    }

    /// A launcher that runs the tool without `exec` reports the tool's
    /// SIGPIPE death as exit 141. `seq`'s output is longer than a pipe
    /// buffer and shorter than the full probe's capture limit.
    #[cfg(unix)]
    #[test]
    fn sigpipe_passes_a_launcher_reporting_its_tools_sigpipe_death() {
        let project =
            crate::audits::behavioral::tests::test_project_with_sh_script("seq 1 100000\nexit $?");
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::Pass),
            "{:?}",
            result.status
        );
    }

    #[test]
    fn sigpipe_passes_a_help_that_exits_non_zero_when_read_in_full_too() {
        let project =
            crate::audits::behavioral::tests::test_project_with_sh_script("echo usage; exit 2");
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::Pass),
            "{:?}",
            result.status
        );
    }

    /// The stand-in ends as Python's documented `BrokenPipeError` recipe
    /// does: once a write fails it exits 1 and writes nothing to stderr.
    #[cfg(unix)]
    #[test]
    fn sigpipe_passes_a_help_that_exits_quietly_once_its_reader_closes_the_pipe() {
        let project = crate::audits::behavioral::tests::test_project_with_sh_script(
            "trap '' PIPE\n\
             i=0\n\
             while [ $i -lt 20000 ]; do\n\
             echo \"usage line $i of a long help text\" 2>/dev/null || exit 1\n\
             i=$((i + 1))\n\
             done",
        );
        let result = SigpipeAudit.run(&project).expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::Pass),
            "{:?}",
            result.status
        );
    }
}
