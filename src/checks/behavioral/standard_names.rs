//! Check: `p6-may-standard-names`.
//!
//! Subcommand verbs MAY follow community-standard names (`get`, `list`,
//! `create`, `update`, `delete`, etc.). MAY-tier — non-conforming verbs are a
//! soft signal, not a failure. Pass when most subcommands match the
//! standard-verb allow-list; Warn when many do not.
//!
//! Universal applicability — runs on any CLI with a runner. The check Skips
//! when the help output exposes no parseable subcommands.

use std::collections::HashSet;

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Community-standard verbs derived from the spec summary text. Includes both
/// CRUD verbs and common meta-commands (`help`, `version`, `init`, etc.) so
/// well-shaped CLIs aren't penalized for shipping a healthy meta surface.
///
/// Subgroups are ordered alphabetically by name; verbs within each subgroup
/// are ordered alphabetically. Keep this property when editing — drift makes
/// review noisy.
const STANDARD_VERBS: &[&str] = &[
    // Action-style
    "apply",
    "audit",
    "build",
    "check",
    "deploy",
    "describe",
    "diff",
    "exec",
    "publish",
    "restart",
    "rollback",
    "run",
    "scale",
    "serve",
    "show",
    "start",
    "stop",
    "test",
    "watch",
    // CRUD-style
    "add",
    "create",
    "delete",
    "get",
    "list",
    "ls",
    "remove",
    "rm",
    "set",
    "update",
    // Discovery / Read-only auxiliaries
    "events",
    "explain",
    "find",
    "history",
    "inspect",
    "logs",
    "search",
    "view",
    // Meta
    "auth",
    "completions",
    "config",
    "doctor",
    "help",
    "info",
    "init",
    "login",
    "logout",
    "schema",
    "status",
    "version",
    // Pkg-mgmt-style
    "clean",
    "fetch",
    "install",
    "pull",
    "push",
    "sync",
    "uninstall",
    "update-self",
    "upgrade",
    // Skill-bundle (P8 alignment)
    "skill",
];

/// Pass threshold — at least this fraction of subcommands must match the
/// standard verb list. Soft signal: 70% leaves room for project-specific
/// verbs without flagging a healthy CLI.
const STANDARD_VERB_PASS_RATIO: f32 = 0.70;

pub struct StandardNamesCheck;

impl Check for StandardNamesCheck {
    fn id(&self) -> &str {
        "p6-standard-names"
    }

    fn label(&self) -> &'static str {
        "Subcommand verbs follow community-standard names"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-may-standard-names"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_standard_names(help),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Low,
        })
    }
}

/// Core unit for tests. Returns Skip when no subcommands are present (the
/// "if CLI uses subcommands" applicability is vacuously satisfied), Pass when
/// at least the threshold fraction matches the allow-list, Warn otherwise.
pub(crate) fn check_standard_names(help: &HelpOutput) -> CheckStatus {
    let standard: HashSet<&str> = STANDARD_VERBS.iter().copied().collect();
    let subs: Vec<&String> = help.subcommands().iter().collect();

    if subs.is_empty() {
        return CheckStatus::Skip("no subcommands parsed from --help".into());
    }

    let total = subs.len();
    let standard_count = subs
        .iter()
        .filter(|name| standard.contains(name.to_lowercase().as_str()))
        .count();

    let ratio = standard_count as f32 / total as f32;
    if ratio >= STANDARD_VERB_PASS_RATIO {
        CheckStatus::Pass
    } else {
        let non_standard: Vec<&str> = subs
            .iter()
            .filter(|name| !standard.contains(name.to_lowercase().as_str()))
            .map(|s| s.as_str())
            .collect();
        CheckStatus::Warn(format!(
            "{}/{} subcommand(s) follow standard verb names. Non-standard: {}. \
             MAY-tier — community-standard verbs (get/list/create/update/delete) \
             help agents predict subcommand behavior across CLIs.",
            standard_count,
            total,
            non_standard.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_STANDARD_VERBS: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  list      List items
  get       Get an item
  create    Create an item
  delete    Delete an item

Options:
  -h, --help  Show help
"#;

    const HELP_NON_STANDARD: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  yeet     Remove an item with prejudice
  bork     Repair a thing
  blarg    Do the blarg
  list     List items

Options:
  -h, --help  Show help
"#;

    const HELP_NO_SUBCOMMANDS: &str = r#"Usage: tool [OPTIONS]

Options:
      --output <FORMAT>   Output format
  -h, --help              Show help
"#;

    #[test]
    fn happy_path_standard_verbs() {
        let help = HelpOutput::from_raw(HELP_STANDARD_VERBS);
        assert_eq!(check_standard_names(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_non_standard_majority() {
        let help = HelpOutput::from_raw(HELP_NON_STANDARD);
        match check_standard_names(&help) {
            CheckStatus::Warn(msg) => {
                assert!(msg.contains("yeet") || msg.contains("bork") || msg.contains("blarg"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_no_subcommands() {
        let help = HelpOutput::from_raw(HELP_NO_SUBCOMMANDS);
        match check_standard_names(&help) {
            CheckStatus::Skip(msg) => assert!(msg.contains("subcommand")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
