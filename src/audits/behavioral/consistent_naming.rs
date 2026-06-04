//! Audit: `p6-should-consistent-naming`.
//!
//! Subcommand naming follows a discoverable convention. SHOULD-tier; gated
//! on the CLI having 2+ user-defined subcommands. The audit is heuristic and
//! recognizes three classifications for each top-level subcommand:
//!
//! 1. **Top-level verb** — name is a known action verb (`get`, `install`,
//!    `audit`, `emit`), OR a non-verb name with no further subcommands /
//!    only non-verb children (a verb-implicit action like `anc emit
//!    coverage-matrix` where `emit` is the action and `coverage-matrix` is
//!    its noun object).
//! 2. **Hierarchical noun-verb** — name is a non-verb whose children are
//!    all verbs (`anc skill install` / `anc skill update`, `gh repo create`
//!    / `gh repo delete`). The noun groups its verbs; the action stays
//!    predictable at the second level.
//! 3. **Mixed** — name is a non-verb whose children mix verbs and
//!    non-verbs at the second level. The action position is unpredictable
//!    even within the group, so an agent can't generalize.
//!
//! Pass condition: every top-level subcommand is either Top-level verb or
//! Hierarchical noun-verb. Warn when any subcommand falls into Mixed. This
//! recognizes the common modern pattern of combining top-level verbs with
//! noun-grouped verb hierarchies (git, gh, kubectl, docker, cargo,
//! anc itself) rather than insisting on strict single-convention surfaces.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Verbs that suggest a subcommand is "verb-named" (verb-first convention).
const COMMON_VERBS: &[&str] = &[
    "add", "audit", "build", "create", "delete", "deploy", "describe", "destroy", "edit", "emit",
    "export", "fetch", "generate", "get", "import", "init", "install", "list", "ls", "make", "new",
    "publish", "pull", "push", "remove", "rm", "run", "search", "serve", "set", "show", "start",
    "status", "stop", "test", "update", "upgrade", "view",
];

/// Subcommand names that are clap built-ins and don't count toward the
/// tool's own naming convention.
const CLAP_BUILTINS: &[&str] = &["help", "completions", "completion", "complete"];

/// Classification for a single top-level subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    /// Verb action: either the name itself is a verb, or it is a non-verb
    /// with no children (a leaf action like `anc emit`) or a non-verb whose
    /// children are themselves nouns (a verb-with-noun-objects shape like
    /// `anc emit coverage-matrix`).
    TopLevelVerb,
    /// Noun groups verbs: name is a non-verb, all children are verbs
    /// (`anc skill install` / `update`). Predictable: the action is the
    /// second-level token.
    HierarchicalNounVerb,
    /// Genuinely inconsistent: non-verb name with mixed verb / non-verb
    /// children at the second level. The action position is unpredictable
    /// even within the group.
    Mixed,
}

fn is_verb(name: &str) -> bool {
    COMMON_VERBS.iter().any(|v| name.eq_ignore_ascii_case(v))
}

fn is_clap_builtin(name: &str) -> bool {
    CLAP_BUILTINS.iter().any(|s| name.eq_ignore_ascii_case(s))
}

