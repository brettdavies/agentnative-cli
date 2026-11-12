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

const SHORT_ALIAS_ROW: &str = "p3-should-version-short";

pub struct VersionAudit;

impl Audit for VersionAudit {
    fn id(&self) -> &str {
        "p3-version"
    }

    fn label(&self) -> &'static str {
        "Version flags (`--version`, short alias)"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-version", SHORT_ALIAS_ROW]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();

        // A `--version` that does not return leaves the alias row unmeasured,
        // because probing the help and the aliases after it only spends more
        // runner timeouts on a CLI that hangs.
        let (status, short_alias) = match probe_version_flag(runner, "--version") {
            FlagProbe::Answered => (
                AuditStatus::Pass,
                audit_short_alias(runner, project.help_output()),
            ),
            FlagProbe::Rejected(reason) => (
                AuditStatus::Fail(reason),
                audit_short_alias(runner, project.help_output()),
            ),
            FlagProbe::NoReturn(reason) => {
                let skip = AuditStatus::Skip(format!("{reason}, so no short alias was run."));
                (AuditStatus::Fail(reason), skip)
            }
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
            row_statuses: vec![(SHORT_ALIAS_ROW, short_alias)],
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

/// The verdict on the short alias row: probe each declared short alias, and
/// run none the help does not declare.
fn audit_short_alias(runner: &BinaryRunner, help: Option<&HelpOutput>) -> AuditStatus {
    let Some(help) = help else {
        return AuditStatus::Warn(
            "`--help` could not be probed, so no short alias was run.".into(),
        );
    };
    let declared = declared_short_aliases(help);
    if declared.is_empty() {
        return AuditStatus::Warn(flag_meaning::noting(
            help,
            SHORT_VERSION_FLAGS,
            &VERSION,
            &format!(
                "`--help` declares no short alias for `--version` ({}), so none was run. \
                 Declaring one shortens version probes for agents.",
                SHORT_VERSION_FLAGS.join(", "),
            ),
        ));
    }
    if declared
        .iter()
        .any(|alias| matches!(probe_version_flag(runner, alias), FlagProbe::Answered))
    {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "no declared short alias responded (tried {}). A working one shortens version \
             probes for agents.",
            declared.join(", "),
        ))
    }
}

/// What one version flag did when run. Each reason is evidence text.
enum FlagProbe {
    /// Exited 0 with non-empty stdout.
    Answered,
    /// Returned, with a non-zero exit or no output.
    Rejected(String),
    /// Timed out, crashed, or could not start.
    NoReturn(String),
}

