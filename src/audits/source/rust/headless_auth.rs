//! Audit: Detect browser-launching code that lacks a headless auth flag.
//!
//! Principle: P1 (Non-Interactive) MUST: a CLI that authenticates through a
//! browser-based flow offers a headless path, because agents cannot open
//! browsers.
//!
//! This is a conditional audit:
//!   Trigger: a call into a browser-launching crate (open, opener, webbrowser)
//!   Pass: an argument definition names a flag P1 accepts
//!   Warn: code opens a browser and no argument definition names one
//!   N/A: no such call; a tool whose only credentials are static has no browser
//!   to avoid, and `p1-must-secret-non-leaky-path` governs how it reads them

use std::sync::LazyLock;

use ast_grep_core::Node;
use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::behavioral::flag_meaning::names_any;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::locations_each;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, SourceLocation};

/// Calls that open a browser, by the crate that does it.
const BROWSER_LAUNCH_SHAPES: [&str; 11] = [
    "open::that($$$ARGS)",
    "open::that_detached($$$ARGS)",
    "open::that_in_background($$$ARGS)",
    "open::with($$$ARGS)",
    "open::with_detached($$$ARGS)",
    "open::with_in_background($$$ARGS)",
    "opener::open($$$ARGS)",
    "opener::open_browser($$$ARGS)",
    "webbrowser::open($$$ARGS)",
    "webbrowser::open_browser($$$ARGS)",
    "webbrowser::open_browser_with_options($$$ARGS)",
];

static BROWSER_LAUNCHES: LazyLock<[Pattern; 11]> =
    LazyLock::new(|| BROWSER_LAUNCH_SHAPES.map(|source| Pattern::new(source, Rust)));

/// The names P1 accepts for a headless authentication path.
const HEADLESS_FLAGS: &[&str] = &["no-browser", "device-code", "remote", "headless"];

/// Accepted names that also mean something unrelated to authentication: a
/// `--headless` renderer, a git `--remote`. A definition that describes one of
/// these has to say it is about signing in.
const AMBIGUOUS_FLAGS: &[&str] = &["remote", "headless"];

/// Words and phrases that put a description on the sign-in side. "browser" alone
/// is not one: `--headless  Run the browser without a window` is a renderer.
const AUTH_WORDS: &[&str] = &[
    "auth",
    "authenticate",
    "authenticates",
    "authentication",
    "authorization",
    "authorize",
    "credential",
    "credentials",
    "device",
    "log in",
    "login",
    "no browser",
    "not open a browser",
    "oauth",
    "oauth2",
    "sign in",
    "signin",
    "token",
    "tokens",
    "without a browser",
    "without opening a browser",
];

/// Attributes that define an argument in clap's derive API.
const ARG_ATTRIBUTES: &[&str] = &["#[arg(", "#[clap(", "#[structopt("];

/// How a derive attribute gives a flag its name, with whitespace removed. Any
/// other quoted value, such as `default_value = "remote"`, names no flag.
const NAMING_ARGUMENTS: &[&str] = &["long=", "long(", "alias=", "alias("];

const NO_BROWSER_LAUNCH: &str = "No call into a browser-launching crate (open, opener, webbrowser) \
                                 found. The requirement applies to tools that authenticate through \
                                 a browser-based flow; static credentials fall under \
                                 p1-must-secret-non-leaky-path.";

const MISSING_FLAG: &str = "Code opens a browser and no argument definition names --no-browser, \
                            --device-code, --remote, or --headless. Agents cannot open a browser, \
                            so a browser-based sign-in needs a headless path; a browser opened for \
                            anything but authentication is outside the requirement. Browser \
                            launches:";

/// Audit trait implementation for headless auth detection.
#[derive(Debug)]
pub struct HeadlessAuthAudit;

impl Audit for HeadlessAuthAudit {
    fn id(&self) -> &str {
        "p1-headless-auth"
    }

