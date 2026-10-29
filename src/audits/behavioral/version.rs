use super::flag_meaning::{self, Meaning};
use crate::audit::Audit;
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Short-form `--version` aliases, in the order they are probed.
///
/// - `-V`: clap default, curl, wget, gzip, less.
/// - `-v`: npm, node, bun, yarn, make.
/// - `-version`: Go's `flag` package single-dash long form, Java tools.
///
/// Any one responding satisfies the SHOULD requirement.
const SHORT_VERSION_FLAGS: &[&str] = &["-V", "-v", "-version"];

const VERSION: Meaning = Meaning {
    what: "the version",
    long_names: &["--version", "-version"],
    words: &["version"],
};

pub struct VersionAudit;

impl Audit for VersionAudit {
    fn id(&self) -> &str {
        "p3-version"
    }

    fn label(&self) -> &'static str {
        "Version flag works (`--version` plus short alias)"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-version", "p3-should-version-short"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();

        // A failed `--version` decides the verdict, so probing the help and
        // the aliases after it only spends more runner timeouts on a CLI
        // that hangs.
        let status = match probe_version_flag(runner, "--version") {
            Ok(()) => audit_short_alias(runner, project.help_output()),
            Err(reason) => AuditStatus::Fail(reason),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P3,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// The short version aliases (`-V`, `-v`, `-version`) that `help` declares
/// as version flags, in probe order.
///
/// A single letter counts only where its own definition means the version,
/// by also declaring `--version` or by saying version in its description:
/// `-v` beside `--verbose` is another flag, and a launcher run with a flag
/// its help does not declare can start the whole app it launches.
pub fn declared_short_aliases(help: &HelpOutput) -> Vec<&'static str> {
    SHORT_VERSION_FLAGS
        .iter()
        .copied()
        .filter(|alias| flag_meaning::find(help, &[alias], &VERSION).is_some())
        .collect()
}

/// The verdict once `--version` works: probe each declared short alias, and
/// run none the help does not declare.
fn audit_short_alias(runner: &BinaryRunner, help: Option<&HelpOutput>) -> AuditStatus {
    let Some(help) = help else {
        return AuditStatus::Warn(
            "`--version` works, but `--help` could not be probed, so no short alias was run."
                .into(),
        );
    };
    let declared = declared_short_aliases(help);
    if declared.is_empty() {
        return AuditStatus::Warn(flag_meaning::noting(
            help,
            SHORT_VERSION_FLAGS,
            &VERSION,
            &format!(
                "`--version` works, but `--help` declares no short alias for it ({}), so none \
                 was run. Declaring one shortens version probes for agents.",
                SHORT_VERSION_FLAGS.join(", "),
            ),
        ));
    }
    if declared
        .iter()
        .any(|alias| probe_version_flag(runner, alias).is_ok())
    {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "`--version` works but no declared short alias responded (tried {}). Adding one \
             shortens version probes for agents.",
            declared.join(", "),
        ))
    }
}

