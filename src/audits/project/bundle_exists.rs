//! Audit: `p8-should-bundle-exists`.
//!
//! CLIs SHOULD ship a top-level agent-discoverable markdown bundle (canonical
//! names: `AGENTS.md` or `SKILL.md`) with YAML frontmatter naming the tool and
//! summarizing capabilities. Universal applicability — every CLI is in scope.
//!
//! Detection (project layer): scan repo root for `AGENTS.md` / `SKILL.md`
//! (case-insensitive). When present, sniff for YAML frontmatter (`---` opener,
//! `name:` field). Pass when both bundle and frontmatter are present; Warn
//! when bundle exists but frontmatter is missing; Warn when neither is found.

use std::path::Path;

use crate::audit::Audit;
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Bundle-file basenames recognized by major agent runtimes. Case-insensitive
/// match so `Agents.md` and `agents.md` are treated equivalently.
const BUNDLE_BASENAMES: &[&str] = &["AGENTS.md", "SKILL.md"];

pub struct BundleExistsAudit;

impl Audit for BundleExistsAudit {
    fn id(&self) -> &str {
        "p8-bundle-exists"
    }

    fn label(&self) -> &'static str {
        "Top-level AGENTS.md / SKILL.md bundle present"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P8
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-should-bundle-exists"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = audit_bundle_exists(&project.path);

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

/// Locate the bundle file (AGENTS.md / SKILL.md) at the repo root, case-
/// insensitive. Returns the discovered path so other P8 audits can gate on
/// the same heuristic without repeating the directory walk.
pub(crate) fn find_bundle(root: &Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        for canonical in BUNDLE_BASENAMES {
            if name_str.eq_ignore_ascii_case(canonical) {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Core unit. SHOULD-tier: every miss is Warn, never Fail. The "no bundle
/// at all" case emits `OptOut` so antecedent propagation (Decision 2a) can
/// collapse the conditional MUSTs/MAYs that depend on `p8-bundle-exists`
/// (`p8-must-bundle-install`, `p8-may-install-all`, `p8-may-bundle-update`)
/// to `n_a` rather than dragging them through their own probe and emitting
/// double-penalty Skips. A bundle that exists but is malformed (missing
/// frontmatter or `name:` field) is a real SHOULD violation and stays
/// `Warn` — the feature is present, just incomplete.
pub(crate) fn audit_bundle_exists(root: &Path) -> AuditStatus {
    let Some(path) = find_bundle(root) else {
        return AuditStatus::OptOut(
            "no top-level AGENTS.md or SKILL.md found. Agents discover \
             skill bundles via filesystem convention; ship one with YAML \
             frontmatter naming the tool."
                .into(),
        );
    };

    let Ok(content) = std::fs::read_to_string(&path) else {
        return AuditStatus::Warn(format!(
            "{} exists but could not be read (permission or encoding).",
            path.display()
        ));
    };

    if !has_yaml_frontmatter(&content) {
        return AuditStatus::Warn(format!(
            "{} exists but lacks YAML frontmatter. Add `---\\nname: <tool>\\n…\\n---` \
             at the top so agent runtimes can index the bundle's metadata.",
            path.display()
        ));
    }

    if !has_name_field(&content) {
        return AuditStatus::Warn(format!(
            "{} has frontmatter but no `name:` field. Agents pin against the \
             tool name; declare it in the bundle's frontmatter.",
            path.display()
        ));
    }

    AuditStatus::Pass
}

fn has_yaml_frontmatter(content: &str) -> bool {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("").trim();
    if first != "---" {
        return false;
    }
    // Walk until we find the closing `---`.
    lines.any(|line| line.trim() == "---")
}

fn has_name_field(content: &str) -> bool {
    let mut in_frontmatter = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                return false;
            }
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter && trimmed.starts_with("name:") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-bundle-exists-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn happy_path_agents_md_with_frontmatter() {
        let dir = temp_dir("agents-fm");
        fs::write(
            dir.join("AGENTS.md"),
            "---\nname: my-tool\nsummary: A useful tool\n---\n\n# Tool docs\n",
        )
        .expect("write");
        assert_eq!(audit_bundle_exists(&dir), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_skill_md_with_frontmatter() {
        let dir = temp_dir("skill-fm");
        fs::write(dir.join("SKILL.md"), "---\nname: my-skill\n---\n\nDocs.\n").expect("write");
        assert_eq!(audit_bundle_exists(&dir), AuditStatus::Pass);
    }

    #[test]
    fn opt_out_no_bundle() {
        let dir = temp_dir("nobundle");
        fs::write(dir.join("README.md"), "# Tool\n").expect("write");
        match audit_bundle_exists(&dir) {
            AuditStatus::OptOut(msg) => assert!(msg.contains("AGENTS.md")),
            other => panic!("expected OptOut, got {other:?}"),
        }
    }

    #[test]
    fn warn_bundle_no_frontmatter() {
        let dir = temp_dir("nofm");
        fs::write(dir.join("AGENTS.md"), "# Tool docs\n").expect("write");
        match audit_bundle_exists(&dir) {
            AuditStatus::Warn(msg) => assert!(msg.contains("frontmatter")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_frontmatter_without_name() {
        let dir = temp_dir("noname");
        fs::write(
            dir.join("AGENTS.md"),
            "---\nsummary: missing name\n---\n\nDocs.\n",
        )
        .expect("write");
        match audit_bundle_exists(&dir) {
            AuditStatus::Warn(msg) => assert!(msg.contains("name:")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn case_insensitive_match() {
        let dir = temp_dir("caseinsensitive");
        fs::write(dir.join("agents.md"), "---\nname: x\n---\n").expect("write");
        assert_eq!(audit_bundle_exists(&dir), AuditStatus::Pass);
    }

    #[test]
    fn find_bundle_returns_path() {
        let dir = temp_dir("findpath");
        let path = dir.join("AGENTS.md");
        fs::write(&path, "---\nname: x\n---\n").expect("write");
        assert_eq!(
            find_bundle(&dir).map(|p| p.file_name().unwrap().to_owned()),
            Some(path.file_name().unwrap().to_owned())
        );
    }
}