    fn label(&self) -> &'static str {
        "Headless auth supported"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-browser"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        // Cross-file scan: the browser launch and the flag often live in
        // different files in well-structured Rust projects (auth logic in
        // src/auth/, CLI flags in src/cli/). Check both signals independently
        // across the whole parsed set, then combine at the project level.
        let files: Vec<HeadlessSignals> = pass::findings(project, Language::Rust, Rust, self)
            .into_iter()
            .map(|(_, file)| file)
            .collect();

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: verdict(&files),
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            row_statuses: Vec::new(),
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for HeadlessAuthAudit {
    type Finding = HeadlessSignals;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> HeadlessSignals {
        headless_signals(file)
    }
}

/// What one file shows about browser launches and a headless flag.
#[derive(Debug)]
pub struct HeadlessSignals {
    browser_launches: Vec<SourceLocation>,
    headless_flag: bool,
}

fn headless_signals(file: &SourceFile<'_, Rust>) -> HeadlessSignals {
    HeadlessSignals {
        browser_launches: locations_each(&file.root(), &*BROWSER_LAUNCHES, &file.label())
            .into_iter()
            .flatten()
            .collect(),
        headless_flag: file.root().dfs().any(|node| defines_headless_flag(&node)),
    }
}

fn verdict(files: &[HeadlessSignals]) -> AuditStatus {
    let launches: Vec<String> = files
        .iter()
        .flat_map(|file| &file.browser_launches)
        .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
        .collect();
    if launches.is_empty() {
        AuditStatus::NotApplicable(NO_BROWSER_LAUNCH.to_string())
    } else if files.iter().any(|file| file.headless_flag) {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!("{MISSING_FLAG}\n{}", launches.join("\n")))
    }
}

/// Whether `node` is an argument definition that credits a headless flag: a
/// clap derive attribute, a builder's `.long(..)`, or an `arg!` macro.
fn defines_headless_flag(node: &Node<'_, StrDoc<Rust>>) -> bool {
    match node.kind().as_ref() {
        "attribute_item" => derive_attribute_credits(node),
        "call_expression" => builder_long_credits(node),
        "macro_invocation" => arg_macro_credits(node),
        _ => false,
    }
}

fn derive_attribute_credits(attribute: &Node<'_, StrDoc<Rust>>) -> bool {
    let text = attribute.text();
    let Some(arguments) = ARG_ATTRIBUTES
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
    else {
        return false;
    };
    let Some(field) = attribute
        .next_all()
        .find(|sibling| !is_attribute_or_comment(sibling))
        .filter(|sibling| sibling.kind() == "field_declaration")
    else {
        return false;
    };
    let compact: String = arguments.split_whitespace().collect();
    let quoted = HEADLESS_FLAGS.iter().copied().find(|flag| {
        NAMING_ARGUMENTS
            .iter()
            .any(|naming| compact.contains(&format!("{naming}\"{flag}\"")))
    });
    let bare_long = arguments
        .trim_end_matches(")]")
        .split(',')
        .any(|argument| argument.trim() == "long");
    let flag = quoted.or_else(|| {
        let name = field.field("name")?.text().replace('_', "-");
        bare_long
            .then(|| HEADLESS_FLAGS.iter().copied().find(|flag| *flag == name))
            .flatten()
    });
    flag.is_some_and(|flag| credits(flag, derive_description(&field).as_deref()))
}

fn is_attribute_or_comment(node: &Node<'_, StrDoc<Rust>>) -> bool {
    matches!(
        node.kind().as_ref(),
        "attribute_item" | "line_comment" | "block_comment"
    )
}

/// The doc comments and `help` attributes on a field, when it has any.
fn derive_description(field: &Node<'_, StrDoc<Rust>>) -> Option<String> {
    let described: Vec<String> = field
        .prev_all()
        .take_while(is_attribute_or_comment)
        .map(|sibling| sibling.text().to_string())
        .filter(|text| text.starts_with("///") || text.contains("help"))
        .collect();
    (!described.is_empty()).then(|| described.join(" "))
}

/// A builder's `Arg::new("x").long("no-browser")`, described by the rest of its
/// method chain.
fn builder_long_credits(call: &Node<'_, StrDoc<Rust>>) -> bool {
    let Some(function) = call.field("function") else {
        return false;
    };
    if !function.text().ends_with(".long") {
        return false;
    }
    let Some(arguments) = call.field("arguments") else {
        return false;
    };
    let arguments = arguments.text();
    HEADLESS_FLAGS
        .iter()
        .find(|flag| arguments.contains(&format!("\"{flag}\"")))
        .is_some_and(|flag| {
            let chain = call
                .ancestors()
                .take_while(|ancestor| {
                    matches!(
                        ancestor.kind().as_ref(),
                        "call_expression" | "field_expression"
                    )
                })
                .last()
                .map_or_else(|| call.text().to_string(), |top| top.text().to_string());
            let description = chain.contains(".help(").then_some(chain.as_str());
            credits(flag, description)
        })
}

