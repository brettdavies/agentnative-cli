//! Audit: Detect prompt-library calls that no stdin TTY check guards.
//!
//! Principle: P1 (Non-Interactive by Default) SHOULD: auto-detect a
//! non-interactive context through a TTY check on stdin, and suppress prompts
//! when no terminal is present.
//!
//! This is a conditional audit:
//!   Trigger: a call into dialoguer, inquire, rpassword, or rustyline
//!   Pass: a stdin TTY check guards every such call
//!   Warn: a call has no such guard; a check on stdout or stderr alone is none
//!   N/A: no such call
//!
//! The check and the prompt often sit in different functions, so a guard is
//! resolved across the project's functions, matched by name. A call is guarded
//! when its function holds the check or calls a helper that holds it, or when
//! its function is reached from inside an `if` whose condition does. A check
//! elsewhere on the call path guards nothing: a tool can ask whether stdin is a
//! terminal to decide where its input comes from, then prompt regardless.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use ast_grep_core::Node;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::evidence_preview;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, SourceLocation};

use super::unwrap::attribute_text_is_cfg_test;

struct PromptLibrary {
    name: &'static str,
    import: &'static str,
    /// The types a prompt is built from and the functions that prompt.
    items: &'static [&'static str],
}

const PROMPT_LIBRARIES: &[PromptLibrary] = &[
    PromptLibrary {
        name: "dialoguer",
        import: "use dialoguer",
        items: &[
            "Confirm",
            "Editor",
            "FuzzySelect",
            "Input",
            "MultiSelect",
            "Password",
            "Select",
            "Sort",
        ],
    },
    PromptLibrary {
        name: "inquire",
        import: "use inquire",
        items: &[
            "Confirm",
            "CustomType",
            "DateSelect",
            "Editor",
            "MultiSelect",
            "Password",
            "Select",
            "Text",
            "prompt_confirmation",
            "prompt_date",
            "prompt_f32",
            "prompt_f64",
            "prompt_secret",
            "prompt_text",
            "prompt_u32",
            "prompt_u64",
            "prompt_usize",
        ],
    },
    PromptLibrary {
        name: "rpassword",
        import: "use rpassword",
        items: &["prompt_password", "read_password"],
    },
    PromptLibrary {
        name: "rustyline",
        import: "use rustyline",
        items: &["DefaultEditor", "Editor"],
    },
];

/// Method calls that ask whether stdin is a terminal, by the end of their
/// callee with whitespace removed.
const STDIN_METHOD_CHECKS: &[&str] = &[
    "stdin().is_terminal",
    "stdin.is_terminal",
    "stdin().lock().is_terminal",
    "stdin.lock().is_terminal",
    "stdin().is_tty",
    "stdin.is_tty",
];

/// Free functions that take the stream to check as an argument.
const STREAM_ARGUMENT_CHECKS: &[&str] = &["is_terminal", "atty::is", "isatty"];

/// Stands in a function's calls for a stdin check, under a name no function has.
const STDIN_CHECK: &str = "<stdin check>";

const NO_PROMPT: &str = "No call into a prompt library (dialoguer, inquire, rpassword, rustyline) \
                         found, so there is no prompt to suppress.";

const UNGUARDED: &str = "Prompt calls with no stdin TTY check guarding them. Check \
                         `std::io::stdin().is_terminal()` in the function that prompts, or gate \
                         the call to it; a check on stdout or stderr alone does not tell a \
                         prompt whether input can arrive:";

/// Audit trait implementation for prompt-call guards.
#[derive(Debug)]
pub struct PromptTtyAudit;

impl Audit for PromptTtyAudit {
    fn id(&self) -> &str {
        "p1-prompt-tty-source"
    }

