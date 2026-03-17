//! Binary entrypoint for Hitch.

use hitch::cli::Cli;
use hitch::config::Config;
use hitch::runtime::run_wrapped_command;

/// Parses command-line arguments, runs Hitch, and exits with the wrapped process status code.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::try_from(Cli::parse())?;
    let exit_code = run_wrapped_command(config)?;
    std::process::exit(exit_code);
}
