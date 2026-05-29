use std::io::IsTerminal;

use clap::{Parser, Subcommand, ValueEnum};

mod error;
mod output;

use error::AppError;

/// Named exit codes for agent-readable process exits.
const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Parser)]
#[command(name = "perfect-rust", version, about = "A perfect CLI tool")]
struct Cli {
    #[arg(long, short = 'q', global = true, env = "PERFECT_QUIET")]
    quiet: bool,

    #[arg(long, default_value = "text", global = true, env = "PERFECT_OUTPUT")]
    output: OutputFormat,

    #[arg(long, global = true, env = "PERFECT_NO_COLOR")]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a check
    Check {
        /// Path to check
        path: String,
    },
    /// Generate shell completions
    Completions {
        shell: clap_complete::Shell,
    },
}

fn main() {
    let is_tty = std::io::stdout().is_terminal();
    let use_color = is_tty && std::env::var("NO_COLOR").is_err();

    let code = match run() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_FAILURE
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, AppError> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Check { path } => {
            let result = check_path(&path)?;
            match cli.output {
                OutputFormat::Json => {
                    let json = serde_json::to_string_pretty(&result)
                        .map_err(|e| AppError::Serialization(e.to_string()))?;
                    output::write_stdout(&json);
                }
                OutputFormat::Text => {
                    if !cli.quiet {
                        output::write_stdout(&format!("Checked: {path}"));
                    }
                }
            }
            Ok(EXIT_SUCCESS)
        }
        Commands::Completions { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "perfect-rust", &mut std::io::stdout());
            Ok(EXIT_SUCCESS)
        }
    }
}

fn check_path(path: &str) -> Result<serde_json::Value, AppError> {
    if !std::path::Path::new(path).exists() {
        return Err(AppError::InvalidInput(format!("path not found: {path}")));
    }
    Ok(serde_json::json!({"path": path, "status": "ok"}))
}
