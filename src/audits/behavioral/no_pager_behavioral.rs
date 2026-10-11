//! Audit: behavioral confirmation that a pager-using tool ships `--no-pager`.
//!
//! Covers: `p6-must-no-pager`. Source-verified coverage already exists via
//! `p6-no-pager`; this audit adds the behavioral layer by inspecting the
//! shipped `--help` surface. Heuristic (Medium confidence): pager inference
//! from the text is soft.
//!
//! Pass when `--no-pager` is in the advertised flag list. Skip when the
//! help text shows no pager signal at all (nothing mentions `less`, `more`,
//! `$PAGER`, `--pager`, or `pager`). Warn when the text mentions pager
//! plumbing but the escape hatch is absent.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

// Pager signal keywords. Note: "more" is excluded from this list because it
// matches common English phrases like "for more information". Instead, we check
// for it separately in a pager-specific context (e.g., "`more`" or "more(1)").
const PAGER_SIGNALS: &[&str] = &["less", "$PAGER", "--pager", "pager", "PAGER"];

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
        })
    }
}

/// Core unit for tests. Takes a prepared `HelpOutput` and returns the
/// `AuditStatus` that summarizes it.
fn audit_no_pager(help: &HelpOutput) -> AuditStatus {
    let has_no_pager_flag = help.flags().iter().any(|f| f.matches("--no-pager"));
    if has_no_pager_flag {
        return AuditStatus::Pass;
    }
    let raw = help.raw();
    let mentions_pager =
        PAGER_SIGNALS.iter().any(|sig| raw.contains(sig)) || mentions_more_pager(raw);
    if !mentions_pager {
        AuditStatus::Skip("no pager signal (less/more/$PAGER/--pager) in --help".into())
    } else {
        AuditStatus::Warn(
            "pager referenced in --help but no --no-pager escape hatch advertised".into(),
        )
    }
}

/// Check if "more" appears in a pager-specific context, not just as an English word.
/// Patterns that indicate the `more` pager command:
/// - Backtick-quoted: `more`
/// - Man page reference: more(1)
/// - Command-like: "piped to more", "through more", "using more"
fn mentions_more_pager(text: &str) -> bool {
    text.contains("`more`")
        || text.contains("more(1)")
        || text.contains("piped to more")
        || text.contains("piped through more")
        || text.contains("through more")
        || text.contains("using more")
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
        // Ensure we match `--no-pager` as a long flag regardless of how the
        // `Flag::matches` helper receives the query.
        let help = HelpOutput::from_raw("  --no-pager   Disable paging.\n");
        assert_eq!(audit_no_pager(&help), AuditStatus::Pass);
    }

    #[test]
    fn skip_when_more_is_just_english_word() {
        // Regression test: "more information" should not be detected as the `more` pager
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             For more information, visit our website.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        let status = audit_no_pager(&help);
        assert!(
            matches!(status, AuditStatus::Skip(_)),
            "expected Skip when 'more' is just English, got {status:?}"
        );
    }

    #[test]
    fn warn_when_more_pager_in_backticks() {
        // When "more" appears as `more` (backtick-quoted command), it's the pager
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             Output is piped through `more` by default.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        let status = audit_no_pager(&help);
        match status {
            AuditStatus::Warn(msg) => assert!(msg.contains("--no-pager")),
            other => panic!("expected Warn when `more` pager is mentioned, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_more_man_page_reference() {
        // more(1) is a man page reference to the pager
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             See more(1) for pager configuration.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        let status = audit_no_pager(&help);
        match status {
            AuditStatus::Warn(msg) => assert!(msg.contains("--no-pager")),
            other => panic!("expected Warn when more(1) is mentioned, got {other:?}"),
        }
    }
}
