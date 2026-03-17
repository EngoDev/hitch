//! Tunnel confirmation and edit flow for auto-detected callback tunnels.

use std::io;

use inquire::{Confirm, Text, validator::Validation};

/// Tunnel launch settings that may be auto-detected and optionally edited by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelConfig {
    /// Local callback port to expose through the reverse tunnel.
    pub port: u16,
    /// Optional SSH user override for the reverse tunnel destination.
    pub user: Option<String>,
    /// SSH origin host or IP address to connect back to.
    pub origin: String,
}

/// Interactive editor used to confirm or correct auto-detected tunnel settings.
pub trait TunnelConfigEditor: Send {
    /// Asks whether the detected configuration looks correct as-is.
    fn confirm_detected_config(&mut self, config: &TunnelConfig) -> io::Result<bool>;
    /// Prompts for a replacement callback port.
    fn edit_port(&mut self, current: u16) -> io::Result<String>;
    /// Prompts for a replacement SSH user.
    fn edit_user(&mut self, current: Option<&str>) -> io::Result<String>;
    /// Prompts for a replacement origin host or IP.
    fn edit_origin(&mut self, current: &str) -> io::Result<String>;
}

/// Runs the confirmation flow and returns the final tunnel configuration to use.
pub fn confirm_tunnel_config<E: TunnelConfigEditor + ?Sized>(
    editor: &mut E,
    detected: TunnelConfig,
) -> io::Result<TunnelConfig> {
    if editor.confirm_detected_config(&detected)? {
        return Ok(detected);
    }

    let port = parse_port(&editor.edit_port(detected.port)?)?;
    let user = parse_user(&editor.edit_user(detected.user.as_deref())?);
    let origin = parse_origin(&editor.edit_origin(&detected.origin)?)?;

    Ok(TunnelConfig { port, user, origin })
}

/// Production confirmation editor implemented with `inquire`.
pub struct InquireTunnelConfigEditor;

impl TunnelConfigEditor for InquireTunnelConfigEditor {
    fn confirm_detected_config(&mut self, config: &TunnelConfig) -> io::Result<bool> {
        eprintln!(
            "[hitch] Detected tunnel details:\n[hitch]   port: {}\n[hitch]   user: {}\n[hitch]   origin: {}",
            config.port,
            config.user.as_deref().unwrap_or("<default>"),
            config.origin
        );
        Confirm::new(&format!(
            "Do these tunnel details look correct? port={}, user={}, origin={}",
            config.port,
            config.user.as_deref().unwrap_or("<default>"),
            config.origin
        ))
        .with_default(true)
        .prompt()
        .map_err(inquire_to_io_error)
    }

    fn edit_port(&mut self, current: u16) -> io::Result<String> {
        Text::new("Port")
            .with_initial_value(&current.to_string())
            .with_validator(|value: &str| match parse_port(value) {
                Ok(_) => Ok(Validation::Valid),
                Err(_) => Ok(Validation::Invalid("Port must be a valid TCP port".into())),
            })
            .prompt()
            .map_err(inquire_to_io_error)
    }

    fn edit_user(&mut self, current: Option<&str>) -> io::Result<String> {
        Text::new("User")
            .with_initial_value(current.unwrap_or(""))
            .prompt()
            .map_err(inquire_to_io_error)
    }

    fn edit_origin(&mut self, current: &str) -> io::Result<String> {
        Text::new("Origin")
            .with_initial_value(current)
            .with_validator(|value: &str| {
                if value.trim().is_empty() {
                    Ok(Validation::Invalid("Origin cannot be empty".into()))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()
            .map_err(inquire_to_io_error)
    }
}

/// Converts an `inquire` error into an I/O error suitable for runtime propagation.
fn inquire_to_io_error(error: inquire::InquireError) -> io::Error {
    io::Error::other(error.to_string())
}

/// Parses and validates a tunnel port, rejecting invalid values and port `0`.
fn parse_port(value: &str) -> io::Result<u16> {
    let port = value.trim().parse::<u16>().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "Port must be a valid TCP port")
    })?;

