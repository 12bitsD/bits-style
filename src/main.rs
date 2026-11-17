use std::process::ExitCode;

use bits_style::scanner::Cli;

fn main() -> ExitCode {
    ExitCode::from(bits_style::scanner::run(Cli::parse_args()))
}
