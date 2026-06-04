//! Audit: `p2-should-consistent-envelope`.
//!
//! JSON output uses a consistent envelope — the same top-level object shape
//! shows up across success and error paths, so consumers don't have to swap
//! parsers depending on outcome. Two probes:
//!
//! 1. **Success envelope**: `--help --output json` (the universally-safe
//!    JSON-mode invocation).
//! 2. **Error envelope**: bad-flag invocation under `--output json`.
//!
//! Both probes are conditioned on the binary advertising `--output json` in
//! `--help`. When advertised, we compare the top-level key sets. The success
//! envelope is allowed to carry payload-shaped keys (`data`, `results`,
//! `items`, `count`, `total`) that the error envelope won't have; any other
//! top-level key present in success but absent in error counts as drift.
//!
//! Warn (SHOULD) when drift is found; Pass when envelopes share their
//! non-payload key sets; Skip when either probe didn't yield valid JSON
//! (the stricter `p2-must-json-errors` will already Fail in that case).

use std::collections::HashSet;

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::{
    advertises_json_output, probe_bad_invocation_json, probe_help_json,
};
use crate::project::Project;
use crate::runner::RunResult;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Keys that legitimately appear only in the success envelope. The spec
/// frames the envelope as "predictable keys" — payload containers naturally
/// drop out of error responses without violating consistency.
const PAYLOAD_KEYS: &[&str] = &["data", "results", "items", "count", "total"];

pub struct ConsistentEnvelopeAudit;

impl Audit for ConsistentEnvelopeAudit {
    fn id(&self) -> &str {
        "p2-consistent-envelope"
    }

    fn label(&self) -> &'static str {
        "JSON success and error envelopes share their non-payload key set"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-should-consistent-envelope"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) if !advertises_json_output(help) => AuditStatus::Skip(
                "binary does not advertise `--output json` in --help; \
                 envelope-consistency only applies to CLIs that opt into the JSON contract."
                    .into(),
            ),
            Some(_) => {
                let runner = project.runner_ref();
                let success = probe_help_json(runner);
                let error = probe_bad_invocation_json(runner);
                audit_envelope(&success, &error)
            }
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

pub(crate) fn audit_envelope(success: &RunResult, error: &RunResult) -> AuditStatus {
    let success_keys = match top_level_keys(success) {
        Some(keys) => keys,
        None => {
            return AuditStatus::Skip(
                "success-mode probe (`--help --output json`) produced no parseable JSON; \
                 cannot compare envelope shapes."
                    .into(),
            );
        }
    };
    let error_keys = match top_level_keys(error) {
        Some(keys) => keys,
        None => {
            return AuditStatus::Skip(
                "error-mode probe (bad flag + `--output json`) produced no parseable JSON; \
                 the stricter `p2-must-json-errors` audit covers this case."
                    .into(),
            );
        }
    };

    let payload: HashSet<&str> = PAYLOAD_KEYS.iter().copied().collect();
    let drifted: Vec<String> = success_keys
        .difference(&error_keys)
        .filter(|k| !payload.contains(k.as_str()))
        .cloned()
        .collect();

    if drifted.is_empty() {
        AuditStatus::Pass
    } else {
        let mut sorted = drifted;
        sorted.sort();
        AuditStatus::Warn(format!(
            "JSON success envelope carries keys absent from the error envelope: {}. \
             Consistent top-level keys let consumers parse with one shape regardless \
             of outcome.",
            sorted.join(", ")
        ))
    }
}

/// Parse the result's stdout (preferred for success) or stderr (preferred
/// for errors) into a top-level JSON object and return its key set. Returns
/// `None` when no parseable JSON object is found.
fn top_level_keys(result: &RunResult) -> Option<HashSet<String>> {
    for channel in [&result.stdout, &result.stderr] {
        let trimmed = channel.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(obj) = value.as_object()
        {
            return Some(obj.keys().cloned().collect());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunStatus;

    fn ok(stdout: &str, stderr: &str) -> RunResult {
        RunResult {
            exit_code: Some(0),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            status: RunStatus::Ok,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn pass_when_envelopes_match() {
        let success = ok(r#"{"schema_version":"1.0","data":[1,2,3]}"#, "");
        let error = ok("", r#"{"schema_version":"1.0","error":"BadFlag"}"#);
        assert_eq!(audit_envelope(&success, &error), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_only_payload_keys_differ() {
        let success = ok(r#"{"version":"1","data":[],"count":0}"#, "");
        let error = ok("", r#"{"version":"1","error":"X"}"#);
        assert_eq!(audit_envelope(&success, &error), AuditStatus::Pass);
    }

    #[test]
    fn warn_on_drift() {
        let success = ok(r#"{"schema_version":"1","trace_id":"abc","data":[]}"#, "");
        let error = ok("", r#"{"error":"BadFlag"}"#);
        match audit_envelope(&success, &error) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("schema_version"));
                assert!(msg.contains("trace_id"));
                assert!(!msg.contains("data"), "payload keys should be filtered");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_success_not_json() {
        let success = ok("plain text help output", "");
        let error = ok("", r#"{"error":"X"}"#);
        match audit_envelope(&success, &error) {
            AuditStatus::Skip(msg) => assert!(msg.contains("success-mode")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_error_not_json() {
        let success = ok(r#"{"data":[]}"#, "");
        let error = ok("", "error: bad flag");
        match audit_envelope(&success, &error) {
            AuditStatus::Skip(msg) => assert!(msg.contains("error-mode")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
