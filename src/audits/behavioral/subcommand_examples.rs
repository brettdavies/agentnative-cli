//! Audit: `p3-must-subcommand-examples`.
//!
//! Every subcommand ships at least one concrete invocation example. Clap's
//! `after_help` is the canonical placement, but tools that hand-write help
//! also satisfy the requirement as long as the example line is present.
//!
//! Detection rubric: a `--help` body contains an example when any line
//! matches one of:
//!
//! - Starts with `$ ` (shell-prompt style — most common).
//! - Starts with the binary name (clap's `Examples:` block often renders
//!   `tool subcommand ...`).
//! - Lives inside a fenced code block (```` ``` ````).
//! - Contains the literal `Examples:` / `EXAMPLES` section header.
//!
//! Fail when any non-skipped subcommand misses an example. Vacuous Skip
//! when the binary has no subcommands.

use crate::audit::Audit;
use crate::audits::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct SubcommandExamplesAudit;

impl Audit for SubcommandExamplesAudit {
    fn id(&self) -> &str {
        "p3-subcommand-examples"
    }

    fn label(&self) -> &'static str {
        "Each subcommand's `--help` ships at least one invocation example"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-subcommand-examples"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(top_help) if top_help.subcommands().is_empty() => AuditStatus::Skip(
                "binary has no subcommands; MUST applies conditionally to CLIs that use them."
                    .into(),
            ),
            Some(top_help) => {
                let runner = project.runner_ref();
                let subhelp = probe_subcommands(runner, top_help);
                let binary_name = project
                    .binary_paths
                    .first()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str());
                audit_subcommand_examples(binary_name, &subhelp)
            }
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

pub(crate) fn audit_subcommand_examples(
    binary_name: Option<&str>,
    subhelp: &[(String, HelpOutput)],
) -> AuditStatus {
    if subhelp.is_empty() {
        return AuditStatus::Skip(
            "no subcommands responded to `--help`; nothing to inspect.".into(),
        );
    }
    let missing: Vec<&str> = subhelp
        .iter()
        .filter(|(_, help)| !has_example_line(help.raw(), binary_name))
        .map(|(name, _)| name.as_str())
        .collect();
    if missing.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Fail(format!(
            "subcommands missing example invocations in their `--help`: {}. \
             Examples teach agents the call shape faster than option tables; \
             use clap's `after_help` or a dedicated `Examples:` block.",
            missing.join(", ")
        ))
    }
}

/// True iff `raw` contains an example line. Heuristic — matches the four
/// shapes documented in the module header.
pub(crate) fn has_example_line(raw: &str, binary_name: Option<&str>) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("examples:") || lower.contains("\nexamples\n") {
        return true;
    }
    if raw.contains("```") {
        return true;
    }
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("$ ") {
            return true;
        }
        if let Some(name) = binary_name
            && !name.is_empty()
            && trimmed.starts_with(name)
            && trimmed.len() > name.len()
        {
            // Followed by whitespace, not just the bare name on its own line.
            let next = &trimmed[name.len()..];
            if next.starts_with(' ') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    #[test]
    fn pass_with_dollar_prompt() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit\n\n  $ tool audit .\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_examples_header() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit\n\nExamples:\n  tool audit .\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_binary_prefix() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit\n\nDescription...\n  tool audit . --output json\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_fenced_block() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit\n\n```\ntool audit .\n```\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_when_subcommand_missing_example() {
        let subhelp = vec![
            (
                "audit".to_string(),
                hp("Usage: tool audit\n\n$ tool audit .\n"),
            ),
            (
                "generate".to_string(),
                hp("Usage: tool generate\n\nOptions: ...\n"),
            ),
        ];
        match audit_subcommand_examples(Some("tool"), &subhelp) {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("generate"));
                assert!(!msg.contains(" audit,"), "audit should pass: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_no_subhelp() {
        let subhelp: Vec<(String, HelpOutput)> = Vec::new();
        match audit_subcommand_examples(Some("tool"), &subhelp) {
            AuditStatus::Skip(msg) => assert!(msg.contains("no subcommands")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn no_false_positive_on_bare_name_line() {
        // A line that is just "tool" (no trailing args) should not count
        // as an example — there's no invocation shape to learn from it.
        assert!(!has_example_line("Usage: tool\n\n  tool\n", Some("tool")));
    }
}