    fn label(&self) -> &'static str {
        "Prompt calls guarded by a stdin TTY check"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-should-tty-detection"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let functions: Vec<FunctionFacts> = pass::findings(project, Language::Rust, Rust, self)
            .into_iter()
            .flat_map(|(_, functions)| functions)
            .collect();

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: verdict(&functions),
            // Guards resolve by function name, not by data flow.
            confidence: Confidence::Medium,
            mitigation: None,
            config_hint: None,
            row_statuses: Vec::new(),
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for PromptTtyAudit {
    type Finding = Vec<FunctionFacts>;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> Vec<FunctionFacts> {
        function_facts(file)
    }
}

/// What one function shows about prompts and stdin checks. `calls` lists each
/// callee by name, in source order, with `STDIN_CHECK` standing for a stdin
/// check.
#[derive(Debug, Default)]
pub struct FunctionFacts {
    name: String,
    calls: Vec<String>,
    gates: Vec<Gate>,
    prompts: Vec<SourceLocation>,
}

/// An `if` in a function, as ranges of the function's `calls`: those in its
/// condition, and every call in the whole expression.
#[derive(Debug)]
struct Gate {
    condition: Range<usize>,
    calls: Range<usize>,
}

fn verdict(functions: &[FunctionFacts]) -> AuditStatus {
    if functions.iter().all(|function| function.prompts.is_empty()) {
        return AuditStatus::NotApplicable(NO_PROMPT.to_string());
    }
    let guarded = guarded_functions(functions);
    let unguarded: Vec<String> = functions
        .iter()
        .filter(|function| !guarded.contains(function.name.as_str()))
        .flat_map(|function| &function.prompts)
        .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
        .collect();
    if unguarded.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!("{UNGUARDED}\n{}", unguarded.join("\n")))
    }
}

/// The names of the functions a stdin check guards: those that hold one, those
/// that call a function that holds one, and everything called from inside an
/// `if` whose condition does either.
fn guarded_functions(functions: &[FunctionFacts]) -> HashSet<&str> {
    let holds_check = |function: &FunctionFacts| function.calls.iter().any(|c| c == STDIN_CHECK);
    let asking: HashSet<&str> = functions
        .iter()
        .filter(|function| holds_check(function))
        .map(|function| function.name.as_str())
        .chain([STDIN_CHECK])
        .collect();
    let asks = |calls: &[String]| calls.iter().any(|call| asking.contains(call.as_str()));

    let mut guarded: HashSet<&str> = functions
        .iter()
        .filter(|function| asks(&function.calls))
        .map(|function| function.name.as_str())
        .collect();

    let mut callees: HashMap<&str, Vec<&str>> = HashMap::new();
    for function in functions {
        callees
            .entry(function.name.as_str())
            .or_default()
            .extend(function.calls.iter().map(String::as_str));
    }
    let mut frontier: Vec<&str> = functions
        .iter()
        .flat_map(|function| {
            function
                .gates
                .iter()
                .filter(|gate| asks(&function.calls[gate.condition.clone()]))
                .flat_map(|gate| &function.calls[gate.calls.clone()])
        })
        .map(String::as_str)
        .collect();
    let mut reached: HashSet<&str> = HashSet::new();
    while let Some(name) = frontier.pop() {
        if reached.insert(name)
            && let Some(called) = callees.get(name)
        {
            frontier.extend(called);
        }
    }
    guarded.extend(reached);
    guarded
}

fn function_facts(file: &SourceFile<'_, Rust>) -> Vec<FunctionFacts> {
    let imported: Vec<&str> = PROMPT_LIBRARIES
        .iter()
        .filter(|library| file.source().contains(library.import))
        .map(|library| library.name)
        .collect();
    let mut scan = FileScan {
        label: file.label(),
        include_tests: file.include_tests(),
        imported,
        functions: Vec::new(),
    };
    scan.visit(file.root(), None);
    scan.functions
}

struct FileScan<'i> {
    label: String,
    include_tests: bool,
    imported: Vec<&'i str>,
    functions: Vec<FunctionFacts>,
}

