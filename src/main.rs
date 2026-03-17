use hitch::cli::Cli;
use hitch::runtime::run_wrapped_command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Cli::parse().into_config();
    let exit_code = run_wrapped_command(config)?;
    std::process::exit(exit_code);
}
