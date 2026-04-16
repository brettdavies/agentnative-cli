use clap::Parser;

mod error;

#[derive(Parser)]
#[command(name = "source-only", version, about = "A minimal CLI")]
struct Cli {
    #[arg(long, default_value = "text")]
    output: String,

    #[arg(long, short = 'q')]
    quiet: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !cli.quiet {
        eprintln!("running source-only");
    }
    Ok(())
}