fn probe_version_flag(runner: &BinaryRunner, flag: &str) -> FlagProbe {
    let result = runner.run(&[flag], &[]);
    match result.status {
        RunStatus::Ok if result.exit_code == Some(0) && !result.stdout.trim().is_empty() => {
            FlagProbe::Answered
        }
        RunStatus::Ok if result.exit_code == Some(0) => {
            FlagProbe::Rejected(format!("`{flag}` produced no output"))
        }
        RunStatus::Ok => FlagProbe::Rejected(format!(
            "`{flag}` exited with code {}",
            result.exit_code.unwrap_or(-1)
        )),
        _ => FlagProbe::NoReturn(format!("`{flag}` failed: {:?}", result.status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::scorecard::build_row_results;
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

    /// `p3-must-version` and `p3-should-version-short`, as the scorecard
    /// reads them.
    fn rows(project: &Project) -> (AuditStatus, AuditStatus) {
        let raw = VersionAudit.run(project).expect("audit should run");
        let catalog: Vec<Box<dyn Audit>> = vec![Box::new(VersionAudit)];
        let rows = build_row_results(&[raw], &catalog);
        let status = |id: &str| {
            rows.iter()
                .find(|(row, _)| row.id == id)
                .unwrap_or_else(|| panic!("no row {id}"))
                .0
                .status
                .clone()
        };
        (status("p3-must-version"), status("p3-should-version-short"))
    }

    fn warn_message(status: AuditStatus) -> String {
        match status {
            AuditStatus::Warn(msg) => msg,
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn version_fail_non_zero_exit() {
        // `/usr/bin/false` exits 1 regardless of args, so `--version` fails the
        // MUST, and its empty help declares no alias for the SHOULD.
        let (must, should) = rows(&test_project_with_runner("/usr/bin/false"));
        assert!(matches!(must, AuditStatus::Fail(_)), "{must:?}");
        assert!(matches!(should, AuditStatus::Warn(_)), "{should:?}");
    }

    #[test]
    fn version_handles_crash() {
        let (must, should) = rows(&test_project_with_sh_script("kill -11 $$"));
        assert!(matches!(must, AuditStatus::Fail(_)), "{must:?}");
        assert!(matches!(should, AuditStatus::Skip(_)), "{should:?}");
    }

    /// A `--version` that does not return leaves the SHOULD unmeasured, and
    /// neither the help nor an alias is probed: against a CLI that hangs on
    /// version flags, each extra probe costs a full runner timeout.
    #[test]
    fn no_alias_is_run_when_the_long_flag_does_not_return() {
        let project = test_project_with_sh_script(
            "echo \"$1\" >> \"$(dirname \"$0\")/argv.log\"\n\
             case \"$1\" in\n\
             --version) kill -11 $$ ;;\n\
             --help) echo 'Options:'; echo '  -V, --version  Print version' ;;\n\
             -V) echo 1.0.0 ;;\n\
             esac",
        );
        let (must, should) = rows(&project);
        assert!(matches!(must, AuditStatus::Fail(_)), "{must:?}");
        match should {
            AuditStatus::Skip(msg) => assert!(msg.contains("no short alias was run"), "{msg}"),
            other => panic!("expected Skip, got {other:?}"),
        }
        assert_eq!(probed(&project), vec!["--version"]);
    }

    /// `--version` returning non-zero costs no timeout, so the alias still
    /// gets its own verdict. Old Unix tools reject `--version` and answer a
    /// declared `-V`.
    #[test]
    fn a_rejected_long_flag_leaves_the_should_row_to_the_alias() {
        let project = logging_project("Options:\n  -V, --version  Print version", "-V");
        let (must, should) = rows(&project);
        assert!(matches!(must, AuditStatus::Fail(_)), "{must:?}");
        assert_eq!(should, AuditStatus::Pass);
        assert_eq!(probed(&project), vec!["--version", "--help", "-V"]);
    }

    #[test]
    fn a_rejected_long_flag_with_no_alias_warns_the_should_row() {
        let project = logging_project("Options:\n  -h, --help  Print help", "-V");
        let (must, should) = rows(&project);
        assert!(matches!(must, AuditStatus::Fail(_)), "{must:?}");
        let msg = warn_message(should);
        assert!(msg.contains("declares no short alias"), "{msg}");
        assert!(!msg.contains("`--version` works"), "{msg}");
        assert_eq!(probed(&project), vec!["--version", "--help"]);
    }

    /// git, gh, and rclone print their version from `--version` and declare
    /// no short alias: the MUST is met, and only the SHOULD has anything to
    /// say.
    #[test]
    fn a_working_long_flag_passes_the_must_row_with_no_alias() {
        let project = logging_project("Options:\n      --version  Print version", "--version");
        let (must, should) = rows(&project);
        assert_eq!(must, AuditStatus::Pass);
        assert!(matches!(should, AuditStatus::Warn(_)), "{should:?}");
    }

    #[test]
    fn a_declared_alias_that_answers_passes() {
        let project = logging_project(
            "Options:\n  -V, --version  Print version\n  -h, --help     Print help",
            "--version|-V",
        );
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
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
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
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
        let (must, should) = rows(&project);
        assert_eq!(must, AuditStatus::Pass);
        let msg = warn_message(should);
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
        let (must, should) = rows(&project);
        assert_eq!(must, AuditStatus::Pass);
        let msg = warn_message(should);
        assert!(
            msg.contains("`--help` declares no short alias for `--version`"),
            "{msg}"
        );
        assert_eq!(probed(&project), vec!["--version", "--help"]);
    }

    #[test]
    fn a_declared_alias_that_fails_warns() {
        let project = logging_project("Options:\n  -V, --version  Print version", "--version");
        let (must, should) = rows(&project);
        assert_eq!(must, AuditStatus::Pass);
        let msg = warn_message(should);
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
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
        assert_eq!(probed(&project), vec!["--version", "--help", "-V"]);
    }

    /// `-V/--version` declares both names on one definition, so `-V` counts
    /// by the long name even where the description does not name the
    /// version.
    #[test]
    fn an_alias_joined_by_a_slash_counts() {
        let project = logging_project(
            "Options:\n  -h/--help     Print help\n  -V/--version  Print the release and exit",
            "--version|-V",
        );
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
        assert_eq!(probed(&project), vec!["--version", "--help", "-V"]);
    }

    #[test]
    fn an_alias_in_parentheses_counts() {
        let project = logging_project(
            "Options:\n  --help (-h)       Print help\n  --version (-V)    Print the version",
            "--version|-V",
        );
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
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
        assert_eq!(rows(&project), (AuditStatus::Pass, AuditStatus::Pass));
        assert_eq!(probed(&project), vec!["--version", "--help", "-version"]);
    }
}
