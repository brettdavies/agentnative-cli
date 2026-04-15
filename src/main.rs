mod check;
mod checks;
mod cli;
mod error;
mod project;
mod runner;
mod scorecard;
mod source;
mod types;

use clap::Parser as _;
use clap_complete::generate;

use check::Check;
use checks::behavioral::all_behavioral_checks;
use checks::project::all_project_checks;
use checks::source::all_source_checks;
use cli::{Cli, Commands, OutputFormat};
use error::AppError;
use project::Project;
use scorecard::{exit_code, format_json, format_text};
use types::{CheckGroup, CheckResult, CheckStatus};

fn main() {
    // Fix SIGPIPE handling so piping to head/grep works correctly.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, AppError> {
    let cli = Cli::parse_from(inject_default_subcommand(std::env::args_os()));

    // --quiet is global (visible in top-level --help for agent discoverability)
    let quiet = cli.quiet;

    // Bare invocation (None) is handled by clap's arg_required_else_help —
    // it prints help and exits before reaching here.
    let (path, command, binary_only, source_only, principle, output, include_tests) =
        match cli.command {
            Some(Commands::Check {
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
            }) => (
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
            ),
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                generate(shell, &mut cmd, "anc", &mut std::io::stdout());
                return Ok(0);
            }
            None => unreachable!("clap arg_required_else_help handles bare invocation"),
        };

    // --command resolves a binary from PATH and runs behavioral checks against
    // it. conflicts_with = "path" ensures only one of the two is provided.
    let resolved_path = match command {
        Some(name) => resolve_command_on_path(&name)?,
        None => path,
    };

    let mut project = Project::discover(&resolved_path)?;
    project.include_tests = include_tests;

    // Collect applicable checks based on flags and auto-detection
    let mut all_checks: Vec<Box<dyn Check>> = Vec::new();

    let has_binary = project.runner.is_some();
    let has_language = project.language.is_some();

    if !source_only {
        if has_binary {
            all_checks.extend(all_behavioral_checks());
        } else if binary_only {
            eprintln!("warning: --binary specified but no binary found");
        } else if has_language {
            eprintln!("warning: no binary found, running source checks only");
        }
    }

    if !binary_only {
        if let Some(lang) = project.language {
            all_checks.extend(all_source_checks(lang));
        } else if source_only {
            eprintln!("warning: --source specified but no language detected");
        }
    }

    // Project checks — always collected when path is a directory and not binary-only
    if !binary_only && project.path.is_dir() {
        all_checks.extend(all_project_checks());
    }

    // Run checks
    let mut results: Vec<CheckResult> = Vec::new();
    for check in &all_checks {
        if !check.applicable(&project) {
            continue;
        }
        let result = match check.run(&project) {
            Ok(r) => r,
            Err(e) => CheckResult {
                id: check.id().to_string(),
                label: check.id().to_string(),
                group: check.group(),
                layer: check.layer(),
                status: CheckStatus::Error(e.to_string()),
            },
        };
        results.push(result);
    }

    // Filter by principle number
    if let Some(p) = principle {
        results.retain(|r| matches_principle(&r.group, p));
    }

    // Format output
    let output_str = match output {
        OutputFormat::Text => format_text(&results, quiet),
        OutputFormat::Json => format_json(&results),
    };
    print!("{output_str}");

    Ok(exit_code(&results))
}

