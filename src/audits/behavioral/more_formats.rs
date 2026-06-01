//! Audit: `p2-may-more-formats`.
//!
//! `--output` accepts formats beyond the core `text` / `json` / `jsonl`
//! (CSV, TSV, YAML are the spec-listed examples). MAY-tier; vacuous Skip
//! when no `--output` flag exists, Warn when only the core formats are
//! advertised, Pass when any additional format token shows up in `--help`.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Format tokens beyond the core trio that satisfy this MAY. Spec-listed
/// examples plus a few common neighbors agents may want to consume.
const EXTRA_FORMATS: &[&str] = &["csv", "tsv", "yaml", "yml", "toml", "xml", "ndjson"];

pub struct MoreFormatsAudit;

impl Audit for MoreFormatsAudit {
    fn id(&self) -> &str {
        "p2-more-formats"
    }

    fn label(&self) -> &'static str {
        "`--output` advertises additional formats beyond text/json"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-may-more-formats"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_more_formats(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

pub(crate) fn audit_more_formats(help: &HelpOutput) -> AuditStatus {
    let has_output_flag = help
        .flags()
        .iter()
        .any(|f| f.matches("--output") || f.matches("--format"));
    if !has_output_flag {
        return AuditStatus::Skip(
            "no `--output` or `--format` flag advertised; vacuous skip for MAY-tier extra formats."
                .into(),
        );
    }

    // Scan the help raw text for any of the extra-format tokens. Tokens
    // typically appear inside `[possible values: text, json, csv]` or
    // similar phrasing emitted by clap and other parsers.
    let raw_lower = help.raw().to_lowercase();
    let found: Vec<&&str> = EXTRA_FORMATS
        .iter()
        .filter(|fmt| raw_lower.contains(*fmt.to_owned()))
        .collect();

    if found.is_empty() {
        AuditStatus::Warn(format!(
            "`--output` / `--format` flag found but no extra format tokens detected \
             (looked for {}). MAY-tier — additional formats let downstream agents \
             pick the shape that matches their pipeline.",
            EXTRA_FORMATS.join(", "),
        ))
    } else {
        AuditStatus::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_output_flag() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match audit_more_formats(&help) {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_only_core_formats() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>  [possible values: text, json]\n  -h, --help\n",
        );
        match audit_more_formats(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("extra format")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_yaml_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>  [possible values: text, json, yaml]\n  -h, --help\n",
        );
        assert_eq!(audit_more_formats(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_csv_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>  [possible values: text, json, csv]\n  -h, --help\n",
        );
        assert_eq!(audit_more_formats(&help), AuditStatus::Pass);
    }
}
