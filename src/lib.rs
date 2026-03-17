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

    #[test]
    fn parses_origin_user_and_wrapped_command() {
        let config = Cli::try_parse_from([
            "hitch", "--user", "alice", "--origin", "10.0.0.5", "--", "aws", "login",
        ])
        .unwrap()
        .into_config();

        assert_eq!(config.user.as_deref(), Some("alice"));
        assert_eq!(config.origin.as_deref(), Some("10.0.0.5"));
        assert_eq!(config.command, vec!["aws", "login"]);
    }

    #[test]
    fn parses_wrapped_command_without_overrides() {
        let config = Cli::try_parse_from(["hitch", "--", "aws", "login"])
            .unwrap()
            .into_config();

        assert_eq!(config.user, None);
        assert_eq!(config.origin, None);
        assert_eq!(config.command, vec!["aws", "login"]);
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
    fn help_mentions_hitch_flags_and_wrapped_command_separator() {
        let mut command = Cli::command();
        let mut help = Vec::new();
        command.write_long_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();

        assert!(help.contains("--origin <HOST>"));
        assert!(help.contains("--user <SSH_USER>"));
        assert!(help.contains("hitch [OPTIONS] -- <COMMAND> [ARGS]..."));
    }
}
