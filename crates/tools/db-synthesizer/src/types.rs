//! Typed configuration surface for the `db-synthesizer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Types.hs.
//!
//! Direct ports of upstream's data declarations:
//!
//! - [`NodeConfigStub`] — `data NodeConfigStub = NodeConfigStub { ncsNodeConfig, ncsAlonzoGenesisFile, ncsShelleyGenesisFile, ncsByronGenesisFile, ncsConwayGenesisFile, ncsDijkstraGenesisFile }`.
//! - [`NodeFilePaths`] — `data NodeFilePaths = NodeFilePaths { nfpConfig, nfpChainDB }`.
//! - [`NodeCredentials`] — `data NodeCredentials = NodeCredentials { credCertFile, credVRFFile, credKESFile, credBulkFile }`.
//! - [`ForgeLimit`] — `data ForgeLimit = ForgeLimitBlock Word64 | ForgeLimitSlot SlotNo | ForgeLimitEpoch Word64`.
//! - [`ForgeResult`] — `newtype ForgeResult = ForgeResult { resultForged :: Int }`.
//! - [`DBSynthesizerOpenMode`] — `data DBSynthesizerOpenMode = OpenCreate | OpenCreateForce | OpenAppend`.
//! - [`DBSynthesizerOptions`] — `data DBSynthesizerOptions = DBSynthesizerOptions { synthLimit, synthOpenMode }`.
//! - [`DBSynthesizerConfig`] — `data DBSynthesizerConfig = DBSynthesizerConfig { confConfigStub, confOptions, confProtocolCredentials, confShelleyGenesis, confDbDir }`.
//!
//! The per-era protocol-configuration records ([`NodeByronProtocolConfiguration`],
//! [`NodeShelleyProtocolConfiguration`], [`NodeAlonzoProtocolConfiguration`],
//! [`NodeConwayProtocolConfiguration`], [`NodeDijkstraProtocolConfiguration`],
//! [`NodeHardForkProtocolConfiguration`]) mirror db-synthesizer's vendored
//! `unstable-cardano-tools/Cardano/Node/Types.hs` — the records
//! `DBSynthesizer/Orphans.hs` writes `FromJSON` for (Phase 4 R3b-2). They
//! live in this module to keep the db-synthesizer config-type surface in one
//! place.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`ncsNodeConfig :: Aeson.Value`**: upstream stores the node-config
//!   JSON as a generic `Aeson.Value` (untyped JSON). Yggdrasil's port
//!   uses `serde_json::Value` for the same role.
//! - **`ProtocolFilepaths` (from `Cardano.Node.Types`)**: upstream
//!   re-uses cardano-node's full operator-credentials surface. Yggdrasil
//!   collapses this to a [`NodeCredentials`]-shaped struct of optional
//!   paths because db-synthesizer only consumes the path values, not
//!   the typed credential machinery.
//! - **`ShelleyGenesis` (from `Ouroboros.Consensus.Shelley.Node`)**:
//!   upstream's parsed genesis structure carries every Shelley field.
//!   Yggdrasil keeps this as `serde_json::Value` for the surface layer;
//!   the typed parsing happens in yggdrasil-ledger's genesis module
//!   when actually loaded for forging.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value as JsonValue;
use yggdrasil_ledger::SlotNo;
use yggdrasil_node_config::RequiresNetworkMagic;

