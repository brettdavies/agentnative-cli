//! Audit: `p4-should-json-error-output`.
//!
//! Errors must respect `--output json`: when JSON output is selected, error
//! messages go to stderr (or stdout, since some CLIs diverge there) as JSON.
//! Weaker than `p2-must-json-errors` — this SHOULD only requires the output
//! parse as JSON; it does not require the spec's `error`/`kind`/`message`
//! triple, since the stricter MUST already covers that.
//!
//! Applicability gate: vacuous Skip when the top-level `--help` declares no
//! JSON mode (`output_flags::json_mode`). Pass when stderr (or stdout) parses
//! as JSON under the declared mode; Warn when JSON mode is opted into but the
//! error comes out as plain text.

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::{
    NO_JSON_MODE, parse_error_envelope, probe_bad_invocation_json, shown,
};
use crate::audits::behavioral::output_flags;
use crate::project::Project;
use crate::runner::RunResult;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct JsonErrorOutputAudit;

impl Audit for JsonErrorOutputAudit {
    fn id(&self) -> &str {
        "p4-json-error-output"
    }

    fn label(&self) -> &'static str {
        "`--output json` produces JSON-formatted errors"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-json-error-output"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => match output_flags::json_mode(help) {
                None => AuditStatus::Skip(format!(
                    "{NO_JSON_MODE}; SHOULD applies only to CLIs that opt into the JSON contract."
                )),
                Some(mode) => {
                    let result = probe_bad_invocation_json(project.runner_ref(), &mode);
                    classify_json_error_output(&result, &shown(&mode))
                }
            },
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

pub(crate) fn classify_json_error_output(result: &RunResult, mode: &str) -> AuditStatus {
    if parse_error_envelope(result).is_some() {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "errors under `{mode}` are not JSON-formatted. Consumers parsing stdout-as-JSON \
             cannot recover the failure without a separate text-parsing path; switch the error \
             writer to honor the active output mode."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunStatus;

    fn fake_result(stderr: &str, stdout: &str) -> RunResult {
        RunResult {
            exit_code: Some(2),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            status: RunStatus::Ok,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn pass_on_json_stderr() {
        let stderr = r#"{"error":"BadFlag"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(stderr, ""), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_on_json_stdout() {
        let stdout = r#"{"error":"BadFlag"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result("", stdout), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_even_without_spec_keys() {
        // SHOULD is weaker than the MUST — any parseable JSON passes here.
        let stderr = r#"{"reason":"x"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(stderr, ""), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_on_plain_text() {
        let stderr = "error: bad flag\nFor more info, try --help.";
        match classify_json_error_output(&fake_result(stderr, ""), "--output json") {
            AuditStatus::Warn(msg) => assert!(msg.contains("not JSON-formatted")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
