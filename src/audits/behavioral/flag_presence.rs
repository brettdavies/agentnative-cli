//! The verdict of an audit that asks one thing of a help: whether it
//! declares any of a set of flags.

use crate::audits::behavioral::flag_meaning::{self, Meaning};
use crate::runner::HelpOutput;
use crate::runner::help_probe::is_single_letter;
use crate::types::AuditStatus;

/// Pass when `help` declares any of `names`, a single letter among them
/// only where its definition gives `meaning`. Otherwise a Warn that carries
/// `message`, closed with the notes that say why a declared name did not
/// count. `meaning` is `None` only for `names` with no single letter.
pub(super) fn pass_or_warn(
    help: &HelpOutput,
    names: &[&str],
    meaning: Option<&Meaning>,
    message: &str,
) -> AuditStatus {
    match meaning {
        Some(meaning) if flag_meaning::find(help, names, meaning).is_some() => AuditStatus::Pass,
        Some(meaning) => AuditStatus::Warn(flag_meaning::noting(help, names, meaning, message)),
        None => {
            debug_assert!(
                !names.iter().any(|name| is_single_letter(name)),
                "a single letter needs a Meaning: {names:?}"
            );
            if help.find_flag(names).is_some() {
                AuditStatus::Pass
            } else {
                AuditStatus::Warn(help.noting_dash_rule(names, message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &["--color", "--colour"];
    const MESSAGE: &str = "no color flag.";

    fn verdict(help: &str) -> AuditStatus {
        pass_or_warn(&HelpOutput::from_raw(help), NAMES, None, MESSAGE)
    }

    #[test]
    fn any_declared_name_passes() {
        assert_eq!(
            verdict(
                "Options:\n      --colour <WHEN>    Colorize.\n  -h, --help             Show help.\n"
            ),
            AuditStatus::Pass
        );
    }

    #[test]
    fn no_declared_name_warns_with_the_message_alone() {
        assert_eq!(
            verdict("Options:\n      --colorize    Colorize.\n  -h, --help        Show help.\n"),
            AuditStatus::Warn(MESSAGE.into())
        );
    }

    #[test]
    fn a_single_dash_spelling_beside_double_dash_names_warns_and_says_why() {
        assert_eq!(
            verdict("Options:\n  -color        Colorize.\n      --help    Show help.\n"),
            AuditStatus::Warn(
                "no color flag. `-color` is declared, but this help also declares double-dash \
                 names, so it does not count as `--color`."
                    .into()
            )
        );
    }

    #[test]
    fn a_single_dash_spelling_passes_in_a_help_without_double_dash_names() {
        assert_eq!(
            verdict("Usage of tool:\n  -color\n    \tColorize\n  -version\n    \tShow version\n"),
            AuditStatus::Pass
        );
    }
}
