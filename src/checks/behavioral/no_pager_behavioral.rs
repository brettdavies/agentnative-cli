//! Check: behavioral confirmation that a pager-using tool ships `--no-pager`.
//!
//! Covers: `p6-must-no-pager`. Source-verified coverage already exists via
//! `p6-no-pager`; this check adds the behavioral layer by inspecting the
//! shipped `--help` surface. Heuristic (Medium confidence): pager inference
//! from the text is soft.
//!
//! Pass when `--no-pager` is in the advertised flag list. Skip when the
//! help text shows no pager signal at all (nothing mentions `less`, `more`,
//! `$PAGER`, `--pager`, or `pager`). Warn when the text mentions pager
//! plumbing but the escape hatch is absent.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const PAGER_SIGNALS: &[&str] = &["less", "more", "$PAGER", "--pager", "pager", "PAGER"];

pub struct NoPagerBehavioralCheck;

impl Check for NoPagerBehavioralCheck {
    fn id(&self) -> &str {
        "p6-no-pager-behavioral"
    }

    fn label(&self) -> &'static str {
        "Pager-using CLI ships --no-pager escape hatch"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-pager"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_no_pager(help),
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

/// Core unit for tests. Takes a prepared `HelpOutput` and returns the
/// `CheckStatus` that summarizes it.
fn check_no_pager(help: &HelpOutput) -> CheckStatus {
    let has_no_pager_flag = help.flags().iter().any(|f| f.matches("--no-pager"));
    if has_no_pager_flag {
        return CheckStatus::Pass;
    }
    let raw = help.raw();
    let mentions_pager = PAGER_SIGNALS.iter().any(|sig| raw.contains(sig));
    if !mentions_pager {
        CheckStatus::Skip("no pager signal (less/more/$PAGER/--pager) in --help".into())
    } else {
        CheckStatus::Warn(
            "pager referenced in --help but no --no-pager escape hatch advertised".into(),
        )
    }
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
        assert_eq!(check_no_pager(&help), CheckStatus::Pass);
    }

    #[test]
    fn skip_when_no_pager_mention_at_all() {
        let help = HelpOutput::from_raw(HELP_NO_PAGER_MENTION);
        let status = check_no_pager(&help);
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn warn_when_pager_mentioned_without_escape() {
        let help = HelpOutput::from_raw(HELP_PAGER_WITHOUT_ESCAPE);
        let status = check_no_pager(&help);
        match status {
            CheckStatus::Warn(msg) => {
                assert!(msg.contains("--no-pager"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn non_english_help_skipped() {
        // Localized help with no pager-adjacent ASCII tokens → Skip via
        // "no signal" branch. The English-only exception is documented.
        let help = HelpOutput::from_raw(HELP_NON_ENGLISH);
        let status = check_no_pager(&help);
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn detects_no_pager_with_mixed_casing() {
        // Ensure we match `--no-pager` as a long flag regardless of how the
        // `Flag::matches` helper receives the query.
        let help = HelpOutput::from_raw("  --no-pager   Disable paging.\n");
        assert_eq!(check_no_pager(&help), CheckStatus::Pass);
    }
}
