//! Audit: Detect presence of AGENTS.md in the project root.
//!
//! Principle: P6 (Composable Structure) — An AGENTS.md file signals agent-readiness
//! and provides instructions for AI agents working with the project.

use crate::audit::Audit;
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct AgentsMdAudit;

impl Audit for AgentsMdAudit {
    fn id(&self) -> &str {
        "p6-agents-md"
    }

    fn label(&self) -> &'static str {
        "AGENTS.md exists"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
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
        let agents_md = project.path.join("AGENTS.md");

        let status = if agents_md.exists() {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn("No AGENTS.md found in project root".into())
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-agents-md-{suffix}-{}-{}",
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
    fn applicable_when_path_is_dir() {
        let dir = temp_dir("applicable");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(AgentsMdAudit.applicable(&project));
    }

    #[test]
    fn pass_when_agents_md_exists() {
        let dir = temp_dir("pass");
        fs::write(dir.join("AGENTS.md"), "# Agent instructions\n").expect("write AGENTS.md");
        let project = Project::discover(&dir).expect("discover test project");
        let result = AgentsMdAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_agents_md_missing() {
        let dir = temp_dir("warn");
        let project = Project::discover(&dir).expect("discover test project");
        let result = AgentsMdAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
    }

    #[test]
    fn metadata_is_correct() {
        let audit = AgentsMdAudit;
        assert_eq!(audit.id(), "p6-agents-md");
        assert_eq!(audit.group(), AuditGroup::P6);
        assert_eq!(audit.layer(), AuditLayer::Project);
    }
}
