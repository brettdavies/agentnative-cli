//! Check: `p4-should-enumerate-valid-set` (Python).
//!
//! Mirrors the Rust counterpart. argparse's `choices=[...]` and click's
//! `click.Choice(...)` both produce error messages that name every valid
//! option. The check verifies one of these patterns is present; the message
//! shape is then guaranteed by the framework.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern_in;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct EnumerateValidSetPythonCheck;

impl Check for EnumerateValidSetPythonCheck {
    fn id(&self) -> &str {
        "p4-enumerate-valid-set"
    }

    fn label(&self) -> &'static str {
        "Closed-set rejection declares valid choices"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-enumerate-valid-set"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Python)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut found_closed_set = false;
        let mut found_framework = false;

        for (_path, parsed_file) in parsed.iter() {
            match check_enumerate_valid_set_python(&parsed_file.source) {
                EnumerateScan::ClosedSetDeclared => {
                    found_closed_set = true;
                    break;
                }
                EnumerateScan::FrameworkWithoutClosedSet => found_framework = true,
                EnumerateScan::NoFramework => {}
            }
        }

        let status = if found_closed_set {
            CheckStatus::Pass
        } else if found_framework {
            CheckStatus::Warn(
                "argparse/click detected but no `choices=` / `click.Choice` \
                 declaration found. Closed-set rejection messages should \
                 enumerate the valid choices."
                    .into(),
            )
        } else {
            CheckStatus::Pass
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnumerateScan {
    ClosedSetDeclared,
    FrameworkWithoutClosedSet,
    NoFramework,
}

/// Inspect a single Python source file. Returns the strongest signal found.
pub(crate) fn check_enumerate_valid_set_python(source: &str) -> EnumerateScan {
    // Closed-set patterns. ast-grep handles the keyword-argument shape better
    // than substring; substring covers `choices=` outside `add_argument` too,
    // which is acceptable false-positive territory.
    if has_pattern_in(source, "choices=$$$_", Language::Python) {
        return EnumerateScan::ClosedSetDeclared;
    }
    if source.contains("click.Choice(") || source.contains("Choice(") && source.contains("click") {
        return EnumerateScan::ClosedSetDeclared;
    }
    // Conservative fallback — any literal `choices=` keyword usage qualifies.
    if source.contains("choices=") {
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
            check_enumerate_valid_set_python(source),
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
            check_enumerate_valid_set_python(source),
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
            check_enumerate_valid_set_python(source),
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
            check_enumerate_valid_set_python(source),
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
        };
        assert!(EnumerateValidSetPythonCheck.applicable(&project));
    }
}