/// Operator-supplied node-configuration stub.
///
/// Upstream:
/// ```haskell
/// data NodeConfigStub = NodeConfigStub
///   { ncsNodeConfig          :: !Aeson.Value
///   , ncsAlonzoGenesisFile   :: !FilePath
///   , ncsShelleyGenesisFile  :: !FilePath
///   , ncsByronGenesisFile    :: !FilePath
///   , ncsConwayGenesisFile   :: !FilePath
///   , ncsDijkstraGenesisFile :: !(Maybe FilePath)
///   }
/// ```
#[derive(Clone, Debug)]
pub struct NodeConfigStub {
    /// Top-level node-config JSON (typed as upstream's `Aeson.Value` →
    /// Rust's `serde_json::Value`).
    pub node_config: JsonValue,
    /// Path to the Alonzo-genesis file.
    pub alonzo_genesis_file: PathBuf,
    /// Path to the Shelley-genesis file.
    pub shelley_genesis_file: PathBuf,
    /// Path to the Byron-genesis file.
    pub byron_genesis_file: PathBuf,
    /// Path to the Conway-genesis file.
    pub conway_genesis_file: PathBuf,
    /// Path to the Dijkstra-genesis file (`None` if the era is not yet
    /// activated in this node's config).
    pub dijkstra_genesis_file: Option<PathBuf>,
}

/// Shelley-era protocol configuration.
///
/// Upstream `NodeShelleyProtocolConfiguration`
/// (`unstable-cardano-tools/Cardano/Node/Types.hs`). Upstream
/// `initProtocol` inline-constructs this from the [`NodeConfigStub`]
/// path (hash `Nothing`), so it carries no `Deserialize` impl.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeShelleyProtocolConfiguration {
    /// Path to the Shelley-genesis file.
    pub shelley_genesis_file: PathBuf,
    /// Optional expected Shelley-genesis hash.
    pub shelley_genesis_file_hash: Option<String>,
}

/// Alonzo-era protocol configuration. See [`NodeShelleyProtocolConfiguration`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeAlonzoProtocolConfiguration {
    /// Path to the Alonzo-genesis file.
    pub alonzo_genesis_file: PathBuf,
    /// Optional expected Alonzo-genesis hash.
    pub alonzo_genesis_file_hash: Option<String>,
}

/// Conway-era protocol configuration. See [`NodeShelleyProtocolConfiguration`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeConwayProtocolConfiguration {
    /// Path to the Conway-genesis file.
    pub conway_genesis_file: PathBuf,
    /// Optional expected Conway-genesis hash.
    pub conway_genesis_file_hash: Option<String>,
}

/// Dijkstra-era protocol configuration. See [`NodeShelleyProtocolConfiguration`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDijkstraProtocolConfiguration {
    /// Path to the Dijkstra-genesis file.
    pub dijkstra_genesis_file: PathBuf,
    /// Optional expected Dijkstra-genesis hash.
    pub dijkstra_genesis_file_hash: Option<String>,
}

/// Upstream `Orphans.hs` hard-codes the Byron update-application name.
fn default_byron_application_name() -> String {
    "cardano-sl".to_owned()
}

/// Upstream `Orphans.hs` defaults the Byron application version to `1`.
fn default_byron_application_version() -> u64 {
    1
}

/// Upstream `Orphans.hs` defaults `RequiresNetworkMagic` to `RequiresNoMagic`.
fn default_byron_req_network_magic() -> RequiresNetworkMagic {
    RequiresNetworkMagic::RequiresNoMagic
}

/// Byron-era protocol configuration.
///
/// Upstream `NodeByronProtocolConfiguration`
/// (`unstable-cardano-tools/Cardano/Node/Types.hs`) — a 9-field record.
/// The `Deserialize` impl mirrors `DBSynthesizer/Orphans.hs`'s `FromJSON`
/// instance: `ByronGenesisFile` and `LastKnownBlockVersion-{Major,Minor}`
/// are required, the rest default. `application_name` is hard-coded
/// `"cardano-sl"` and never read from JSON — upstream
/// `pure (ApplicationName "cardano-sl")`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NodeByronProtocolConfiguration {
    /// Path to the Byron-genesis file.
    #[serde(rename = "ByronGenesisFile")]
    pub byron_genesis_file: PathBuf,
    /// Optional expected Byron-genesis hash.
    #[serde(rename = "ByronGenesisHash", default)]
    pub byron_genesis_file_hash: Option<String>,
    /// Whether Byron headers carry the network magic inline.
    #[serde(
        rename = "RequiresNetworkMagic",
        default = "default_byron_req_network_magic"
    )]
    pub byron_req_network_magic: RequiresNetworkMagic,
    /// Optional PBFT signature threshold.
    #[serde(rename = "PBftSignatureThreshold", default)]
    pub byron_pbft_signature_thresh: Option<f64>,
    /// Byron update-application name — hard-coded `"cardano-sl"`.
    #[serde(skip, default = "default_byron_application_name")]
    pub byron_application_name: String,
    /// Byron application (software) version.
    #[serde(
        rename = "ApplicationVersion",
        default = "default_byron_application_version"
    )]
    pub byron_application_version: u64,
    /// Supported protocol version — major component.
    #[serde(rename = "LastKnownBlockVersion-Major")]
    pub byron_supported_protocol_version_major: u16,
    /// Supported protocol version — minor component.
    #[serde(rename = "LastKnownBlockVersion-Minor")]
    pub byron_supported_protocol_version_minor: u16,
    /// Supported protocol version — alt component.
    #[serde(rename = "LastKnownBlockVersion-Alt", default)]
    pub byron_supported_protocol_version_alt: u8,
}

