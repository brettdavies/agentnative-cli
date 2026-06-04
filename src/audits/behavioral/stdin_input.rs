//! Audit: `p6-should-stdin-input`.
//!
//! Commands that accept input data read from stdin when no file argument is
//! provided. SHOULD-tier, gated on the CLI advertising input-accepting
//! commands (subcommand verbs like `process`, `parse`, `convert`, `analyze`,
//! `validate`, `format`).
//!
//! Behavioral signal: `--help` advertises stdin support via mentions of
//! "stdin", "standard input", or `-` as a path placeholder.

use crate::audit::Audit;
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
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_stdin_input(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

pub(crate) fn audit_stdin_input(help: &HelpOutput) -> AuditStatus {
    let has_input_verb = help
        .subcommands()
        .iter()
        .any(|s| INPUT_VERBS.iter().any(|verb| s.eq_ignore_ascii_case(verb)));
    if !has_input_verb {
        return AuditStatus::Skip(
            "no input-accepting subcommand detected (process/parse/convert/transform/\
             analyze/validate/format/lint/audit); vacuous skip for the conditional \
             SHOULD."
                .into(),
        );
    }

    let raw_lower = help.raw().to_lowercase();
    let has_stdin = STDIN_INDICATORS.iter().any(|sig| raw_lower.contains(sig));

    if has_stdin {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
            "input-accepting subcommand present but `--help` does not mention \
             stdin or `-` as a path placeholder. SHOULD-tier — agents \
             piping data into the tool expect stdin to work when no file arg \
             is provided."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_input_subcommand() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Run server.\n  status   Show status.\n",
        );
        match audit_stdin_input(&help) {
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
        assert_eq!(audit_stdin_input(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_input_subcommand_without_stdin_signal() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  process    Process the file.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_stdin_input(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("stdin")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
