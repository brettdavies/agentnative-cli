//! Check: `p6-should-consistent-naming`.
//!
//! Subcommand naming follows a consistent `noun verb` or `verb noun`
//! convention throughout the tool. SHOULD-tier; gated on the CLI having
//! subcommands. The check is heuristic: a CLI that mixes single-word verbs
//! (`get`, `set`) with multi-word noun-first patterns (`config set`) signals
//! inconsistency.
//!
//! We classify each top-level subcommand by whether its name is a common
//! verb (`get`, `set`, `list`, `add`, `remove`, `delete`, etc.). The
//! check Passes when all subcommands fall on the same side of the verb/noun
//! split, Warns when the surface mixes patterns.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Verbs that suggest a subcommand is "verb-named" (verb-first convention).
const COMMON_VERBS: &[&str] = &[
    "add", "build", "check", "create", "delete", "deploy", "describe", "destroy", "edit", "export",
    "fetch", "generate", "get", "import", "init", "install", "list", "ls", "make", "new",
    "publish", "pull", "push", "remove", "rm", "run", "search", "serve", "set", "show", "start",
    "status", "stop", "test", "update", "upgrade", "view",
];

pub struct ConsistentNamingCheck;

impl Check for ConsistentNamingCheck {
    fn id(&self) -> &str {
        "p6-consistent-naming"
    }

    fn label(&self) -> &'static str {
        "Subcommand naming follows a consistent verb/noun convention"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-should-consistent-naming"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_consistent_naming(help),
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

pub(crate) fn check_consistent_naming(help: &HelpOutput) -> CheckStatus {
    // Skip clap-built-in subcommands that ride along regardless of the
    // tool's own naming convention.
    let cmds: Vec<&String> = help
        .subcommands()
        .iter()
        .filter(|s| !matches!(s.as_str(), "help" | "completions"))
        .collect();

    if cmds.len() < 2 {
        return CheckStatus::Skip(
            "fewer than 2 user-defined subcommands; vacuous skip for the conditional \
             SHOULD."
                .into(),
        );
    }

    let mut verb_count = 0;
    let mut non_verb_count = 0;
    for cmd in &cmds {
        if COMMON_VERBS.iter().any(|v| cmd.eq_ignore_ascii_case(v)) {
            verb_count += 1;
        } else {
            non_verb_count += 1;
        }
    }

    if verb_count == 0 || non_verb_count == 0 {
        // All on one side — consistent.
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(format!(
            "subcommand naming mixes verb-first ({verb_count}) and noun-first \
             ({non_verb_count}) patterns. SHOULD-tier — pick `verb noun` or \
             `noun verb` and apply it consistently so agents can predict \
             names. Inspect `--help` to confirm; the verb list is a heuristic."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_all_verbs() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n  add      Add an item.\n  remove   Remove.\n",
        );
        assert_eq!(check_consistent_naming(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_when_all_nouns() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  config     Manage config.\n  cluster    Manage clusters.\n",
        );
        assert_eq!(check_consistent_naming(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_mixed() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n  config    Manage config.\n  status    Show status.\n",
        );
        match check_consistent_naming(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("mixes")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_only_one_command() {
        let help = HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n");
        match check_consistent_naming(&help) {
            CheckStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
