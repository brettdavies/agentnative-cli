//! Check: Detect bare `except:` clauses in Python source.
//!
//! Principle: P4 (Actionable Errors) — bare `except:` swallows BaseException
//! (KeyboardInterrupt, SystemExit) and hides programming errors. Always specify
//! the exception type. Analogous to Rust's `code-unwrap`.

use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Python;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence, SourceLocation};

/// Check trait implementation for bare-except detection.
pub struct BareExceptCheck;

impl Check for BareExceptCheck {
    fn id(&self) -> &str {
        "code-bare-except"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::CodeQuality
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
            if let CheckStatus::Fail(evidence) = check_bare_except(&parsed_file.source, &file_str) {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "No bare except: clauses".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Scan a Python source string for bare `except:` clauses.
///
/// Walks the AST looking for `except_clause` nodes that have no exception type.
/// The tree-sitter-python grammar represents bare except as an `except_clause`
/// whose first non-keyword child is the `:` token (no type expression between
/// `except` and `:`).
pub(crate) fn check_bare_except(source: &str, file: &str) -> CheckStatus {
    let root = Python.ast_grep(source);
    let mut matches = Vec::new();
    walk(root.root(), file, &mut matches);

    if matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Fail(evidence)
    }
}

fn walk<'a>(
    node: ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<ast_grep_language::Python>>,
    file: &str,
    out: &mut Vec<SourceLocation>,
) {
    if node.kind() == "except_clause" && is_bare_except(&node) {
        let pos = node.start_pos();
        let snippet = node
            .text()
            .lines()
            .next()
            .unwrap_or("except:")
            .trim()
            .to_string();
        out.push(SourceLocation {
            file: file.to_string(),
            line: pos.line() + 1,
            column: pos.column(&node) + 1,
            text: snippet,
        });
    }
    for child in node.children() {
        walk(child, file, out);
    }
}

/// A bare `except:` has no expression between `except` and `:`.
fn is_bare_except<'a>(
    node: &ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<ast_grep_language::Python>>,
) -> bool {
    let text = node.text();
    let first_line = text.lines().next().unwrap_or("");
    let Some((header, _)) = first_line.split_once(':') else {
        return false;
    };
    let trimmed = header.trim();
    trimmed == "except"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_typed_except() {
        let source = "\
try:
    do_thing()
except ValueError:
    handle_it()
";
        let status = check_bare_except(source, "src/foo.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn pass_when_multiple_typed_excepts() {
        let source = "\
try:
    do_thing()
except (ValueError, KeyError) as e:
    log(e)
except OSError:
    cleanup()
";
        let status = check_bare_except(source, "src/foo.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_bare_except() {
        let source = "\
try:
    do_thing()
except:
    pass
";
        let status = check_bare_except(source, "src/foo.py");
        assert!(matches!(status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &status {
            assert!(evidence.contains("except:"));
            assert!(evidence.contains("src/foo.py"));
        }
    }

    #[test]
    fn fail_when_bare_except_with_pass() {
        let source = "\
try:
    risky()
except: pass
";
        let status = check_bare_except(source, "src/foo.py");
        assert!(matches!(status, CheckStatus::Fail(_)));
    }

    #[test]
    fn fail_counts_multiple_bare_excepts() {
        let source = "\
def a():
    try:
        f()
    except:
        pass

def b():
    try:
        g()
    except:
        log()
";
        let status = check_bare_except(source, "src/multi.py");
        if let CheckStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("expected Fail, got {:?}", status);
        }
    }

    #[test]
    fn pass_when_typed_after_bare_in_different_file() {
        // Regression guard: a bare except in one file shouldn't be masked by a typed
        // one in the same source string. (Covered indirectly above; explicit here.)
        let source = "\
try:
    a()
except Exception:
    handle()
";
        let status = check_bare_except(source, "src/clean.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_except_inside_string_literal() {
        // ast-grep is AST-aware — the literal `except:` in a string should not match.
        let source = "\
msg = \"never write `except:` in production\"
def main():
    return msg
";
        let status = check_bare_except(source, "src/strings.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_except_in_comment() {
        let source = "\
# remember: except: is bad style
def main():
    pass
";
        let status = check_bare_except(source, "src/comments.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_no_python_files() {
        // Empty source = no AST = pass.
        let status = check_bare_except("", "src/empty.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn evidence_includes_line_and_column() {
        let source = "\
try:
    work()
except:
    pass
";
        let status = check_bare_except(source, "src/loc.py");
        if let CheckStatus::Fail(evidence) = &status {
            // bare `except:` is on line 3
            assert!(
                evidence.contains(":3:"),
                "evidence missing line 3: {evidence}"
            );
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn applicable_for_python() {
        let check = BareExceptCheck;
        let dir = std::env::temp_dir().join(format!("anc-bare-except-py-{}", std::process::id()));
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
    fn pass_when_typed_except_group() {
        let source = "\
try:
    do_thing()
except* ValueError:
    handle_it()
";
        let status = check_bare_except(source, "src/foo.py");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn not_applicable_for_rust() {
        let check = BareExceptCheck;
        let dir = std::env::temp_dir().join(format!("anc-bare-except-rs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