    if port == 0 {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Port must be between 1 and 65535",
        ))
    } else {
        Ok(port)
    }
}

/// Normalizes an edited SSH user string, treating empty input as “no explicit user”.
fn parse_user(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parses and validates an edited origin host or IP.
fn parse_origin(value: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Origin cannot be empty",
        ))
    } else {
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{TunnelConfig, TunnelConfigEditor, confirm_tunnel_config};

    #[derive(Default)]
    struct ScriptedEditor {
        confirm: bool,
        port_response: Option<String>,
        user_response: Option<String>,
        origin_response: Option<String>,
        call_order: Vec<&'static str>,
    }

    impl TunnelConfigEditor for ScriptedEditor {
        fn confirm_detected_config(&mut self, _config: &TunnelConfig) -> io::Result<bool> {
            self.call_order.push("confirm");
            Ok(self.confirm)
        }

        fn edit_port(&mut self, _current: u16) -> io::Result<String> {
            self.call_order.push("port");
            Ok(self.port_response.take().unwrap())
        }

        fn edit_user(&mut self, _current: Option<&str>) -> io::Result<String> {
            self.call_order.push("user");
            Ok(self.user_response.take().unwrap())
        }

        fn edit_origin(&mut self, _current: &str) -> io::Result<String> {
            self.call_order.push("origin");
            Ok(self.origin_response.take().unwrap())
        }
    }

    #[test]
    fn accepts_detected_values_without_edits() {
        let detected = TunnelConfig {
            port: 3001,
            user: Some("engodev".into()),
            origin: "100.70.126.5".into(),
        };
        let mut editor = ScriptedEditor {
            confirm: true,
            ..Default::default()
        };

        let result = confirm_tunnel_config(&mut editor, detected.clone()).unwrap();

        assert_eq!(result, detected);
        assert_eq!(editor.call_order, vec!["confirm"]);
    }

    #[test]
    fn applies_edits_in_port_user_origin_order() {
        let detected = TunnelConfig {
            port: 3001,
            user: Some("engodev".into()),
            origin: "100.70.126.5".into(),
        };
        let mut editor = ScriptedEditor {
            confirm: false,
            port_response: Some("4000".into()),
            user_response: Some("alice".into()),
            origin_response: Some("203.0.113.10".into()),
            ..Default::default()
        };

        let result = confirm_tunnel_config(&mut editor, detected).unwrap();

        assert_eq!(
            result,
            TunnelConfig {
                port: 4000,
                user: Some("alice".into()),
                origin: "203.0.113.10".into(),
            }
        );
        assert_eq!(editor.call_order, vec!["confirm", "port", "user", "origin"]);
    }

    #[test]
    fn empty_user_clears_explicit_ssh_user() {
        let detected = TunnelConfig {
            port: 3001,
            user: Some("engodev".into()),
            origin: "100.70.126.5".into(),
        };
        let mut editor = ScriptedEditor {
            confirm: false,
            port_response: Some("3001".into()),
            user_response: Some(String::new()),
            origin_response: Some("100.70.126.5".into()),
            ..Default::default()
        };

        let result = confirm_tunnel_config(&mut editor, detected).unwrap();

        assert_eq!(result.user, None);
    }

    #[test]
    fn rejects_invalid_port_input() {
        let detected = TunnelConfig {
            port: 3001,
            user: Some("engodev".into()),
            origin: "100.70.126.5".into(),
        };
        let mut editor = ScriptedEditor {
            confirm: false,
            port_response: Some("not-a-port".into()),
            user_response: Some("engodev".into()),
            origin_response: Some("100.70.126.5".into()),
            ..Default::default()
        };

        let error = confirm_tunnel_config(&mut editor, detected).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_zero_port_input() {
        let detected = TunnelConfig {
            port: 3001,
            user: Some("engodev".into()),
            origin: "100.70.126.5".into(),
        };
        let mut editor = ScriptedEditor {
            confirm: false,
            port_response: Some("0".into()),
            user_response: Some("engodev".into()),
            origin_response: Some("100.70.126.5".into()),
            ..Default::default()
        };

        let error = confirm_tunnel_config(&mut editor, detected).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
