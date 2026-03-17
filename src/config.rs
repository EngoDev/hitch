//! Configuration types for Hitch invocation modes.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::cli::Cli;

/// The execution mode selected from the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// Wrap a child command and auto-detect callback tunnel details from its output.
    Command { command: Vec<String> },
    /// Open a reverse tunnel directly for the specified local port.
    Port { port: u16 },
}

/// Fully validated runtime configuration derived from CLI arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    /// Optional SSH origin override supplied on the command line.
    pub origin: Option<String>,
    /// Optional SSH user override supplied on the command line.
    pub user: Option<String>,
    /// The selected execution mode.
    pub mode: Mode,
}

/// Error returned when a [`Cli`] value cannot be converted into a valid [`Config`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigConversionError {
    message: &'static str,
}

impl ConfigConversionError {
    /// Creates a new conversion error with a static message.
    pub const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl Display for ConfigConversionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl Error for ConfigConversionError {}

impl TryFrom<Cli> for Config {
    type Error = ConfigConversionError;

    /// Converts parsed CLI arguments into validated runtime configuration.
    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        let mode = match (cli.port, cli.command) {
            (Some(port), None) => Mode::Port { port },
            (None, Some(command)) => Mode::Command { command },
            _ => {
                return Err(ConfigConversionError::new(
                    "CLI validation guarantees exactly one invocation mode",
                ));
            }
        };

        Ok(Self {
            origin: cli.origin,
            user: cli.user,
            mode,
        })
    }
}
