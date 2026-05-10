//! CLI argument types — `TxSubmitNodeParams` and friends.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/CLI/Types.hs.
//!
//! Direct ports:
//!
//! - [`ConfigFile`] — `newtype ConfigFile = ConfigFile { unConfigFile :: FilePath }`.
//! - [`GenesisFile`] — `newtype GenesisFile = GenesisFile { unGenesisFile :: FilePath }`.
//! - [`SocketPath`] — corresponds to upstream `Cardano.Api.SocketPath`
//!   (a re-exported `File 'Out` envelope around `FilePath`). Yggdrasil
//!   collapses the layered envelope to a direct `PathBuf` newtype since
//!   the polymorphic `File 'In/'Out` machinery has no semantic role at
//!   this surface.
//! - [`ConsensusModeParams`] — Cardano-Api's mode discriminator. Tx-submit
//!   only uses `CardanoMode`; the Byron / Shelley mode entry points are
//!   not exposed by this binary.
//! - [`NetworkId`] — direct port of upstream `Cardano.Api.NetworkId`
//!   (Mainnet | Testnet NetworkMagic). Yggdrasil's CLI parser produces
//!   [`crate::parser::NetworkMagic`]; [`From`] glue lifts the parser
//!   value into this typed surface.
//! - [`TxSubmitCommand`] — `data TxSubmitCommand = TxSubmitRun !TxSubmitNodeParams | TxSubmitVersion`.
//! - [`TxSubmitNodeParams`] — `data TxSubmitNodeParams = TxSubmitNodeParams { tspConfigFile, tspProtocol, tspNetworkId, tspSocketPath, tspWebserverConfig, tspMetricsPort }`.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Cardano.Api.SocketPath`'s polymorphic `File 'Out` envelope — see
//!   [`SocketPath`] note above.
//! - The full `Cardano.Api.NetworkId` surface (network-magic
//!   abstraction, era-aware variants) — tx-submit only branches on
//!   Mainnet vs Testnet(magic), so the simplified Rust enum is
//!   semantically complete for this binary.

use std::path::PathBuf;

use crate::parser::NetworkMagic;
use crate::rest::types::WebserverConfig;

/// Path to the tx-submit web API configuration file.
///
/// Upstream: `newtype ConfigFile = ConfigFile { unConfigFile :: FilePath }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConfigFile(pub PathBuf);

impl ConfigFile {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        ConfigFile(path.into())
    }

    /// Reference the underlying path. Mirrors upstream `unConfigFile`.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for ConfigFile {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Path to a genesis file.
///
/// Upstream: `newtype GenesisFile = GenesisFile { unGenesisFile :: FilePath }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GenesisFile(pub PathBuf);

impl GenesisFile {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        GenesisFile(path.into())
    }

    /// Reference the underlying path. Mirrors upstream `unGenesisFile`.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for GenesisFile {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Path to a cardano-node socket.
///
/// Mirrors upstream `Cardano.Api.SocketPath` (a re-exported `File 'Out`
/// envelope around `FilePath`); Yggdrasil collapses the layered
/// envelope to a direct `PathBuf` newtype.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SocketPath(pub PathBuf);

impl SocketPath {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        SocketPath(path.into())
    }

    /// Reference the underlying path.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for SocketPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Consensus mode parameters.
///
/// Upstream `Cardano.Api.ConsensusModeParams` supports Cardano | Byron |
/// Shelley modes; tx-submit only uses Cardano mode (Byron/Shelley are
/// historical SDK paths that don't appear in tx-submit's parser).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ConsensusModeParams {
    /// Cardano hard-fork-combinator mode (the only practical option).
    #[default]
    CardanoMode,
}

/// Network identifier.
///
/// Upstream: `data NetworkId = Mainnet | Testnet !NetworkMagic`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NetworkId {
    /// Cardano mainnet (canonical magic).
    Mainnet,
    /// Testnet with caller-supplied network magic.
    Testnet(u32),
}

impl From<NetworkMagic> for NetworkId {
    fn from(magic: NetworkMagic) -> Self {
        match magic {
            NetworkMagic::Mainnet => NetworkId::Mainnet,
            NetworkMagic::Testnet(m) => NetworkId::Testnet(m),
        }
    }
}

/// Validated, fully-specified runtime parameters for the tx-submit
/// daemon.
///
/// Upstream:
/// ```haskell
/// data TxSubmitNodeParams = TxSubmitNodeParams
///   { tspConfigFile      :: !ConfigFile
///   , tspProtocol        :: !ConsensusModeParams
///   , tspNetworkId       :: !NetworkId
///   , tspSocketPath      :: !SocketPath
///   , tspWebserverConfig :: !WebserverConfig
///   , tspMetricsPort     :: !Int
///   }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxSubmitNodeParams {
    /// Path to the tx-submit web API configuration file.
    pub config_file: ConfigFile,
    /// Consensus mode (always Cardano in practice).
    pub protocol: ConsensusModeParams,
    /// Network identifier.
    pub network_id: NetworkId,
    /// Path to the cardano-node socket.
    pub socket_path: SocketPath,
    /// Web-server bind config.
    pub webserver_config: WebserverConfig,
    /// Port for the Prometheus metrics endpoint.
    pub metrics_port: u16,
}

/// Top-level command dispatch surface.
///
/// Upstream: `data TxSubmitCommand = TxSubmitRun !TxSubmitNodeParams | TxSubmitVersion`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxSubmitCommand {
    /// Run the tx-submit daemon with the supplied params.
    TxSubmitRun(TxSubmitNodeParams),
    /// Emit the version banner.
    TxSubmitVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_round_trips_through_pathbuf() {
        let path = PathBuf::from("/etc/submit-api.json");
        let cf = ConfigFile::new(&path);
        assert_eq!(cf.as_path(), path);
    }

    #[test]
    fn genesis_file_round_trips_through_pathbuf() {
        let gf = GenesisFile::new("/etc/genesis.json");
        assert_eq!(gf.as_path().to_str(), Some("/etc/genesis.json"));
    }

    #[test]
    fn socket_path_round_trips_through_pathbuf() {
        let sp = SocketPath::new("/run/cardano-node.socket");
        assert_eq!(sp.as_path().to_str(), Some("/run/cardano-node.socket"));
    }

    #[test]
    fn network_id_lifts_from_mainnet() {
        let nid: NetworkId = NetworkMagic::Mainnet.into();
        assert_eq!(nid, NetworkId::Mainnet);
    }

    #[test]
    fn network_id_lifts_from_testnet_magic() {
        let nid: NetworkId = NetworkMagic::Testnet(2).into();
        assert_eq!(nid, NetworkId::Testnet(2));
    }

    #[test]
    fn consensus_mode_default_is_cardano() {
        assert_eq!(
            ConsensusModeParams::default(),
            ConsensusModeParams::CardanoMode
        );
    }

    #[test]
    fn tx_submit_command_run_carries_params() {
        let params = TxSubmitNodeParams {
            config_file: ConfigFile::new("/etc/c.json"),
            protocol: ConsensusModeParams::CardanoMode,
            network_id: NetworkId::Mainnet,
            socket_path: SocketPath::new("/run/n.socket"),
            webserver_config: WebserverConfig::new("127.0.0.1", 8090),
            metrics_port: 8081,
        };
        let cmd = TxSubmitCommand::TxSubmitRun(params.clone());
        match cmd {
            TxSubmitCommand::TxSubmitRun(p) => assert_eq!(p, params),
            TxSubmitCommand::TxSubmitVersion => panic!("expected Run"),
        }
    }
}
