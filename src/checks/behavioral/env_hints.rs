//! Check: `--help` advertises environment-variable bindings for flags.
//!
//! Covers: `p1-must-env-var`. Source-verified coverage already exists via
//! `p1-env-flags-source`; this check adds the behavioral layer by inspecting
//! the shipped `--help` surface. Heuristic (Medium confidence): it reads
//! clap-style `[env: FOO]` annotations, which are the canonical but not the
//! only way tools advertise env bindings.
//!
//! Skip when there are no flags at all — a tool with no flags has nothing
//! to bind to env vars. Warn when flags exist but no bindings are visible.

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct EnvHintsCheck;

impl Check for EnvHintsCheck {
    fn id(&self) -> &str {
        "p1-env-hints"
    }

    fn label(&self) -> &'static str {
        "Flags advertise env-var bindings in --help"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-env-var"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_env_hints(help.flags().len(), help.env_hints().len()),
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

/// Core unit. Takes parsed-flag count and parsed-env-hint count and returns
/// the `CheckStatus` that summarizes them.
fn check_env_hints(flag_count: usize, env_hint_count: usize) -> CheckStatus {
    if flag_count == 0 {
        return CheckStatus::Skip("target exposes no flags in --help".into());
    }
    if env_hint_count > 0 {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(format!(
            "{flag_count} flag(s) found in --help but no `[env: NAME]` bindings advertised"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::HelpOutput;

    const HELP_WITH_ENV: &str = r#"Usage: foo [OPTIONS]

Options:
  -q, --quiet    Suppress output [env: FOO_QUIET=]
  -h, --help     Print help
"#;

    const HELP_NO_ENV: &str = r#"Usage: foo [OPTIONS]

Options:
  -q, --quiet    Suppress output
  -h, --help     Print help
"#;

    const HELP_NO_FLAGS: &str = r#"Usage: foo ARG
A tool that takes one positional argument.
"#;

    // Non-English help: parser returns zero env hints and zero flags when
    // English conventions don't appear. Per the coverage-matrix exception,
    // this is documented English-only behavior.
    const HELP_NON_ENGLISH: &str = r#"用法: outil URL

参数:
  URL      目标
"#;

    #[test]
    fn happy_path_env_hint_present() {
        let help = HelpOutput::from_raw(HELP_WITH_ENV);
        let status = check_env_hints(help.flags().len(), help.env_hints().len());
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn skip_when_no_flags() {
        let help = HelpOutput::from_raw(HELP_NO_FLAGS);
        let status = check_env_hints(help.flags().len(), help.env_hints().len());
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn warn_when_flags_but_no_env_hints() {
        let help = HelpOutput::from_raw(HELP_NO_ENV);
        let status = check_env_hints(help.flags().len(), help.env_hints().len());
        match status {
            CheckStatus::Warn(msg) => {
                assert!(msg.contains("env"));
                assert!(msg.contains("flag"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn non_english_help_skipped_or_warned() {
        // Localized help with no ASCII options block — parsers return empty
        // flags + empty env_hints. Skip (no flags to bind).
        let help = HelpOutput::from_raw(HELP_NON_ENGLISH);
        let status = check_env_hints(help.flags().len(), help.env_hints().len());
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn unit_core_returns_pass_with_any_hint() {
        assert_eq!(check_env_hints(3, 1), CheckStatus::Pass);
        assert_eq!(check_env_hints(3, 10), CheckStatus::Pass);
    }

    #[test]
    fn unit_core_warns_when_zero_hints() {
        let status = check_env_hints(5, 0);
        assert!(matches!(status, CheckStatus::Warn(_)));
    }
}