/// Hard-fork protocol configuration — the `Test*HardForkAtEpoch`
/// overrides.
///
/// Upstream `NodeHardForkProtocolConfiguration`
/// (`unstable-cardano-tools/Cardano/Node/Types.hs`) — an 8-field record
/// (the `*HardForkAtVersion` fields of `cardano-node`'s separate copy of
/// `Cardano.Node.Types` are absent from this vendored copy). The
/// `Deserialize` impl mirrors `DBSynthesizer/Orphans.hs`: every key is
/// optional.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct NodeHardForkProtocolConfiguration {
    /// Whether not-yet-released eras are advertised. Must stay `false`
    /// on mainnet.
    #[serde(rename = "TestEnableDevelopmentHardForkEras", default)]
    pub test_enable_development_hard_fork_eras: bool,
    /// Force the Shelley hard fork at this epoch (testing only).
    #[serde(rename = "TestShelleyHardForkAtEpoch", default)]
    pub test_shelley_hard_fork_at_epoch: Option<u64>,
    /// Force the Allegra hard fork at this epoch (testing only).
    #[serde(rename = "TestAllegraHardForkAtEpoch", default)]
    pub test_allegra_hard_fork_at_epoch: Option<u64>,
    /// Force the Mary hard fork at this epoch (testing only).
    #[serde(rename = "TestMaryHardForkAtEpoch", default)]
    pub test_mary_hard_fork_at_epoch: Option<u64>,
    /// Force the Alonzo hard fork at this epoch (testing only).
    #[serde(rename = "TestAlonzoHardForkAtEpoch", default)]
    pub test_alonzo_hard_fork_at_epoch: Option<u64>,
    /// Force the Babbage hard fork at this epoch (testing only).
    #[serde(rename = "TestBabbageHardForkAtEpoch", default)]
    pub test_babbage_hard_fork_at_epoch: Option<u64>,
    /// Force the Conway hard fork at this epoch (testing only).
    #[serde(rename = "TestConwayHardForkAtEpoch", default)]
    pub test_conway_hard_fork_at_epoch: Option<u64>,
    /// Force the Dijkstra hard fork at this epoch (testing only).
    #[serde(rename = "TestDijkstraHardForkAtEpoch", default)]
    pub test_dijkstra_hard_fork_at_epoch: Option<u64>,
}

/// Operator-supplied node file paths.
///
/// Upstream:
/// ```haskell
/// data NodeFilePaths = NodeFilePaths
///   { nfpConfig  :: !FilePath
///   , nfpChainDB :: !FilePath
///   }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct NodeFilePaths {
    /// Path to the node config file.
    pub config: PathBuf,
    /// Path to the chain DB directory.
    pub chain_db: PathBuf,
}

