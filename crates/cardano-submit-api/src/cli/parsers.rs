//! optparse-applicative-equivalent CLI parser (clap-based shell).
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/CLI/Parsers.hs.
//!
//! Direct ports:
//!
//! - [`into_command`] — composite parser returning a [`TxSubmitCommand`].
//!   Mirrors upstream `pTxSubmit envCli` which combines `TxSubmitRun
//!   <$> (TxSubmitNodeParams <$> pConfigFile <*> pConsensusModeParams
//!   <*> pNetworkId envCli <*> pSocketPath' <*> pWebserverConfig 8090
//!   <*> pMetricsPort 8081) <|> pVersion`.
//! - [`config_file_from_args`] / [`socket_path_from_args`] /
//!   [`metrics_port_from_args`] — strict-mirror analogs of upstream's
//!   per-field parsers (`pConfigFile`, `pSocketPath'`, `pMetricsPort`).
//!   Yggdrasil's flag-level argument parsing is centralized in
//!   [`crate::parser::parse_args`]; this module's role is to bridge the
//!   parsed [`crate::parser::Args`] struct into the upstream-shaped
//!   [`TxSubmitNodeParams`] / [`TxSubmitCommand`] surface, applying the
//!   same defaults (`8090` for `--port`, `8081` for `--metrics-port`)
//!   and the same mandatory-field semantics (`--config`,
//!   `--socket-path`, `--mainnet|--testnet-magic`).
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Cardano.CLI.Environment.EnvCli` — upstream's parser threads the
//!   process-environment derived defaults (e.g. `CARDANO_NODE_SOCKET_PATH`)
//!   into network-id selection. The Yggdrasil CLI surface is
//!   environment-blind for this binary; `--mainnet|--testnet-magic`
//!   is a hard requirement and the parser rejects argv that omits it.
//!   If env-driven defaults are needed in the future, this is the
//!   integration point.
//! - `Options.Applicative.Parser` combinators (`Opt.flag'`, `<**>`,
//!   `<|>`, `<*>`) — upstream uses applicative parser composition;
//!   the Rust bridge is a plain flat-mapping function over the already-
//!   parsed [`Args`] struct.

use crate::cli::types::{
    ConfigFile, ConsensusModeParams, NetworkId, SocketPath, TxSubmitCommand, TxSubmitNodeParams,
};
use crate::parser::Args;
use crate::rest::parsers::from_args as webserver_from_args;

/// Default port for the tx-submit web API. Mirrors upstream
/// `pWebserverConfig 8090` argument.
pub const DEFAULT_WEBSERVER_PORT: u16 = 8090;
/// Default port for the Prometheus metrics endpoint. Mirrors upstream
/// `pMetricsPort 8081` argument.
pub const DEFAULT_METRICS_PORT: u16 = 8081;

/// Errors when promoting parsed [`Args`] into a [`TxSubmitCommand`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CommandError {
    /// A flag required to construct a fully-specified
    /// [`TxSubmitNodeParams`] was missing from argv.
    #[error("missing required flag: --{0}")]
    MissingFlag(&'static str),
}

/// Promote parsed [`Args`] into a [`TxSubmitCommand::TxSubmitRun`].
///
/// Mirrors upstream `pTxSubmit` for the `TxSubmitRun` branch. The
/// `TxSubmitVersion` branch is unreachable here because [`crate::parser::parse_args`]
/// short-circuits on `--version` before any [`Args`] value is produced;
/// callers wanting Version semantics should match on
/// [`crate::parser::ParseError::VersionRequested`] upstream of this
/// function.
///
/// Mandatory flags (matching upstream's parser):
///
/// - `--config FILEPATH`
/// - `--socket-path FILEPATH`
/// - `--mainnet | --testnet-magic NATURAL`
///
/// Optional flags (defaults applied):
///
/// - `--listen-address` → `127.0.0.1`
/// - `--port` → `8090`
/// - `--metrics-port` → `8081`
pub fn into_command(args: &Args) -> Result<TxSubmitCommand, CommandError> {
    let config_file = config_file_from_args(args)?;
    let socket_path = socket_path_from_args(args)?;
    let network_id = network_id_from_args(args)?;
    let webserver_config = webserver_from_args(args, DEFAULT_WEBSERVER_PORT);
    let metrics_port = metrics_port_from_args(args);

    Ok(TxSubmitCommand::TxSubmitRun(TxSubmitNodeParams {
        config_file,
        protocol: ConsensusModeParams::CardanoMode,
        network_id,
        socket_path,
        webserver_config,
        metrics_port,
    }))
}

/// Mirror of upstream `pConfigFile`. Errors when the flag is absent.
pub fn config_file_from_args(args: &Args) -> Result<ConfigFile, CommandError> {
    args.config
        .as_deref()
        .map(ConfigFile::new)
        .ok_or(CommandError::MissingFlag("config"))
}

