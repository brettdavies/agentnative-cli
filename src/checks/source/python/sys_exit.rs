//! Check: Detect `sys.exit()` calls outside `if __name__ == "__main__":` guards.
//!
//! Principle: P4 (Actionable Errors) — library code should `raise` or `return`,
//! not call `sys.exit()`. Reserve `sys.exit()` for the entry-point script under
//! the `__main__` guard. Analogous to Rust's `p4-process-exit`.

use ast_grep_core::Node;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_language::Python;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, SourceLocation};

/// Check trait implementation for sys.exit() outside __main__ guard.
pub struct SysExitCheck;

impl Check for SysExitCheck {
    fn id(&self) -> &str {
        "p4-sys-exit"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Python)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            // __main__.py is the Python entry point — sys.exit() is expected there,
            // just as process::exit() is expected in Rust's main.rs.
            if file_str.ends_with("__main__.py") {
                continue;
            }
            let result = check_sys_exit(&parsed_file.source, &file_str);
            if let CheckStatus::Fail(evidence) = result.status {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: "p4-sys-exit".to_string(),
            label: "No sys.exit() outside __main__ guard".to_string(),
            group: CheckGroup::P4,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Scan a Python source string for `sys.exit(...)` calls outside the
/// `if __name__ == "__main__":` guard.
pub(crate) fn check_sys_exit(source: &str, file: &str) -> CheckResult {
    let matches = find_unguarded_sys_exits(source, file);

    let status = if matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Fail(evidence)
    };

    CheckResult {
        id: "p4-sys-exit".to_string(),
        label: "No sys.exit() outside __main__ guard".to_string(),
        group: CheckGroup::P4,
        layer: CheckLayer::Source,
        status,
    }
}

fn find_unguarded_sys_exits(source: &str, file: &str) -> Vec<SourceLocation> {
    let root = Python.ast_grep(source);
    let mut out = Vec::new();
    walk(root.root(), file, false, &mut out);
    out
}

fn walk<'a>(
    node: Node<'a, StrDoc<Python>>,
    file: &str,
    inside_main_guard: bool,
    out: &mut Vec<SourceLocation>,
) {
    // Detect `if __name__ == "__main__":` and treat its body as guarded.
    let entering_guard = node.kind() == "if_statement" && is_main_guard(&node);

    if !inside_main_guard && is_sys_exit_call(&node) {
        let pos = node.start_pos();
        let snippet = node
            .text()
            .lines()
            .next()
            .unwrap_or("sys.exit(...)")
            .trim()
            .to_string();
        out.push(SourceLocation {
            file: file.to_string(),
            line: pos.line() + 1,
            column: pos.column(&node) + 1,
            text: snippet,
        });
    }

    let child_guarded = inside_main_guard || entering_guard;
    for child in node.children() {
        walk(child, file, child_guarded, out);
    }
}

/// Match `sys.exit(...)` call expressions.
///
/// In tree-sitter-python a call like `sys.exit(1)` is a `call` node whose
/// `function` child is an `attribute` node with `object=sys` and `attribute=exit`.
fn is_sys_exit_call<'a>(node: &Node<'a, StrDoc<Python>>) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let Some(func) = node.children().next() else {
        return false;
    };
    if func.kind() != "attribute" {
        return false;
    }
    let text = func.text();
    text == "sys.exit" || text.replace(char::is_whitespace, "") == "sys.exit"
}

/// Detect `if __name__ == "__main__":` guards.
///
/// We accept either ordering of the comparison and either string-quote style.
fn is_main_guard<'a>(node: &Node<'a, StrDoc<Python>>) -> bool {
    // The first child of an if_statement is the `if` keyword; the condition
    // follows. We simplify by scanning the node's text for the canonical
    // condition form on the first line.
    let text = node.text();
    let first_line = text.lines().next().unwrap_or("").trim();
    let header = first_line.trim_end_matches(':').trim();
    matches!(
        header,
        "if __name__ == \"__main__\""
            | "if __name__ == '__main__'"
            | "if \"__main__\" == __name__"
            | "if '__main__' == __name__"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_sys_exit_inside_main_guard() {
        let source = "\
import sys

def main():
    return 0

if __name__ == \"__main__\":
    sys.exit(main())
";
        let result = check_sys_exit(source, "src/cli.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_when_main_guard_uses_single_quotes() {
        let source = "\
import sys

if __name__ == '__main__':
    sys.exit(0)
";
        let result = check_sys_exit(source, "src/cli.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_sys_exit_at_module_level() {
        let source = "\
import sys
sys.exit(1)
";
        let result = check_sys_exit(source, "src/bad.py");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains("sys.exit"));
            assert!(evidence.contains("src/bad.py"));
        }
    }

    #[test]
    fn fail_when_sys_exit_in_function_outside_guard() {
        let source = "\
import sys

def fail_hard(msg):
    print(msg)
    sys.exit(2)

fail_hard('boom')
";
        let result = check_sys_exit(source, "src/lib.py");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn evidence_records_line_number() {
        let source = "\
import sys
print('hi')
sys.exit(7)
";
        let result = check_sys_exit(source, "src/loc.py");
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains(":3:"), "expected line 3, got: {evidence}");
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn ignores_builtin_exit() {
        // `exit()` (the REPL builtin) is intentionally not flagged — it has
        // different semantics and is not what this check targets.
        let source = "\
exit(1)
";
        let result = check_sys_exit(source, "src/builtin.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_unrelated_sys_calls() {
        let source = "\
import sys
sys.stderr.write('hi')
print(sys.argv)
";
        let result = check_sys_exit(source, "src/sys_other.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_when_no_sys_exit_anywhere() {
        let source = "\
def add(a, b):
    return a + b
";
        let result = check_sys_exit(source, "src/clean.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn fail_counts_multiple_unguarded_exits() {
        let source = "\
import sys

def a():
    sys.exit(1)

def b():
    sys.exit(2)
";
        let result = check_sys_exit(source, "src/multi.py");
        if let CheckStatus::Fail(evidence) = &result.status {
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn nested_block_inside_guard_is_still_guarded() {
        // Calls in nested ifs/loops under the __main__ guard are still considered
        // inside the guard (the body of the guard is a CLI entry-point script).
        let source = "\
import sys

if __name__ == \"__main__\":
    try:
        run()
    except RuntimeError:
        sys.exit(1)
";
        let result = check_sys_exit(source, "src/cli.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_sys_exit_in_string() {
        let source = "\
msg = \"call sys.exit(1) on failure\"
print(msg)
";
        let result = check_sys_exit(source, "src/strings.py");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_python() {
        let check = SysExitCheck;
        let dir = std::env::temp_dir().join(format!("anc-sysexit-py-{}", std::process::id()));
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
        let check = SysExitCheck;
        let dir = std::env::temp_dir().join(format!("anc-sysexit-rs-{}", std::process::id()));
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
    fn run_skips_dunder_main_py() {
        let check = SysExitCheck;
        let dir =
            std::env::temp_dir().join(format!("anc-sysexit-skip-main-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("create src dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write pyproject");
        std::fs::write(src.join("__main__.py"), "import sys\nsys.exit(0)\n")
            .expect("write __main__.py");
        let project = Project::discover(&dir).expect("discover");
        let result = check.run(&project).expect("check ran");
        assert_eq!(result.status, CheckStatus::Pass);
    }
}
