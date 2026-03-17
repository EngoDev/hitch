use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub struct ResolveOriginError;

impl Display for ResolveOriginError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not determine tunnel origin: pass --origin or run hitch inside an SSH session"
        )
    }
}

impl Error for ResolveOriginError {}

pub fn resolve_origin(
    cli_origin: Option<&str>,
    ssh_connection: Option<&str>,
) -> Result<String, ResolveOriginError> {
    if let Some(origin) = cli_origin {
        return Ok(origin.to_string());
    }

    ssh_connection
        .and_then(|value| value.split_whitespace().next())
        .map(str::to_string)
        .ok_or(ResolveOriginError)
}

#[cfg(test)]
mod tests {
    use super::resolve_origin;

    #[test]
    fn cli_origin_takes_precedence_over_ssh_connection() {
        let origin = resolve_origin(
            Some("10.0.0.5"),
            Some("203.0.113.10 51234 198.51.100.20 22"),
        )
        .unwrap();

        assert_eq!(origin, "10.0.0.5");
    }

    #[test]
    fn resolves_origin_from_ssh_connection() {
        let origin = resolve_origin(None, Some("203.0.113.10 51234 198.51.100.20 22")).unwrap();

        assert_eq!(origin, "203.0.113.10");
    }

    #[test]
    fn reports_missing_origin_when_no_input_is_available() {
        let error = resolve_origin(None, None).unwrap_err();

        assert_eq!(
            error.to_string(),
            "could not determine tunnel origin: pass --origin or run hitch inside an SSH session"
        );
    }
}