/// Operator-supplied node credentials.
///
/// Upstream:
/// ```haskell
/// data NodeCredentials = NodeCredentials
///   { credCertFile :: !(Maybe FilePath)
///   , credVRFFile  :: !(Maybe FilePath)
///   , credKESFile  :: !(Maybe FilePath)
///   , credBulkFile :: !(Maybe FilePath)
///   }
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct NodeCredentials {
    /// Path to the operational certificate.
    pub cert_file: Option<PathBuf>,
    /// Path to the VRF signing key.
    pub vrf_file: Option<PathBuf>,
    /// Path to the KES signing key.
    pub kes_file: Option<PathBuf>,
    /// Path to a bulk credentials file (multi-pool batches).
    pub bulk_file: Option<PathBuf>,
}

/// How long the synthesizer should forge before stopping.
///
/// Upstream:
/// ```haskell
/// data ForgeLimit
///   = ForgeLimitBlock !Word64
///   | ForgeLimitSlot !SlotNo
///   | ForgeLimitEpoch !Word64
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ForgeLimit {
    /// Stop after forging this many blocks.
    Block(u64),
    /// Stop at this slot number.
    Slot(SlotNo),
    /// Stop after this many epochs.
    Epoch(u64),
}

/// Outcome of a forging run.
///
/// Upstream: `newtype ForgeResult = ForgeResult { resultForged :: Int }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ForgeResult {
    /// Number of blocks actually forged.
    pub forged: i64,
}

/// How to open the target ChainDB directory.
///
/// Upstream:
/// ```haskell
/// data DBSynthesizerOpenMode = OpenCreate | OpenCreateForce | OpenAppend
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum DBSynthesizerOpenMode {
    /// Open an empty target directory; refuse to overwrite.
    #[default]
    OpenCreate,
    /// Open the target directory unconditionally; overwrite if non-empty.
    OpenCreateForce,
    /// Append to an existing ChainDB.
    OpenAppend,
}

/// Operator-supplied options for the synthesizer run.
///
/// Upstream:
/// ```haskell
/// data DBSynthesizerOptions = DBSynthesizerOptions
///   { synthLimit    :: !ForgeLimit
///   , synthOpenMode :: !DBSynthesizerOpenMode
///   }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DBSynthesizerOptions {
    /// When to stop forging.
    pub limit: ForgeLimit,
    /// How to open the target directory.
    pub open_mode: DBSynthesizerOpenMode,
}

/// Top-level operator-supplied configuration.
///
/// Upstream:
/// ```haskell
/// data DBSynthesizerConfig = DBSynthesizerConfig
///   { confConfigStub          :: NodeConfigStub
///   , confOptions             :: DBSynthesizerOptions
///   , confProtocolCredentials :: ProtocolFilepaths
///   , confShelleyGenesis      :: ShelleyGenesis
///   , confDbDir               :: FilePath
///   }
/// ```
#[derive(Clone, Debug)]
pub struct DBSynthesizerConfig {
    /// Operator-supplied node-config stub (with re-resolved genesis paths).
    pub config_stub: NodeConfigStub,
    /// Synthesizer-run options (forge limit + open mode).
    pub options: DBSynthesizerOptions,
    /// Operator-supplied protocol credentials (cert / VRF / KES / bulk).
    pub protocol_credentials: NodeCredentials,
    /// Parsed Shelley genesis JSON. Stored as `serde_json::Value` at
    /// the surface layer; typed parsing happens at use-site in
    /// yggdrasil-ledger.
    pub shelley_genesis: JsonValue,
    /// Path to the target chain-DB directory.
    pub db_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_node_config_stub() -> NodeConfigStub {
        NodeConfigStub {
            node_config: JsonValue::Null,
            alonzo_genesis_file: PathBuf::from("/etc/alonzo.json"),
            shelley_genesis_file: PathBuf::from("/etc/shelley.json"),
            byron_genesis_file: PathBuf::from("/etc/byron.json"),
            conway_genesis_file: PathBuf::from("/etc/conway.json"),
            dijkstra_genesis_file: None,
        }
    }

    #[test]
    fn node_config_stub_round_trip() {
        let stub = empty_node_config_stub();
        assert_eq!(stub.alonzo_genesis_file.to_str(), Some("/etc/alonzo.json"));
        assert!(stub.dijkstra_genesis_file.is_none());
    }

