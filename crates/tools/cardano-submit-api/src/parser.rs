//! CLI argument parser for the `cardano-submit-api` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parser shell wrapping the
//! upstream optparse-applicative parser embedded in
//! `cardano-submit-api/app/Main.hs::main` + `Cardano/TxSubmit/CLI/Parsers.hs`.
//! clap can't match optparse-applicative's exact help-text format
//! byte-for-byte (different conventions for flag rendering, ANSI
//! escape codes, multi-line option descriptions), so this module
//! bypasses clap's auto-generated `--help` / `--version` and emits
//! hand-crafted byte-equivalent output captured from the upstream
//! binary at R335. The captured fixtures live at
//! `crates/cardano-submit-api/tests/fixtures/upstream-{help,version}.txt`
//! and are the source of truth for both the runtime help-printing
//! path and the golden tests.

/// Byte-for-byte mirror of upstream `cardano-submit-api --help`
/// (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `cardano-submit-api --version`
/// (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments for the `cardano-submit-api` binary.
///
/// R335 skeleton: holds raw string fields for every flag upstream
/// supports. Concrete typed fields (PathBuf, NetworkMagic, etc.)
/// land at R337 alongside the full Types port.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Args {
    /// `--config FILEPATH` — path to the tx-submit web API
    /// configuration file (mandatory in real use).
    pub config: Option<String>,
    /// `--socket-path FILEPATH` — path to a cardano-node socket
    /// (mandatory in real use).
    pub socket_path: Option<String>,
    /// Network magic discriminator: `--mainnet` (sentinel value)
    /// or `--testnet-magic NATURAL`.
    pub network_magic: Option<NetworkMagic>,
    /// `--listen-address HOST` — bind address for the API server.
    /// Default `127.0.0.1`.
    pub listen_address: Option<String>,
    /// `--port INT` — API server port. Default `8090`.
    pub port: Option<u16>,
    /// `--metrics-port PORT` — Prometheus metrics port. Default
    /// `8081`.
    pub metrics_port: Option<u16>,
    /// `--epoch-slots SLOTS` — Byron-era slots-per-epoch. Default
    /// `21600`.
    pub epoch_slots: Option<u64>,
}

/// Network magic discriminator. Mirrors upstream's `NetworkId`
/// surface as exposed via `--mainnet` (sentinel) or
/// `--testnet-magic <natural>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkMagic {
    /// `--mainnet` — uses the canonical mainnet magic id.
    Mainnet,
    /// `--testnet-magic <natural>` — caller-supplied magic id.
    Testnet(u32),
}

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` / `-v` was seen.
    #[error("(--version requested)")]
    VersionRequested,
    /// An unknown flag was passed.
    #[error("Invalid option `{0}'")]
    UnknownFlag(String),
    /// A flag requiring a value was passed without one.
    #[error("flag `{0}' requires a value")]
    MissingValue(String),
    /// A flag's value failed to parse.
    #[error("flag `{0}' has invalid value: {1}")]
    InvalidValue(String, String),
}

/// Parse a slice of command-line arguments into [`Args`].
///
/// R335 skeleton: handles the full upstream surface (--config,
/// --socket-path, --mainnet/--testnet-magic, --listen-address,
/// --port, --metrics-port, --epoch-slots, --cardano-mode, --help,
/// --version). Validation of mandatory flags lands at R336.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Args::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => return Err(ParseError::VersionRequested),
            "--mainnet" => out.network_magic = Some(NetworkMagic::Mainnet),
            "--cardano-mode" => {
                // Implicit default; upstream documents it but no value needed.
            }
            "--config" => {
                let v = take_value(&mut iter, &arg)?;
                out.config = Some(v);
            }
            "--socket-path" => {
                let v = take_value(&mut iter, &arg)?;
                out.socket_path = Some(v);
            }
            "--listen-address" => {
                let v = take_value(&mut iter, &arg)?;
                out.listen_address = Some(v);
            }
            "--port" => {
                let v = take_value(&mut iter, &arg)?;
                out.port = Some(parse_u16(&arg, &v)?);
            }
            "--metrics-port" => {
                let v = take_value(&mut iter, &arg)?;
                out.metrics_port = Some(parse_u16(&arg, &v)?);
            }
            "--testnet-magic" => {
                let v = take_value(&mut iter, &arg)?;
                let magic: u32 = v.parse().map_err(|e: std::num::ParseIntError| {
                    ParseError::InvalidValue(arg.clone(), e.to_string())
                })?;
                out.network_magic = Some(NetworkMagic::Testnet(magic));
            }
            "--epoch-slots" => {
                let v = take_value(&mut iter, &arg)?;
                let slots: u64 = v.parse().map_err(|e: std::num::ParseIntError| {
                    ParseError::InvalidValue(arg.clone(), e.to_string())
                })?;
                out.epoch_slots = Some(slots);
            }
            other if other.starts_with('-') => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
            other => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
        }
    }

    Ok(out)
}

fn take_value<I, S>(iter: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ParseError>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    iter.next()
        .map(|v| v.as_ref().to_string())
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

fn parse_u16(flag: &str, value: &str) -> Result<u16, ParseError> {
    value.parse().map_err(|e: std::num::ParseIntError| {
        ParseError::InvalidValue(flag.to_string(), e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_help_long() {
        assert_eq!(parse_args(["--help"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_help_short() {
        assert_eq!(parse_args(["-h"]), Err(ParseError::HelpRequested));
    }

    #[test]
    fn detects_version_long() {
        assert_eq!(parse_args(["--version"]), Err(ParseError::VersionRequested));
    }

    #[test]
    fn parses_mainnet_flag() {
        let args = parse_args(["--mainnet"]).expect("parses");
        assert_eq!(args.network_magic, Some(NetworkMagic::Mainnet));
    }

    #[test]
    fn parses_testnet_magic() {
        let args = parse_args(["--testnet-magic", "1"]).expect("parses");
        assert_eq!(args.network_magic, Some(NetworkMagic::Testnet(1)));
    }

    #[test]
    fn parses_full_canonical_invocation() {
        let args = parse_args([
            "--config",
            "/etc/submit-api.json",
            "--mainnet",
            "--socket-path",
            "/run/cardano-node.socket",
            "--port",
            "8090",
            "--metrics-port",
            "8081",
        ])
        .expect("parses");
        assert_eq!(args.config.as_deref(), Some("/etc/submit-api.json"));
        assert_eq!(
            args.socket_path.as_deref(),
            Some("/run/cardano-node.socket")
        );
        assert_eq!(args.network_magic, Some(NetworkMagic::Mainnet));
        assert_eq!(args.port, Some(8090));
        assert_eq!(args.metrics_port, Some(8081));
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(matches!(
            parse_args(["--definitely-not-real"]),
            Err(ParseError::UnknownFlag(_)),
        ));
    }

    #[test]
    fn rejects_missing_value() {
        assert!(matches!(
            parse_args(["--config"]),
            Err(ParseError::MissingValue(_)),
        ));
    }

    #[test]
    fn rejects_invalid_port() {
        assert!(matches!(
            parse_args(["--port", "not-a-number"]),
            Err(ParseError::InvalidValue(_, _)),
        ));
    }

    #[test]
    fn help_text_starts_with_canonical_usage_line() {
        assert!(HELP_TEXT.starts_with("Usage: cardano-submit-api"));
    }

    #[test]
    fn version_text_matches_upstream() {
        assert!(VERSION_TEXT.starts_with("cardano-submit-api 11.0.0"));
    }
}
