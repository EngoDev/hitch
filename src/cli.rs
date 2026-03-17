use std::env;
use std::ffi::OsString;

use clap::{Arg, Command, error::ErrorKind};

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub origin: Option<String>,
    pub user: Option<String>,
    pub command: Vec<String>,
}

impl Cli {
    pub fn command() -> Command {
        Command::new("hitch")
            .about("Wrap a login command and establish callback tunneling when needed.")
            .arg(
                Arg::new("origin")
                    .long("origin")
                    .value_name("HOST")
                    .help("Override the SSH origin host or IP used for the reverse tunnel."),
            )
            .arg(
                Arg::new("user")
                    .long("user")
                    .value_name("SSH_USER")
                    .help("Override the SSH user used for the reverse tunnel."),
            )
            .after_help(
                "Usage:\n  hitch [OPTIONS] -- <COMMAND> [ARGS]...\n\nExamples:\n  hitch -- aws sso login\n  hitch --origin 203.0.113.10 --user alice -- gh auth login",
            )
    }

    pub fn parse() -> Self {
        Self::try_parse_from(env::args_os()).unwrap_or_else(|error| error.exit())
    }

    pub fn try_parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        let separator_index = args.iter().position(|arg| arg == "--");

        let Some(separator_index) = separator_index else {
            let error_kind = if args.len() <= 1 {
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            } else {
                ErrorKind::UnknownArgument
            };
            let message = if args.len() <= 1 {
                "a wrapped command is required after `--`"
            } else {
                "wrapped command must be passed after `--`"
            };

            return Err(Self::command().error(error_kind, message));
        };

        if separator_index + 1 >= args.len() {
            return Err(Self::command().error(
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
                "a wrapped command is required after `--`",
            ));
        }

        let option_args = args[..separator_index].to_vec();
        let matches = Self::command().try_get_matches_from(option_args)?;

        Ok(Self {
            origin: matches.get_one::<String>("origin").cloned(),
            user: matches.get_one::<String>("user").cloned(),
            command: args[separator_index + 1..]
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        })
    }

    pub fn into_config(self) -> Config {
        Config {
            origin: self.origin,
            user: self.user,
            command: self.command,
        }
    }
}
