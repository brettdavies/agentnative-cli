//! The verdict of an audit that asks one thing of a help: whether it
//! declares any of a set of flags.

use crate::runner::HelpOutput;
use crate::types::AuditStatus;

/// Pass when `help` declares any of `names`. Otherwise a Warn that carries
/// `message`, closed with [`HelpOutput::dash_rule_note`] when one applies.
pub(super) fn pass_or_warn(help: &HelpOutput, names: &[&str], message: &str) -> AuditStatus {
    if help.find_flag(names).is_some() {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(help.noting_dash_rule(names, message))
    }
}
