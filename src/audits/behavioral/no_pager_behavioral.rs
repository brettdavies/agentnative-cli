//! Audit: behavioral confirmation that a pager-using tool ships `--no-pager`.
//!
//! Covers: `p6-must-no-pager`. Source-verified coverage already exists via
//! `p6-no-pager`; this audit adds the behavioral layer by inspecting the
//! shipped `--help` surface. Heuristic (Medium confidence): pager inference
//! from the text is soft.
//!
//! Pass when `--no-pager` is in the advertised flag list. Skip when the
//! help text shows no pager signal at all: nothing mentions `$PAGER`,
//! `--pager`, or `pager`, and nothing names `less` or `more` as a program.
//! Warn when the text mentions pager plumbing but the escape hatch is absent.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Words that name a pager wherever they appear.
const PAGER_WORDS: &[&str] = &["$PAGER", "--pager", "pager", "PAGER"];

/// Pager programs whose names are also English words, so each counts only
/// where [`names_program`] finds the help presenting it as a program.
const PAGER_PROGRAMS: &[&str] = &["less", "more"];

/// `piped through less`: one of these verbs, then one of
/// [`PIPE_PREPOSITIONS`], then the program.
const PIPE_VERBS: &[&str] = &["pipe", "pipes", "piped", "piping"];
const PIPE_PREPOSITIONS: &[&str] = &["through", "to", "into"];

const QUOTES: &[char] = &['`', '\'', '"'];

pub struct NoPagerBehavioralAudit;

impl Audit for NoPagerBehavioralAudit {
    fn id(&self) -> &str {
        "p6-no-pager-behavioral"
    }

    fn label(&self) -> &'static str {
        "Pager-using CLI ships --no-pager escape hatch"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-pager"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_no_pager(help),
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
            pass_evidence: None,
        })
    }
}

/// Core unit for tests. Takes a prepared `HelpOutput` and returns the
/// `AuditStatus` that summarizes it.
fn audit_no_pager(help: &HelpOutput) -> AuditStatus {
    let has_no_pager_flag = help.find_flag(&["--no-pager"]).is_some();
    if has_no_pager_flag {
        return AuditStatus::Pass;
    }
    if !mentions_pager(help.raw()) {
        AuditStatus::Skip(
            "no pager signal ($PAGER, --pager, pager, or less/more named as a program) in --help"
                .into(),
        )
    } else {
        AuditStatus::Warn(help.noting_dash_rule(
            &["--no-pager"],
            "pager referenced in --help but no --no-pager escape hatch advertised",
        ))
    }
}

fn mentions_pager(raw: &str) -> bool {
    PAGER_WORDS.iter().any(|word| raw.contains(word))
        || PAGER_PROGRAMS
            .iter()
            .any(|program| names_program(raw, program))
}

/// Whether `raw` presents `program` as a program: the whole word as a man
/// page reference (`less(1)`), opening a quoted command (`` `less` ``,
/// `'less -R'`), or after a pipe verb (`piped through less`). A bare pipe
/// before it does not count: ripgrep's `rg -p foo | less -R` is the user
/// paging the tool's output, which says the tool starts no pager itself.
fn names_program(raw: &str, program: &str) -> bool {
    raw.match_indices(program).any(|(start, _)| {
        let before = &raw[..start];
        let after = &raw[start + program.len()..];
        is_word_edge(before.chars().next_back())
            && is_word_edge(after.chars().next())
            && (after.starts_with("(1)")
                || opens_quoted_command(before, after)
                || follows_pipe_verb(before))
    })
}

