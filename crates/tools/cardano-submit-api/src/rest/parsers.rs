//! REST request parsers (CBOR body decode, content-type negotiation).
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Rest/Parsers.hs.
//!
//! Direct ports:
//!
//! - [`from_args`] — fold the parsed CLI [`Args`] into a fully-specified
//!   [`WebserverConfig`]. Upstream `pWebserverConfig defaultPort` is an
//!   `Options.Applicative.Parser` that parallel-parses
//!   `--listen-address` + `--port` directly into `WebserverConfig`. The
//!   Rust crate's [`crate::parser::parse_args`] already consumes the
//!   raw argv; this module's role is to bridge the parsed surface into
//!   the typed config.
//!
//! Defaults match upstream:
//!
//! | Flag                | Upstream default           | Yggdrasil constant                |
//! |---------------------|----------------------------|-----------------------------------|
//! | `--listen-address`  | `127.0.0.1`                | [`DEFAULT_LISTEN_ADDRESS`]        |
//! | `--port`            | caller-supplied            | [`from_args`]'s `default_port`    |

use crate::parser::Args;
use crate::rest::types::WebserverConfig;

/// Default bind host — matches upstream's `value "127.0.0.1" <> showDefault`.
pub const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1";

/// Build a [`WebserverConfig`] from parsed CLI [`Args`].
///
/// `default_port` is supplied by the caller because upstream
/// `pWebserverConfig defaultPort` parameterizes its `Port` parser the
/// same way. Tx-submit uses `8090`; other call sites may override.
pub fn from_args(args: &Args, default_port: u16) -> WebserverConfig {
    WebserverConfig::new(
        args.listen_address
            .clone()
            .unwrap_or_else(|| DEFAULT_LISTEN_ADDRESS.to_string()),
        args.port.unwrap_or(default_port),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_args_use_defaults() {
        let args = Args::default();
        let config = from_args(&args, 8090);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8090);
    }

    #[test]
    fn listen_address_overrides_default() {
        let args = Args {
            listen_address: Some("0.0.0.0".to_string()),
            ..Args::default()
        };
        let config = from_args(&args, 8090);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn port_overrides_default() {
        let args = Args {
            port: Some(9090),
            ..Args::default()
        };
        let config = from_args(&args, 8090);
        assert_eq!(config.port, 9090);
    }

    #[test]
    fn both_overrides_apply() {
        let args = Args {
            listen_address: Some("::1".to_string()),
            port: Some(9090),
            ..Args::default()
        };
        let config = from_args(&args, 8090);
        assert_eq!(config.host, "::1");
        assert_eq!(config.port, 9090);
    }
}
