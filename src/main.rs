use std::process::ExitCode;

use clap::{Parser, Subcommand};

use bits_style::bench::{self, BenchArgs};
use bits_style::scanner::{self, StyleArgs};

#[derive(Debug, Parser)]
#[command(name = "bits", version, about = "Bits’s Coding Style scanner")]
struct Cli {
    #[command(subcommand)]
    command: TopCommand,
}

#[derive(Debug, Subcommand)]
enum TopCommand {
    Style(StyleArgs),
    Bench(BenchArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        TopCommand::Style(args) => scanner::run_style(args),
        TopCommand::Bench(args) => match bench::run(args) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("bits bench: {error}");
                2
            }
        },
    };
    ExitCode::from(code)
}
