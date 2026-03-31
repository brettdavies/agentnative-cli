mod check;
mod checks;
mod cli;
mod error;
mod project;
mod runner;
mod scorecard;
mod source;
mod types;

use std::path::PathBuf;

use clap::Parser as _;
use clap_complete::generate;

use check::Check;
use checks::behavioral::all_behavioral_checks;
use checks::source::all_source_checks;
use cli::{Cli, Commands, OutputFormat};
use error::AppError;
use project::Project;
use scorecard::{exit_code, format_json, format_text};
use types::{CheckGroup, CheckResult, CheckStatus};

fn main() {
    // Fix SIGPIPE handling so piping to head/grep works correctly.
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
    let cli = Cli::parse();

    // Extract check parameters, defaulting None command to `check .`
    let (path, binary_only, source_only, principle, output, quiet, _include_tests) =
        match cli.command {
            Some(Commands::Check {
                path,
                binary,
                source,
                principle,
                output,
                quiet,
                include_tests,
            }) => (
                path,
                binary,
                source,
                principle,
                output,
                quiet,
                include_tests,
            ),
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                generate(shell, &mut cmd, "agentnative", &mut std::io::stdout());
                return Ok(0);
            }
            None => (
                PathBuf::from("."),
                false,
                false,
                None,
                OutputFormat::Text,
                false,
                false,
            ),
        };

    let project = Project::discover(&path)?;

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
                group: CheckGroup::P1,
                layer: types::CheckLayer::Behavioral,
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

fn matches_principle(group: &CheckGroup, principle: u8) -> bool {
    matches!(
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
