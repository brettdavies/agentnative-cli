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
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, MitigationInfo};

/// Documentation pointer appended to the audit's Warn evidence so authors of
/// CLIs with social-platform or domain-specific vocabulary discover the
/// `.anc.toml [p6] domain_verbs` opt-in. The pattern doc lives in
/// `docs/solutions/` (a symlink to a separate repo); the path is committed
/// to that repo independently of this crate.
const DOMAIN_VERBS_DOCS_URL: &str =
    "docs/solutions/architecture-patterns/anc-toml-domain-verbs-pattern-2026-06-03.md";

/// Cap on the number of domain-verb matches listed in the Pass evidence
/// string. Beyond this, the formatter appends `, ...` so text-mode rendering
/// stays tidy without truncating signal needed for the structured
/// `MitigationInfo.domain_match_examples` field.
const DOMAIN_MATCH_EXAMPLES_LIMIT: usize = 5;

/// Composite result of `audit_standard_names`. `status` is the verdict that
/// drives the scorecard row; `mitigation` is populated only when the verdict
/// was assisted by `.anc.toml [p6] domain_verbs` recognition, giving the
/// scorecard a structured signal that distinguishes a self-declared Pass
/// from an unassisted one.
#[derive(Debug, PartialEq)]
pub(crate) struct StandardNamesResult {
    pub status: AuditStatus,
    pub mitigation: Option<MitigationInfo>,
}

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
    // Cross-domain notification / lifecycle verbs (file managers, mail
    // clients, notification systems all use these — kept in built-ins).
    // Platform-specific verbs (post / repost / unrepost / quote / like /
    // unlike / dm) were intentionally removed from this list; they belong
    // in per-CLI `.anc.toml [p6] domain_verbs` (see
    // `docs/solutions/architecture-patterns/anc-toml-domain-verbs-pattern-2026-06-03.md`).
    "archive",
    "block",
    "bookmark",
    "follow",
    "mute",
    "reply",
    "subscribe",
    "unarchive",
    "unblock",
    "unfollow",
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
        let result = match anc_toml::load(&project.path) {
            AncConfigLoad::Invalid(msg) => StandardNamesResult {
                status: AuditStatus::Warn(msg),
                mitigation: None,
            },
            other => {
                let cfg = other.as_config();
                let domain_verbs: &[String] =
                    cfg.map(|c| c.p6.domain_verbs.as_slice()).unwrap_or(&[]);
                match project.help_output() {
                    None => StandardNamesResult {
                        status: AuditStatus::Skip("could not probe --help".into()),
                        mitigation: None,
                    },
                    Some(help) => audit_standard_names(help, domain_verbs),
                }
            }
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: result.status,
            confidence: Confidence::Low,
            mitigation: result.mitigation,
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
///
/// When at least one subcommand is recognized via `domain_verbs` (not via
/// the built-in list), the returned `mitigation` field carries the
/// transparency signal so the scorecard distinguishes a domain-assisted
/// Pass from an unassisted one. `mitigation` is `None` for built-ins-only
/// Pass, for Warn, and for Skip.
pub(crate) fn audit_standard_names(
    help: &HelpOutput,
    domain_verbs: &[String],
) -> StandardNamesResult {
    let standard: HashSet<&str> = STANDARD_VERBS.iter().copied().collect();
    let domain: HashSet<&str> = domain_verbs.iter().map(String::as_str).collect();
    let subs: Vec<&String> = help.subcommands().iter().collect();

    if subs.is_empty() {
        return StandardNamesResult {
            status: AuditStatus::Skip("no subcommands parsed from --help".into()),
            mitigation: None,
        };
    }

    let total = subs.len();
    let mut builtin_matches: Vec<&str> = Vec::new();
    let mut domain_matches: Vec<&str> = Vec::new();
    let mut non_standard: Vec<&str> = Vec::new();

    for name in subs.iter() {
        let lower = name.to_lowercase();
        if standard.contains(lower.as_str()) {
            builtin_matches.push(name.as_str());
        } else if domain.contains(lower.as_str()) {
            domain_matches.push(name.as_str());
        } else {
            non_standard.push(name.as_str());
        }
    }

    let recognized = builtin_matches.len() + domain_matches.len();
    let ratio = recognized as f32 / total as f32;
    if ratio >= STANDARD_VERB_PASS_RATIO {
        let mitigation = if domain_matches.is_empty() {
            None
        } else {
            let examples: Vec<String> = domain_matches
                .iter()
                .take(DOMAIN_MATCH_EXAMPLES_LIMIT)
                .map(|s| (*s).to_string())
                .collect();
            Some(MitigationInfo {
                using_domain_verbs: true,
                domain_match_count: domain_matches.len(),
                domain_match_examples: examples,
                builtin_match_count: builtin_matches.len(),
                subcommand_total: total,
            })
        };
        StandardNamesResult {
            status: AuditStatus::Pass,
            mitigation,
        }
    } else {
        StandardNamesResult {
            status: AuditStatus::Warn(format!(
                "{}/{} subcommand(s) follow standard verb names. Non-standard: {}. \
                 MAY-tier — community-standard verbs (get/list/create/update/delete) \
                 help agents predict subcommand behavior across CLIs. \
                 Per-CLI vocabulary (social, billing, etc.) can opt in via .anc.toml \
                 [p6] domain_verbs; see {}.",
                recognized,
                total,
                non_standard.join(", "),
                DOMAIN_VERBS_DOCS_URL,
            )),
            mitigation: None,
        }
    }
}

/// Format the Pass-with-mitigation evidence string surfaced on the
/// `p6-standard-names` scorecard row. Public to `crate` because the
/// scorecard view layer composes the prose from the structured
/// `MitigationInfo` carried on the `AuditResult`. The Pass row's evidence
/// names the built-in vs domain split so a downstream reader sees at a
/// glance how much of the verdict depended on the per-CLI opt-in.
pub(crate) fn format_pass_evidence(mitigation: &MitigationInfo) -> String {
    let recognized_total = mitigation.builtin_match_count + mitigation.domain_match_count;
    let total = mitigation.subcommand_total;
    let examples_rendered = if mitigation.domain_match_count <= DOMAIN_MATCH_EXAMPLES_LIMIT {
        mitigation.domain_match_examples.join(", ")
    } else {
        format!("{}, ...", mitigation.domain_match_examples.join(", "))
    };
    format!(
        "{}/{} subcommands standard ({} via .anc.toml [p6].domain_verbs: [{}])",
        recognized_total, total, mitigation.domain_match_count, examples_rendered,
    )
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

    const HELP_CROSS_DOMAIN_VERBS: &str = r#"Usage: notif [OPTIONS] <COMMAND>

Commands:
  archive    Archive items
  bookmark   Bookmark items
  follow     Follow a thread
  mute       Mute notifications
  subscribe  Subscribe to a feed

Options:
  -h, --help  Show help
"#;

    const HELP_MIXED_WITH_MENTIONS: &str = r#"Usage: x [OPTIONS] <COMMAND>

Commands:
  archive    Archive a post
  follow     Follow a user
  mentions   List mentions

Options:
  -h, --help  Show help
"#;

    const HELP_SOCIAL_PLATFORM: &str = r#"Usage: x [OPTIONS] <COMMAND>

Commands:
  archive    Archive a post
  bookmark   Bookmark a post
  follow     Follow a user
  post       Publish a post
  like       Like a post

Options:
  -h, --help  Show help
"#;

    const HELP_NONSENSE_VERBS: &str = r#"Usage: nonsense [OPTIONS] <COMMAND>

Commands:
  yeet     Remove with prejudice
  bork     Repair a thing
  blarg    Do the blarg

Options:
  -h, --help  Show help
"#;

    const HELP_CASE_MISMATCH: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  post     Publish a post

Options:
  -h, --help  Show help
"#;

    const HELP_DUP_BUILTIN: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  archive   Archive items
  delete    Delete items
  list      List items

Options:
  -h, --help  Show help
"#;

    #[test]
    fn happy_path_standard_verbs() {
        let help = HelpOutput::from_raw(HELP_STANDARD_VERBS);
        let r = audit_standard_names(&help, &[]);
        assert_eq!(r.status, AuditStatus::Pass);
        assert!(r.mitigation.is_none());
    }

    #[test]
    fn warn_non_standard_majority() {
        let help = HelpOutput::from_raw(HELP_NON_STANDARD);
        match audit_standard_names(&help, &[]).status {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("yeet") || msg.contains("bork") || msg.contains("blarg"));
                assert!(
                    msg.contains(DOMAIN_VERBS_DOCS_URL),
                    "warn evidence must point at docs/solutions opt-in: {msg}"
                );
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_no_subcommands() {
        let help = HelpOutput::from_raw(HELP_NO_SUBCOMMANDS);
        match audit_standard_names(&help, &[]).status {
            AuditStatus::Skip(msg) => assert!(msg.contains("subcommand")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn builtin_recognizes_cross_domain_verbs() {
        // After the platform-verb trim, the surviving cross-domain
        // additions (archive, bookmark, follow, mute, subscribe and their
        // `un-` partners) stay in the built-in list and Pass unaided.
        let help = HelpOutput::from_raw(HELP_CROSS_DOMAIN_VERBS);
        let r = audit_standard_names(&help, &[]);
        assert_eq!(r.status, AuditStatus::Pass);
        assert!(
            r.mitigation.is_none(),
            "cross-domain built-ins must Pass without using domain_verbs"
        );
    }

    #[test]
    fn mentions_unknown_without_domain_verbs() {
        let help = HelpOutput::from_raw(HELP_MIXED_WITH_MENTIONS);
        match audit_standard_names(&help, &[]).status {
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
        let r = audit_standard_names(&help, &domain);
        assert_eq!(r.status, AuditStatus::Pass);
        let m = r
            .mitigation
            .expect("domain-assisted pass populates mitigation");
        assert!(m.using_domain_verbs);
        assert_eq!(m.domain_match_count, 1);
        assert_eq!(m.domain_match_examples, vec!["mentions".to_string()]);
    }

    #[test]
    fn empty_domain_verbs_no_op() {
        let help = HelpOutput::from_raw(HELP_MIXED_WITH_MENTIONS);
        // Regression: an empty domain_verbs slice (the loaded-but-empty
        // case) must behave identically to `Absent`.
        let r = audit_standard_names(&help, &[]);
        assert!(matches!(r.status, AuditStatus::Warn(_)));
        assert!(r.mitigation.is_none());
    }

    #[test]
    fn domain_verb_duplicating_builtin_is_harmless() {
        let help = HelpOutput::from_raw(HELP_DUP_BUILTIN);
        // `archive` is in the built-in list AND in domain_verbs —
        // recognition must dedupe via set-membership semantics, not
        // double-count. The audit must Pass via built-ins, NOT report
        // domain_verbs assistance (because `archive` was matched by the
        // built-in check first).
        let domain = vec!["archive".to_string()];
        let r = audit_standard_names(&help, &domain);
        assert_eq!(r.status, AuditStatus::Pass);
        assert!(
            r.mitigation.is_none(),
            "duplicate domain entry must NOT trigger mitigation when built-in already matches: {:?}",
            r.mitigation
        );
    }

    // ── R5 adversarial coverage (PR #76 follow-up plan) ──────────────────

    #[test]
    fn nonsense_domain_verbs_pass_with_transparency_flag() {
        // R5(a): a CLI whose subcommands are all nonsense words can Pass
        // by declaring those nonsense words in `.anc.toml domain_verbs`.
        // The audit Passes (this is the user's risk to take) but the
        // mitigation field MUST surface the assistance so the scorecard
        // distinguishes a self-declared Pass from an earned one.
        let help = HelpOutput::from_raw(HELP_NONSENSE_VERBS);
        let domain = vec!["yeet".to_string(), "bork".to_string(), "blarg".to_string()];
        let r = audit_standard_names(&help, &domain);
        assert_eq!(r.status, AuditStatus::Pass);
        let m = r
            .mitigation
            .expect("self-declared pass must populate mitigation");
        assert!(m.using_domain_verbs);
        assert_eq!(m.domain_match_count, 3);
        // Examples preserve encounter order (matches subcommand order
        // in --help output).
        assert_eq!(
            m.domain_match_examples,
            vec!["yeet".to_string(), "bork".to_string(), "blarg".to_string()]
        );
    }

    #[test]
    fn case_mismatch_domain_verbs_does_not_match() {
        // R5(b): `domain_verbs` entries are compared verbatim against the
        // lowercased subcommand name. `"Post"` (capital P) does NOT match
        // `post` (lowercased by the audit). Documented behavior; pinning
        // it prevents a silent regression if someone "fixes" the case
        // handling without thinking about the contract.
        let help = HelpOutput::from_raw(HELP_CASE_MISMATCH);
        let domain = vec!["Post".to_string()];
        let r = audit_standard_names(&help, &domain);
        // `post` not matched by the case-sensitive domain check; 0/1
        // recognized; the audit Warns.
        assert!(matches!(r.status, AuditStatus::Warn(_)));
        assert!(r.mitigation.is_none());
    }

    #[test]
    fn pass_without_anc_toml_omits_transparency_fields() {
        // R5(c): a built-ins-only Pass MUST NOT carry mitigation. Tested
        // here as the unit-level contract; the scorecard JSON elision
        // (`skip_serializing_if = "Option::is_none"`) is a downstream
        // consequence.
        let help = HelpOutput::from_raw(HELP_STANDARD_VERBS);
        let r = audit_standard_names(&help, &[]);
        assert_eq!(r.status, AuditStatus::Pass);
        assert!(r.mitigation.is_none());
    }

    #[test]
    fn empty_domain_verbs_omits_transparency_fields() {
        // R5(d): a `.anc.toml` with `[p6] domain_verbs = []` must behave
        // identically to an absent file for transparency purposes — Pass
        // (when warranted) carries no mitigation; the audit row is
        // byte-identical to the no-config case.
        let help = HelpOutput::from_raw(HELP_STANDARD_VERBS);
        let domain: Vec<String> = Vec::new();
        let r = audit_standard_names(&help, &domain);
        assert_eq!(r.status, AuditStatus::Pass);
        assert!(r.mitigation.is_none());
    }

    #[test]
    fn social_cli_documented_example_passes() {
        // R5(e): the social-CLI example from the docs/solutions pattern
        // (xurl-rs-shaped vocabulary) Passes when `.anc.toml` declares the
        // platform vocabulary. Mitigation surfaces every platform verb
        // that survived the built-in trim; the survival of `archive`,
        // `bookmark`, `follow` as built-ins keeps the ratio at Pass
        // without help, so the test must assert specifically against the
        // platform-only matches.
        let help = HelpOutput::from_raw(HELP_SOCIAL_PLATFORM);
        let domain = vec![
            "post".to_string(),
            "like".to_string(),
            "repost".to_string(),
            "dm".to_string(),
            "quote".to_string(),
        ];
        let r = audit_standard_names(&help, &domain);
        assert_eq!(r.status, AuditStatus::Pass);
        let m = r
            .mitigation
            .expect("social-CLI Pass via domain_verbs populates mitigation");
        assert!(m.using_domain_verbs);
        // HELP_SOCIAL_PLATFORM lists 5 commands: archive, bookmark, follow
        // (built-ins) + post, like (domain). domain_match_count is 2.
        assert_eq!(m.domain_match_count, 2);
        assert_eq!(
            m.domain_match_examples,
            vec!["post".to_string(), "like".to_string()]
        );
    }

    #[test]
    fn format_pass_evidence_caps_examples() {
        // The Pass evidence string lists at most the first
        // DOMAIN_MATCH_EXAMPLES_LIMIT (5) domain matches and appends
        // `, ...` when more matches exist. Pins the truncation contract
        // so text-mode rendering stays predictable.
        let m = MitigationInfo {
            using_domain_verbs: true,
            domain_match_count: 7,
            domain_match_examples: vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
            ],
            builtin_match_count: 3,
            subcommand_total: 12,
        };
        let evidence = format_pass_evidence(&m);
        assert!(
            evidence.ends_with("[a, b, c, d, e, ...])"),
            "expected truncated example list with trailing ellipsis: {evidence}"
        );
        assert!(evidence.starts_with("10/12 subcommands standard (7 via .anc.toml"));
    }

    #[test]
    fn format_pass_evidence_omits_ellipsis_when_at_limit() {
        let m = MitigationInfo {
            using_domain_verbs: true,
            domain_match_count: 3,
            domain_match_examples: vec!["x".to_string(), "y".to_string(), "z".to_string()],
            builtin_match_count: 2,
            subcommand_total: 7,
        };
        let evidence = format_pass_evidence(&m);
        assert!(
            evidence.ends_with("[x, y, z])"),
            "expected non-truncated example list: {evidence}"
        );
        assert!(evidence.starts_with("5/7 subcommands standard"));
    }
}
