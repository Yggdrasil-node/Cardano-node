//! Operator-facing types for the `cardano-testnet` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Start/Types.hs.
//!
//! R359 ports the simple operator-facing knobs from upstream's
//! Start/Types.hs: numeric newtypes, enum variants, and small helper
//! types that don't pull in the deeper `Cardano.Api` /
//! `Cardano.Ledger.*` machinery. The full record types
//! (`CardanoTestnetCliOptions`, `TestnetCreationOptions`,
//! `GenesisOptions`, `NodeOption`, etc.) carry era-aware fields whose
//! Rust port depends on yggdrasil-ledger's era-aware genesis and
//! protocol-parameter surface — those land in subsequent rounds.
//!
//! Carve-outs (NOT ported in R359; tracked under `remaining_work`):
//!
//! - **`Cardano.Api.cardanoEra`** + per-era variants
//!   (`AnyShelleyBasedEra`, `AnyCardanoEra`): the era-aware machinery
//!   needs the full yggdrasil-ledger era surface to be exposed at
//!   crate boundaries before testnet types can carry typed era
//!   discriminators. Port lands when the testnet runtime round wires
//!   the era selector.
//! - **`Cardano.Ledger.Alonzo.Genesis.AlonzoGenesis` /
//!   `Cardano.Ledger.Conway.Genesis.ConwayGenesis`**: parsed
//!   per-era genesis records. Yggdrasil keeps these as
//!   `serde_json::Value` at this surface for now; typed parsing
//!   happens at use-site in yggdrasil-ledger.
//! - **`Hedgehog.MonadTest`**: upstream uses Hedgehog for the testnet
//!   harness. The Rust port uses `proptest` per the plan's
//!   pre-approved carve-out (R416 cardano-testnet skeleton +
//!   subsequent rounds map upstream's Process/Property modules to
//!   Rust's tokio::process + proptest equivalents).

use std::path::PathBuf;

/// The default value for the `--testnet-magic` option for
/// `cardano-testnet`. Mirrors upstream `defaultTestnetMagic = 42`.
pub const DEFAULT_TESTNET_MAGIC: i64 = 42;

/// Identifier of an individual node within a testnet topology.
///
/// Upstream: `newtype NodeId = NodeId Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NodeId(pub i32);

/// Number of stake-pool operator (SPO) nodes in the testnet.
///
/// Upstream: `newtype NumPools = NumPools Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NumPools(pub i32);

/// Number of relay nodes in the testnet.
///
/// Upstream: `newtype NumRelays = NumRelays Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NumRelays(pub i32);

/// Number of DReps (delegated representatives, Conway+) in the testnet.
///
/// Upstream: `newtype NumDReps = NumDReps Int`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NumDReps(pub i32);

/// Path to a user-provided node-config YAML/JSON file (used by
/// `create-env` to seed the testnet topology).
///
/// Upstream: `newtype InputNodeConfigFile = InputNodeConfigFile FilePath`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InputNodeConfigFile(pub PathBuf);

impl InputNodeConfigFile {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        InputNodeConfigFile(path.into())
    }

    /// Borrow the underlying path.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for InputNodeConfigFile {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Whether the testnet harness should rewrite genesis-file timestamps
/// during `create-env` to make them current.
///
/// Upstream: `data UpdateTimestamps = UpdateTimestamps | DontUpdateTimestamps`,
/// with `instance Default UpdateTimestamps where def = DontUpdateTimestamps`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum UpdateTimestamps {
    /// Rewrite timestamps.
    UpdateTimestamps,
    /// Leave timestamps as-is — the upstream `Default`.
    #[default]
    DontUpdateTimestamps,
}

/// The on-chain protocol parameters a freshly-created testnet starts
/// with.
///
/// Upstream: `data TestnetOnChainParams` (`Testnet/Start/Types.hs`),
/// with `instance Default TestnetOnChainParams where def = DefaultParams`.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub enum TestnetOnChainParams {
    /// The testnet's built-in default parameters (the upstream `Default`).
    #[default]
    DefaultParams,
    /// Parameters from a JSON file in the Blockfrost
    /// `epochs/latest/parameters` shape.
    OnChainParamsFile(PathBuf),
    /// Current mainnet on-chain parameters (fetched at runtime from
    /// [`MAINNET_PARAMS_URL`]).
    OnChainParamsMainnet,
}

