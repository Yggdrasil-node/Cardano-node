//! REST API request / response types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Rest/Types.hs.
//!
//! Direct ports:
//!
//! - [`WebserverConfig`] — `data WebserverConfig = WebserverConfig { wcHost, wcPort }`.
//!   Upstream uses `Warp.HostPreference` + `Warp.Port`; the Rust port
//!   keeps the same shape with `String` + `u16` plus a `to_socket_addr`
//!   helper that mirrors upstream `toWarpSettings`'s role of bridging
//!   the config struct into a server-binding-ready value.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Warp.HostPreference` / `Warp.Port` / `Warp.Settings` — upstream's
//!   web server is Warp; the Rust port targets axum (R341), so the
//!   bridge type is [`std::net::SocketAddr`] instead of `Warp.Settings`.
//!   The semantic role — "fully-resolved bind address" — is the same.

use std::fmt;
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::str::FromStr;

/// Bind-address + port for the cardano-submit-api HTTP server.
///
/// Upstream: `data WebserverConfig = WebserverConfig { wcHost :: Warp.HostPreference, wcPort :: Warp.Port }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebserverConfig {
    /// Bind host. Accepts an IPv4/IPv6 literal or `*` (wildcard).
    pub host: String,
    /// Bind port.
    pub port: u16,
}

impl WebserverConfig {
    /// Construct a config from a host string and port.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        WebserverConfig {
            host: host.into(),
            port,
        }
    }

    /// Resolve `host:port` to a [`SocketAddr`].
    ///
    /// `host` must be an IPv4 / IPv6 literal, or one of the wildcard
    /// sentinels (`*`, `0.0.0.0`, `::`) which are mapped to
    /// [`IpAddr::V4(0.0.0.0)`].
    ///
    /// Mirrors the role of upstream `toWarpSettings`: turn the loose
    /// configuration record into a server-binding-ready value. Under
    /// axum, that value is `SocketAddr`; under Warp, it's
    /// `Warp.Settings`. The semantic mapping is otherwise identical.
    pub fn to_socket_addr(&self) -> Result<SocketAddr, AddrParseError> {
        let ip = match self.host.as_str() {
            "*" | "0.0.0.0" | "::" => IpAddr::from([0, 0, 0, 0]),
            other => IpAddr::from_str(other)?,
        };
        Ok(SocketAddr::new(ip, self.port))
    }
}

impl fmt::Display for WebserverConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_upstream_show_instance() {
        let config = WebserverConfig::new("127.0.0.1", 8090);
        assert_eq!(config.to_string(), "127.0.0.1:8090");
    }

    #[test]
    fn ipv4_literal_resolves_to_socket_addr() {
        let config = WebserverConfig::new("127.0.0.1", 8090);
        let addr = config.to_socket_addr().expect("resolves");
        assert_eq!(addr.to_string(), "127.0.0.1:8090");
    }

    #[test]
    fn wildcard_star_resolves_to_unspecified_v4() {
        let config = WebserverConfig::new("*", 8090);
        let addr = config.to_socket_addr().expect("resolves");
        assert_eq!(addr.ip().to_string(), "0.0.0.0");
        assert_eq!(addr.port(), 8090);
    }

    #[test]
    fn wildcard_zero_zero_zero_zero_resolves_to_unspecified_v4() {
        let config = WebserverConfig::new("0.0.0.0", 8090);
        let addr = config.to_socket_addr().expect("resolves");
        assert_eq!(addr.ip().to_string(), "0.0.0.0");
    }

    #[test]
    fn ipv6_literal_resolves() {
        let config = WebserverConfig::new("::1", 9090);
        let addr = config.to_socket_addr().expect("resolves");
        assert_eq!(addr.to_string(), "[::1]:9090");
    }

    #[test]
    fn invalid_host_string_returns_parse_error() {
        let config = WebserverConfig::new("not-an-ip", 8090);
        assert!(config.to_socket_addr().is_err());
    }

    #[test]
    fn webserver_config_clone_eq() {
        let a = WebserverConfig::new("127.0.0.1", 8090);
        let b = a.clone();
        assert_eq!(a, b);
    }
}