impl FileScan<'_> {
    fn visit(&mut self, node: Node<'_, StrDoc<Rust>>, enclosing: Option<usize>) {
        let kind = node.kind();
        let enclosing = if kind == "function_item" {
            self.functions.push(FunctionFacts {
                name: node
                    .field("name")
                    .map(|name| name.text().to_string())
                    .unwrap_or_default(),
                ..FunctionFacts::default()
            });
            Some(self.functions.len() - 1)
        } else {
            enclosing
        };
        if let Some(index) = enclosing {
            match kind.as_ref() {
                "call_expression" => self.record_call(&node, index),
                "macro_invocation" if macro_checks_stdin(&node) => {
                    self.functions[index].calls.push(STDIN_CHECK.to_string());
                }
                // A macro's arguments parse as flat tokens, so a helper called
                // inside `ensure!(is_interactive(), ..)` arrives as an
                // identifier, not a call.
                "identifier" if parent_is_token_tree(&node) => {
                    self.functions[index].calls.push(node.text().to_string());
                }
                _ => {}
            }
        }
        let condition = (kind == "if_expression")
            .then(|| node.field("condition").map(|condition| condition.range()))
            .flatten();
        let first_call = enclosing.map_or(0, |index| self.functions[index].calls.len());
        let mut condition_end = first_call;
        for child in node.children() {
            let is_condition = condition.as_ref() == Some(&child.range());
            self.visit(child, enclosing);
            if is_condition && let Some(index) = enclosing {
                condition_end = self.functions[index].calls.len();
            }
        }
        if condition_end > first_call
            && let Some(index) = enclosing
        {
            let facts = &mut self.functions[index];
            facts.gates.push(Gate {
                condition: first_call..condition_end,
                calls: first_call..facts.calls.len(),
            });
        }
    }

    fn record_call(&mut self, node: &Node<'_, StrDoc<Rust>>, index: usize) {
        let Some(function) = node.field("function") else {
            return;
        };
        let callee = compact(&function.text());
        let arguments = node
            .field("arguments")
            .map(|arguments| compact(&arguments.text()))
            .unwrap_or_default();
        let facts = &mut self.functions[index];
        if checks_stdin(&callee, &arguments) {
            facts.calls.push(STDIN_CHECK.to_string());
        }
        facts.calls.push(callee_name(&callee).to_string());

        // In `Confirm::new().interact()` only the inner `Confirm::new()` names
        // the library; the method calls chained on it are field expressions.
        if function.kind() != "field_expression"
            && is_prompt_call(&callee, &self.imported)
            && (self.include_tests || !is_test_gated(node))
        {
            let pos = node.start_pos();
            let location = SourceLocation {
                file: self.label.clone(),
                line: pos.line() + 1,
                column: pos.column(node) + 1,
                text: evidence_preview(&node.text(), 60),
            };
            self.functions[index].prompts.push(location);
        }
    }
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

fn checks_stdin(callee: &str, arguments: &str) -> bool {
    STDIN_METHOD_CHECKS
        .iter()
        .any(|check| callee.ends_with(check))
        || (STREAM_ARGUMENT_CHECKS
            .iter()
            .any(|check| callee.ends_with(check))
            && (arguments == "(0)"
                || ["stdin", "Stdin", "STDIN"]
                    .iter()
                    .any(|stream| arguments.contains(stream))))
}

fn macro_checks_stdin(node: &Node<'_, StrDoc<Rust>>) -> bool {
    let text = node.text();
    text.contains("stdin") && {
        let text = compact(&text);
        STDIN_METHOD_CHECKS
            .iter()
            .any(|check| text.contains(&format!("{check}(")))
    }
}

fn parent_is_token_tree(node: &Node<'_, StrDoc<Rust>>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "token_tree")
}

/// The last path segment of a callee: `run` for `self.run` and
/// `crate::cli::run::<T>`.
fn callee_name(callee: &str) -> &str {
    let path = callee.split("::<").next().unwrap_or(callee);
    path.rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or(path)
}

/// Whether `callee` names a prompt library's item: through the library's path,
/// or bare in a file that imports the library.
fn is_prompt_call(callee: &str, imported: &[&str]) -> bool {
    let callee = callee.trim_start_matches("::");
    PROMPT_LIBRARIES.iter().any(|library| {
        let qualified = callee
            .strip_prefix(library.name)
            .and_then(|rest| rest.strip_prefix("::"));
        let item = match qualified {
            Some(item) => item,
            None if imported.contains(&library.name) => callee,
            None => return false,
        };
        item.split("::")
            .next()
            .is_some_and(|first| library.items.contains(&first))
    })
}

