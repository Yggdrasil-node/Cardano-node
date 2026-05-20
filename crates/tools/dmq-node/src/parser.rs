//! CLI argument parser for the `dmq-node` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/Configuration/CLIOptions.hs.
//!
//! Direct port of upstream's `parseCLIOptions :: Parser PartialConfig`.
//! The 10-flag grammar accepted by upstream (modulo Rust identifier
//! conventions):
//!
//! | Upstream flag                          | Field                    |
//! |----------------------------------------|--------------------------|
//! | `--host-addr IPv4`                     | `host_addr`              |
//! | `--host-ipv6-addr IPv6`                | `host_ipv6_addr`         |
//! | `-p` / `--port PORT_NUMBER`            | `port_number`            |
//! | `--local-socket FILENAME`              | `local_address`          |
//! | `-c` / `--configuration-file FILENAME` | `config_file`            |
//! | `-t` / `--topology-file FILENAME`      | `topology_file`          |
//! | `--cardano-node-socket FILENAME`       | `cardano_node_socket`    |
//! | `--cardano-network-magic NATURAL`      | `cardano_network_magic`  |
//! | `--dmq-network-magic NATURAL`          | `network_magic`          |
//! | `-v` / `--version`                     | `show_version`           |
//! | `-h` / `--help`                        | (short-circuits)         |
//!
//! `--help` / `--version` text is byte-equivalent to the upstream
//! `dmq-node` binary; fixtures captured at R335 live at
//! `crates/tools/dmq-node/tests/fixtures/upstream-{help,version}.txt`.
//! Note that upstream wires `--version` as a switch that flips
//! `show_version: Some(true)`, so it does NOT short-circuit at parse
//! time; a separate parse-error variant is reserved for `--help`
//! since upstream's `helper` combinator does short-circuit on it.

use crate::types::{LocalAddress, NetworkMagic, PartialConfig};

/// Byte-for-byte mirror of upstream `dmq-node --help` (captured at R335).
pub const HELP_TEXT: &str = include_str!("../tests/fixtures/upstream-help.txt");

/// Byte-for-byte mirror of upstream `dmq-node --version` (captured at R335).
pub const VERSION_TEXT: &str = include_str!("../tests/fixtures/upstream-version.txt");

/// Parsed command-line arguments — the [`PartialConfig`] form before
/// merge with file-derived defaults.
///
/// Mirrors upstream `parseCLIOptions :: Parser PartialConfig`.
pub type Args = PartialConfig;

/// Errors from CLI argument parsing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// `--help` / `-h` was seen.
    #[error("(--help requested)")]
    HelpRequested,
    /// `--version` / `-v` was seen — emitted by the runtime
    /// help-printing path (the in-grammar `--version` flag is
    /// represented in [`Args::show_version`] for downstream
    /// dispatch). Upstream's `helper` combinator wraps `--help`
    /// only; `--version` flows through as a regular switch.
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

