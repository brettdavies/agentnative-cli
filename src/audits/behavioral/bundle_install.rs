//! Audit: `p8-must-bundle-install`.
//!
//! When a CLI ships a skill bundle, it MUST provide an install path
//! (`tool skill install [<host>]`) that registers the bundle with installed
//! agent runtimes. Non-canonical alternatives (`tool init --skill`,
//! `tool skills add`, `tool agents add`) are accepted as soft-pass with
//! advisory evidence.
//!
//! Applicability: gates on `find_bundle()` (the same heuristic
//! `p8-bundle-exists` uses). When no bundle is present at the project root,
//! the requirement is vacuously satisfied.

use crate::audit::Audit;
use crate::audits::project::bundle_exists::find_bundle;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct BundleInstallAudit;

impl Audit for BundleInstallAudit {
    fn id(&self) -> &str {
        "p8-bundle-install"
    }

    fn label(&self) -> &'static str {
        "Skill bundle has install path (`tool skill install [<host>]`)"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P8
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-must-bundle-install"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        // Vacuous Pass when no bundle is present — the requirement is gated
        // on "if CLI ships an agent skill bundle".
        if find_bundle(&project.path).is_none() {
            return Ok(AuditResult {
                id: self.id().to_string(),
                label: self.label().into(),
                group: self.group(),
                layer: self.layer(),
                status: AuditStatus::Pass,
                confidence: Confidence::High,
                mitigation: None,
            });
        }

        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_bundle_install(help),
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

/// Core unit for tests. Looks for `skill install` via subcommand parsing,
/// then falls back to non-canonical install patterns in raw help text.
pub(crate) fn audit_bundle_install(help: &HelpOutput) -> AuditStatus {
    // Canonical: `skill` is a top-level subcommand. The `skill install`
    // detail is not always exposed at top-level help, so accept any
    // top-level `skill` subcommand as the canonical surface.
    let subs = help.subcommands();
    let has_skill_subcommand = subs.iter().any(|s| s.eq_ignore_ascii_case("skill"));
    if has_skill_subcommand {
        return AuditStatus::Pass;
    }

    // Non-canonical alternatives accepted as Pass — `init --skill`,
    // `skills add`, `agents add`. The spec calls these out explicitly:
    // "Non-canonical alternatives are acceptable but SHOULD migrate toward
    // `tool skill install`." Surfacing the migration hint requires an
    // advisory-evidence-on-Pass enum extension that isn't in scope here.
    let raw = help.raw().to_lowercase();
    let non_canonical_patterns = ["init --skill", "skills add", "agents add"];
    if non_canonical_patterns.iter().any(|p| raw.contains(p)) {
        return AuditStatus::Pass;
    }

    AuditStatus::Fail(
        "skill bundle present but no install path (`skill install`, \
         `init --skill`, etc.) advertised in --help. Without an install \
         path the bundle stays unread until a human manually copies it."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_WITH_SKILL_SUBCMD: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  audit     Run audits
  skill     Manage skill bundle installation
  schema    Print output schema

Options:
  -h, --help  Show help
"#;

    const HELP_NO_SKILL_SURFACE: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  audit     Run audits
  schema    Print output schema

Options:
  -h, --help  Show help
"#;

    const HELP_INIT_SKILL: &str = r#"Usage: tool [OPTIONS] <COMMAND>

Commands:
  audit     Run audits
  init      Initialize the project; pass `init --skill` to install bundle.

Options:
  -h, --help  Show help
"#;

    #[test]
    fn happy_path_skill_subcommand() {
        let help = HelpOutput::from_raw(HELP_WITH_SKILL_SUBCMD);
        assert_eq!(audit_bundle_install(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_with_non_canonical_init_skill() {
        let help = HelpOutput::from_raw(HELP_INIT_SKILL);
        assert_eq!(audit_bundle_install(&help), AuditStatus::Pass);
    }

    #[test]
    fn fail_no_skill_install_path() {
        let help = HelpOutput::from_raw(HELP_NO_SKILL_SURFACE);
        match audit_bundle_install(&help) {
            AuditStatus::Fail(msg) => assert!(msg.contains("install")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
