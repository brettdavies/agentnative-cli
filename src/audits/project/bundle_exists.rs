//! Audit: `p8-should-bundle-exists`.
//!
//! CLIs SHOULD ship a top-level agent-discoverable markdown bundle (canonical
//! names: `AGENTS.md` or `SKILL.md`) with YAML frontmatter naming the tool and
//! summarizing capabilities. Universal applicability — every CLI is in scope.
//!
//! Detection (project layer): scan repo root for `AGENTS.md` / `SKILL.md`
//! (case-insensitive). When present, sniff for YAML frontmatter (`---` opener,
//! `name:` field). Pass when any bundle file carries both; Warn when a bundle
//! exists but none does; OptOut when no bundle is found.

use std::path::{Path, PathBuf};

use crate::audit::Audit;
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Bundle-file basenames recognized by major agent runtimes. Case-insensitive
/// match so `Agents.md` and `agents.md` are treated equivalently.
const BUNDLE_BASENAMES: &[&str] = &["AGENTS.md", "SKILL.md"];

#[derive(Debug)]
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
            config_hint: None,
            row_statuses: Vec::new(),
            pass_evidence: None,
        })
    }
}

/// Every bundle file (AGENTS.md / SKILL.md) at the repo root, case-insensitive.
/// Sorted because `read_dir` order differs by filesystem, and a repo holding
/// two files has to grade the same on all of them.
fn bundle_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            BUNDLE_BASENAMES
                .iter()
                .any(|canonical| name.eq_ignore_ascii_case(canonical))
        })
        .map(|entry| entry.path())
        .collect();
    files.sort();
    files
}

/// Locate a bundle file at the repo root. Returns the discovered path so other
/// P8 audits can gate on the same heuristic without repeating the directory
/// walk.
pub(crate) fn find_bundle(root: &Path) -> Option<PathBuf> {
    bundle_files(root).into_iter().next()
}

/// Core unit. SHOULD-tier: every miss is Warn, never Fail. The "no bundle
/// at all" case emits `OptOut` so antecedent propagation (Decision 2a) can
/// collapse the conditional MUSTs/MAYs that depend on `p8-bundle-exists`
/// (`p8-must-bundle-install`, `p8-may-install-all`, `p8-may-bundle-update`)
/// to `n_a` rather than dragging them through their own probe and emitting
/// double-penalty Skips. A bundle that exists but is malformed (missing
/// frontmatter or `name:` field) is a real SHOULD violation and stays
/// `Warn` — the feature is present, just incomplete. Either canonical file
/// meets the requirement on its own, so an `AGENTS.md` of plain project notes
/// does not outweigh a `SKILL.md` that carries the frontmatter.
pub(crate) fn audit_bundle_exists(root: &Path) -> AuditStatus {
    let mut bundles = bundle_files(root).into_iter();
    let Some(first) = bundles.next() else {
        return AuditStatus::OptOut(
            "no top-level AGENTS.md or SKILL.md found. Agents discover \
             skill bundles via filesystem convention; ship one with YAML \
             frontmatter naming the tool."
                .into(),
        );
    };

    let first_status = audit_bundle_file(&first);
    if first_status == AuditStatus::Pass
        || bundles.any(|path| audit_bundle_file(&path) == AuditStatus::Pass)
    {
        return AuditStatus::Pass;
    }
    first_status
}

/// Grade one bundle file. Evidence names the file, never its absolute path,
/// which would put the operator's directory layout in a shared scorecard.
fn audit_bundle_file(path: &Path) -> AuditStatus {
    let name = path.file_name().unwrap_or_default().to_string_lossy();

    let Ok(content) = std::fs::read_to_string(path) else {
        return AuditStatus::Warn(format!(
            "{name} exists but could not be read (permission or encoding)."
        ));
    };

    if !has_yaml_frontmatter(&content) {
        return AuditStatus::Warn(format!(
            "{name} exists but lacks YAML frontmatter. Add `---\\nname: <tool>\\n…\\n---` \
             at the top so agent runtimes can index the bundle's metadata."
        ));
    }

    if !has_name_field(&content) {
        return AuditStatus::Warn(format!(
            "{name} has frontmatter but no `name:` field. Agents pin against the \
             tool name; declare it in the bundle's frontmatter."
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

    // Whichever file `read_dir` lists first, one of these two repos has its
    // qualifying bundle second.
    #[test]
    fn either_file_with_frontmatter_meets_the_requirement_beside_a_plain_one() {
        for (with_frontmatter, plain) in [("SKILL.md", "AGENTS.md"), ("AGENTS.md", "SKILL.md")] {
            let dir = temp_dir("both");
            fs::write(dir.join(with_frontmatter), "---\nname: x\n---\n").expect("write");
            fs::write(dir.join(plain), "# Project notes\n").expect("write");
            assert_eq!(
                audit_bundle_exists(&dir),
                AuditStatus::Pass,
                "{with_frontmatter} has frontmatter, {plain} does not"
            );
        }
    }

    #[test]
    fn two_files_without_frontmatter_warn_on_the_first_by_name() {
        let dir = temp_dir("bothplain");
        fs::write(dir.join("SKILL.md"), "# Skill\n").expect("write");
        fs::write(dir.join("AGENTS.md"), "# Agents\n").expect("write");
        match audit_bundle_exists(&dir) {
            AuditStatus::Warn(msg) => assert!(msg.starts_with("AGENTS.md "), "{msg}"),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn evidence_names_the_file_not_its_absolute_path() {
        let dir = temp_dir("evidence");
        fs::write(dir.join("AGENTS.md"), "# Tool docs\n").expect("write");
        match audit_bundle_exists(&dir) {
            AuditStatus::Warn(msg) => assert!(
                !msg.contains(dir.to_string_lossy().as_ref()),
                "evidence carries the absolute path: {msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
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
