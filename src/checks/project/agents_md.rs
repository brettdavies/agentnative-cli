//! Check: Detect presence of AGENTS.md in the project root.
//!
//! Principle: P6 (Composable Structure) — An AGENTS.md file signals agent-readiness
//! and provides instructions for AI agents working with the project.

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct AgentsMdCheck;

impl Check for AgentsMdCheck {
    fn id(&self) -> &str {
        "p6-agents-md"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let agents_md = project.path.join("AGENTS.md");

        let status = if agents_md.exists() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn("No AGENTS.md found in project root".into())
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "AGENTS.md exists".into(),
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
        assert!(AgentsMdCheck.applicable(&project));
    }

    #[test]
    fn pass_when_agents_md_exists() {
        let dir = temp_dir("pass");
        fs::write(dir.join("AGENTS.md"), "# Agent instructions\n").expect("write AGENTS.md");
        let project = Project::discover(&dir).expect("discover test project");
        let result = AgentsMdCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_agents_md_missing() {
        let dir = temp_dir("warn");
        let project = Project::discover(&dir).expect("discover test project");
        let result = AgentsMdCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn metadata_is_correct() {
        let check = AgentsMdCheck;
        assert_eq!(check.id(), "p6-agents-md");
        assert_eq!(check.group(), CheckGroup::P6);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