/// The URL of the up-to-date mainnet on-chain-parameters file
/// (Blockfrost format), used by
/// [`TestnetOnChainParams::OnChainParamsMainnet`].
///
/// Mirror of the target of upstream `mainnetParamsRequest`.
pub const MAINNET_PARAMS_URL: &str = "https://raw.githubusercontent.com/input-output-hk/cardano-parameters/refs/heads/main/mainnet/parameters.json";

/// RPC server toggle — whether to start a JSON-RPC server alongside
/// the testnet nodes.
///
/// Upstream: `data RpcSupport = RpcDisabled | RpcEnabled`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum RpcSupport {
    /// JSON-RPC server disabled (default).
    #[default]
    RpcDisabled,
    /// JSON-RPC server enabled.
    RpcEnabled,
}

/// Logging format used by spawned testnet nodes.
///
/// Upstream: `data NodeLoggingFormat = NodeLoggingFormatAsJson | NodeLoggingFormatAsText`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum NodeLoggingFormat {
    /// JSON-formatted log records.
    #[default]
    AsJson,
    /// Human-readable text log records.
    AsText,
}

impl NodeLoggingFormat {
    /// Mirror of upstream `readNodeLoggingFormat`. Accepts the strings
    /// `"json"` (`NodeLoggingFormatAsJson`) and `"text"`
    /// (`NodeLoggingFormatAsText`); any other string is rejected.
    pub fn from_string(s: &str) -> Result<Self, ParseError> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(NodeLoggingFormat::AsJson),
            "text" => Ok(NodeLoggingFormat::AsText),
            other => Err(ParseError::UnknownNodeLoggingFormat(other.to_string())),
        }
    }
}

/// Whether to record genesis hashes in the generated config.
///
/// Upstream: `data GenesisHashesPolicy = WithHashes | WithoutHashes`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum GenesisHashesPolicy {
    /// Embed genesis hashes (default; recommended).
    #[default]
    WithHashes,
    /// Omit genesis hashes (legacy operator workflow).
    WithoutHashes,
}

/// Source for KES (key-evolution-secure) credentials when forging.
///
/// Upstream: `data PraosCredentialsSource = UseKesKeyFile | UseKesSocket`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum PraosCredentialsSource {
    /// Read KES credentials from a file on disk (default).
    #[default]
    UseKesKeyFile,
    /// Talk to a kes-agent socket for KES credentials.
    UseKesSocket,
}

/// User-provided data wrapper. Mirrors upstream
/// `data UserProvidedData a = UserProvidedData a | NoUserProvidedData`.
///
/// Used by genesis-creation paths to distinguish operator-supplied
/// values from generated ones.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum UserProvidedData<A> {
    /// Operator supplied this value.
    UserProvidedData(A),
    /// Operator did not supply this value (testnet should generate it).
    #[default]
    NoUserProvidedData,
}

impl<A> UserProvidedData<A> {
    /// Borrow the inner value if present.
    pub fn as_ref(&self) -> Option<&A> {
        match self {
            UserProvidedData::UserProvidedData(a) => Some(a),
            UserProvidedData::NoUserProvidedData => None,
        }
    }

    /// Convert to a plain `Option`.
    pub fn into_option(self) -> Option<A> {
        match self {
            UserProvidedData::UserProvidedData(a) => Some(a),
            UserProvidedData::NoUserProvidedData => None,
        }
    }
}

/// Runtime options for testnet nodes — independent of how the
/// environment was created (from scratch or from a `--node-env` path).
///
/// Mirror of upstream `data TestnetRuntimeOptions` with
/// `instance Default` (`runtimeEnableNewEpochStateLogging = True`,
/// `runtimeEnableRpc = RpcDisabled`, `runtimeKESSource = def`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestnetRuntimeOptions {
    /// Whether new-epoch-state logging is enabled.
    pub runtime_enable_new_epoch_state_logging: bool,
    /// Whether to enable gRPC endpoints on testnet nodes.
    pub runtime_enable_rpc: RpcSupport,
    /// Where forging nodes source their KES credentials.
    pub runtime_kes_source: PraosCredentialsSource,
}

impl Default for TestnetRuntimeOptions {
    fn default() -> Self {
        TestnetRuntimeOptions {
            runtime_enable_new_epoch_state_logging: true,
            runtime_enable_rpc: RpcSupport::RpcDisabled,
            runtime_kes_source: PraosCredentialsSource::default(),
        }
    }
}

