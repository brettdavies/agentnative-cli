//! Audit: `p4-should-enumerate-valid-set` (Python).
//!
//! Mirrors the Rust counterpart. argparse's `choices=[...]` and click's
//! `click.Choice(...)` both produce error messages that name every valid
//! option. The audit verifies one of these patterns is present; the message
//! shape is then guaranteed by the framework.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Python;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

static CHOICES: LazyLock<Pattern> = LazyLock::new(|| Pattern::new("choices=$$$_", Python));

#[derive(Debug)]
pub struct EnumerateValidSetPythonAudit;

impl Audit for EnumerateValidSetPythonAudit {
    fn id(&self) -> &str {
        "p4-enumerate-valid-set"
    }

    fn label(&self) -> &'static str {
        "Closed-set rejection declares valid choices"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-enumerate-valid-set"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Python)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Python(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let scans = pass::findings(project, Language::Python, Python, self);
        let found = |signal: EnumerateScan| scans.iter().any(|(_, scan)| *scan == signal);
        let found_closed_set = found(EnumerateScan::ClosedSetDeclared);
        let found_framework = found(EnumerateScan::FrameworkWithoutClosedSet);

        let status = if found_closed_set {
            AuditStatus::Pass
        } else if found_framework {
            AuditStatus::Warn(
                "argparse/click detected but no `choices=` / `click.Choice` \
                 declaration found. Closed-set rejection messages should \
                 enumerate the valid choices."
                    .into(),
            )
        } else {
            AuditStatus::Pass
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Python> for EnumerateValidSetPythonAudit {
    type Finding = EnumerateScan;

    fn scan(&self, file: &SourceFile<'_, Python>) -> EnumerateScan {
        audit_enumerate_valid_set_python(file)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnumerateScan {
    ClosedSetDeclared,
    FrameworkWithoutClosedSet,
    NoFramework,
}

/// Inspect a single Python source file. Returns the strongest signal found.
pub(crate) fn audit_enumerate_valid_set_python(file: &SourceFile<'_, Python>) -> EnumerateScan {
    let source = file.source();
    // Closed-set patterns. ast-grep handles the keyword-argument shape better
    // than substring; substring covers `choices=` outside `add_argument` too,
    // which is acceptable false-positive territory.
    if source.contains("click.Choice(") || source.contains("Choice(") && source.contains("click") {
        return EnumerateScan::ClosedSetDeclared;
    }
    // Conservative fallback — any literal `choices=` keyword usage qualifies.
    if source.contains("choices=") {
        return EnumerateScan::ClosedSetDeclared;
    }
    if file.root().find_all(&*CHOICES).next().is_some() {
        return EnumerateScan::ClosedSetDeclared;
    }

    let framework_signals = [
        "argparse",
        "ArgumentParser",
        "import click",
        "@click.command",
        "@click.group",
    ];
    let has_framework = framework_signals.iter().any(|sig| source.contains(sig));

    if has_framework {
        EnumerateScan::FrameworkWithoutClosedSet
    } else {
        EnumerateScan::NoFramework
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_argparse_choices() {
        let source = r#"
import argparse

p = argparse.ArgumentParser()
p.add_argument('--mode', choices=['fast', 'slow'])
"#;
        assert_eq!(
            audit_enumerate_valid_set_python(&pass::python(source, "src/cli.py")),
            EnumerateScan::ClosedSetDeclared
        );
    }

    #[test]
    fn happy_path_click_choice() {
        let source = r#"
import click

@click.command()
@click.option('--mode', type=click.Choice(['fast', 'slow']))
def cli(mode):
    pass
"#;
        assert_eq!(
            audit_enumerate_valid_set_python(&pass::python(source, "src/cli.py")),
            EnumerateScan::ClosedSetDeclared
        );
    }

    #[test]
    fn warn_argparse_without_choices() {
        let source = r#"
import argparse

p = argparse.ArgumentParser()
p.add_argument('--mode', help='operating mode')
"#;
        assert_eq!(
            audit_enumerate_valid_set_python(&pass::python(source, "src/cli.py")),
            EnumerateScan::FrameworkWithoutClosedSet
        );
    }

    #[test]
    fn vacuous_pass_no_framework() {
        let source = r#"
def main():
    print("hello")
"#;
        assert_eq!(
            audit_enumerate_valid_set_python(&pass::python(source, "src/cli.py")),
            EnumerateScan::NoFramework
        );
    }

    #[test]
    fn applicable_for_python() {
        use crate::project::{Language, Project};
        use std::path::PathBuf;
        use std::sync::OnceLock;

        let project = Project {
            path: PathBuf::from("."),
            language: Some(Language::Python),
            binary_paths: vec![],
            manifest_path: None,
            runner: None,
            include_tests: false,
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
            prepared_findings: Default::default(),
            anc_config: Default::default(),
            inventory: Default::default(),
        };
        assert!(EnumerateValidSetPythonAudit.applicable(&project));
    }
}
