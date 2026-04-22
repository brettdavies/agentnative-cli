mod argv;
mod check;
mod checks;
mod cli;
mod error;
mod principles;
mod project;
mod runner;
mod scorecard;
mod source;
mod types;

use clap::Parser as _;
use clap_complete::generate;

use argv::inject_default_subcommand;
use check::Check;
use checks::behavioral::all_behavioral_checks;
use checks::project::all_project_checks;
use checks::source::all_source_checks;
use cli::{Cli, Commands, GenerateKind, OutputFormat};
use error::AppError;
use principles::matrix;
use principles::registry::{ExceptionCategory, SUPPRESSION_EVIDENCE_PREFIX, suppresses};
use project::Project;
use scorecard::audience;
use scorecard::{exit_code, format_json, format_text};
use types::{CheckGroup, CheckResult, CheckStatus, Confidence};

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

    // Bare invocation (no args at all) is handled by clap's arg_required_else_help.
    // A flag-only invocation like `anc -q` parses successfully with `command =
    // None` — render help to stderr and exit 2 to mirror clap's contract.
    let (path, command, binary_only, source_only, principle, output, include_tests, audit_profile) =
        match cli.command {
            Some(Commands::Check {
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
                audit_profile,
            }) => (
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
                audit_profile,
            ),
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                generate(shell, &mut cmd, "anc", &mut std::io::stdout());
                return Ok(0);
            }
            Some(Commands::Generate { artifact }) => {
                return run_generate(artifact);
            }
            None => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                eprintln!("{}", cmd.render_help());
                return Ok(2);
            }
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

    // Translate the CLI-facing AuditProfile into the registry's
    // ExceptionCategory. Kept local so the registry stays CLI-agnostic.
    let exception_category: Option<ExceptionCategory> = audit_profile.map(Into::into);

    // Run checks. When an audit_profile is set, checks whose IDs appear in
    // the suppression table short-circuit to Skip with structured evidence
    // — they still appear in `results[]` so the scorecard shows what was
    // excluded and why.
    let mut results: Vec<CheckResult> = Vec::new();
    for check in &all_checks {
        if !check.applicable(&project) {
            continue;
        }
        if let Some(cat) = exception_category {
            if suppresses(check.id(), cat) {
                results.push(CheckResult {
                    id: check.id().to_string(),
                    label: check.label().to_string(),
                    group: check.group(),
                    layer: check.layer(),
                    status: CheckStatus::Skip(format!(
                        "{SUPPRESSION_EVIDENCE_PREFIX}{}",
                        cat.as_kebab_case()
                    )),
                    confidence: Confidence::High,
                });
                continue;
            }
        }
        let result = match check.run(&project) {
            Ok(r) => r,
            Err(e) => CheckResult {
                id: check.id().to_string(),
                label: check.label().to_string(),
                group: check.group(),
                layer: check.layer(),
                status: CheckStatus::Error(e.to_string()),
                confidence: Confidence::High,
            },
        };
        results.push(result);
    }

    // Filter by principle number
    if let Some(p) = principle {
        results.retain(|r| matches_principle(&r.group, p));
    }

    // Compute audience from the 4 signal checks. Read-only over results;
    // Returns None when any signal check is missing from the vector — the
    // suppression loop above is the usual reason signal checks drop out
    // (e.g., human-tui suppresses `p1-non-interactive`).
    let audience_label = audience::classify(&results);
    let audit_profile_label = exception_category.map(|c| c.as_kebab_case().to_string());

    // Format output. `format_json` needs the check catalog so it can map
    // result IDs back to the requirements each check covers.
    let output_str = match output {
        OutputFormat::Text => format_text(&results, quiet),
        OutputFormat::Json => {
            format_json(&results, &all_checks, audience_label, audit_profile_label)
        }
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

fn run_generate(artifact: GenerateKind) -> Result<i32, AppError> {
    match artifact {
        GenerateKind::CoverageMatrix {
            out,
            json_out,
            check,
        } => {
            let catalog = checks::all_checks_catalog();

            // Dangling `covers()` references are a registry bug — surface
            // them before writing artifacts so CI catches the regression
            // at `generate --check` time too.
            let dangling = matrix::dangling_cover_ids(&catalog);
            if !dangling.is_empty() {
                for (check_id, req_id) in &dangling {
                    eprintln!("error: check `{check_id}` covers unknown requirement `{req_id}`");
                }
                return Err(AppError::ProjectDetection(anyhow::anyhow!(
                    "registry drift: {} dangling requirement reference(s)",
                    dangling.len()
                )));
            }

            let m = matrix::build(&catalog);
            let rendered_md = matrix::render_markdown(&m);
            let rendered_json = matrix::render_json(&m);

            if check {
                // Drift mode: compare generated output to committed artifacts.
                // Fail with actionable evidence so CI points the operator at
                // `anc generate coverage-matrix` as the fix.
                let existing_md = std::fs::read_to_string(&out).unwrap_or_default();
                let existing_json = std::fs::read_to_string(&json_out).unwrap_or_default();
                let md_matches = normalize_trailing_newline(&existing_md)
                    == normalize_trailing_newline(&rendered_md);
                let json_matches = normalize_trailing_newline(&existing_json)
                    == normalize_trailing_newline(&rendered_json);
                if !md_matches {
                    eprintln!(
                        "error: {} is out of date — run `anc generate coverage-matrix`",
                        out.display()
                    );
                }
                if !json_matches {
                    eprintln!(
                        "error: {} is out of date — run `anc generate coverage-matrix`",
                        json_out.display()
                    );
                }
                return Ok(if md_matches && json_matches { 0 } else { 2 });
            }

            if let Some(parent) = out.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        AppError::ProjectDetection(anyhow::anyhow!(
                            "creating parent dir for {}: {e}",
                            out.display()
                        ))
                    })?;
                }
            }
            if let Some(parent) = json_out.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        AppError::ProjectDetection(anyhow::anyhow!(
                            "creating parent dir for {}: {e}",
                            json_out.display()
                        ))
                    })?;
                }
            }
            std::fs::write(&out, &rendered_md).map_err(|e| {
                AppError::ProjectDetection(anyhow::anyhow!("writing {}: {e}", out.display()))
            })?;
            std::fs::write(&json_out, &rendered_json).map_err(|e| {
                AppError::ProjectDetection(anyhow::anyhow!("writing {}: {e}", json_out.display()))
            })?;
            eprintln!(
                "wrote {} ({} rows) and {}",
                out.display(),
                m.rows.len(),
                json_out.display()
            );
            Ok(0)
        }
    }
}

fn normalize_trailing_newline(s: &str) -> &str {
    s.trim_end_matches('\n')
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