/// Resolve a command name to an absolute path by shelling out to `which`
/// (Unix) or `where` (Windows). Returns a clear, actionable error when the
/// name cannot be found on PATH. Subsequent `Project::discover()` validates
/// that the resolved path is an executable file.
fn resolve_command_on_path(name: &str) -> Result<std::path::PathBuf, AppError> {
    let locator = if cfg!(windows) { "where" } else { "which" };

    let output = std::process::Command::new(locator)
        .arg(name)
        .output()
        .map_err(|e| {
            AppError::ProjectDetection(anyhow::anyhow!("failed to invoke `{locator}`: {e}"))
        })?;

    if !output.status.success() {
        return Err(AppError::ProjectDetection(anyhow::anyhow!(
            "command '{name}' not found on PATH"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // `which` and `where` both print one match per line; take the first.
    let first = stdout.lines().next().map(str::trim).unwrap_or("");
    if first.is_empty() {
        return Err(AppError::ProjectDetection(anyhow::anyhow!(
            "command '{name}' not found on PATH"
        )));
    }

    Ok(std::path::PathBuf::from(first))
}

/// Inject `check` as the default subcommand when the first non-flag argument
/// is not a recognized subcommand.
///
/// Bare invocation (no args beyond the program name) is left untouched so
/// clap's `arg_required_else_help` still prints help and exits 2. This is a
/// non-negotiable fork-bomb guard: when agentnative dogfoods itself, a bare
/// spawn must not recurse into `check .`.
fn inject_default_subcommand<I>(args: I) -> Vec<std::ffi::OsString>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let args: Vec<std::ffi::OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        return args;
    }

    // Derive subcommand names from clap introspection so the list cannot drift.
    let cmd = <Cli as clap::CommandFactory>::command();
    let known: Vec<String> = cmd
        .get_subcommands()
        .flat_map(|sc| {
            std::iter::once(sc.get_name().to_string()).chain(sc.get_all_aliases().map(String::from))
        })
        .collect();

    for token in args.iter().skip(1) {
        let s = token.to_string_lossy();
        if s.starts_with('-') {
            // Skip leading global flags (e.g., -q, --quiet, --help, --version).
            continue;
        }
        if known.iter().any(|k| k.as_str() == s.as_ref()) {
            // Explicit subcommand — pass through unchanged.
            return args;
        }
        // First non-flag token is not a subcommand — treat as path/flag-value
        // for the implicit `check` subcommand and inject it at position 1.
        let mut injected = Vec::with_capacity(args.len() + 1);
        injected.push(args[0].clone());
        injected.push(std::ffi::OsString::from("check"));
        injected.extend(args.into_iter().skip(1));
        return injected;
    }

    // No non-flag tokens (e.g., `anc --help`, `anc -q`) — let clap handle it.
    args
}

fn matches_principle(group: &CheckGroup, principle: u8) -> bool {
    // CodeQuality and ProjectStructure checks are cross-cutting — always include them.
    matches!(
        group,
        CheckGroup::CodeQuality | CheckGroup::ProjectStructure
    ) || matches!(
        (group, principle),
        (CheckGroup::P1, 1)
            | (CheckGroup::P2, 2)
            | (CheckGroup::P3, 3)
            | (CheckGroup::P4, 4)
            | (CheckGroup::P5, 5)
            | (CheckGroup::P6, 6)
            | (CheckGroup::P7, 7)
    )
}

#[cfg(test)]
mod inject_tests {
    use super::inject_default_subcommand;
    use std::ffi::OsString;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn names(v: Vec<OsString>) -> Vec<String> {
        v.into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn bare_invocation_is_untouched() {
        // Fork-bomb guard: no injection for bare `anc`.
        let out = inject_default_subcommand(args(&["anc"]));
        assert_eq!(names(out), vec!["anc"]);
    }

    #[test]
    fn dot_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "."]));
        assert_eq!(names(out), vec!["anc", "check", "."]);
    }

    #[test]
    fn global_short_flag_before_path_gets_check_injected_in_canonical_position() {
        // `check` goes before the global flag so clap parses
        // ["anc", "check", "-q", "."] cleanly.
        let out = inject_default_subcommand(args(&["anc", "-q", "."]));
        assert_eq!(names(out), vec!["anc", "check", "-q", "."]);
    }

    #[test]
    fn global_long_flag_before_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "--quiet", "."]));
        assert_eq!(names(out), vec!["anc", "check", "--quiet", "."]);
    }

    #[test]
    fn explicit_check_subcommand_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "check", "."]));
        assert_eq!(names(out), vec!["anc", "check", "."]);
    }

    #[test]
    fn explicit_completions_subcommand_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "completions", "bash"]));
        assert_eq!(names(out), vec!["anc", "completions", "bash"]);
    }

    #[test]
    fn help_flag_alone_is_untouched() {
        // `anc --help` — no non-flag token, no injection.
        let out = inject_default_subcommand(args(&["anc", "--help"]));
        assert_eq!(names(out), vec!["anc", "--help"]);
    }

    #[test]
    fn version_flag_alone_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "--version"]));
        assert_eq!(names(out), vec!["anc", "--version"]);
    }

    #[test]
    fn quiet_flag_alone_is_untouched() {
        // `anc -q` with no path — clap decides what to do (error / help).
        let out = inject_default_subcommand(args(&["anc", "-q"]));
        assert_eq!(names(out), vec!["anc", "-q"]);
    }

    #[test]
    fn directory_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "/some/dir"]));
        assert_eq!(names(out), vec!["anc", "check", "/some/dir"]);
    }

    #[test]
    fn trailing_flags_pass_through() {
        let out = inject_default_subcommand(args(&["anc", ".", "--output", "json"]));
        assert_eq!(names(out), vec!["anc", "check", ".", "--output", "json"]);
    }
}
