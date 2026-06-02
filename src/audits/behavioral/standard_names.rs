//! Audit: `p6-may-standard-names`.
//!
//! Subcommand verbs MAY follow community-standard names (`get`, `list`,
//! `create`, `update`, `delete`, etc.). MAY-tier — non-conforming verbs are a
//! soft signal, not a failure. Pass when most subcommands match the
//! standard-verb allow-list; Warn when many do not.
//!
//! Universal applicability — runs on any CLI with a runner. The audit Skips
//! when the help output exposes no parseable subcommands.
//!
//! Two extension points compose the recognized verb set:
//!
//! 1. [`STANDARD_VERBS`] — the conservative, cross-CLI built-in list.
//! 2. The audit target's `.anc.toml` (`[p6] domain_verbs = [...]`) — per-CLI
//!    platform vocabulary that the built-in list deliberately omits (an X
//!    CLI's `mentions`, a billing CLI's `charge`, etc.). See [`crate::anc_toml`].

use std::collections::HashSet;

use crate::anc_toml::{self, AncConfigLoad};
use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

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
    // Social / notification platform verbs
    "archive",
    "block",
    "bookmark",
    "dm",
    "follow",
    "like",
    "mute",
    "post",
    "quote",
    "reply",
    "repost",
    "subscribe",
    "unarchive",
    "unblock",
    "unfollow",
    "unlike",
    "unmute",
    "unsubscribe",
];

/// Pass threshold — at least this fraction of subcommands must match the
/// standard verb list. Soft signal: 70% leaves room for project-specific
/// verbs without flagging a healthy CLI.
const STANDARD_VERB_PASS_RATIO: f32 = 0.70;

pub struct StandardNamesAudit;

impl Audit for StandardNamesAudit {
    fn id(&self) -> &str {
        "p6-standard-names"
    }

    fn label(&self) -> &'static str {
        "Subcommand verbs follow community-standard names"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-may-standard-names"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        // `.anc.toml` is the per-CLI extension point. Surface parse errors
        // as the primary signal (Warn with the loader's evidence) — a
        // malformed config is the actionable finding here; the verb check
        // would only mask it.
        let status = match anc_toml::load(&project.path) {
            AncConfigLoad::Invalid(msg) => AuditStatus::Warn(msg),
            other => {
                let cfg = other.as_config();
                let domain_verbs: &[String] =
                    cfg.map(|c| c.p6.domain_verbs.as_slice()).unwrap_or(&[]);
                match project.help_output() {
                    None => AuditStatus::Skip("could not probe --help".into()),
                    Some(help) => audit_standard_names(help, domain_verbs),
                }
            }
        };

        Ok(AuditResult {
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
///
/// `domain_verbs` extends the built-in [`STANDARD_VERBS`] list with per-CLI
/// platform vocabulary (typically loaded from `.anc.toml`). Recognition is
/// case-insensitive on the subcommand name; entries in `domain_verbs` are
/// matched verbatim against the lower-cased name.
pub(crate) fn audit_standard_names(help: &HelpOutput, domain_verbs: &[String]) -> AuditStatus {
    let standard: HashSet<&str> = STANDARD_VERBS.iter().copied().collect();
    let domain: HashSet<&str> = domain_verbs.iter().map(String::as_str).collect();
    let subs: Vec<&String> = help.subcommands().iter().collect();

    if subs.is_empty() {
        return AuditStatus::Skip("no subcommands parsed from --help".into());
    }

    let total = subs.len();
    let standard_count = subs
        .iter()
        .filter(|name| {
            let lower = name.to_lowercase();
            standard.contains(lower.as_str()) || domain.contains(lower.as_str())
        })
        .count();

    let ratio = standard_count as f32 / total as f32;
    if ratio >= STANDARD_VERB_PASS_RATIO {
        AuditStatus::Pass
    } else {
        let non_standard: Vec<&str> = subs
            .iter()
            .filter(|name| {
                let lower = name.to_lowercase();
                !standard.contains(lower.as_str()) && !domain.contains(lower.as_str())
            })
            .map(|s| s.as_str())
            .collect();
        AuditStatus::Warn(format!(
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

    const HELP_SOCIAL_VERBS: &str = r#"Usage: x [OPTIONS] <COMMAND>

Commands:
  post       Publish a post
  like       Like a post
  repost     Repost a post
  bookmark   Bookmark a post
  follow     Follow a user

Options:
  -h, --help  Show help
"#;

    const HELP_MIXED_WITH_MENTIONS: &str = r#"Usage: x [OPTIONS] <COMMAND>

Commands:
  post       Publish a post
  like       Like a post
  mentions   List mentions

Options:
  -h, --help  Show help
"#;

    const HELP_DUP_BUILTIN: &str = r#"Usage: x [OPTIONS] <COMMAND>

Commands:
  post     Publish a post
  like     Like a post
  delete   Delete a post

Options:
  -h, --help  Show help
"#;

    #[test]
    fn happy_path_standard_verbs() {
        let help = HelpOutput::from_raw(HELP_STANDARD_VERBS);
        assert_eq!(audit_standard_names(&help, &[]), AuditStatus::Pass);
    }

    #[test]
    fn warn_non_standard_majority() {
        let help = HelpOutput::from_raw(HELP_NON_STANDARD);
        match audit_standard_names(&help, &[]) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("yeet") || msg.contains("bork") || msg.contains("blarg"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_no_subcommands() {
        let help = HelpOutput::from_raw(HELP_NO_SUBCOMMANDS);
        match audit_standard_names(&help, &[]) {
            AuditStatus::Skip(msg) => assert!(msg.contains("subcommand")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn expanded_builtin_recognizes_social_verbs() {
        let help = HelpOutput::from_raw(HELP_SOCIAL_VERBS);
        assert_eq!(audit_standard_names(&help, &[]), AuditStatus::Pass);
    }

    #[test]
    fn mentions_unknown_without_domain_verbs() {
        let help = HelpOutput::from_raw(HELP_MIXED_WITH_MENTIONS);
        match audit_standard_names(&help, &[]) {
            AuditStatus::Warn(msg) => {
                assert!(
                    msg.contains("mentions"),
                    "expected `mentions` in non-standard list, got: {msg}"
                );
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn mentions_recognized_with_domain_verbs() {
        let help = HelpOutput::from_raw(HELP_MIXED_WITH_MENTIONS);
        let domain = vec!["mentions".to_string()];
        assert_eq!(audit_standard_names(&help, &domain), AuditStatus::Pass);
    }

    #[test]
    fn empty_domain_verbs_no_op() {
        let help = HelpOutput::from_raw(HELP_MIXED_WITH_MENTIONS);
        // Regression: an empty domain_verbs slice (the loaded-but-empty
        // case) must behave identically to `Absent`.
        assert!(matches!(
            audit_standard_names(&help, &[]),
            AuditStatus::Warn(_)
        ));
    }

    #[test]
    fn domain_verb_duplicating_builtin_is_harmless() {
        let help = HelpOutput::from_raw(HELP_DUP_BUILTIN);
        // `post` is in the built-in list AND in domain_verbs — recognition
        // must dedupe via set-membership semantics, not double-count.
        let domain = vec!["post".to_string()];
        let status = audit_standard_names(&help, &domain);
        assert_eq!(status, AuditStatus::Pass);
    }
}
