pub mod cli;
pub mod config;
pub mod detect;
pub mod origin;
pub mod runtime;
pub mod status;
pub mod tunnel;

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use crate::config::Mode;

    #[test]
    fn parses_origin_user_and_wrapped_command() {
        let config = Cli::try_parse_from([
            "hitch", "--user", "alice", "--origin", "10.0.0.5", "--", "aws", "login",
        ])
        .unwrap()
        .into_config();

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
        let config = Cli::try_parse_from(["hitch", "--", "aws", "login"])
            .unwrap()
            .into_config();

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
        let config = Cli::try_parse_from(["hitch", "--port", "38983"])
            .unwrap()
            .into_config();

        assert_eq!(config.origin, None);
        assert_eq!(config.user, None);
        assert_eq!(config.mode, Mode::Port { port: 38983 });
    }

    #[test]
    fn parses_port_mode_with_origin_and_user() {
        let config = Cli::try_parse_from([
            "hitch", "--port", "38983", "--origin", "10.0.0.5", "--user", "alice",
        ])
        .unwrap()
        .into_config();

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
    }
}
