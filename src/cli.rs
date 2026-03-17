use std::env;
use std::ffi::OsString;

use clap::{Arg, Command, error::ErrorKind};

use crate::config::{Config, Mode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub origin: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub command: Option<Vec<String>>,
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
            .arg(
                Arg::new("port")
                    .long("port")
                    .value_name("PORT")
                    .value_parser(clap::value_parser!(u16))
                    .help("Open a reverse tunnel directly for the specified local port."),
            )
            .after_help(
                "Usage:\n  hitch [OPTIONS] -- <COMMAND> [ARGS]...\n  hitch [OPTIONS] --port <PORT>\n\nExamples:\n  hitch -- aws sso login\n  hitch --origin 203.0.113.10 --user alice -- gh auth login\n  hitch --origin 203.0.113.10 --port 38983",
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
        let option_args = match separator_index {
            Some(index) => args[..index].to_vec(),
            None => args.clone(),
        };
        let matches = Self::command().try_get_matches_from(option_args)?;
        let port = matches.get_one::<u16>("port").copied();

        if port.is_some() && separator_index.is_some() {
            return Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "--port cannot be combined with a wrapped command",
            ));
        }

        if let Some(port) = port {
            return Ok(Self {
                origin: matches.get_one::<String>("origin").cloned(),
                user: matches.get_one::<String>("user").cloned(),
                port: Some(port),
                command: None,
            });
        }

        let Some(separator_index) = separator_index else {
            let error_kind = if args.len() <= 1 {
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            } else {
                ErrorKind::UnknownArgument
            };
            let message = if args.len() <= 1 {
                "either --port <PORT> or a wrapped command after `--` is required"
            } else {
                "wrapped command must be passed after `--`, or use --port <PORT>"
            };

            return Err(Self::command().error(error_kind, message));
        };

        if separator_index + 1 >= args.len() {
            return Err(Self::command().error(
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
                "a wrapped command is required after `--`",
            ));
        }

        Ok(Self {
            origin: matches.get_one::<String>("origin").cloned(),
            user: matches.get_one::<String>("user").cloned(),
            port: None,
            command: Some(
                args[separator_index + 1..]
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
            ),
        })
    }

    pub fn into_config(self) -> Config {
        Config {
            origin: self.origin,
            user: self.user,
            mode: match (self.port, self.command) {
                (Some(port), None) => Mode::Port { port },
                (None, Some(command)) => Mode::Command { command },
                _ => unreachable!("CLI validation guarantees exactly one invocation mode"),
            },
        }
    }
}
