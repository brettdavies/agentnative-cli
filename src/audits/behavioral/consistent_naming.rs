//! Audit: `p6-should-consistent-naming`.
//!
//! Subcommand naming follows a consistent `noun verb` or `verb noun`
//! convention throughout the tool. SHOULD-tier; gated on the CLI having
//! subcommands. The audit is heuristic: a CLI that mixes single-word verbs
//! (`get`, `set`) with multi-word noun-first patterns (`config set`) signals
//! inconsistency.
//!
//! We classify each top-level subcommand by whether its name is a common
//! verb (`get`, `set`, `list`, `add`, `remove`, `delete`, etc.). The
//! audit Passes when all subcommands fall on the same side of the verb/noun
//! split, Warns when the surface mixes patterns.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Verbs that suggest a subcommand is "verb-named" (verb-first convention).
const COMMON_VERBS: &[&str] = &[
    "add", "build", "audit", "create", "delete", "deploy", "describe", "destroy", "edit", "export",
    "fetch", "generate", "get", "import", "init", "install", "list", "ls", "make", "new",
    "publish", "pull", "push", "remove", "rm", "run", "search", "serve", "set", "show", "start",
    "status", "stop", "test", "update", "upgrade", "view",
];

pub struct ConsistentNamingAudit;

impl Audit for ConsistentNamingAudit {
    fn id(&self) -> &str {
        "p6-consistent-naming"
    }

    fn label(&self) -> &'static str {
        "Subcommand naming follows a consistent verb/noun convention"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-should-consistent-naming"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_consistent_naming(help),
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

pub(crate) fn audit_consistent_naming(help: &HelpOutput) -> AuditStatus {
    // Skip clap-built-in subcommands that ride along regardless of the
    // tool's own naming convention.
    let cmds: Vec<&String> = help
        .subcommands()
        .iter()
        .filter(|s| !matches!(s.as_str(), "help" | "completions"))
        .collect();

    if cmds.len() < 2 {
        return AuditStatus::Skip(
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
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
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
        assert_eq!(audit_consistent_naming(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_all_nouns() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  config     Manage config.\n  cluster    Manage clusters.\n",
        );
        assert_eq!(audit_consistent_naming(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_mixed() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n  config    Manage config.\n  status    Show status.\n",
        );
        match audit_consistent_naming(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("mixes")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_only_one_command() {
        let help = HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n");
        match audit_consistent_naming(&help) {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