/// Options for the `--node-env` path — start a testnet from a
/// pre-existing environment directory.
///
/// Mirror of upstream `data TestnetEnvOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestnetEnvOptions {
    /// Path to the pre-existing testnet environment.
    pub env_path: PathBuf,
    /// Whether to rewrite genesis timestamps before starting.
    pub env_update_timestamps: UpdateTimestamps,
}

/// Options realized by writing fields into the Shelley genesis file.
///
/// Mirror of upstream `data GenesisOptions` with `instance Default`
/// (magic 42, epoch length 500 slots, slot length 0.1 s, active-slot
/// coefficient 0.05). Upstream derives `Eq`; the Rust port can only
/// derive `PartialEq` because two fields are `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenesisOptions {
    /// The testnet network magic.
    pub genesis_testnet_magic: i64,
    /// An epoch's duration, in slots.
    pub genesis_epoch_length: i64,
    /// Slot length, in seconds.
    pub genesis_slot_length: f64,
    /// The active-slot coefficient.
    pub genesis_active_slots_coeff: f64,
}

impl Default for GenesisOptions {
    fn default() -> Self {
        GenesisOptions {
            genesis_testnet_magic: DEFAULT_TESTNET_MAGIC,
            genesis_epoch_length: 500,
            genesis_slot_length: 0.1,
            genesis_active_slots_coeff: 0.05,
        }
    }
}

/// Whether a testnet node is a stake-pool operator or a relay. The
/// string list is extra CLI arguments appended when starting the node.
///
/// Mirror of upstream `data NodeOption`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeOption {
    /// A stake-pool-operator (block-producing) node, with extra args.
    SpoNodeOptions(Vec<String>),
    /// A relay node, with extra args.
    RelayNodeOptions(Vec<String>),
}

impl NodeOption {
    /// Whether this is an SPO node. Mirror of upstream
    /// `isSpoNodeOptions`.
    pub fn is_spo(&self) -> bool {
        matches!(self, NodeOption::SpoNodeOptions(_))
    }

    /// Whether this is a relay node. Mirror of upstream
    /// `isRelayNodeOptions`.
    pub fn is_relay(&self) -> bool {
        matches!(self, NodeOption::RelayNodeOptions(_))
    }
}

/// The default testnet node set — one SPO and two relays.
///
/// Mirror of upstream `cardanoDefaultTestnetNodeOptions`.
pub fn cardano_default_testnet_node_options() -> Vec<NodeOption> {
    vec![
        NodeOption::SpoNodeOptions(Vec::new()),
        NodeOption::RelayNodeOptions(Vec::new()),
        NodeOption::RelayNodeOptions(Vec::new()),
    ]
}

