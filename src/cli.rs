//! Command-line interface.

use std::time::Duration;

use clap::{Parser, ValueEnum};

/// The slow-HTTP attack variant to run.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Mode {
    /// Slowloris: open connections and dribble out request *headers*, never
    /// sending the terminating blank line. (slowhttptest `-H`)
    Headers,
    /// Slow POST: send a large `Content-Length` then dribble out the request
    /// *body* far slower than advertised. (slowhttptest `-B`)
    Body,
    /// Slow read: send a complete request, advertise a tiny receive window and
    /// read the response a few bytes at a time. (slowhttptest `-R`)
    Read,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Mode::Headers => "headers (Slowloris)",
            Mode::Body => "body (slow POST)",
            Mode::Read => "read (slow read)",
        };
        f.write_str(s)
    }
}

/// A small, statically-linkable slow-HTTP resilience tester.
///
/// Intended for AUTHORIZED testing of servers you own or have written
/// permission to test. Running this against systems without authorization may
/// be illegal.
#[derive(Parser, Debug)]
#[command(name = "slowloris", version, about, long_about = None)]
pub(crate) struct Args {
    /// Target URL, e.g. `http://127.0.0.1:8080/` or `https://example.test`.
    pub(crate) url: String,

    /// Attack mode.
    #[arg(short, long, value_enum, default_value_t = Mode::Headers)]
    pub(crate) mode: Mode,

    /// Number of concurrent connections to maintain.
    #[arg(short = 'c', long, default_value_t = 150)]
    pub(crate) connections: usize,

    /// Seconds between follow-up data on each connection (the "slow" interval).
    #[arg(short = 'i', long, default_value_t = 15)]
    pub(crate) interval: u64,

    /// New connections to open per second while ramping up / replacing dead ones.
    #[arg(short = 'r', long, default_value_t = 50)]
    pub(crate) rate: usize,

    /// Stop after this many seconds (0 = run until Ctrl-C).
    #[arg(short = 'l', long, default_value_t = 0)]
    pub(crate) duration: u64,

    /// Request path override (otherwise taken from the URL).
    #[arg(long)]
    pub(crate) path: Option<String>,

    /// Advertised Content-Length for `body` mode (bytes dribbled out slowly).
    #[arg(long, default_value_t = 8192)]
    pub(crate) content_length: usize,

    /// Per-socket connect/IO timeout in seconds.
    #[arg(long, default_value_t = 10)]
    pub(crate) timeout: u64,

    /// User-Agent header value (a random desktop UA is used if unset).
    #[arg(long)]
    pub(crate) user_agent: Option<String>,

    /// Skip TLS certificate verification (for self-signed / internal test hosts).
    #[arg(short = 'k', long)]
    pub(crate) insecure: bool,

    /// Print per-connection detail.
    #[arg(short, long)]
    pub(crate) verbose: bool,
}

impl Args {
    pub(crate) fn interval(&self) -> Duration {
        Duration::from_secs(self.interval.max(1))
    }
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout.max(1))
    }
}

/// A parsed target: scheme, host, port, path.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) tls: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

impl Target {
    /// Parse a URL. Accepts `http://` / `https://`; a bare `host:port` is
    /// treated as plain HTTP.
    pub(crate) fn parse(url: &str, path_override: Option<&str>) -> Result<Target, String> {
        let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (false, r)
        } else {
            (false, url)
        };

        // Split host[:port] from the path. `find` returns a byte index at the
        // start of '/', which is a valid char boundary for `split_at`.
        let (authority, url_path) = match rest.find('/') {
            Some(idx) => rest.split_at(idx),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(format!("could not parse host from URL: {url:?}"));
        }

        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p
                    .parse()
                    .map_err(|_| format!("invalid port in URL: {p:?}"))?;
                (h.to_string(), port)
            }
            None => (authority.to_string(), if tls { 443 } else { 80 }),
        };

        let path = path_override.map_or_else(|| url_path.to_string(), ToString::to_string);

        Ok(Target {
            tls,
            host,
            port,
            path,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests assert on fixed, known-valid inputs; a failed parse surfacing as a panic is the test failing"
)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_with_port_and_path() {
        let t = Target::parse("https://example.test:8443/foo", None).unwrap();
        assert!(t.tls);
        assert_eq!(t.host, "example.test");
        assert_eq!(t.port, 8443);
        assert_eq!(t.path, "/foo");
    }

    #[test]
    fn defaults_scheme_and_port() {
        let t = Target::parse("example.test", None).unwrap();
        assert!(!t.tls);
        assert_eq!(t.port, 80);
        assert_eq!(t.path, "/");

        let t = Target::parse("https://example.test", None).unwrap();
        assert_eq!(t.port, 443);
    }

    #[test]
    fn path_override_wins() {
        let t = Target::parse("http://h/orig", Some("/new")).unwrap();
        assert_eq!(t.path, "/new");
    }

    #[test]
    fn rejects_empty_host() {
        assert!(Target::parse("http:///path", None).is_err());
    }
}
