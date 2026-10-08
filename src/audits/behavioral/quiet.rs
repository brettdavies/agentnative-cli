use crate::audit::Audit;
use crate::audits::behavioral::flag_presence::pass_or_warn;
use crate::project::Project;
use crate::runner::{HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const QUIET_FLAGS: &[&str] = &["--quiet", "-q"];

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
        let ran = project.runner_ref().run(&["--help"], &[]).status;
        let status = match (ran, project.help_output()) {
            (RunStatus::Ok, Some(help)) => audit_quiet(help),
            _ => AuditStatus::Warn("could not run --help to detect quiet flag".into()),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P7,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

pub(crate) fn audit_quiet(help: &HelpOutput) -> AuditStatus {
    pass_or_warn(
        help,
        QUIET_FLAGS,
        "no option definition in --help declares --quiet or -q; usage lines are not read.",
    )
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
    fn a_help_that_crashes_is_not_searched() {
        // Prints a quiet flag, then dies on a signal.
        let project =
            test_project_with_sh_script("echo '  -q, --quiet  Suppress output'\nkill -11 $$");
        let result = QuietAudit.run(&project).expect("audit should run");
        assert_eq!(
            result.status,
            AuditStatus::Warn("could not run --help to detect quiet flag".into())
        );
    }

    /// One definition line per tool whose help carries `-q` inside another
    /// flag's name and declares no quiet flag.
    const NO_QUIET_FLAG: &[(&str, &str)] = &[
        (
            "eza 0.23.5",
            "      --no-quotes                  don't quote file names with spaces",
        ),
        (
            "helm 4.3.0",
            "      --qps float32                     queries per second used when communicating with the Kubernetes API, not including bursting",
        ),
        (
            "scc 4.1.0",
            "      --file-list-queue-size int            the size of the queue of files found and ready to be read into memory (default 16)",
        ),
        (
            "yq 4.54.1",
            "      --ini-preserve-quotes               preserve surrounding quotes on INI values during round-trip",
        ),
    ];

    const SEARCHED: &str =
        "no option definition in --help declares --quiet or -q; usage lines are not read.";

    #[test]
    fn a_flag_whose_name_contains_q_is_not_a_quiet_flag() {
        let passed: Vec<&str> = NO_QUIET_FLAG
            .iter()
            .filter(|(_, line)| {
                let help =
                    HelpOutput::from_raw(format!("Options:\n{line}\n  -h, --help    Show help.\n"));
                audit_quiet(&help) == AuditStatus::Pass
            })
            .map(|(tool, _)| *tool)
            .collect();
        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn a_declared_quiet_flag_passes_by_either_name() {
        for line in [
            "  -q, --quiet    Say less.",
            "      --quiet    Say less.",
            "  -q             Say less.",
        ] {
            let help =
                HelpOutput::from_raw(format!("Options:\n{line}\n  -h, --help    Show help.\n"));
            assert_eq!(audit_quiet(&help), AuditStatus::Pass, "{line}");
        }
    }

    #[test]
    fn a_quiet_flag_shown_only_in_a_usage_line_warns_and_names_what_was_searched() {
        let help = HelpOutput::from_raw(
            "Usage: quill [-q] [-o FILE] <input>\n\nOptions:\n  -o, --out FILE    Write output here.\n  -h, --help        Show help.\n",
        );
        assert_eq!(audit_quiet(&help), AuditStatus::Warn(SEARCHED.into()));
    }

    #[test]
    fn a_single_dash_quiet_beside_double_dash_names_warns_and_says_why() {
        let help = HelpOutput::from_raw(
            "Options:\n  -quiet        Say less.\n      --help    Show help.\n",
        );
        match audit_quiet(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.starts_with(SEARCHED), "{msg}");
                assert!(msg.ends_with("so it does not count as `--quiet`."), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_single_dash_quiet_passes_in_a_help_without_double_dash_names() {
        let help = HelpOutput::from_raw(
            "Usage of tool:\n  -quiet\n    \tSay less\n  -version\n    \tShow version\n",
        );
        assert_eq!(audit_quiet(&help), AuditStatus::Pass);
    }

    #[test]
    fn quiet_not_applicable_without_runner() {
        let mut project = test_project_with_sh_script("echo hi");
        project.runner = None;
        assert!(!QuietAudit.applicable(&project));
    }
}