/// Errors from parsing operator-supplied option strings.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// Unknown `--node-logging-format` value (must be `json` or `text`).
    #[error("unknown node-logging-format: {0}")]
    UnknownNodeLoggingFormat(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_testnet_magic_matches_upstream() {
        assert_eq!(DEFAULT_TESTNET_MAGIC, 42);
    }

    #[test]
    fn num_pools_round_trip() {
        let p = NumPools(3);
        assert_eq!(p.0, 3);
        assert!(NumPools(1) < NumPools(2));
    }

    #[test]
    fn num_relays_round_trip() {
        let r = NumRelays(5);
        assert_eq!(r.0, 5);
    }

    #[test]
    fn num_dreps_round_trip() {
        let d = NumDReps(7);
        assert_eq!(d.0, 7);
    }

    #[test]
    fn node_id_ord_is_natural() {
        assert!(NodeId(1) < NodeId(2));
    }

    #[test]
    fn input_node_config_file_round_trip() {
        let f = InputNodeConfigFile::new("/etc/node.json");
        assert_eq!(f.as_path().to_str(), Some("/etc/node.json"));
    }

    #[test]
    fn update_timestamps_default_matches_upstream() {
        // Upstream `instance Default UpdateTimestamps where
        // def = DontUpdateTimestamps`.
        assert_eq!(
            UpdateTimestamps::default(),
            UpdateTimestamps::DontUpdateTimestamps
        );
    }

    #[test]
    fn testnet_on_chain_params_default_matches_upstream() {
        // Upstream `instance Default TestnetOnChainParams where
        // def = DefaultParams`.
        assert_eq!(
            TestnetOnChainParams::default(),
            TestnetOnChainParams::DefaultParams
        );
    }

    #[test]
    fn testnet_on_chain_params_file_carries_path() {
        let p = TestnetOnChainParams::OnChainParamsFile(PathBuf::from("/tmp/params.json"));
        match p {
            TestnetOnChainParams::OnChainParamsFile(path) => {
                assert_eq!(path.to_str(), Some("/tmp/params.json"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn mainnet_params_url_is_the_blockfrost_file() {
        assert!(MAINNET_PARAMS_URL.starts_with("https://"));
        assert!(MAINNET_PARAMS_URL.ends_with("parameters.json"));
    }

    #[test]
    fn testnet_runtime_options_default_matches_upstream() {
        let d = TestnetRuntimeOptions::default();
        assert!(d.runtime_enable_new_epoch_state_logging);
        assert_eq!(d.runtime_enable_rpc, RpcSupport::RpcDisabled);
        assert_eq!(d.runtime_kes_source, PraosCredentialsSource::UseKesKeyFile);
    }

    #[test]
    fn genesis_options_default_matches_upstream() {
        let d = GenesisOptions::default();
        assert_eq!(d.genesis_testnet_magic, 42);
        assert_eq!(d.genesis_epoch_length, 500);
        assert_eq!(d.genesis_slot_length, 0.1);
        assert_eq!(d.genesis_active_slots_coeff, 0.05);
    }

    #[test]
    fn node_option_spo_relay_predicates() {
        let spo = NodeOption::SpoNodeOptions(vec!["--foo".to_string()]);
        let relay = NodeOption::RelayNodeOptions(Vec::new());
        assert!(spo.is_spo() && !spo.is_relay());
        assert!(relay.is_relay() && !relay.is_spo());
    }

    #[test]
    fn default_node_options_are_one_spo_two_relays() {
        let nodes = cardano_default_testnet_node_options();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes.iter().filter(|n| n.is_spo()).count(), 1);
        assert_eq!(nodes.iter().filter(|n| n.is_relay()).count(), 2);
    }

    #[test]
    fn testnet_env_options_carries_path_and_timestamp_policy() {
        let opts = TestnetEnvOptions {
            env_path: PathBuf::from("/tmp/env"),
            env_update_timestamps: UpdateTimestamps::UpdateTimestamps,
        };
        assert_eq!(opts.env_path.to_str(), Some("/tmp/env"));
    }

    #[test]
    fn rpc_support_default_is_disabled() {
        assert_eq!(RpcSupport::default(), RpcSupport::RpcDisabled);
    }

    #[test]
    fn node_logging_format_default_is_json() {
        assert_eq!(NodeLoggingFormat::default(), NodeLoggingFormat::AsJson);
    }

    #[test]
    fn node_logging_format_from_string_accepts_json() {
        assert_eq!(
            NodeLoggingFormat::from_string("json"),
            Ok(NodeLoggingFormat::AsJson)
        );
        assert_eq!(
            NodeLoggingFormat::from_string("JSON"),
            Ok(NodeLoggingFormat::AsJson)
        );
    }

    #[test]
    fn node_logging_format_from_string_accepts_text() {
        assert_eq!(
            NodeLoggingFormat::from_string("text"),
            Ok(NodeLoggingFormat::AsText)
        );
    }

    #[test]
    fn node_logging_format_from_string_rejects_unknown() {
        let err = NodeLoggingFormat::from_string("xml").expect_err("rejected");
        assert!(matches!(err, ParseError::UnknownNodeLoggingFormat(_)));
    }

    #[test]
    fn genesis_hashes_policy_default_is_with_hashes() {
        assert_eq!(
            GenesisHashesPolicy::default(),
            GenesisHashesPolicy::WithHashes
        );
    }

    #[test]
    fn praos_credentials_source_default_is_use_key_file() {
        assert_eq!(
            PraosCredentialsSource::default(),
            PraosCredentialsSource::UseKesKeyFile
        );
    }

    #[test]
    fn user_provided_data_default_is_no_provided() {
        let d: UserProvidedData<i32> = UserProvidedData::default();
        assert_eq!(d, UserProvidedData::NoUserProvidedData);
        assert!(d.as_ref().is_none());
        assert!(d.into_option().is_none());
    }

    #[test]
    fn user_provided_data_carries_value() {
        let d: UserProvidedData<String> = UserProvidedData::UserProvidedData("hello".to_string());
        assert_eq!(d.as_ref().map(String::as_str), Some("hello"));
        assert_eq!(d.into_option(), Some("hello".to_string()));
    }
}
