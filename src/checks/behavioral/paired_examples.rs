//! Check: `p3-should-paired-examples`.
//!
//! Examples should show human and agent invocations side by side — a text
//! example followed by an `--output json` equivalent within a few lines.
//! Agents learn the call shape twice over: once for interactive use, once
//! for programmatic consumption.
//!
//! Rubric: scan the top-level help body plus each subcommand's help body
//! for an `--output json` (or `--json`) token within 5 lines of an example
//! line (matching the same heuristics `p3-must-subcommand-examples` uses).
//! Pass when any body shows a pair; Warn otherwise.

use crate::check::Check;
#[cfg(test)]
use crate::checks::behavioral::subcommand_examples::has_example_line;
use crate::checks::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Window in lines between a text example and its JSON counterpart.
const PAIR_WINDOW_LINES: usize = 5;

pub struct PairedExamplesCheck;

impl Check for PairedExamplesCheck {
    fn id(&self) -> &str {
        "p3-paired-examples"
    }

    fn label(&self) -> &'static str {
        "Help text pairs human and `--output json` example invocations"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P3
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-should-paired-examples"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(top_help) => {
                let binary_name = project
                    .binary_paths
                    .first()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str());
                let runner = project.runner_ref();
                let subhelp = probe_subcommands(runner, top_help);
                check_paired_examples(binary_name, top_help, &subhelp)
            }
        };
        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

pub(crate) fn check_paired_examples(
    binary_name: Option<&str>,
    top_help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> CheckStatus {
    if has_paired_example(top_help.raw(), binary_name) {
        return CheckStatus::Pass;
    }
    for (_, help) in subhelp {
        if has_paired_example(help.raw(), binary_name) {
            return CheckStatus::Pass;
        }
    }
    CheckStatus::Warn(format!(
        "no paired text + `--output json` example found within {PAIR_WINDOW_LINES} \
         lines in top-level or any subcommand `--help`. Pairing keeps agents \
         from reverse-engineering the JSON invocation from the text one."
    ))
}

/// True iff `raw` shows an example line followed by an `--output json` (or
/// `--json`) reference within [`PAIR_WINDOW_LINES`] lines.
fn has_paired_example(raw: &str, binary_name: Option<&str>) -> bool {
    let lines: Vec<&str> = raw.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line_is_example(line, binary_name) {
            continue;
        }
        let end = (i + 1 + PAIR_WINDOW_LINES).min(lines.len());
        for follow in &lines[i + 1..end] {
            if mentions_json_output(follow) {
                return true;
            }
        }
    }
    false
}

fn line_is_example(line: &str, binary_name: Option<&str>) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("$ ") {
        return true;
    }
    if let Some(name) = binary_name
        && !name.is_empty()
        && trimmed.starts_with(name)
        && trimmed.len() > name.len()
        && trimmed[name.len()..].starts_with(' ')
    {
        return true;
    }
    false
}

fn mentions_json_output(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("--output json")
        || lower.contains("--output=json")
        || lower.contains("--json")
        || lower.contains("-o json")
}

/// Fallback hook so `p3-must-subcommand-examples`'s rubric can compose with
/// ours when we eventually extract a single example-line predicate. Kept as
/// a thin wrapper for now to make future consolidation straightforward.
#[cfg(test)]
fn line_is_example_for_test(line: &str, binary_name: Option<&str>) -> bool {
    // Sanity tie-back: anything our line-level predicate sees as an example
    // must also satisfy the body-level predicate in `subcommand_examples`.
    let saw = line_is_example(line, binary_name);
    if saw {
        // The body-level scanner inspects the whole text; a single-line input
        // is the simplest case where the predicates must agree.
        assert!(has_example_line(line, binary_name));
    }
    saw
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    #[test]
    fn pass_when_top_help_has_pair() {
        let top = hp("\
Examples:
  $ tool list
  $ tool list --output json
");
        let subhelp: Vec<(String, HelpOutput)> = Vec::new();
        assert_eq!(
            check_paired_examples(Some("tool"), &top, &subhelp),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_when_subcommand_has_pair() {
        let top = hp("Usage: tool [COMMAND]\n");
        let subhelp = vec![(
            "list".to_string(),
            hp("\
Examples:
  tool list
  tool list --json
"),
        )];
        assert_eq!(
            check_paired_examples(Some("tool"), &top, &subhelp),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_when_pair_uses_output_equals() {
        let top = hp("\
Examples:
  $ tool query 'name'
  $ tool query 'name' --output=json
");
        assert_eq!(
            check_paired_examples(Some("tool"), &top, &[]),
            CheckStatus::Pass
        );
    }

    #[test]
    fn warn_when_only_text_example() {
        let top = hp("\
Examples:
  $ tool list
");
        match check_paired_examples(Some("tool"), &top, &[]) {
            CheckStatus::Warn(msg) => assert!(msg.contains("paired")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_pair_outside_window() {
        let mut text = String::from("$ tool list\n");
        for _ in 0..(PAIR_WINDOW_LINES + 2) {
            text.push_str("filler line\n");
        }
        text.push_str("$ tool list --output json\n");
        let top = hp(&text);
        match check_paired_examples(Some("tool"), &top, &[]) {
            CheckStatus::Warn(_) => {}
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn line_predicate_agrees_with_body_predicate() {
        assert!(line_is_example_for_test("$ tool list", Some("tool")));
        assert!(line_is_example_for_test("  tool check .", Some("tool")));
        assert!(!line_is_example_for_test("plain text", Some("tool")));
    }
}