    #[test]
    fn node_file_paths_round_trip() {
        let p = NodeFilePaths {
            config: PathBuf::from("/etc/c.json"),
            chain_db: PathBuf::from("/var/lib/cardano/db"),
        };
        assert_eq!(p.config.to_str(), Some("/etc/c.json"));
        assert_eq!(p.chain_db.to_str(), Some("/var/lib/cardano/db"));
    }

    #[test]
    fn node_credentials_default_all_none() {
        let c = NodeCredentials::default();
        assert!(c.cert_file.is_none());
        assert!(c.vrf_file.is_none());
        assert!(c.kes_file.is_none());
        assert!(c.bulk_file.is_none());
    }

    #[test]
    fn forge_limit_block_round_trip() {
        let l = ForgeLimit::Block(1000);
        assert_eq!(l, ForgeLimit::Block(1000));
        match l {
            ForgeLimit::Block(n) => assert_eq!(n, 1000),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn forge_limit_slot_round_trip() {
        let l = ForgeLimit::Slot(SlotNo(50_000));
        match l {
            ForgeLimit::Slot(s) => assert_eq!(s, SlotNo(50_000)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn forge_limit_epoch_round_trip() {
        let l = ForgeLimit::Epoch(5);
        match l {
            ForgeLimit::Epoch(e) => assert_eq!(e, 5),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn forge_result_round_trip() {
        let r = ForgeResult { forged: 42 };
        assert_eq!(r.forged, 42);
    }

    #[test]
    fn db_synthesizer_open_mode_default_is_open_create() {
        assert_eq!(
            DBSynthesizerOpenMode::default(),
            DBSynthesizerOpenMode::OpenCreate
        );
    }

    #[test]
    fn db_synthesizer_options_construction() {
        let opts = DBSynthesizerOptions {
            limit: ForgeLimit::Block(1000),
            open_mode: DBSynthesizerOpenMode::OpenCreateForce,
        };
        assert_eq!(opts.limit, ForgeLimit::Block(1000));
        assert_eq!(opts.open_mode, DBSynthesizerOpenMode::OpenCreateForce);
    }

    #[test]
    fn db_synthesizer_config_construction() {
        let config = DBSynthesizerConfig {
            config_stub: empty_node_config_stub(),
            options: DBSynthesizerOptions {
                limit: ForgeLimit::Slot(SlotNo(100_000)),
                open_mode: DBSynthesizerOpenMode::default(),
            },
            protocol_credentials: NodeCredentials::default(),
            shelley_genesis: JsonValue::Null,
            db_dir: PathBuf::from("/var/lib/cardano/db"),
        };
        assert_eq!(config.db_dir.to_str(), Some("/var/lib/cardano/db"));
        assert_eq!(config.options.limit, ForgeLimit::Slot(SlotNo(100_000)));
    }

    #[test]
    fn node_byron_protocol_configuration_parses_minimal_with_defaults() {
        // Only the three required keys; everything else defaults.
        let json = r#"{
            "ByronGenesisFile": "byron.json",
            "LastKnownBlockVersion-Major": 1,
            "LastKnownBlockVersion-Minor": 0
        }"#;
        let cfg: NodeByronProtocolConfiguration = serde_json::from_str(json).expect("parses");
        assert_eq!(cfg.byron_genesis_file, PathBuf::from("byron.json"));
        assert_eq!(cfg.byron_genesis_file_hash, None);
        assert_eq!(
            cfg.byron_req_network_magic,
            RequiresNetworkMagic::RequiresNoMagic,
        );
        assert_eq!(cfg.byron_pbft_signature_thresh, None);
        // Hard-coded upstream — never read from JSON.
        assert_eq!(cfg.byron_application_name, "cardano-sl");
        assert_eq!(cfg.byron_application_version, 1);
        assert_eq!(cfg.byron_supported_protocol_version_major, 1);
        assert_eq!(cfg.byron_supported_protocol_version_minor, 0);
        assert_eq!(cfg.byron_supported_protocol_version_alt, 0);
    }

    #[test]
    fn node_byron_protocol_configuration_reads_explicit_fields_and_ignores_unknown() {
        let json = r#"{
            "Protocol": "Cardano",
            "ByronGenesisFile": "b.json",
            "ByronGenesisHash": "abc123",
            "RequiresNetworkMagic": "RequiresMagic",
            "PBftSignatureThreshold": 0.22,
            "ApplicationVersion": 3,
            "LastKnownBlockVersion-Major": 2,
            "LastKnownBlockVersion-Minor": 1,
            "LastKnownBlockVersion-Alt": 4,
            "SomeUnrelatedKey": [1, 2, 3]
        }"#;
        let cfg: NodeByronProtocolConfiguration = serde_json::from_str(json).expect("parses");
        assert_eq!(cfg.byron_genesis_file_hash, Some("abc123".to_owned()));
        assert_eq!(
            cfg.byron_req_network_magic,
            RequiresNetworkMagic::RequiresMagic
        );
        assert_eq!(cfg.byron_pbft_signature_thresh, Some(0.22));
        assert_eq!(cfg.byron_application_version, 3);
        assert_eq!(cfg.byron_supported_protocol_version_major, 2);
        assert_eq!(cfg.byron_supported_protocol_version_alt, 4);
        // The hard-coded name holds even when other keys are present.
        assert_eq!(cfg.byron_application_name, "cardano-sl");
    }

    #[test]
    fn node_hard_fork_protocol_configuration_parses_empty_object() {
        let cfg: NodeHardForkProtocolConfiguration = serde_json::from_str("{}").expect("parses");
        assert!(!cfg.test_enable_development_hard_fork_eras);
        assert_eq!(cfg.test_shelley_hard_fork_at_epoch, None);
        assert_eq!(cfg.test_conway_hard_fork_at_epoch, None);
        assert_eq!(cfg.test_dijkstra_hard_fork_at_epoch, None);
    }

    #[test]
    fn node_hard_fork_protocol_configuration_reads_epoch_overrides() {
        let json = r#"{
            "TestEnableDevelopmentHardForkEras": true,
            "TestShelleyHardForkAtEpoch": 1,
            "TestConwayHardForkAtEpoch": 42
        }"#;
        let cfg: NodeHardForkProtocolConfiguration = serde_json::from_str(json).expect("parses");
        assert!(cfg.test_enable_development_hard_fork_eras);
        assert_eq!(cfg.test_shelley_hard_fork_at_epoch, Some(1));
        assert_eq!(cfg.test_conway_hard_fork_at_epoch, Some(42));
        assert_eq!(cfg.test_mary_hard_fork_at_epoch, None);
    }

    #[test]
    fn era_protocol_configurations_construct() {
        let shelley = NodeShelleyProtocolConfiguration {
            shelley_genesis_file: PathBuf::from("s.json"),
            shelley_genesis_file_hash: None,
        };
        let alonzo = NodeAlonzoProtocolConfiguration {
            alonzo_genesis_file: PathBuf::from("a.json"),
            alonzo_genesis_file_hash: Some("h".to_owned()),
        };
        let conway = NodeConwayProtocolConfiguration {
            conway_genesis_file: PathBuf::from("c.json"),
            conway_genesis_file_hash: None,
        };
        let dijkstra = NodeDijkstraProtocolConfiguration {
            dijkstra_genesis_file: PathBuf::from("d.json"),
            dijkstra_genesis_file_hash: None,
        };
        assert_eq!(shelley.shelley_genesis_file, PathBuf::from("s.json"));
        assert_eq!(alonzo.alonzo_genesis_file_hash, Some("h".to_owned()));
        assert_eq!(conway.conway_genesis_file, PathBuf::from("c.json"));
        assert_eq!(dijkstra.dijkstra_genesis_file, PathBuf::from("d.json"));
    }
}