/// Whether `node` sits inside an item a `#[cfg(test)]` attribute gates.
fn is_test_gated(node: &Node<'_, StrDoc<Rust>>) -> bool {
    node.ancestors().any(|item| {
        item.prev_all()
            .take_while(|sibling| {
                matches!(
                    sibling.kind().as_ref(),
                    "attribute_item" | "line_comment" | "block_comment"
                )
            })
            .any(|sibling| attribute_text_is_cfg_test(sibling.text().as_ref()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(source: &str) -> AuditStatus {
        verdict(&function_facts(&pass::rust(source, "src/main.rs")))
    }

    fn status_across(files: &[(&'static str, &str)]) -> AuditStatus {
        let functions: Vec<FunctionFacts> = files
            .iter()
            .flat_map(|(file, source)| function_facts(&pass::rust(source, file)))
            .collect();
        verdict(&functions)
    }

    #[test]
    fn a_call_inside_a_stdin_check_passes() {
        let source = r#"
use dialoguer::Confirm;
use std::io::IsTerminal;
fn main() {
    if std::io::stdin().is_terminal() {
        Confirm::new().with_prompt("Proceed?").interact();
    }
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_call_after_an_early_return_on_stdin_passes() {
        let source = r#"
fn confirm() -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    dialoguer::Confirm::new().interact()
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_call_guarded_only_by_stdout_warns_and_names_the_call() {
        let source = r#"
use dialoguer::Confirm;
fn main() {
    if std::io::stdout().is_terminal() {
        Confirm::new().interact();
    }
}
"#;
        match status(source) {
            AuditStatus::Warn(evidence) => {
                assert!(
                    evidence.contains("src/main.rs:5:9 — Confirm::new()"),
                    "{evidence}"
                );
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_call_guarded_only_by_stderr_warns() {
        let source = r#"
fn ask() -> String {
    if std::io::stderr().is_terminal() {
        return rpassword::prompt_password("Password: ");
    }
    String::new()
}
"#;
        assert!(matches!(status(source), AuditStatus::Warn(_)));
    }

    #[test]
    fn a_stdout_check_beside_a_stdin_check_still_passes() {
        let source = r#"
fn gate() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}
fn ask() {
    if gate() {
        inquire::Confirm::new("Proceed?").prompt();
    }
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn colored_output_alone_does_not_trigger() {
        let source = r#"
use colored::Colorize;
fn main() {
    println!("{}", "hi".green());
}
"#;
        assert!(
            matches!(status(source), AuditStatus::NotApplicable(_)),
            "{:?}",
            status(source)
        );
    }

    #[test]
    fn a_stdin_read_with_no_library_does_not_trigger() {
        let source = r#"
fn main() {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line);
}
"#;
        assert!(matches!(status(source), AuditStatus::NotApplicable(_)));
    }

    #[test]
    fn a_type_named_like_a_prompt_outside_the_library_does_not_trigger() {
        let source = r#"
use crate::models::Password;
fn main() {
    Password::new("hunter2");
}
"#;
        assert!(matches!(status(source), AuditStatus::NotApplicable(_)));
    }

    #[test]
    fn every_library_and_call_shape_triggers() {
        for call in [
            "dialoguer::Confirm::new().interact()",
            "dialoguer::Input::<String>::new().interact_text()",
            "dialoguer::Select::with_theme(&theme).interact()",
            "inquire::Text::new(\"Name?\").prompt()",
            "inquire::prompt_confirmation(\"Sure?\")",
            "rpassword::prompt_password(\"Password: \")",
            "rpassword::read_password()",
            "rustyline::DefaultEditor::new()",
            "rustyline::Editor::<()>::new()",
        ] {
            let source = format!("fn ask() {{\n    {call};\n}}\n");
            assert!(
                matches!(status(&source), AuditStatus::Warn(_)),
                "{call}: {:?}",
                status(&source)
            );
        }
    }

    #[test]
    fn a_bare_call_counts_in_a_file_that_imports_the_library() {
        let source = r#"
use rpassword::prompt_password;
fn ask() {
    prompt_password("Password: ");
}
"#;
        assert!(matches!(status(source), AuditStatus::Warn(_)));
    }

    #[test]
    fn a_helper_that_checks_stdin_guards_its_caller() {
        let source = r#"
use dialoguer::Confirm;
fn is_interactive() -> bool {
    std::io::stdin().is_terminal()
}
fn ask() {
    if is_interactive() {
        Confirm::new().interact();
    }
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_method_helper_in_another_file_guards_its_caller() {
        let output = r#"
impl Output {
    pub fn is_interactive_terminal(&self) -> bool {
        !self.no_interactive && std::io::stdin().is_terminal()
    }
}
"#;
        let command = r#"
use dialoguer::Select;
fn choose(out: &Output) {
    if out.is_interactive_terminal() {
        Select::new().interact();
    }
}
"#;
        assert_eq!(
            status_across(&[("src/output.rs", output), ("src/command.rs", command)]),
            AuditStatus::Pass
        );
    }

    #[test]
    fn a_caller_that_checks_stdin_guards_the_prompting_function() {
        let source = r#"
use dialoguer::Input;
fn prompt_name() -> String {
    Input::new().interact_text()
}
fn name() -> String {
    if std::io::stdin().is_terminal() { prompt_name() } else { String::new() }
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_gate_guards_everything_called_under_it() {
        let source = r#"
use dialoguer::Confirm;
fn main() {
    if std::io::stdin().is_terminal() {
        run();
    }
}
fn run() {
    delete();
}
fn delete() {
    Confirm::new().interact();
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_gate_on_a_helper_guards_the_function_it_calls() {
        let source = r#"
use dialoguer::Confirm;
impl Output {
    fn is_interactive_terminal(&self) -> bool {
        std::io::stdin().is_terminal()
    }
}
fn prompt_user() -> bool {
    Confirm::new().interact()
}
fn delete(out: &Output) -> bool {
    if out.is_interactive_terminal() { prompt_user() } else { false }
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    /// xh's shape: `run` asks about stdin to decide whether to read a
    /// request body from it, and `parse_auth` prompts on a path `run` reaches.
    #[test]
    fn a_check_elsewhere_on_the_call_path_guards_nothing() {
        let source = r#"
fn run(args: Cli) {
    let use_stdin = !(args.ignore_stdin || io::stdin().is_terminal());
    if let Some(auth) = args.auth {
        parse_auth(&auth);
    }
}
fn parse_auth(auth: &str) -> String {
    rpassword::prompt_password("password: ")
}
"#;
        match status(source) {
            AuditStatus::Warn(evidence) => {
                assert!(evidence.contains("prompt_password"), "{evidence}")
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_macro_holding_the_check_guards_the_call() {
        let source = r#"
fn ask() {
    anyhow::ensure!(std::io::stdin().is_terminal(), "needs a terminal");
    dialoguer::Confirm::new().interact();
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_helper_called_inside_a_macro_guards_the_call() {
        let source = r#"
fn is_interactive() -> bool {
    std::io::stdin().is_terminal()
}
fn ask() {
    anyhow::ensure!(is_interactive(), "needs a terminal");
    dialoguer::Confirm::new().interact();
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_check_on_an_unrelated_path_does_not_guard_the_call() {
        let source = r#"
use dialoguer::Confirm;
fn read_input() -> String {
    if std::io::stdin().is_terminal() { String::new() } else { slurp() }
}
fn delete() {
    Confirm::new().interact();
}
"#;
        assert!(matches!(status(source), AuditStatus::Warn(_)));
    }

    #[test]
    fn every_stdin_check_spelling_guards() {
        for check in [
            "std::io::stdin().is_terminal()",
            "io::stdin().is_terminal()",
            "stdin().is_terminal()",
            "stdin.is_terminal()",
            "std::io::stdin().lock().is_terminal()",
            "std::io::stdin()\n        .is_terminal()",
            "stdin().is_tty()",
            "IsTerminal::is_terminal(&io::stdin())",
            "atty::is(atty::Stream::Stdin)",
            "libc::isatty(0)",
            "libc::isatty(libc::STDIN_FILENO)",
        ] {
            let source = format!(
                "fn ask() {{\n    if {check} {{\n        dialoguer::Confirm::new().interact();\n    }}\n}}\n"
            );
            assert_eq!(status(&source), AuditStatus::Pass, "{check}");
        }
    }

    #[test]
    fn a_mention_in_a_comment_or_string_is_no_check() {
        let source = r#"
fn ask() {
    // stdin().is_terminal() would go here
    println!("stdin is_terminal");
    dialoguer::Confirm::new().interact();
}
"#;
        assert!(matches!(status(source), AuditStatus::Warn(_)));
    }

    #[test]
    fn a_prompt_in_test_code_counts_only_with_include_tests() {
        let source = r#"
#[cfg(test)]
mod tests {
    fn fixture() {
        dialoguer::Confirm::new().interact();
    }
}
"#;
        assert!(matches!(status(source), AuditStatus::NotApplicable(_)));

        let file = SourceFile::parse(std::path::Path::new("src/lib.rs"), source, Rust, true);
        assert!(matches!(
            verdict(&function_facts(&file)),
            AuditStatus::Warn(_)
        ));
    }

    #[test]
    fn metadata_names_the_requirement_it_checks() {
        let audit = PromptTtyAudit;
        assert_eq!(audit.id(), "p1-prompt-tty-source");
        assert_eq!(audit.group(), AuditGroup::P1);
        assert_eq!(audit.layer(), AuditLayer::Source);
        assert_eq!(audit.covers(), ["p1-should-tty-detection"]);
    }
}