fn is_word_edge(neighbor: Option<char>) -> bool {
    !neighbor.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn opens_quoted_command(before: &str, after: &str) -> bool {
    before
        .chars()
        .next_back()
        .filter(|c| QUOTES.contains(c))
        .is_some_and(|quote| after.starts_with(quote) || after.starts_with(" -"))
}

fn follows_pipe_verb(before: &str) -> bool {
    if !before.ends_with(char::is_whitespace) {
        return false;
    }
    let mut words = before.split_whitespace().rev();
    let (Some(preposition), Some(verb)) = (words.next(), words.next()) else {
        return false;
    };
    PIPE_PREPOSITIONS.contains(&preposition.to_ascii_lowercase().as_str())
        && PIPE_VERBS.contains(&verb.to_ascii_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_WITH_NO_PAGER: &str = r#"Usage: tool [OPTIONS]

Options:
  --no-pager    Disable paged output.
  --pager       Use custom pager.
  -h, --help    Show help.
"#;

    const HELP_PAGER_WITHOUT_ESCAPE: &str = r#"Usage: tool [OPTIONS]

Long output is piped through less by default. Set $PAGER to override.

Options:
  -h, --help    Show help.
"#;

    const HELP_NO_PAGER_MENTION: &str = r#"Usage: tool [OPTIONS]

Options:
  -q, --quiet   Suppress output.
  -h, --help    Show help.
"#;

    const HELP_NON_ENGLISH: &str = r#"用法: outil [选项]

选项:
  -h, --help    显示帮助
"#;

    #[test]
    fn happy_path_no_pager_flag_present() {
        let help = HelpOutput::from_raw(HELP_WITH_NO_PAGER);
        assert_eq!(audit_no_pager(&help), AuditStatus::Pass);
    }

    #[test]
    fn skip_when_no_pager_mention_at_all() {
        let help = HelpOutput::from_raw(HELP_NO_PAGER_MENTION);
        let status = audit_no_pager(&help);
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn warn_when_pager_mentioned_without_escape() {
        let help = HelpOutput::from_raw(HELP_PAGER_WITHOUT_ESCAPE);
        let status = audit_no_pager(&help);
        match status {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("--no-pager"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    fn is_skip(help: &str) -> bool {
        matches!(
            audit_no_pager(&HelpOutput::from_raw(help)),
            AuditStatus::Skip(_)
        )
    }

    /// `less` and `more` as English words, inside longer words, or piped to
    /// by the user, from real help text: fd, gh, terraform, ripgrep.
    const ORDINARY_WORDS: &[(&str, &str)] = &[
        (
            "fd: unless, regardless",
            "      --no-ignore-vcs\n          rules described by --no-ignore are still ignored unless otherwise specified.\n          such that fd will respect all git related ignore rules regardless of whether you're\n",
        ),
        (
            "fd: less than",
            "  -S, --size <size>\n             '-': file size must be less than or equal to this\n",
        ),
        (
            "gh: seamlessly, for more information",
            "Work seamlessly with GitHub from the command line.\n\nLEARN MORE\n  Use `gh <command> <subcommand> --help` for more information about a command.\n",
        ),
        (
            "terraform: less common, more advanced",
            "The available commands for execution are listed below.\nThe primary workflow commands are given first, followed by\nless common or more advanced commands.\n",
        ),
        (
            "ripgrep: the user piping output into less",
            "    -p, --pretty\n        This flag is useful when you still want pretty output even if you're\n        piping ripgrep to another program or file. For example: rg -p foo |\n        less -R.\n",
        ),
        (
            "longer words",
            "Lossless output, useless otherwise. Furthermore, moreover, evermore.\n",
        ),
        (
            "a flag named --less",
            "Options:\n      --less    Print less detail\n      --more    Print more detail\n",
        ),
    ];

    #[test]
    fn less_and_more_as_ordinary_words_are_no_pager_signal() {
        let warned: Vec<&str> = ORDINARY_WORDS
            .iter()
            .filter(|(_, help)| !is_skip(help))
            .map(|(what, _)| *what)
            .collect();
        assert!(warned.is_empty(), "{warned:?}");
    }

    /// Each way of naming a pager program the audit counts.
    const PROGRAM_NAMED: &[(&str, &str)] = &[
        (
            "an environment default",
            "Environment:\n  PAGER=less    Pager for long output\n",
        ),
        (
            "a man page reference",
            "Long output is shown in less(1) when stdout is a terminal.\n",
        ),
        (
            "more's man page",
            "Long output is shown in more(1) when stdout is a terminal.\n",
        ),
        (
            "backticks",
            "Long output opens in `less` when stdout is a terminal.\n",
        ),
        (
            "a quoted command with options",
            "Long output opens in 'less -R' when stdout is a terminal.\n",
        ),
        (
            "a double-quoted command",
            "Long output opens in \"more\" when stdout is a terminal.\n",
        ),
        (
            "piped through",
            "Long output is piped through less by default.\n",
        ),
        (
            "piped to, across a wrap",
            "Long output is piped\n    to more by default.\n",
        ),
    ];

    #[test]
    fn less_and_more_named_as_programs_are_pager_signals() {
        let skipped: Vec<&str> = PROGRAM_NAMED
            .iter()
            .filter(|(_, help)| is_skip(help))
            .map(|(what, _)| *what)
            .collect();
        assert!(skipped.is_empty(), "{skipped:?}");
    }

    #[test]
    fn non_english_help_skipped() {
        // Localized help with no pager-adjacent ASCII tokens → Skip via
        // "no signal" branch. The English-only exception is documented.
        let help = HelpOutput::from_raw(HELP_NON_ENGLISH);
        let status = audit_no_pager(&help);
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn detects_no_pager_with_mixed_casing() {
        let help = HelpOutput::from_raw("  --no-pager   Disable paging.\n");
        assert_eq!(audit_no_pager(&help), AuditStatus::Pass);
    }

    #[test]
    fn a_single_dash_no_pager_beside_double_dash_names_warns_and_says_why() {
        let help = HelpOutput::from_raw(
            "Options:\n  -no-pager     Do not send output through less.\n      --help    Show help.\n",
        );
        match audit_no_pager(&help) {
            AuditStatus::Warn(msg) => assert!(
                msg.ends_with("`-no-pager` is declared, but this help also declares double-dash names, so it does not count as `--no-pager`."),
                "{msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