/// Probe a single version flag against the target binary. Returns `Ok(())`
/// when the binary exits 0 with non-empty stdout (the same heuristic the
/// original `--version`-only audit used). Returns `Err(reason)` otherwise,
/// with a human-readable failure cause suitable for evidence text.
fn probe_version_flag(runner: &BinaryRunner, flag: &str) -> Result<(), String> {
    let result = runner.run(&[flag], &[]);
    match result.status {
        RunStatus::Ok if result.exit_code == Some(0) && !result.stdout.trim().is_empty() => Ok(()),
        RunStatus::Ok if result.exit_code == Some(0) => Err(format!("`{flag}` produced no output")),
        RunStatus::Ok => Err(format!(
            "`{flag}` exited with code {}",
            result.exit_code.unwrap_or(-1)
        )),
        _ => Err(format!("`{flag}` failed: {:?}", result.status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::types::AuditStatus;

    /// A script project whose `--help` prints `help`, whose `answers` case
    /// pattern prints a version and exits 0, and which logs every first
    /// argument it is run with to `argv.log` beside it.
    fn logging_project(help: &str, answers: &str) -> Project {
        test_project_with_sh_script(&format!(
            "echo \"$1\" >> \"$(dirname \"$0\")/argv.log\"\n\
             case \"$1\" in\n\
             --help) cat <<'HELP'\n{help}\nHELP\n;;\n\
             {answers}) echo 1.0.0 ;;\n\
             *) exit 1 ;;\n\
             esac"
        ))
    }

    /// The first arguments the project's script was run with, in order.
    fn probed(project: &Project) -> Vec<String> {
        std::fs::read_to_string(project.path.join("argv.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn warn_message(status: AuditStatus) -> String {
        match status {
            AuditStatus::Warn(msg) => msg,
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn version_fail_non_zero_exit() {
        // `/bin/false` exits 1 regardless of args, so `--version` fails and
        // the audit returns Fail (the MUST is not satisfied).
        let project = test_project_with_runner("/bin/false");
        let result = VersionAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }

    #[test]
    fn version_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = VersionAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
    }

    /// A failed `--version` decides the verdict, so neither the help nor the
    /// short aliases are probed: against a CLI that hangs on version flags,
    /// each extra probe costs a full runner timeout.
    #[test]
    fn short_aliases_not_probed_after_long_flag_fails() {
        let project = logging_project("Options:\n  -V, --version  Print version", "-V");
        let result = VersionAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Fail(_)));
        assert_eq!(probed(&project), vec!["--version"]);
    }

    #[test]
    fn a_declared_alias_that_answers_passes() {
        let project = logging_project(
            "Options:\n  -V, --version  Print version\n  -h, --help     Print help",
            "--version|-V",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
        assert_eq!(probed(&project), vec!["--version", "--help", "-V"]);
    }

    /// Cursor's CLI declares `-v, --version`; run as `cursor -V`, the
    /// launcher starts the whole app instead.
    #[test]
    fn an_alias_the_help_does_not_declare_is_never_run() {
        let project = logging_project(
            "Options:\n  -v, --version  Print version\n  -h, --help     Print help",
            "--version|-v|-V",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
        let probed = probed(&project);
        assert!(
            !probed.iter().any(|arg| arg == "-V"),
            "ran an undeclared alias: {probed:?}"
        );
        assert!(probed.iter().any(|arg| arg == "-v"), "{probed:?}");
    }

    #[test]
    fn an_alias_declared_for_another_flag_is_not_run() {
        let project = logging_project(
            "Options:\n  -v, --verbose  More output\n      --version  Print version",
            "--version|-v|-V",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        let msg = warn_message(result.status);
        assert!(msg.contains("declares no short alias"), "{msg}");
        assert!(
            msg.contains(
                "`-v` is declared, but its definition (`-v, --verbose  More output`) does not \
                 mean the version."
            ),
            "{msg}"
        );
        assert_eq!(probed(&project), vec!["--version", "--help"]);
    }

    #[test]
    fn no_alias_is_run_when_the_help_declares_none() {
        let project = logging_project(
            "Options:\n      --version  Print version",
            "--version|-v|-V|-version",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        let msg = warn_message(result.status);
        assert!(
            msg.contains("`--help` declares no short alias for it"),
            "{msg}"
        );
        assert_eq!(probed(&project), vec!["--version", "--help"]);
    }

    #[test]
    fn a_declared_alias_that_fails_warns() {
        let project = logging_project("Options:\n  -V, --version  Print version", "--version");
        let result = VersionAudit.run(&project).expect("audit should run");
        let msg = warn_message(result.status);
        assert!(
            msg.contains("no declared short alias responded (tried -V)"),
            "{msg}"
        );
    }

    /// less declares `-V  ........  --version`, a layout the parser reads as
    /// `-V` with a description that names the version.
    #[test]
    fn an_alias_whose_description_names_the_version_counts() {
        let project = logging_project(
            "  -?  ........  --help\n                  Display help (from command line).\n  \
             -V  ........  --version\n                  Display the version number of \"less\".",
            "--version|-V",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
        assert_eq!(probed(&project), vec!["--version", "--help", "-V"]);
    }

    /// Go's `flag` package declares `-version`, the alias itself, as the
    /// version flag.
    #[test]
    fn a_single_dash_version_word_is_its_own_alias() {
        let project = logging_project(
            "Usage: depot [options]\n\n  -help      Show help.\n  -version   Show the current version.",
            "--version|-version",
        );
        let result = VersionAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
        assert_eq!(probed(&project), vec!["--version", "--help", "-version"]);
    }
}
