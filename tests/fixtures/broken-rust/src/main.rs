use clap::Parser;
use std::process;

#[derive(Parser)]
#[command(name = "broken")]
struct Cli {
    #[arg(long)]
    name: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let name = cli.name.unwrap();
    let count: i32 = "42".parse().unwrap();

    println!("Hello, {}! Count: {}", name, count);
    println!("Done processing");

    if count < 0 {
        process::exit(1);
    }
}

fn helper() {
    let data = std::fs::read_to_string("config.toml").unwrap();
    println!("{}", data);
    process::exit(2);
}
