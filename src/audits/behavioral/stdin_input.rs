//! Audit: `p6-should-stdin-input`.
//!
//! Commands that accept input data read from stdin when no file argument is
//! provided. SHOULD-tier, gated on the CLI advertising input-accepting
//! commands (subcommand verbs like `process`, `parse`, `convert`, `analyze`,
//! `validate`, `format`).
//!
//! Behavioral signal: `--help` advertises stdin support via mentions of
//! "stdin", "standard input", or `-` as a path placeholder. The audit reads
//! each input-accepting subcommand's own `--help`, where a CLI documents its
//! input (`biome lint --stdin-file-path`), and the top-level help, whose
//! mention counts for every subcommand. P6 asks it of every command that
//! accepts input, so Pass needs all of them covered.

use crate::audit::Audit;
use crate::audits::behavioral::subcommand_help::{coverage, probe_named};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Verbs that suggest a subcommand consumes input data.
const INPUT_VERBS: &[&str] = &[
    "process",
    "parse",
    "convert",
    "transform",
    "analyze",
    "validate",
    "format",
    "lint",
    "audit",
];

/// Signals in `--help` that indicate stdin support.
const STDIN_INDICATORS: &[&str] = &[
    "stdin",
    "standard input",
    "read from standard",
    "read from `-`",
    "`-` for stdin",
    "use `-` to read",
];

pub struct StdinInputAudit;

impl Audit for StdinInputAudit {
    fn id(&self) -> &str {
        "p6-stdin-input"
    }

    fn label(&self) -> &'static str {
        "Input-accepting commands read from stdin when no file is given"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-should-stdin-input"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let (status, pass_evidence) = match project.help_output() {
            None => (AuditStatus::Skip("could not probe --help".into()), None),
            Some(help) => {
                let subhelp = probe_named(project.runner_ref(), &input_subcommands(help));
                audit_stdin_input(help, &subhelp)
            }
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
            config_hint: None,
            pass_evidence,
        })
    }
}

fn input_subcommands(help: &HelpOutput) -> Vec<&str> {
    help.subcommands()
        .iter()
        .map(String::as_str)
        .filter(|s| INPUT_VERBS.iter().any(|verb| s.eq_ignore_ascii_case(verb)))
        .collect()
}

/// The first stdin indicator `help` mentions.
fn find_stdin_mention(help: &HelpOutput) -> Option<String> {
    let raw_lower = help.raw().to_lowercase();
    STDIN_INDICATORS
        .iter()
        .find(|sig| raw_lower.contains(*sig))
        .map(|sig| (*sig).to_string())
}

/// The status, plus the Pass evidence naming each input-accepting
/// subcommand and where its help mentions stdin.
pub(crate) fn audit_stdin_input(
    help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> (AuditStatus, Option<String>) {
    let triggers = input_subcommands(help);
    if triggers.is_empty() {
        return (
            AuditStatus::Skip(
                "no input-accepting subcommand detected (process/parse/convert/transform/\
                 analyze/validate/format/lint/audit); vacuous skip for the conditional \
                 SHOULD."
                    .into(),
            ),
            None,
        );
    }

    let cov = coverage(&triggers, help, subhelp, find_stdin_mention);
    if cov.without.is_empty() {
        return (
            AuditStatus::Pass,
            Some(format!(
                "every input-accepting subcommand's --help mentions stdin: {}.",
                cov.with_list()
            )),
        );
    }
    let covered = if cov.with.is_empty() {
        String::new()
    } else {
        format!(" Mentions it: {}.", cov.with_list())
    };
    (
        AuditStatus::Warn(format!(
            "input-accepting subcommand(s) whose --help does not mention stdin or `-` \
             as a path placeholder: {}.{covered} SHOULD-tier: agents piping data into \
             the tool expect stdin to work when no file arg is provided.",
            cov.without_list(),
        )),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Excerpts of biome 2.5's `biome --help` and `biome lint --help`.
    const BIOME_HELP: &str = "\
Biome's command-line interface for checking, formatting, and linting files, managing configuration,
and interacting with the daemon.

Usage: biome COMMAND ...

Available options:
    -h, --help     Prints help information
    -V, --version  Prints version information

Available commands:
    version        Prints the Biome CLI version and information about the connected daemon server,
                   if any, then exits.
    check          Checks the specified files for formatting, linting, and assist actions.
    lint           Runs the linter on the specified files.
    clean          Removes the Biome daemon server log files.
";

    const BIOME_LINT_HELP: &str = "\
Runs the linter on the specified files.

Usage: biome lint [--write] [--unsafe] [--suppress] [--staged] [--changed] [PATH]...

        --stdin-file-path=PATH  Reads code from standard input and writes the processed code to
                              standard output.
        --staged              Lints only staged files. This option is intended for local use.
";

    // Excerpts of vhs 0.12's `vhs --help` and `vhs validate --help`.
    const VHS_HELP: &str = "\
Run a given tape file and generates its outputs.

Usage:
  vhs <file> [flags]
  vhs [command]

Available Commands:
  help        Help about any command
  new         Create a new tape file with example tape file contents and documentation
  serve       Start the VHS SSH server
  validate    Validate a glob file path and parses all the files to ensure they are valid without running them.

Flags:
  -h, --help             help for vhs
  -o, --output strings   file name(s) of video output
";

    const VHS_VALIDATE_HELP: &str = "\
Validate a glob file path and parses all the files to ensure they are valid without running them.

Usage:
  vhs validate <file>... [flags]

Flags:
  -h, --help   help for validate
";

    fn sub(name: &str, raw: &str) -> (String, HelpOutput) {
        (name.to_string(), HelpOutput::from_raw(raw))
    }

    #[test]
    fn skip_when_no_input_subcommand() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Run server.\n  status   Show status.\n",
        );
        match audit_stdin_input(&help, &[]).0 {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_input_subcommand_and_stdin_mentioned() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  parse    Parse input.\n\n\
             Reads from stdin when no file is given.\n",
        );
        assert_eq!(audit_stdin_input(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_input_subcommand_without_stdin_signal() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  process    Process the file.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_stdin_input(&help, &[]).0 {
            AuditStatus::Warn(msg) => assert!(msg.contains("stdin")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn stdin_documented_on_the_input_subcommand_passes() {
        let help = HelpOutput::from_raw(BIOME_HELP);
        let subhelp = vec![sub("lint", BIOME_LINT_HELP)];
        let (status, evidence) = audit_stdin_input(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some("every input-accepting subcommand's --help mentions stdin: lint (stdin).")
        );
    }

    #[test]
    fn warn_names_the_input_subcommand_without_stdin() {
        let help = HelpOutput::from_raw(VHS_HELP);
        let subhelp = vec![sub("validate", VHS_VALIDATE_HELP)];
        match audit_stdin_input(&help, &subhelp) {
            (AuditStatus::Warn(msg), None) => {
                assert!(msg.contains("as a path placeholder: validate."), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
