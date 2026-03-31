use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct JsonOutputCheck;

impl Check for JsonOutputCheck {
    fn id(&self) -> &str {
        "p2-json-output"
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner.as_ref().unwrap();
        let result = runner.run(&["--help"], &[]);

        let status = match result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", result.stdout, result.stderr);
                let lower = output.to_lowercase();
                if lower.contains("--output") || lower.contains("--format") {
                    CheckStatus::Skip(
                        "--output/--format flag detected; manual verification needed".into(),
                    )
                } else {
                    CheckStatus::Skip("no --output/--format flag detected".into())
                }
            }
            _ => CheckStatus::Skip("could not run --help to detect output flags".into()),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Structured output support".into(),
            group: CheckGroup::P2,
            layer: CheckLayer::Behavioral,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_sh_script;
    use crate::types::CheckStatus;

    #[test]
    fn json_output_detects_format_flag() {
        let project = test_project_with_sh_script("echo '--format json  Choose output format'");
        let result = JsonOutputCheck.run(&project).unwrap();
        match &result.status {
            CheckStatus::Skip(msg) => assert!(msg.contains("manual verification")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn json_output_no_flag() {
        let project = test_project_with_sh_script("echo 'just some help text'");
        let result = JsonOutputCheck.run(&project).unwrap();
        match &result.status {
            CheckStatus::Skip(msg) => assert!(msg.contains("no --output")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
