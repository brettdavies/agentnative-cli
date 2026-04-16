//! Check: Detect NO_COLOR environment variable handling in Python source.
//!
//! Principle: P6 (Composable Structure) — CLIs must respect NO_COLOR.
//! See https://no-color.org/
//!
//! The behavioral check is the primary gate; this source check returns Warn
//! (not Fail) when NO_COLOR is absent — many libraries (rich, click, colorama)
//! handle NO_COLOR transparently without explicit lookups in user code.

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Python;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern_in;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Check trait implementation for NO_COLOR detection in Python.
pub struct NoColorPythonCheck;

impl Check for NoColorPythonCheck {
    fn id(&self) -> &str {
        "p6-no-color"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Python)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut found_any = false;

        for (_path, parsed_file) in parsed.iter() {
            if source_handles_no_color(&parsed_file.source) {
                found_any = true;
                break;
            }
        }

        let status = if found_any {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "No reference to NO_COLOR found in any Python source file. CLIs should respect \
                 the NO_COLOR convention. Many libraries (rich, click, colorama) handle this \
                 transparently — verify via the behavioral check. See https://no-color.org/"
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: "p6-no-color".to_string(),
            label: "Respects NO_COLOR".to_string(),
            group: CheckGroup::P6,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Returns true if the source references NO_COLOR via a recognized Python
/// env-var access pattern, or as a string literal anywhere in the AST.
pub(crate) fn source_handles_no_color(source: &str) -> bool {
    // Common explicit access patterns.
    let access_patterns = [
        r#"os.environ.get("NO_COLOR")"#,
        r#"os.environ.get('NO_COLOR')"#,
        r#"os.getenv("NO_COLOR")"#,
        r#"os.getenv('NO_COLOR')"#,
        r#"os.environ["NO_COLOR"]"#,
        r#"os.environ['NO_COLOR']"#,
        r#"environ.get("NO_COLOR")"#,
        r#"environ.get('NO_COLOR')"#,
        r#"getenv("NO_COLOR")"#,
        r#"getenv('NO_COLOR')"#,
    ];
    for p in access_patterns {
        if has_pattern_in(source, p, Language::Python) {
            return true;
        }
    }

    // Fallback: any string literal "NO_COLOR" or 'NO_COLOR' in the AST.
    has_string_literal(source, "NO_COLOR")
}

/// Match `"NO_COLOR"` or `'NO_COLOR'` as a string literal anywhere in the AST.
fn has_string_literal(source: &str, needle: &str) -> bool {
    let root = Python.ast_grep(source);
    let pat_double = Pattern::try_new(&format!(r#""{needle}""#), Python).ok();
    let pat_single = Pattern::try_new(&format!(r#"'{needle}'"#), Python).ok();

    if let Some(p) = &pat_double {
        if root.root().find(p).is_some() {
            return true;
        }
    }
    if let Some(p) = &pat_single {
        if root.root().find(p).is_some() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_os_environ_get() {
        let source = "\
import os

def setup():
    if os.environ.get(\"NO_COLOR\"):
        disable_color()
";
        assert!(source_handles_no_color(source));
    }

    #[test]
    fn pass_with_os_getenv() {
        let source = "\
import os

if os.getenv('NO_COLOR') is not None:
    disable_color()
";
        assert!(source_handles_no_color(source));
    }

    #[test]
    fn pass_with_subscript_access() {
        let source = "\
import os
val = os.environ[\"NO_COLOR\"]
";
        assert!(source_handles_no_color(source));
    }

    #[test]
    fn pass_with_imported_environ() {
        let source = "\
from os import environ
val = environ.get('NO_COLOR')
";
        assert!(source_handles_no_color(source));
    }

    #[test]
    fn pass_with_string_literal_constant() {
        // Recognized via the string-literal fallback.
        let source = "\
NO_COLOR_ENV = \"NO_COLOR\"

def disable():
    import os
    return os.environ.get(NO_COLOR_ENV)
";
        assert!(source_handles_no_color(source));
    }

    #[test]
    fn warn_when_no_no_color_anywhere() {
        let source = "\
def main():
    print(\"hello\")
";
        assert!(!source_handles_no_color(source));
    }

    #[test]
    fn ignored_in_comments() {
        // tree-sitter-python doesn't parse comment text as string literals.
        let source = "\
# remember NO_COLOR support
def main():
    print('hi')
";
        assert!(!source_handles_no_color(source));
    }

    #[test]
    fn applicable_for_python() {
        let check = NoColorPythonCheck;
        let dir = std::env::temp_dir().join(format!("anc-nocolor-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(check.applicable(&project));
    }

    #[test]
    fn not_applicable_for_rust() {
        let check = NoColorPythonCheck;
        let dir = std::env::temp_dir().join(format!("anc-nocolor-py-rs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }

    #[test]
    fn run_emits_warn_status_when_missing() {
        let check = NoColorPythonCheck;
        let dir =
            std::env::temp_dir().join(format!("anc-nocolor-warn-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write pyproject");
        std::fs::write(dir.join("src/app.py"), "def main():\n    print('hi')\n")
            .expect("write app.py");
        let project = Project::discover(&dir).expect("discover");
        let result = check.run(&project).expect("check ran");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }
}