/// Mirror of upstream `pSocketPath'`. Errors when the flag is absent.
pub fn socket_path_from_args(args: &Args) -> Result<SocketPath, CommandError> {
    args.socket_path
        .as_deref()
        .map(SocketPath::new)
        .ok_or(CommandError::MissingFlag("socket-path"))
}

/// Mirror of upstream `pNetworkId envCli`. The Yggdrasil parser surfaces
/// the network-id via `--mainnet` (sentinel) or `--testnet-magic NATURAL`;
/// this fn errors if neither flag was supplied.
pub fn network_id_from_args(args: &Args) -> Result<NetworkId, CommandError> {
    args.network_magic
        .map(NetworkId::from)
        .ok_or(CommandError::MissingFlag("mainnet|testnet-magic"))
}

/// Mirror of upstream `pMetricsPort 8081`. Always succeeds because
/// `--metrics-port` is optional with a default.
pub fn metrics_port_from_args(args: &Args) -> u16 {
    args.metrics_port.unwrap_or(DEFAULT_METRICS_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::NetworkMagic;

    fn full_args() -> Args {
        Args {
            config: Some("/etc/c.json".to_string()),
            socket_path: Some("/run/n.socket".to_string()),
            network_magic: Some(NetworkMagic::Mainnet),
            listen_address: None,
            port: None,
            metrics_port: None,
            epoch_slots: None,
        }
    }

    #[test]
    fn into_command_accepts_full_canonical_args() {
        let args = full_args();
        let cmd = into_command(&args).expect("validates");
        match cmd {
            TxSubmitCommand::TxSubmitRun(params) => {
                assert_eq!(params.config_file.as_path().to_str(), Some("/etc/c.json"));
                assert_eq!(params.socket_path.as_path().to_str(), Some("/run/n.socket"));
                assert_eq!(params.network_id, NetworkId::Mainnet);
                assert_eq!(params.protocol, ConsensusModeParams::CardanoMode);
                assert_eq!(params.webserver_config.host, "127.0.0.1");
                assert_eq!(params.webserver_config.port, 8090);
                assert_eq!(params.metrics_port, 8081);
            }
            TxSubmitCommand::TxSubmitVersion => panic!("expected Run"),
        }
    }

    #[test]
    fn into_command_rejects_missing_config() {
        let mut args = full_args();
        args.config = None;
        assert_eq!(
            into_command(&args),
            Err(CommandError::MissingFlag("config"))
        );
    }

    #[test]
    fn into_command_rejects_missing_socket_path() {
        let mut args = full_args();
        args.socket_path = None;
        assert_eq!(
            into_command(&args),
            Err(CommandError::MissingFlag("socket-path"))
        );
    }

    #[test]
    fn into_command_rejects_missing_network_magic() {
        let mut args = full_args();
        args.network_magic = None;
        assert_eq!(
            into_command(&args),
            Err(CommandError::MissingFlag("mainnet|testnet-magic"))
        );
    }

    #[test]
    fn into_command_propagates_listen_address_override() {
        let mut args = full_args();
        args.listen_address = Some("0.0.0.0".to_string());
        let cmd = into_command(&args).expect("validates");
        match cmd {
            TxSubmitCommand::TxSubmitRun(params) => {
                assert_eq!(params.webserver_config.host, "0.0.0.0");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn into_command_propagates_port_override() {
        let mut args = full_args();
        args.port = Some(9090);
        let cmd = into_command(&args).expect("validates");
        match cmd {
            TxSubmitCommand::TxSubmitRun(params) => {
                assert_eq!(params.webserver_config.port, 9090);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn into_command_propagates_metrics_port_override() {
        let mut args = full_args();
        args.metrics_port = Some(7777);
        let cmd = into_command(&args).expect("validates");
        match cmd {
            TxSubmitCommand::TxSubmitRun(params) => {
                assert_eq!(params.metrics_port, 7777);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn into_command_propagates_testnet_magic() {
        let mut args = full_args();
        args.network_magic = Some(NetworkMagic::Testnet(2));
        let cmd = into_command(&args).expect("validates");
        match cmd {
            TxSubmitCommand::TxSubmitRun(params) => {
                assert_eq!(params.network_id, NetworkId::Testnet(2));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn config_file_from_args_round_trips() {
        let args = Args {
            config: Some("/x".to_string()),
            ..Args::default()
        };
        assert_eq!(config_file_from_args(&args), Ok(ConfigFile::new("/x")));
    }

    #[test]
    fn socket_path_from_args_round_trips() {
        let args = Args {
            socket_path: Some("/y".to_string()),
            ..Args::default()
        };
        assert_eq!(socket_path_from_args(&args), Ok(SocketPath::new("/y")));
    }

    #[test]
    fn metrics_port_from_args_returns_default_when_absent() {
        let args = Args::default();
        assert_eq!(metrics_port_from_args(&args), DEFAULT_METRICS_PORT);
    }
}
