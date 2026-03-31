mod checks;
mod source;
mod types;

use checks::{global_flags, no_color, unwrap};
use types::CheckStatus;

fn main() {
    println!("agentnative v0.1.0 — ast-grep API spike");
    println!();

    // Demo: run all 3 PoC checks against inline test source
    let bad_source = r#"
use std::env;

#[derive(Parser)]
struct Cli {
    #[arg(long = "output")]
    output: Option<String>,

    #[arg(long = "quiet")]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check,
}

fn main() {
    let config = load_config().unwrap();
    let data = fetch_data().unwrap();
}
"#;

    let checks = vec![
        unwrap::check_unwrap(bad_source, "example.rs"),
        global_flags::check_global_flags(bad_source, "example.rs"),
        no_color::check_no_color(bad_source, "example.rs"),
    ];

    for result in &checks {
        let icon = match &result.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn(_) => "WARN",
            CheckStatus::Fail(_) => "FAIL",
            CheckStatus::Skip(_) => "SKIP",
            CheckStatus::Error(_) => "ERR ",
        };
        println!("[{icon}] {} ({})", result.label, result.id);
        match &result.status {
            CheckStatus::Warn(e) | CheckStatus::Fail(e) => {
                for line in e.lines() {
                    println!("       {line}");
                }
            }
            CheckStatus::Skip(reason) => println!("       {reason}"),
            _ => {}
        }
    }

    let fail_count = checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Fail(_)))
        .count();
    let warn_count = checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Warn(_)))
        .count();

    println!();
    println!(
        "{} checks: {} pass, {} warn, {} fail",
        checks.len(),
        checks.len() - fail_count - warn_count,
        warn_count,
        fail_count
    );

    std::process::exit(if fail_count > 0 { 1 } else { 0 });
}