/// `arg!(--headless "Render without a window")`, or `--"no-browser"` for a name
/// a bare token cannot spell.
fn arg_macro_credits(invocation: &Node<'_, StrDoc<Rust>>) -> bool {
    let Some(name) = invocation.field("macro") else {
        return false;
    };
    let name = name.text();
    if name != "arg" && !name.ends_with("::arg") {
        return false;
    }
    let text = invocation.text();
    HEADLESS_FLAGS
        .iter()
        .find(|flag| text.contains(&format!("--{flag}")) || text.contains(&format!("--\"{flag}\"")))
        .is_some_and(|flag| credits(flag, text.contains(" \"").then_some(text.as_ref())))
}

/// Whether a definition of `flag` counts, given what it says about itself. A
/// definition with no description has only its name to go on, which P1 accepts.
fn credits(flag: &str, description: Option<&str>) -> bool {
    !AMBIGUOUS_FLAGS.contains(&flag)
        || description.is_none_or(|description| names_any(description, AUTH_WORDS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_headless_auth(file: &SourceFile<'_, Rust>) -> AuditStatus {
        verdict(&[headless_signals(file)])
    }

    fn status(source: &str) -> AuditStatus {
        audit_headless_auth(&pass::rust(source, "src/auth.rs"))
    }

    #[track_caller]
    fn assert_warns_naming_the_launch(status: AuditStatus) {
        match status {
            AuditStatus::Warn(evidence) => {
                assert!(
                    evidence.contains("no argument definition names"),
                    "{evidence}"
                );
                assert!(evidence.contains("src/auth.rs:"), "{evidence}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn device_code_on_an_argument_definition_passes() {
        let source = r#"
use clap::Parser;
fn do_oauth_flow(url: &str) {
    webbrowser::open(url);
}
#[derive(Parser)]
struct Cli {
    #[arg(long = "device-code")]
    device_code: bool,
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn every_accepted_flag_passes_beside_a_browser_launch() {
        for flag in ["no_browser", "device_code", "remote", "headless"] {
            let source = format!(
                "fn sign_in(url: &str) {{\n    open::that(url);\n}}\nstruct Cli {{\n    #[arg(long)]\n    {flag}: bool,\n}}\n"
            );
            assert_eq!(status(&source), AuditStatus::Pass, "{flag}");
        }
    }

    #[test]
    fn a_headless_rendering_flag_credits_nothing() {
        let source = r#"
fn do_oauth_flow(url: &str) {
    open::that(url);
}
struct Cli {
    /// Render charts without opening a window.
    #[arg(long)]
    headless: bool,
    #[arg(long, help = "Git remote to push to")]
    remote: String,
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn a_headless_browser_driver_flag_credits_nothing() {
        let source = r#"
fn sign_in(url: &str) {
    opener::open_browser(url);
}
struct Cli {
    /// Run the browser without a visible window.
    #[arg(long)]
    headless: bool,
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn a_headless_flag_described_as_signing_in_passes() {
        for description in [
            "/// Sign in on another device.",
            "/// Authenticate without a browser.",
            "/// Skip the OAuth redirect.",
        ] {
            let source = format!(
                "fn sign_in(url: &str) {{\n    open::that(url);\n}}\nstruct Cli {{\n    {description}\n    #[arg(long)]\n    headless: bool,\n}}\n"
            );
            assert_eq!(status(&source), AuditStatus::Pass, "{description}");
        }
    }

    #[test]
    fn flag_text_off_an_argument_definition_credits_nothing() {
        let source = r#"
// --no-browser would go here, --headless too
fn sign_in(url: &str) {
    open::that(url);
    Command::new("chrome").arg("--headless").arg("--no-browser").spawn();
    println!("try --device-code");
}
struct Cli {
    #[arg(long)]
    verbose: bool,
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn an_arg_attribute_naming_a_different_long_flag_credits_nothing() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
struct Cli {
    #[arg(long = "verbose")]
    headless: bool,
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn a_value_that_reads_like_a_flag_name_credits_nothing() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
struct Cli {
    #[arg(long, default_value = "remote")]
    mode: String,
    #[arg(long = "render", value_name = "headless")]
    render: String,
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn an_alias_defines_a_flag() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
struct Cli {
    #[arg(long = "manual", visible_alias = "no-browser")]
    manual: bool,
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_clap_attribute_defines_a_flag() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
struct Cli {
    /// Print the URL instead of opening it.
    #[clap(long, env = "TOOL_NO_BROWSER")]
    no_browser: bool,
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_field_with_comments_between_its_attribute_and_name_defines_a_flag() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
struct Cli {
    #[arg(long)]
    /// Print the URL instead of opening it.
    #[arg(env = "TOOL_NO_BROWSER")]
    no_browser: bool,
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_builder_long_defines_a_flag() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
fn command() -> Command {
    Command::new("tool").arg(Arg::new("no-browser").long("no-browser").action(ArgAction::SetTrue))
}
"#;
        assert_eq!(status(source), AuditStatus::Pass);
    }

    #[test]
    fn a_builder_headless_is_read_by_its_help() {
        let rendering = r#"
fn sign_in(url: &str) {
    open::that(url);
}
fn command() -> Command {
    Command::new("tool").arg(Arg::new("headless").long("headless").help("Render without a window"))
}
"#;
        assert_warns_naming_the_launch(status(rendering));

        let signing_in = rendering.replace("Render without a window", "Sign in on another device");
        assert_eq!(status(&signing_in), AuditStatus::Pass);
    }

    #[test]
    fn an_arg_macro_defines_a_flag() {
        for definition in [
            r#"arg!(--"no-browser" "Print the URL")"#,
            r#"clap::arg!(--"device-code")"#,
            r#"arg!(--headless "Sign in on another device")"#,
        ] {
            let source = format!(
                "fn sign_in(url: &str) {{\n    open::that(url);\n}}\nfn command() -> Command {{\n    Command::new(\"tool\").arg({definition})\n}}\n"
            );
            assert_eq!(status(&source), AuditStatus::Pass, "{definition}");
        }
    }

    #[test]
    fn an_arg_macro_headless_is_read_by_its_help() {
        let source = r#"
fn sign_in(url: &str) {
    open::that(url);
}
fn command() -> Command {
    Command::new("tool").arg(arg!(--headless "Render without a window"))
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn an_api_key_tool_with_an_auth_named_function_is_not_applicable() {
        let source = r#"
pub fn authorization_header(api_key: &str) -> String {
    format!("Bearer {api_key}")
}
"#;
        assert!(
            matches!(status(source), AuditStatus::NotApplicable(_)),
            "{:?}",
            status(source)
        );
    }

    #[test]
    fn an_oauth_token_refresh_with_no_browser_is_not_applicable() {
        let source = r#"
pub async fn refresh_oauth2_token(client: &Client) {}
pub(crate) fn build_auth_url() -> String { String::new() }
"#;
        assert!(matches!(status(source), AuditStatus::NotApplicable(_)));
    }

    #[test]
    fn every_browser_launching_call_triggers() {
        for call in [
            "open::that(url)",
            "open::that_detached(url)",
            "open::that_in_background(url)",
            "open::with(url, \"firefox\")",
            "open::with_detached(url, \"firefox\")",
            "open::with_in_background(url, \"firefox\")",
            "opener::open(url)",
            "opener::open_browser(url)",
            "webbrowser::open(url)",
            "webbrowser::open_browser(Browser::Default, url)",
            "webbrowser::open_browser_with_options(Browser::Default, url, opts)",
        ] {
            let source = format!("fn sign_in(url: &str) {{\n    {call};\n}}\n");
            assert_warns_naming_the_launch(status(&source));
        }
    }

    #[test]
    fn a_launch_inside_a_closure_triggers() {
        let source = r#"
fn sign_in() {
    let opener = move |url: &str| {
        let opened = open::that(url);
        opened
    };
}
"#;
        assert_warns_naming_the_launch(status(source));
    }

    #[test]
    fn a_launch_in_one_file_and_the_flag_in_another_pass() {
        let auth = headless_signals(&pass::rust(
            "pub fn sign_in(url: &str) {\n    webbrowser::open(url);\n}\n",
            "src/auth.rs",
        ));
        let cli = headless_signals(&pass::rust(
            "struct Cli {\n    #[arg(long = \"no-browser\")]\n    no_browser: bool,\n}\n",
            "src/cli.rs",
        ));
        assert_eq!(verdict(&[auth, cli]), AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = HeadlessAuthAudit;
        let dir = std::env::temp_dir().join(format!("anc-headless-rust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(audit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let audit = HeadlessAuthAudit;
        let dir = std::env::temp_dir().join(format!("anc-headless-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
