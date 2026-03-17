//! Hitch library crate.

/// Command-line parsing.
pub mod cli;
/// Runtime configuration types.
pub mod config;
/// Tunnel confirmation and editing flow.
pub mod confirm;
/// URL detection for callback discovery.
pub mod detect;
/// Origin resolution from CLI flags and SSH environment.
pub mod origin;
/// Runtime orchestration and PTY handling.
pub mod runtime;
/// User-facing status message formatting.
pub mod status;
/// SSH tunnel launching and lifecycle management.
pub mod tunnel;

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use crate::config::{Config, Mode};

    #[test]
    fn parses_origin_user_and_wrapped_command() {
        let config: Config = Cli::try_parse_from([
            "hitch", "--user", "alice", "--origin", "10.0.0.5", "--", "aws", "login",
        ])
        .unwrap()
        .try_into()
        .unwrap();

        assert_eq!(config.user.as_deref(), Some("alice"));
        assert_eq!(config.origin.as_deref(), Some("10.0.0.5"));
        assert_eq!(
            config.mode,
            Mode::Command {
                command: vec!["aws".to_string(), "login".to_string()],
            }
        );
    }

    #[test]
    fn parses_wrapped_command_without_overrides() {
        let config: Config = Cli::try_parse_from(["hitch", "--", "aws", "login"])
            .unwrap()
            .try_into()
            .unwrap();

        assert_eq!(config.user, None);
        assert_eq!(config.origin, None);
        assert_eq!(
            config.mode,
            Mode::Command {
                command: vec!["aws".to_string(), "login".to_string()],
            }
        );
    }

    #[test]
    fn parses_port_mode_without_command() {
        let config: Config = Cli::try_parse_from(["hitch", "--port", "38983"])
            .unwrap()
            .try_into()
            .unwrap();

        assert_eq!(config.origin, None);
        assert_eq!(config.user, None);
        assert_eq!(config.mode, Mode::Port { port: 38983 });
    }

    #[test]
    fn parses_port_mode_with_origin_and_user() {
        let config: Config = Cli::try_parse_from([
            "hitch", "--port", "38983", "--origin", "10.0.0.5", "--user", "alice",
        ])
        .unwrap()
        .try_into()
        .unwrap();

        assert_eq!(config.origin.as_deref(), Some("10.0.0.5"));
        assert_eq!(config.user.as_deref(), Some("alice"));
        assert_eq!(config.mode, Mode::Port { port: 38983 });
    }

    #[test]
    fn rejects_missing_wrapped_command() {
        let error = Cli::try_parse_from(["hitch"]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn rejects_wrapped_command_without_double_dash() {
        let error = Cli::try_parse_from(["hitch", "aws", "login"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn rejects_combining_port_mode_with_wrapped_command() {
        let error =
            Cli::try_parse_from(["hitch", "--port", "38983", "--", "aws", "login"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn rejects_port_zero() {
        let error = Cli::try_parse_from(["hitch", "--port", "0"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn help_mentions_hitch_flags_and_wrapped_command_separator() {
        let mut command = Cli::command();
        let mut help = Vec::new();
        command.write_long_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();

        assert!(help.contains("--origin <HOST>"));
        assert!(help.contains("--user <SSH_USER>"));
        assert!(help.contains("hitch [OPTIONS] -- <COMMAND> [ARGS]..."));
        assert!(help.contains("--port <PORT>"));
        assert!(help.contains("hitch [OPTIONS] --port <PORT>"));
    }

    #[test]
    fn readme_mentions_port_mode_usage() {
        let readme = include_str!("../README.md");

        assert!(readme.contains("hitch [--origin <host>] [--user <ssh-user>] --port <port>"));
        assert!(readme.contains("hitch --origin 203.0.113.10 --port 38983"));
        assert!(readme.contains("asks you to confirm the detected tunnel details"));
        assert!(readme.contains("curl -fsSL https://hitch.sh | bash"));
        assert!(readme.contains("cargo install hitch --locked"));
    }
}