/// Parse a slice of command-line arguments into the [`PartialConfig`]
/// form. Mirror of upstream `parseCLIOptions`.
pub fn parse_args<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = PartialConfig::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        let arg = arg.as_ref().to_string();
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::HelpRequested),
            "-v" | "--version" => {
                out.show_version = Some(true);
            }
            "--host-addr" => {
                let v = take_value(&mut iter, &arg)?;
                out.host_addr = Some(v);
            }
            "--host-ipv6-addr" => {
                let v = take_value(&mut iter, &arg)?;
                out.host_ipv6_addr = Some(v);
            }
            "-p" | "--port" => {
                let v = take_value(&mut iter, &arg)?;
                out.port_number = Some(parse_u16(&arg, &v)?);
            }
            "--local-socket" => {
                let v = take_value(&mut iter, &arg)?;
                out.local_address = Some(LocalAddress::new(v));
            }
            "-c" | "--configuration-file" => {
                let v = take_value(&mut iter, &arg)?;
                out.config_file = Some(std::path::PathBuf::from(v));
            }
            "-t" | "--topology-file" => {
                let v = take_value(&mut iter, &arg)?;
                out.topology_file = Some(std::path::PathBuf::from(v));
            }
            "--cardano-node-socket" => {
                let v = take_value(&mut iter, &arg)?;
                out.cardano_node_socket = Some(std::path::PathBuf::from(v));
            }
            "--cardano-network-magic" => {
                let v = take_value(&mut iter, &arg)?;
                out.cardano_network_magic = Some(NetworkMagic(parse_u32(&arg, &v)?));
            }
            "--dmq-network-magic" => {
                let v = take_value(&mut iter, &arg)?;
                out.network_magic = Some(NetworkMagic(parse_u32(&arg, &v)?));
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

fn parse_u32(flag: &str, value: &str) -> Result<u32, ParseError> {
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
    fn version_long_flag_sets_show_version() {
        let args = parse_args(["--version"]).expect("parses");
        assert_eq!(args.show_version, Some(true));
    }

    #[test]
    fn version_short_flag_sets_show_version() {
        let args = parse_args(["-v"]).expect("parses");
        assert_eq!(args.show_version, Some(true));
    }

    #[test]
    fn parses_host_addr() {
        let args = parse_args(["--host-addr", "127.0.0.1"]).expect("parses");
        assert_eq!(args.host_addr.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn parses_host_ipv6_addr() {
        let args = parse_args(["--host-ipv6-addr", "::1"]).expect("parses");
        assert_eq!(args.host_ipv6_addr.as_deref(), Some("::1"));
    }

    #[test]
    fn parses_port_long() {
        let args = parse_args(["--port", "3001"]).expect("parses");
        assert_eq!(args.port_number, Some(3001));
    }

    #[test]
    fn parses_port_short() {
        let args = parse_args(["-p", "4002"]).expect("parses");
        assert_eq!(args.port_number, Some(4002));
    }

    #[test]
    fn parses_local_socket() {
        let args = parse_args(["--local-socket", "/tmp/dmq.sock"]).expect("parses");
        assert_eq!(
            args.local_address
                .as_ref()
                .map(|a| a.as_path().to_str().unwrap_or("")),
            Some("/tmp/dmq.sock")
        );
    }

    #[test]
    fn parses_configuration_file_long() {
        let args = parse_args(["--configuration-file", "/etc/dmq.json"]).expect("parses");
        assert_eq!(
            args.config_file.as_ref().map(|p| p.to_str().unwrap_or("")),
            Some("/etc/dmq.json")
        );
    }

    #[test]
    fn parses_configuration_file_short() {
        let args = parse_args(["-c", "/etc/dmq.json"]).expect("parses");
        assert_eq!(
            args.config_file.as_ref().map(|p| p.to_str().unwrap_or("")),
            Some("/etc/dmq.json")
        );
    }

    #[test]
    fn parses_topology_file_long() {
        let args = parse_args(["--topology-file", "/etc/dmq-topology.json"]).expect("parses");
        assert_eq!(
            args.topology_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/etc/dmq-topology.json")
        );
    }

    #[test]
    fn parses_topology_file_short() {
        let args = parse_args(["-t", "/etc/topo.json"]).expect("parses");
        assert_eq!(
            args.topology_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/etc/topo.json")
        );
    }

    #[test]
    fn parses_cardano_node_socket() {
        let args = parse_args(["--cardano-node-socket", "/run/cardano.socket"]).expect("parses");
        assert_eq!(
            args.cardano_node_socket
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/run/cardano.socket")
        );
    }

    #[test]
    fn parses_cardano_network_magic() {
        let args = parse_args(["--cardano-network-magic", "764824073"]).expect("parses");
        assert_eq!(args.cardano_network_magic, Some(NetworkMagic(764_824_073)));
    }

    #[test]
    fn parses_dmq_network_magic() {
        let args = parse_args(["--dmq-network-magic", "7"]).expect("parses");
        assert_eq!(args.network_magic, Some(NetworkMagic(7)));
    }

    #[test]
    fn parses_full_canonical_invocation() {
        let args = parse_args([
            "--host-addr",
            "0.0.0.0",
            "--port",
            "3001",
            "--local-socket",
            "dmq-node.socket",
            "--configuration-file",
            "dmq-node.json",
            "--topology-file",
            "dmq-node-topology.json",
            "--cardano-node-socket",
            "node.socket",
            "--cardano-network-magic",
            "764824073",
            "--dmq-network-magic",
            "0",
        ])
        .expect("parses");
        assert_eq!(args.host_addr.as_deref(), Some("0.0.0.0"));
        assert_eq!(args.port_number, Some(3001));
        assert_eq!(args.cardano_network_magic, Some(NetworkMagic(764_824_073)));
        assert_eq!(args.network_magic, Some(NetworkMagic(0)));
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
            parse_args(["--host-addr"]),
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
    fn rejects_invalid_network_magic() {
        assert!(matches!(
            parse_args(["--cardano-network-magic", "abc"]),
            Err(ParseError::InvalidValue(_, _)),
        ));
    }

    #[test]
    fn parsed_args_resolve_to_full_configuration() {
        let parsed = parse_args(["--host-addr", "10.0.0.1", "--port", "4000"]).expect("parses");
        let config = parsed.resolve();
        assert_eq!(config.host_addr, "10.0.0.1");
        assert_eq!(config.port_number, 4000);
        // Defaults should fill in:
        assert_eq!(config.host_ipv6_addr, "::");
        assert_eq!(config.config_file.to_str(), Some("dmq-node.json"));
    }

    #[test]
    fn empty_args_resolve_to_full_defaults() {
        let argv: Vec<String> = Vec::new();
        let parsed = parse_args(&argv).expect("parses");
        let config = parsed.resolve();
        assert_eq!(config.host_addr, "0.0.0.0");
        assert_eq!(config.port_number, 3001);
        assert!(!config.show_version);
    }

    #[test]
    fn help_fixture_non_empty() {
        assert!(!HELP_TEXT.is_empty());
    }

    #[test]
    fn version_fixture_non_empty() {
        assert!(!VERSION_TEXT.is_empty());
    }
}