fn classify(name: &str, children: &[String]) -> Classification {
    if is_verb(name) {
        return Classification::TopLevelVerb;
    }
    let real_children: Vec<&String> = children.iter().filter(|c| !is_clap_builtin(c)).collect();
    if real_children.is_empty() {
        // Non-verb top-level with no children: treat as a leaf action (the
        // subcommand itself is verb-implicit, e.g. anc `emit` historically).
        return Classification::TopLevelVerb;
    }
    let verb_count = real_children.iter().filter(|c| is_verb(c)).count();
    let non_verb_count = real_children.len() - verb_count;
    if verb_count > 0 && non_verb_count == 0 {
        Classification::HierarchicalNounVerb
    } else if verb_count == 0 {
        // All children are non-verbs: verb-with-noun-objects pattern. The
        // name is the action even though it is not in our verb list.
        Classification::TopLevelVerb
    } else {
        Classification::Mixed
    }
}

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
        let status = match (project.help_output(), project.runner.as_ref()) {
            (Some(help), Some(runner)) => {
                let probe = |name: &str| probe_subcommand_children(runner, name);
                audit_consistent_naming(help, probe)
            }
            _ => AuditStatus::Skip("could not probe --help".into()),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

/// Probe `<bin> <subcmd> --help`, parse its `Commands:` section, return
/// the second-level subcommand names. Returns an empty vec when the probe
/// fails or the subcommand has no children. Cached by the runner.
fn probe_subcommand_children(runner: &BinaryRunner, name: &str) -> Vec<String> {
    let result = runner.run(&[name, "--help"], &[]);
    match result.status {
        RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
            let mut raw = String::with_capacity(result.stdout.len() + result.stderr.len());
            raw.push_str(&result.stdout);
            raw.push_str(&result.stderr);
            if raw.trim().is_empty() {
                return Vec::new();
            }
            HelpOutput::from_raw(raw).subcommands().to_vec()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn audit_consistent_naming(
    top_help: &HelpOutput,
    children_of: impl Fn(&str) -> Vec<String>,
) -> AuditStatus {
    let cmds: Vec<&String> = top_help
        .subcommands()
        .iter()
        .filter(|s| !is_clap_builtin(s))
        .collect();

    if cmds.len() < 2 {
        return AuditStatus::Skip(
            "fewer than 2 user-defined subcommands; vacuous skip for the conditional SHOULD."
                .into(),
        );
    }

    let mut top_level_verb: Vec<&str> = Vec::new();
    let mut hierarchical: Vec<&str> = Vec::new();
    let mut mixed: Vec<&str> = Vec::new();

    for cmd in &cmds {
        let children = if is_verb(cmd) {
            Vec::new() // skip probing for known verbs to keep the runner cache tight
        } else {
            children_of(cmd.as_str())
        };
        match classify(cmd, &children) {
            Classification::TopLevelVerb => top_level_verb.push(cmd.as_str()),
            Classification::HierarchicalNounVerb => hierarchical.push(cmd.as_str()),
            Classification::Mixed => mixed.push(cmd.as_str()),
        }
    }

    if mixed.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "subcommand naming is inconsistent: {} non-verb subcommand(s) ({}) mix verb and non-verb children at the second level, so an agent cannot predict where the action lives. SHOULD-tier: pick a consistent shape (all verb-first, all noun-verb hierarchy, or any combination where each non-verb group's children are uniformly verbs). The verb list is a heuristic; inspect `--help` to confirm.",
            mixed.len(),
            mixed.join(", "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_children(_: &str) -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn pass_when_all_verbs() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n  add      Add an item.\n  remove   Remove.\n",
        );
        assert_eq!(
            audit_consistent_naming(&help, no_children),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_when_all_nouns_with_verb_children() {
        // Nouns grouping verbs: `tool config get`, `tool config set`,
        // `tool cluster create`, `tool cluster delete`. Hierarchical
        // noun-verb pattern.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  config     Manage config.\n  cluster    Manage clusters.\n",
        );
        let probe = |name: &str| match name {
            "config" => vec!["get".into(), "set".into()],
            "cluster" => vec!["create".into(), "delete".into()],
            _ => vec![],
        };
        assert_eq!(audit_consistent_naming(&help, probe), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_anc_pattern() {
        // The core fix: top-level verb (`audit`) + verb-with-noun-objects
        // (`emit coverage-matrix`, `emit schema`) + noun-verb hierarchy
        // (`skill install`, `skill update`). Each is internally
        // predictable; the mix passes.
        let help = HelpOutput::from_raw(
            "Usage: anc [COMMAND]\n\nCommands:\n  audit          Audit a CLI.\n  emit           Render artifacts.\n  skill          Manage the skill bundle.\n",
        );
        let probe = |name: &str| match name {
            "emit" => vec!["coverage-matrix".into(), "schema".into()],
            "skill" => vec!["install".into(), "update".into()],
            _ => vec![],
        };
        assert_eq!(audit_consistent_naming(&help, probe), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_mixed_top_level_with_consistent_subtrees() {
        // git-style: top-level verbs (`pull`, `push`) plus noun-grouped
        // verbs (`remote add`, `remote remove`, `branch list`, `branch
        // delete`). Modern CLIs routinely combine the two; the audit
        // should not penalize this.
        let help = HelpOutput::from_raw(
            "Usage: vcs [COMMAND]\n\nCommands:\n  pull     Pull from remote.\n  push     Push to remote.\n  remote   Manage remotes.\n  branch   Manage branches.\n",
        );
        let probe = |name: &str| match name {
            "remote" => vec!["add".into(), "remove".into()],
            "branch" => vec!["list".into(), "delete".into()],
            _ => vec![],
        };
        assert_eq!(audit_consistent_naming(&help, probe), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_noun_has_truly_mixed_children() {
        // The genuine bad case: a non-verb subcommand whose children
        // include both verbs and non-verbs. The action position is
        // ambiguous within the group.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  build    Build it.\n  config   Manage config.\n",
        );
        let probe = |name: &str| match name {
            "config" => vec!["get".into(), "credentials".into(), "set".into()], // get + set verbs, credentials noun
            _ => vec![],
        };
        match audit_consistent_naming(&help, probe) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("inconsistent"));
                assert!(msg.contains("config"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_non_verb_top_level_has_no_children() {
        // Non-verb name with no children is a leaf action (verb-implicit).
        // Example: `tool metadata`, `tool version` — both nouns by name,
        // both verbs by function. Should not pre-fix warn against a tool
        // that legitimately mixes these with conventional verbs.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  build      Build it.\n  metadata   Show project metadata.\n  version    Show version.\n",
        );
        assert_eq!(
            audit_consistent_naming(&help, no_children),
            AuditStatus::Pass
        );
    }

    #[test]
    fn skip_when_only_one_command() {
        let help = HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n");
        match audit_consistent_naming(&help, no_children) {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_only_clap_builtins() {
        // A CLI with only `help` and `completions` (no user-defined
        // subcommands) should skip, not pass.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  help         Print help.\n  completions  Generate completions.\n",
        );
        match audit_consistent_naming(&help, no_children) {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn clap_builtins_in_children_are_filtered() {
        // `tool config help` should not count as a noun child of config;
        // it is a clap built-in. config's user-defined children (get, set)
        // are all verbs, so the classification is HierarchicalNounVerb.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Audit.\n  config   Manage config.\n",
        );
        let probe = |name: &str| match name {
            "config" => vec![
                "get".into(),
                "set".into(),
                "help".into(),
                "completions".into(),
            ],
            _ => vec![],
        };
        assert_eq!(audit_consistent_naming(&help, probe), AuditStatus::Pass);
    }

    #[test]
    fn warn_message_lists_offending_subcommands() {
        // The Warn evidence must name every Mixed subcommand so the
        // operator can target the fix.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Audit.\n  config   Config.\n  user     User.\n",
        );
        let probe = |name: &str| match name {
            "config" => vec!["get".into(), "schema".into()], // mixed
            "user" => vec!["create".into(), "profile".into()], // mixed
            _ => vec![],
        };
        match audit_consistent_naming(&help, probe) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("config"));
                assert!(msg.contains("user"));
                assert!(msg.contains('2'));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn classify_helpers_handle_case_insensitively() {
        assert!(is_verb("AUDIT"));
        assert!(is_verb("Audit"));
        assert!(is_verb("audit"));
        assert!(is_clap_builtin("HELP"));
        assert!(is_clap_builtin("Completions"));
        assert!(!is_verb("skill"));
        assert!(!is_clap_builtin("audit"));
    }
}
