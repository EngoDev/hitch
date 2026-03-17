use std::sync::OnceLock;

use regex::Regex;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionEvent {
    StartTunnel {
        host: String,
        port: u16,
        url: String,
    },
    WarnNonLoopback {
        host: String,
        url: String,
    },
    WarnMissingPort {
        host: String,
        url: String,
    },
}

#[derive(Debug, Default)]
pub struct UrlDetector {
    tunnel_started: bool,
    emitted_non_loopback_warning: bool,
    emitted_missing_port_warning: bool,
}

impl UrlDetector {
    pub fn consume(&mut self, text: &str) -> Option<DetectionEvent> {
        if self.tunnel_started {
            return None;
        }

        let mut pending_warning = None;

        for candidate in url_regex().find_iter(text) {
            for url in candidate_urls(candidate.as_str()) {
                let Some(host) = display_host(&url) else {
                    continue;
                };

                if is_loopback_host(&host) {
                    if let Some(port) = url.port() {
                        self.tunnel_started = true;
                        return Some(DetectionEvent::StartTunnel {
                            host,
                            port,
                            url: url.to_string(),
                        });
                    }

                    if !self.emitted_missing_port_warning && pending_warning.is_none() {
                        pending_warning = Some(DetectionEvent::WarnMissingPort {
                            host,
                            url: url.to_string(),
                        });
                    }
                } else if !self.emitted_non_loopback_warning && pending_warning.is_none() {
                    pending_warning = Some(DetectionEvent::WarnNonLoopback {
                        host,
                        url: url.to_string(),
                    });
                }
            }
        }

        match pending_warning {
            Some(DetectionEvent::WarnNonLoopback { host, url }) => {
                self.emitted_non_loopback_warning = true;
                Some(DetectionEvent::WarnNonLoopback { host, url })
            }
            Some(DetectionEvent::WarnMissingPort { host, url }) => {
                self.emitted_missing_port_warning = true;
                Some(DetectionEvent::WarnMissingPort { host, url })
            }
            Some(DetectionEvent::StartTunnel { .. }) => unreachable!(),
            None => None,
        }
    }
}

fn candidate_urls(text: &str) -> Vec<Url> {
    let Ok(url) = Url::parse(text) else {
        return Vec::new();
    };

    let mut urls = Vec::new();
    collect_candidate_urls(&url, &mut urls);
    urls
}

fn collect_candidate_urls(url: &Url, urls: &mut Vec<Url>) {
    urls.push(url.clone());

    for (_, value) in url.query_pairs() {
        let value = value.trim();
        if !value.starts_with("http://") && !value.starts_with("https://") {
            continue;
        }

        if let Ok(nested) = Url::parse(value) {
            collect_candidate_urls(&nested, urls);
        }
    }
}

fn url_regex() -> &'static Regex {
    static URL_REGEX: OnceLock<Regex> = OnceLock::new();
    URL_REGEX.get_or_init(|| Regex::new(r#"https?://[^\s<>\")']+"#).unwrap())
}

fn display_host(url: &Url) -> Option<String> {
    url.host_str().map(|host| {
        if host.starts_with('[') && host.ends_with(']') {
            host.to_string()
        } else if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        }
    })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

#[cfg(test)]
mod tests {
    use super::{DetectionEvent, UrlDetector};

    #[test]
    fn extracts_loopback_url_from_surrounding_text() {
        let mut detector = UrlDetector::default();

        let event = detector.consume("Open this URL: http://localhost:4567/callback and continue.");

        assert_eq!(
            event,
            Some(DetectionEvent::StartTunnel {
                host: "localhost".to_string(),
                port: 4567,
                url: "http://localhost:4567/callback".to_string(),
            })
        );
    }

    #[test]
    fn accepts_ipv4_and_ipv6_loopback_hosts() {
        let mut ipv4 = UrlDetector::default();
        let mut ipv6 = UrlDetector::default();

        let ipv4_event = ipv4.consume("http://127.0.0.1:7000/callback");
        let ipv6_event = ipv6.consume("http://[::1]:9000/callback");

        assert_eq!(
            ipv4_event,
            Some(DetectionEvent::StartTunnel {
                host: "127.0.0.1".to_string(),
                port: 7000,
                url: "http://127.0.0.1:7000/callback".to_string(),
            })
        );
        assert_eq!(
            ipv6_event,
            Some(DetectionEvent::StartTunnel {
                host: "[::1]".to_string(),
                port: 9000,
                url: "http://[::1]:9000/callback".to_string(),
            })
        );
    }

    #[test]
    fn warns_when_redirect_url_is_not_loopback() {
        let mut detector = UrlDetector::default();

        let event = detector.consume("Open https://example.com/callback?code=123");

        assert_eq!(
            event,
            Some(DetectionEvent::WarnNonLoopback {
                host: "example.com".to_string(),
                url: "https://example.com/callback?code=123".to_string(),
            })
        );
    }

    #[test]
    fn warns_when_loopback_url_has_no_port() {
        let mut detector = UrlDetector::default();

        let event = detector.consume("Open http://localhost/callback");

        assert_eq!(
            event,
            Some(DetectionEvent::WarnMissingPort {
                host: "localhost".to_string(),
                url: "http://localhost/callback".to_string(),
            })
        );
    }

    #[test]
    fn first_valid_loopback_url_wins() {
        let mut detector = UrlDetector::default();

        let first =
            detector.consume("open http://example.com/auth then http://127.0.0.1:4567/callback");
        let second = detector.consume("again http://localhost:9999/callback");

        assert_eq!(
            first,
            Some(DetectionEvent::StartTunnel {
                host: "127.0.0.1".to_string(),
                port: 4567,
                url: "http://127.0.0.1:4567/callback".to_string(),
            })
        );
        assert_eq!(second, None);
    }

    #[test]
    fn repeated_valid_urls_do_not_trigger_duplicate_tunnels() {
        let mut detector = UrlDetector::default();

        let first = detector.consume("http://localhost:4567/callback");
        let second = detector.consume("http://localhost:4567/callback");

        assert!(matches!(first, Some(DetectionEvent::StartTunnel { .. })));
        assert_eq!(second, None);
    }

    #[test]
    fn detects_embedded_redirect_uri_inside_authorize_url() {
        let mut detector = UrlDetector::default();

        let event = detector.consume("https://il-central-1.signin.aws.amazon.com/v1/authorize?response_type=code&client_id=arn%3Aaws%3Asignin%3A%3A%3Adevtools%2Fsame-device&state=f6132fe0-f1b4-4c0f-9768-bbf658a12468&code_challenge_method=SHA-256&scope=openid&redirect_uri=http%3A%2F%2F127.0.0.1%3A46625%2Foauth%2Fcallback&code_challenge=XwjseNZvJUrH2AHPqKfgAR3TfWEfH1Yqh_CcqsG-h1Y");

        assert_eq!(
            event,
            Some(DetectionEvent::StartTunnel {
                host: "127.0.0.1".to_string(),
                port: 46625,
                url: "http://127.0.0.1:46625/oauth/callback".to_string(),
            })
        );
    }
}
