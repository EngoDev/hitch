pub fn found_loopback_url(url: &str, host: &str, port: u16) -> String {
    format!("Found localhost redirect URL: {url} (host: {host}, port: {port})")
}

pub fn starting_tunnel(destination: &str, port: u16) -> String {
    format!("Starting reverse tunnel for port {port} to {destination}")
}

pub fn opening_direct_tunnel(port: u16) -> String {
    format!("Opening reverse tunnel for requested port {port}")
}

pub fn non_loopback_redirect(url: &str, host: &str) -> String {
    format!(
        "Found redirect URL {url} with host {host}, but it does not lead to localhost. Check the original login command configuration."
    )
}

pub fn missing_callback_port(url: &str, host: &str) -> String {
    format!(
        "Found redirect URL {url} with localhost host {host}, but no callback port was present."
    )
}

pub fn missing_origin() -> String {
    "could not determine tunnel origin: pass --origin or run hitch inside an SSH session"
        .to_string()
}

pub fn tunnel_launch_failed(error: &std::io::Error) -> String {
    format!("Could not establish tunnel: {error}")
}

#[cfg(test)]
mod tests {
    use super::{missing_origin, non_loopback_redirect};

    #[test]
    fn non_loopback_status_mentions_localhost_configuration() {
        let message = non_loopback_redirect("https://example.com/callback", "example.com");

        assert!(message.contains("does not lead to localhost"));
        assert!(message.contains("Check the original login command configuration"));
    }

    #[test]
    fn missing_origin_status_explains_how_to_fix_it() {
        let message = missing_origin();

        assert!(message.contains("could not determine tunnel origin"));
        assert!(message.contains("--origin"));
        assert!(message.contains("SSH session"));
    }
}
