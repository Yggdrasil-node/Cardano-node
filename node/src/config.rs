//! Node configuration file types.
//!
//! The configuration format follows the same JSON convention used by the
//! official Cardano node runtime. A config file is a JSON object with
//! a primary peer address, optional ordered bootstrap relays, network magic,
//! protocol versions, and consensus parameters.
//!
//! Reference: `cardano-node/configuration/` in the IntersectMBO repository.

use std::collections::BTreeMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use yggdrasil_ledger::ProtocolParameters;
pub use yggdrasil_network::derive_peer_snapshot_freshness;
use yggdrasil_network::{
    ConsensusMode, LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement,
    LocalRootConfig, PeerAccessPoint, PeerSnapshotFreshness, PublicRootConfig, TopologyConfig,
    UseLedgerPeers, eligible_ledger_peer_candidates, ordered_peer_fallbacks,
    resolve_peer_access_points,
};
use yggdrasil_plutus::CostModel;

#[derive(Debug)]
struct ResolvedTopologyPeers {
    primary_peer: SocketAddr,
    fallback_peers: Vec<SocketAddr>,
    local_roots: Vec<LocalRootConfig>,
    public_roots: Vec<PublicRootConfig>,
    use_ledger_after_slot: Option<u64>,
    peer_snapshot_file: Option<String>,
}

/// Loaded peer snapshot metadata derived from the upstream
/// `peerSnapshotFile` JSON formats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedPeerSnapshot {
    /// Slot represented by the snapshot file, when available.
    pub slot: Option<u64>,
    /// Normalized ledger and big-ledger peer sets resolved from the snapshot.
    pub snapshot: LedgerPeerSnapshot,
}

/// Errors returned while loading a configured peer snapshot file.
#[derive(Debug, Error)]
pub enum PeerSnapshotLoadError {
    /// Reading the snapshot file failed.
    #[error("failed to read peer snapshot file {path}: {source}")]
    Io {
        /// File path that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// Snapshot JSON decoding failed.
    #[error("failed to parse peer snapshot file {path}: {source}")]
    Json {
        /// File path containing invalid JSON.
        path: PathBuf,
        /// Underlying JSON parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Trace dispatcher options for a single tracing namespace.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TraceNamespaceConfig {
    /// Optional severity override for the namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Optional detail level override for the namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Backend list such as `Stdout HumanFormatColoured` or `Forwarder`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<String>,
    /// Optional namespace-level rate limit.
    #[serde(
        default,
        rename = "maxFrequency",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_frequency: Option<f64>,
}

/// Forwarder queue sizing aligned with the upstream node tracing config.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TraceOptionForwarder {
    /// Path to the Unix socket for trace forwarding.
    #[serde(default = "default_trace_forwarder_socket_path", rename = "socketPath")]
    pub socket_path: String,
    /// Maximum buffered connection events.
    #[serde(
        default = "default_trace_forwarder_conn_queue_size",
        rename = "connQueueSize"
    )]
    pub conn_queue_size: u64,
    /// Maximum buffered disconnection events.
    #[serde(
        default = "default_trace_forwarder_disconn_queue_size",
        rename = "disconnQueueSize"
    )]
    pub disconn_queue_size: u64,
    /// Maximum reconnect delay in seconds.
    #[serde(
        default = "default_trace_forwarder_max_reconnect_delay",
        rename = "maxReconnectDelay"
    )]
    pub max_reconnect_delay: u64,
}

fn default_trace_forwarder_socket_path() -> String {
    "/tmp/cardano-trace-forwarder.sock".to_owned()
}

/// Canonical mainnet network magic — the discriminant that distinguishes
/// Cardano mainnet from test networks (preprod / preview / private
/// testnets). Every NtN and NtC handshake verifies this byte-for-byte,
/// and several preflight checks (e.g. `RequiresNetworkMagic` sanity) key
/// their canonical-default decision on whether `network_magic` equals
/// this value.
///
/// Reference: `cardano-node` `Cardano.Chain.Genesis.Data` mainnet config
/// / `protocolMagicId = 764824073`.
pub const MAINNET_NETWORK_MAGIC: u32 = 764_824_073;

/// Canonical preprod (public long-running testnet) network magic.
///
/// Reference: `cardano-configurations` preprod `shelley-genesis.json`
/// `networkMagic = 1`. Used by `--network preprod` and by every vendored
/// preprod config we ship.
pub const PREPROD_NETWORK_MAGIC: u32 = 1;

/// Canonical preview (shorter-cycle public testnet) network magic.
///
/// Reference: `cardano-configurations` preview `shelley-genesis.json`
/// `networkMagic = 2`. Used by `--network preview` and by every vendored
/// preview config we ship.
pub const PREVIEW_NETWORK_MAGIC: u32 = 2;

/// The Cardano network ID that mainnet reward / Shelley addresses must
/// carry in their high nibble. Distinct from [`MAINNET_NETWORK_MAGIC`]
/// (the handshake discriminant): the network ID lives inside every
/// address byte string and is checked at transaction-validation time.
///
/// Reference: `Cardano.Ledger.Api.Tx.Address` / `cardano-ledger`
/// `Network = Mainnet | Testnet`; the encoded form puts mainnet at `1`.
pub const MAINNET_NETWORK_ID: u8 = 1;

/// The Cardano network ID for ALL test networks (preprod, preview, and
/// any private testnet). Same encoding rule as [`MAINNET_NETWORK_ID`]
/// but represents `Network = Testnet` upstream.
pub const TESTNET_NETWORK_ID: u8 = 0;

/// Canonical major protocol version for the Conway era — the current
/// `MaxMajorProtVer` ceiling used as the default for
/// [`NodeConfigFile::max_major_protocol_version`].
///
/// Reference: upstream `Ouroboros.Consensus.Protocol.Abstract`
/// `MaxMajorProtVer`; Conway hard-fork advanced this from Babbage's 8
/// to 9, and on-chain pp-update has since advanced it to 10 on mainnet.
/// A hard-fork to a new era would add a new constant (e.g.
/// `NEXT_ERA_MAJOR_PROTOCOL_VERSION = 11`) and this one would remain as
/// the Conway-era ceiling.
pub const CONWAY_MAJOR_PROTOCOL_VERSION: u64 = 10;

/// Runtime consensus mode used for governor churn-regime selection.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ConsensusModeConfig {
    /// Plain Praos mode.
    #[serde(rename = "PraosMode")]
    PraosMode,
    /// Genesis consensus mode.
    #[serde(rename = "GenesisMode")]
    GenesisMode,
}

/// Whether on-the-wire Byron-era headers carry an explicit network magic.
///
/// Maps to upstream `RequiresNetworkMagic` from `cardano-node`'s
/// `Cardano.Crypto.ProtocolMagic`. Mainnet uses `RequiresNoMagic` (the
/// magic is implicit from the header's structural placement); test
/// networks (preprod/preview) use `RequiresMagic` (the magic is included
/// inline). Shelley-era and later handle magic separately, so this flag
/// is primarily relevant for Byron-era header decoding and for documenting
/// operator intent.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum RequiresNetworkMagic {
    /// Byron headers omit the magic (mainnet behaviour).
    #[serde(rename = "RequiresNoMagic")]
    RequiresNoMagic,
    /// Byron headers carry the magic inline (testnet behaviour).
    #[serde(rename = "RequiresMagic")]
    RequiresMagic,
}

impl RequiresNetworkMagic {
    /// Default for a given mainnet magic. Returns `RequiresNoMagic` only
    /// for the canonical [`MAINNET_NETWORK_MAGIC`]; every other magic is
    /// treated as a test network requiring inline magic, matching upstream
    /// `Cardano.Chain.Genesis.Config.mkConfigFromGenesisData` defaults.
    pub fn default_for_magic(network_magic: u32) -> Self {
        if network_magic == MAINNET_NETWORK_MAGIC {
            Self::RequiresNoMagic
        } else {
            Self::RequiresMagic
        }
    }
}

impl ConsensusModeConfig {
    /// Convert to the network-owned consensus mode type.
    pub fn to_network_mode(self) -> ConsensusMode {
        match self {
            Self::PraosMode => ConsensusMode::PraosMode,
            Self::GenesisMode => ConsensusMode::GenesisMode,
        }
    }
}

/// On-disk node configuration parsed from a JSON file.
///
/// CLI flags can override individual fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfigFile {
    /// Peer address in `host:port` form.
    pub peer_addr: SocketAddr,
    /// Optional local listen address for inbound node-to-node diffusion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_listen_addr: Option<SocketAddr>,
    /// Ordered fallback bootstrap relay addresses tried after `peer_addr`.
    #[serde(default)]
    pub bootstrap_peers: Vec<SocketAddr>,
    /// Ordered local root groups parsed from the topology file.
    #[serde(default)]
    pub local_roots: Vec<LocalRootConfig>,
    /// Ordered public root groups parsed from the topology file.
    #[serde(default)]
    pub public_roots: Vec<PublicRootConfig>,
    /// Slot after which ledger peers should be preferred when available.
    #[serde(default)]
    pub use_ledger_after_slot: Option<u64>,
    /// Peer snapshot file name used by upstream topology handling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_snapshot_file: Option<String>,
    /// Root directory for immutable, volatile, and ledger snapshot storage.
    #[serde(default = "default_storage_dir")]
    pub storage_dir: PathBuf,
    /// Minimum slot delta between persisted ledger checkpoints.
    #[serde(default = "default_checkpoint_interval_slots")]
    pub checkpoint_interval_slots: u64,
    /// Maximum number of persisted typed ledger checkpoints to retain.
    #[serde(default = "default_max_ledger_snapshots")]
    pub max_ledger_snapshots: usize,
    /// Maximum number of warm peers the BlockFetch pool may concurrently
    /// dispatch range requests to.
    ///
    /// Default `1` keeps the proven single-peer pipeline and is therefore
    /// byte-for-byte equivalent to historical behaviour: ChainSync drives
    /// a single leading peer, BlockFetch issues one range at a time, and
    /// blocks are validated/applied in strict slot order.
    ///
    /// Operator opt-in for `> 1` is groundwork for the multi-peer
    /// concurrent BlockFetch follow-up (Phase 3 item 5 of
    /// `docs/PARITY_PLAN.md`). The pool foundation in
    /// `crates/network/src/blockfetch_pool.rs` already exposes the
    /// per-peer concurrency cap, range-splitter, and reorder buffer
    /// needed to honour values `> 1`; the runtime wiring that actually
    /// reads this knob and dispatches across N peers is the next slice.
    /// Until that lands, values `> 1` parse but are effectively clamped
    /// to single-peer behaviour at runtime — a non-default here is a
    /// declaration of intent, not an immediate behaviour change.
    ///
    /// Reference: `Ouroboros.Network.BlockFetch.Decision` —
    /// `bfcMaxConcurrencyDeadline = 1`, `bfcMaxConcurrencyBulkSync = 2`
    /// (upstream typically caps at 2 per fetch-mode).
    #[serde(default = "default_max_concurrent_block_fetch_peers")]
    pub max_concurrent_block_fetch_peers: u8,
    /// The network magic for handshake (mainnet = 764824073).
    pub network_magic: u32,
    /// Whether on-the-wire Byron-era headers carry the network magic
    /// inline. Matches the upstream `RequiresNetworkMagic` config key.
    /// `None` means the value defaults to
    /// [`RequiresNetworkMagic::default_for_magic`] for the configured
    /// `network_magic` (mainnet → `RequiresNoMagic`, otherwise
    /// `RequiresMagic`). Currently parsed for upstream-config
    /// compatibility and operator-intent documentation; the ledger Byron
    /// header decoder already handles both shapes structurally.
    #[serde(
        rename = "RequiresNetworkMagic",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub requires_network_magic: Option<RequiresNetworkMagic>,
    /// Minimum Cardano node version reported by the operator's config.
    /// Matches the upstream `MinNodeVersion` config key. Currently parsed
    /// for upstream-config compatibility but no version gate is enforced
    /// (no semantic action is taken based on this value).
    #[serde(
        rename = "MinNodeVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_node_version: Option<String>,
    /// Upstream `Protocol` field (typically the literal string `"Cardano"`).
    /// Parsed for byte-for-byte compatibility with vendored upstream
    /// `config.json` files; semantically Yggdrasil only implements the
    /// Cardano protocol, so the value is documentation-only.
    #[serde(rename = "Protocol", default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Optional path to an upstream `checkpoints.json` file.  Matches
    /// `CheckpointsFile` in the official Cardano node configuration.
    /// Currently parsed for upstream-config compatibility; the full
    /// upstream "checkpoint pinning" feature (where listed
    /// `(slot, header_hash)` pairs are treated as authoritative chain
    /// anchors that no rollback may cross) is a separate follow-up
    /// slice. See `Cardano.Node.Configuration.Checkpoints`.
    #[serde(
        rename = "CheckpointsFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub checkpoints_file: Option<String>,
    /// Expected Blake2b-256 hash (lowercase hex) of the checkpoint file
    /// referenced by [`Self::checkpoints_file`]. Matches
    /// `CheckpointsFileHash` in the official Cardano node configuration.
    /// Verified at `validate-config` time against the raw-bytes digest of
    /// `checkpoints_file` via
    /// [`crate::genesis::verify_genesis_file_hash`] (era-agnostic, no
    /// canonical-CBOR step). Enforcement at the loader level will land
    /// alongside the checkpoint-pinning feature.
    #[serde(
        rename = "CheckpointsFileHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub checkpoints_file_hash: Option<String>,
    /// Byron-era last-known block version triplet `(major, minor, alt)`.
    /// Maps to upstream `LastKnownBlockVersion-Major` /
    /// `LastKnownBlockVersion-Minor` / `LastKnownBlockVersion-Alt`. The
    /// fields parse `(u32, u32, u32)` defaults of `(0, 0, 0)` when absent
    /// and round-trip into the exact upstream JSON key shape via
    /// individual `rename` annotations. Currently documentation-only — the
    /// active Shelley+ protocol-version proposal lives in
    /// [`Self::protocol_versions`].
    #[serde(
        rename = "LastKnownBlockVersion-Major",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_known_block_version_major: Option<u32>,
    /// See [`Self::last_known_block_version_major`].
    #[serde(
        rename = "LastKnownBlockVersion-Minor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_known_block_version_minor: Option<u32>,
    /// See [`Self::last_known_block_version_major`].
    #[serde(
        rename = "LastKnownBlockVersion-Alt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_known_block_version_alt: Option<u32>,
    /// Protocol version numbers to propose during handshake.
    pub protocol_versions: Vec<u32>,
    /// Slots per KES period for header verification (mainnet: 129600).
    #[serde(default = "default_slots_per_kes_period")]
    pub slots_per_kes_period: u64,
    /// Maximum KES evolutions for header verification (mainnet: 62).
    #[serde(default = "default_max_kes_evolutions")]
    pub max_kes_evolutions: u64,
    /// Number of slots per epoch (mainnet Shelley: 432000).
    #[serde(default = "default_epoch_length")]
    pub epoch_length: u64,
    /// Slots per epoch in the Byron region (defaults to 21,600 on the
    /// public networks).  Used together with [`Self::byron_to_shelley_slot`]
    /// for era-aware slot→epoch math during sync.
    #[serde(default = "default_byron_epoch_length")]
    pub byron_epoch_length: u64,
    /// Absolute slot of the first Shelley block (Byron→Shelley boundary).
    /// `None` indicates the network has no Byron prefix (e.g. preview).
    /// On mainnet this is `4_492_800` (epoch 208 × 21,600), on preprod
    /// this is `86_400` (epoch 4 × 21,600).
    #[serde(default)]
    pub byron_to_shelley_slot: Option<u64>,
    /// First Shelley epoch number.  Together with [`Self::byron_to_shelley_slot`]
    /// drives [`yggdrasil_consensus::EpochSchedule`].
    #[serde(default)]
    pub first_shelley_epoch: Option<u64>,
    /// Security parameter `k` (mainnet: 2160).
    #[serde(default = "default_security_param_k")]
    pub security_param_k: u64,
    /// Active slot coefficient `f` (mainnet: 0.05).
    #[serde(default = "default_active_slot_coeff")]
    pub active_slot_coeff: f64,
    /// Maximum major protocol version accepted in block headers.
    ///
    /// Blocks whose protocol-version major component exceeds this value are
    /// rejected during verification.  Matches `MaxMajorProtVer` from
    /// `Ouroboros.Consensus.Protocol.Abstract` in the Haskell node; the
    /// Conway-era default is 10.  Accepts the upstream operator-config
    /// key `MaxKnownMajorProtocolVersion` from the official `cardano-node`
    /// `config.json` so vendored configs parse without translation.
    #[serde(
        default = "default_max_major_protocol_version",
        alias = "MaxKnownMajorProtocolVersion"
    )]
    pub max_major_protocol_version: u64,
    /// KeepAlive heartbeat interval in seconds. `null` disables heartbeats.
    #[serde(default)]
    pub keepalive_interval_secs: Option<u64>,
    /// Peer-sharing handshake wire value (0 = disabled, >=1 = enabled).
    ///
    /// This value is advertised in node-to-node handshake version data and
    /// also drives governor association-mode decisions.
    #[serde(default = "default_peer_sharing", alias = "PeerSharing")]
    pub peer_sharing: u8,
    /// Runtime consensus mode used for peer-governor churn regime selection.
    #[serde(default = "default_consensus_mode", alias = "ConsensusMode")]
    pub consensus_mode: ConsensusModeConfig,
    /// Governor tick interval in seconds. Defaults to 5.
    #[serde(default = "default_governor_tick_interval_secs")]
    pub governor_tick_interval_secs: u64,
    /// Target number of known peers the governor maintains.
    ///
    /// Accepts the upstream config key `TargetNumberOfKnownPeers` as an
    /// alias so vendored / operator-supplied configs that use the official
    /// `cardano-node` key names parse without translation.
    #[serde(
        default = "default_governor_target_known",
        alias = "TargetNumberOfKnownPeers"
    )]
    pub governor_target_known: usize,
    /// Target number of established (warm + hot) peers the governor maintains.
    ///
    /// Upstream alias: `TargetNumberOfEstablishedPeers`.
    #[serde(
        default = "default_governor_target_established",
        alias = "TargetNumberOfEstablishedPeers"
    )]
    pub governor_target_established: usize,
    /// Target number of active (hot) peers the governor maintains.
    ///
    /// Upstream alias: `TargetNumberOfActivePeers`.
    #[serde(
        default = "default_governor_target_active",
        alias = "TargetNumberOfActivePeers"
    )]
    pub governor_target_active: usize,
    /// Target number of known big-ledger peers the governor maintains.
    ///
    /// Upstream alias: `TargetNumberOfKnownBigLedgerPeers`.
    #[serde(
        default = "default_governor_target_known_big_ledger",
        alias = "TargetNumberOfKnownBigLedgerPeers"
    )]
    pub governor_target_known_big_ledger: usize,
    /// Target number of established big-ledger peers the governor maintains.
    ///
    /// Upstream alias: `TargetNumberOfEstablishedBigLedgerPeers`.
    #[serde(
        default = "default_governor_target_established_big_ledger",
        alias = "TargetNumberOfEstablishedBigLedgerPeers"
    )]
    pub governor_target_established_big_ledger: usize,
    /// Target number of active big-ledger peers the governor maintains.
    ///
    /// Upstream alias: `TargetNumberOfActiveBigLedgerPeers`.
    #[serde(
        default = "default_governor_target_active_big_ledger",
        alias = "TargetNumberOfActiveBigLedgerPeers"
    )]
    pub governor_target_active_big_ledger: usize,
    /// Whether local logging output is enabled.
    #[serde(rename = "TurnOnLogging", default = "default_turn_on_logging")]
    pub turn_on_logging: bool,
    /// Whether namespace-based trace dispatch is enabled.
    #[serde(
        rename = "UseTraceDispatcher",
        default = "default_use_trace_dispatcher"
    )]
    pub use_trace_dispatcher: bool,
    /// Whether metrics production is enabled for tracing backends.
    #[serde(rename = "TurnOnLogMetrics", default = "default_turn_on_log_metrics")]
    pub turn_on_log_metrics: bool,
    /// Optional node name carried in trace objects and metrics labels.
    #[serde(
        rename = "TraceOptionNodeName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub trace_option_node_name: Option<String>,
    /// Optional metrics name prefix used by upstream-compatible tracing output.
    #[serde(
        rename = "TraceOptionMetricsPrefix",
        default = "default_trace_option_metrics_prefix"
    )]
    pub trace_option_metrics_prefix: String,
    /// Resource sampling interval in milliseconds.
    #[serde(
        rename = "TraceOptionResourceFrequency",
        default = "default_trace_option_resource_frequency"
    )]
    pub trace_option_resource_frequency: u64,
    /// Forwarder reconnect and queue sizing.
    #[serde(
        rename = "TraceOptionForwarder",
        default = "default_trace_option_forwarder"
    )]
    pub trace_option_forwarder: TraceOptionForwarder,
    /// Namespace trace options following the official node config shape.
    #[serde(rename = "TraceOptions", default = "default_trace_options")]
    pub trace_options: BTreeMap<String, TraceNamespaceConfig>,
    /// Path to the NtC (node-to-client) Unix domain socket.
    ///
    /// When configured, the `Run` command starts an NtC local server on this
    /// socket, allowing CLI tools and wallets to issue queries and submit
    /// transactions via the LocalStateQuery / LocalTxSubmission protocols.
    ///
    /// Matches `SocketPath` in the official Cardano node configuration.
    #[serde(
        rename = "SocketPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub socket_path: Option<String>,
    /// Relative path to the Shelley genesis file.  Matches `ShelleyGenesisFile`
    /// in the official Cardano node configuration.
    #[serde(
        rename = "ShelleyGenesisFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_genesis_file: Option<String>,
    /// Expected Blake2b-256 hash (lowercase hex) of the Shelley genesis
    /// file referenced by [`Self::shelley_genesis_file`]. Matches
    /// `ShelleyGenesisHash` in the official Cardano node configuration.
    /// When set, [`crate::genesis::verify_genesis_file_hash`] is invoked
    /// at startup to verify the file matches; mismatches abort startup
    /// rather than silently using a wrong genesis.
    #[serde(
        rename = "ShelleyGenesisHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_genesis_hash: Option<String>,
    /// Relative path to the Byron genesis file.  Matches `ByronGenesisFile`
    /// in the official Cardano node configuration.
    #[serde(
        rename = "ByronGenesisFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byron_genesis_file: Option<String>,
    /// Expected Byron genesis hash. **Currently parsed but not verified**
    /// because upstream Byron hashing uses canonical CBOR (round-trips the
    /// JSON through canonical CBOR and hashes that), which has not yet
    /// been ported to Rust. Tracked separately from the Shelley-family
    /// hashes which use raw-file Blake2b-256.
    #[serde(
        rename = "ByronGenesisHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byron_genesis_hash: Option<String>,
    /// Relative path to the Alonzo genesis file.  Matches `AlonzoGenesisFile`
    /// in the official Cardano node configuration.
    #[serde(
        rename = "AlonzoGenesisFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub alonzo_genesis_file: Option<String>,
    /// Expected Blake2b-256 hash of the Alonzo genesis file. Matches
    /// `AlonzoGenesisHash` in the official Cardano node configuration.
    #[serde(
        rename = "AlonzoGenesisHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub alonzo_genesis_hash: Option<String>,
    /// Relative path to the Conway genesis file.  Matches `ConwayGenesisFile`
    /// in the official Cardano node configuration.
    #[serde(
        rename = "ConwayGenesisFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub conway_genesis_file: Option<String>,
    /// Expected Blake2b-256 hash of the Conway genesis file. Matches
    /// `ConwayGenesisHash` in the official Cardano node configuration.
    #[serde(
        rename = "ConwayGenesisHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub conway_genesis_hash: Option<String>,
    /// Relative path to a P2P topology file.  Matches `TopologyFilePath` in
    /// the official Cardano node configuration.  When set, the topology file
    /// overrides any inline `local_roots`, `public_roots`, `bootstrap_peers`,
    /// `use_ledger_after_slot`, and `peer_snapshot_file` values in this config.
    ///
    /// Reference: `Cardano.Node.Types.TopologyFile` in cardano-node.
    #[serde(
        rename = "TopologyFilePath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub topology_file_path: Option<String>,

    // ── Block producer credentials ──────────────────────────────────────
    /// Path to the KES signing key file (text-envelope format).
    ///
    /// Matches the `--shelley-kes-key` CLI flag in the official Cardano node.
    /// Required for block production.
    #[serde(
        rename = "ShelleyKesKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_kes_key: Option<String>,

    /// Path to the VRF signing key file (text-envelope format).
    ///
    /// Matches the `--shelley-vrf-key` CLI flag in the official Cardano node.
    /// Required for block production.
    #[serde(
        rename = "ShelleyVrfKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_vrf_key: Option<String>,

    /// Path to the operational certificate file (text-envelope format).
    ///
    /// Matches the `--shelley-operational-certificate` CLI flag in the
    /// official Cardano node.  Required for block production.
    #[serde(
        rename = "ShelleyOperationalCertificate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_operational_certificate: Option<String>,

    /// Path to the stake-pool cold verification key file (text-envelope).
    ///
    /// Used as the block header `issuer_vkey` when forging blocks and to
    /// verify that the configured operational certificate is signed by the
    /// same cold key.
    #[serde(
        rename = "ShelleyOperationalCertificateIssuerVkey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shelley_operational_certificate_issuer_vkey: Option<String>,
}

/// Errors returned while loading a P2P topology file.
#[derive(Debug, Error)]
pub enum TopologyFileError {
    /// Reading the topology file failed.
    #[error("failed to read topology file {path}: {source}")]
    Io {
        /// File path that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// Topology JSON decoding failed.
    #[error("failed to parse topology file {path}: {source}")]
    Json {
        /// File path containing invalid JSON.
        path: PathBuf,
        /// Underlying JSON parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Load a P2P topology file from disk matching the upstream format.
///
/// The expected JSON format is:
/// ```json
/// {
///   "bootstrapPeers": [{"address": "...", "port": 3001}],
///   "localRoots": [{"accessPoints": [...], "advertise": false, "valency": 1, "trustable": false}],
///   "publicRoots": [{"accessPoints": [...], "advertise": false}],
///   "useLedgerAfterSlot": 128908821,
///   "peerSnapshotFile": "peer-snapshot.json"
/// }
/// ```
///
/// Reference: `Ouroboros.Network.Diffusion.Topology.NetworkTopology` and
/// `Cardano.Node.Configuration.TopologyP2P.readTopologyFile`.
pub fn load_topology_file(path: &Path) -> Result<TopologyConfig, TopologyFileError> {
    let contents = fs::read_to_string(path).map_err(|source| TopologyFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_str::<TopologyConfig>(&contents).map_err(|source| TopologyFileError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Apply a loaded topology file to a [`NodeConfigFile`], overriding the
/// inline peer topology fields.
///
/// This mirrors how the official Cardano node treats `--topology`: the
/// topology file is the authority for peer discovery and its fields replace
/// any values set in the main config.
pub fn apply_topology_to_config(cfg: &mut NodeConfigFile, topology: &TopologyConfig) {
    cfg.local_roots = topology.local_roots.clone();
    cfg.public_roots = topology.public_roots.clone();
    cfg.use_ledger_after_slot = topology.use_ledger_peers.to_after_slot();
    cfg.peer_snapshot_file = topology.peer_snapshot_file.clone();

    // Rebuild bootstrap_peers from the topology.
    let resolved = topology.resolved_root_providers();
    let mut addrs = Vec::new();
    for peer in resolved.public_roots.bootstrap_peers.iter() {
        if !addrs.contains(peer) {
            addrs.push(*peer);
        }
    }
    cfg.bootstrap_peers = addrs;
}

fn default_storage_dir() -> PathBuf {
    PathBuf::from("data")
}

fn default_checkpoint_interval_slots() -> u64 {
    2160
}

fn default_max_ledger_snapshots() -> usize {
    8
}

impl NodeConfigFile {
    /// Returns the expected Cardano network id for reward-account validation.
    ///
    /// Mainnet uses [`MAINNET_NETWORK_ID`] (`1`); every other magic is
    /// considered a test network and uses [`TESTNET_NETWORK_ID`] (`0`).
    pub fn expected_network_id(&self) -> u8 {
        if self.network_magic == MAINNET_NETWORK_MAGIC {
            MAINNET_NETWORK_ID
        } else {
            TESTNET_NETWORK_ID
        }
    }

    /// Build the era-aware [`yggdrasil_consensus::EpochSchedule`] for this
    /// network from the configured Shelley `epoch_length`, Byron
    /// `byron_epoch_length`, and the optional Byron→Shelley boundary
    /// (`byron_to_shelley_slot` + `first_shelley_epoch`).
    pub fn epoch_schedule(&self) -> yggdrasil_consensus::EpochSchedule {
        let shelley = yggdrasil_consensus::EpochSize(self.epoch_length);
        match (self.byron_to_shelley_slot, self.first_shelley_epoch) {
            (Some(boundary), Some(first)) => yggdrasil_consensus::EpochSchedule::with_byron_prefix(
                shelley,
                self.byron_epoch_length,
                boundary,
                first,
            ),
            _ => yggdrasil_consensus::EpochSchedule::fixed(shelley),
        }
    }

    /// Rebuild the network-owned topology configuration from the node config.
    /// Load and build a [`ProtocolParameters`] from the configured genesis files.
    ///
    /// Returns `None` when neither `ShelleyGenesisFile` nor `AlonzoGenesisFile`
    /// is configured (e.g. integration tests using programmatic configs), so
    /// callers can safely fall back to `ProtocolParameters::default()`.
    ///
    /// Returns an error if a configured path exists but cannot be parsed.
    pub fn load_genesis_protocol_params(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Option<ProtocolParameters>, crate::genesis::GenesisLoadError> {
        use crate::genesis::{
            build_protocol_parameters, load_alonzo_genesis, load_conway_genesis,
            load_shelley_genesis,
        };

        let shelley_path = self.shelley_genesis_file.as_deref().map(|f| {
            let p = std::path::Path::new(f);
            if let Some(base) = config_base_dir {
                base.join(p)
            } else {
                p.to_path_buf()
            }
        });
        let alonzo_path = self.alonzo_genesis_file.as_deref().map(|f| {
            let p = std::path::Path::new(f);
            if let Some(base) = config_base_dir {
                base.join(p)
            } else {
                p.to_path_buf()
            }
        });
        let conway_path = self.conway_genesis_file.as_deref().map(|f| {
            let p = std::path::Path::new(f);
            if let Some(base) = config_base_dir {
                base.join(p)
            } else {
                p.to_path_buf()
            }
        });

        match (shelley_path, alonzo_path) {
            (Some(sp), Some(ap)) => {
                let shelley = load_shelley_genesis(&sp)?;
                let alonzo = load_alonzo_genesis(&ap)?;
                let conway = if let Some(cp) = conway_path {
                    Some(load_conway_genesis(&cp)?)
                } else {
                    None
                };
                Ok(Some(build_protocol_parameters(
                    &shelley,
                    &alonzo,
                    conway.as_ref(),
                )))
            }
            _ => Ok(None),
        }
    }

    /// Load the parsed Shelley bootstrap bundle from the configured genesis
    /// file when one is present.
    pub fn load_shelley_genesis_bootstrap(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Option<crate::genesis::ShelleyGenesisBootstrap>, crate::genesis::GenesisLoadError>
    {
        use crate::genesis::load_shelley_genesis_bootstrap;

        let Some(path) = self.shelley_genesis_file.as_deref() else {
            return Ok(None);
        };

        let path = if let Some(base) = config_base_dir {
            base.join(Path::new(path))
        } else {
            Path::new(path).to_path_buf()
        };

        Ok(Some(load_shelley_genesis_bootstrap(&path)?))
    }

    /// Load the Byron genesis UTxO entries from the configured Byron genesis
    /// file when one is present.
    pub fn load_byron_genesis_utxo(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Vec<crate::genesis::ByronGenesisUtxoEntry>, crate::genesis::GenesisLoadError> {
        use crate::genesis::load_byron_genesis_utxo;

        let Some(path) = self.byron_genesis_file.as_deref() else {
            return Ok(Vec::new());
        };

        let path = if let Some(base) = config_base_dir {
            base.join(Path::new(path))
        } else {
            Path::new(path).to_path_buf()
        };

        load_byron_genesis_utxo(&path)
    }

    /// Verify the Blake2b-256 hashes of the configured Shelley / Alonzo /
    /// Conway genesis files against the operator-supplied
    /// `*GenesisHash` declarations.
    ///
    /// For each `(file_path, expected_hash)` pair where both sides are
    /// present, this method invokes
    /// [`crate::genesis::verify_genesis_file_hash`] and short-circuits on
    /// the first mismatch. Pairs where either the file path or the
    /// expected hash is `None` are skipped (no expectation, no check).
    /// Byron is intentionally skipped because upstream Byron hashing uses
    /// canonical CBOR rather than raw-file Blake2b-256; that case is
    /// tracked separately and will become a follow-up slice.
    ///
    /// Returns `Ok(())` when every checked file matches, or
    /// [`crate::genesis::GenesisLoadError::HashMismatch`] /
    /// [`crate::genesis::GenesisLoadError::InvalidHashHex`] on the first
    /// failure.
    ///
    /// Reference: `cardano-node` `Cardano.Node.Configuration.POM` —
    /// `parseGenesisHash` startup verification.
    pub fn verify_known_genesis_hashes(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<(), crate::genesis::GenesisLoadError> {
        use crate::genesis::verify_genesis_file_hash;

        let resolve = |relative: &str| -> std::path::PathBuf {
            let p = Path::new(relative);
            if let Some(base) = config_base_dir {
                base.join(p)
            } else {
                p.to_path_buf()
            }
        };

        let pairs = [
            (
                self.shelley_genesis_file.as_deref(),
                self.shelley_genesis_hash.as_deref(),
                "ShelleyGenesisHash",
            ),
            (
                self.alonzo_genesis_file.as_deref(),
                self.alonzo_genesis_hash.as_deref(),
                "AlonzoGenesisHash",
            ),
            (
                self.conway_genesis_file.as_deref(),
                self.conway_genesis_hash.as_deref(),
                "ConwayGenesisHash",
            ),
        ];

        for (file, expected, field) in pairs {
            if let (Some(file), Some(expected)) = (file, expected) {
                verify_genesis_file_hash(&resolve(file), expected, field)?;
            }
        }
        Ok(())
    }

    /// Load the genesis [`yggdrasil_ledger::EnactState`] from the configured
    /// Conway genesis file when a `constitution` section is present.
    pub fn load_genesis_enact_state(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Option<yggdrasil_ledger::EnactState>, crate::genesis::GenesisLoadError> {
        use crate::genesis::{build_genesis_enact_state, load_conway_genesis};

        let Some(path) = self.conway_genesis_file.as_deref() else {
            return Ok(None);
        };

        let path = if let Some(base) = config_base_dir {
            base.join(Path::new(path))
        } else {
            Path::new(path).to_path_buf()
        };

        let conway = load_conway_genesis(&path)?;
        build_genesis_enact_state(Some(&conway))
    }

    /// Load the simplified CEK [`CostModel`] from the configured Alonzo
    /// genesis file when a named Plutus cost-model map is available.
    pub fn load_plutus_cost_model(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Option<CostModel>, crate::genesis::GenesisCostModelError> {
        use crate::genesis::{build_plutus_cost_model, load_alonzo_genesis, load_conway_genesis};

        let Some(path) = self.alonzo_genesis_file.as_deref() else {
            return Ok(None);
        };

        let path = if let Some(base) = config_base_dir {
            base.join(Path::new(path))
        } else {
            Path::new(path).to_path_buf()
        };

        let alonzo = load_alonzo_genesis(&path)?;

        let conway = match self.conway_genesis_file.as_deref() {
            Some(path) => {
                let path = if let Some(base) = config_base_dir {
                    base.join(Path::new(path))
                } else {
                    Path::new(path).to_path_buf()
                };
                Some(load_conway_genesis(&path)?)
            }
            None => None,
        };

        build_plutus_cost_model(&alonzo, conway.as_ref())
    }

    pub fn topology_config(&self) -> TopologyConfig {
        TopologyConfig {
            bootstrap_peers: yggdrasil_network::UseBootstrapPeers::UseBootstrapPeers(
                self.bootstrap_peers
                    .iter()
                    .map(|addr| yggdrasil_network::PeerAccessPoint {
                        address: addr.ip().to_string(),
                        port: addr.port(),
                    })
                    .collect(),
            ),
            local_roots: self.local_roots.clone(),
            public_roots: self.public_roots.clone(),
            use_ledger_peers: self.use_ledger_peers_policy(),
            peer_snapshot_file: self.peer_snapshot_file.clone(),
        }
    }

    /// Return the typed network-owned ledger-peer policy for this config.
    pub fn use_ledger_peers_policy(&self) -> UseLedgerPeers {
        match self.use_ledger_after_slot {
            None => UseLedgerPeers::DontUseLedgerPeers,
            Some(0) => UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::Always),
            Some(slot) => UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::After(slot)),
        }
    }

    /// Returns ordered fallback peers derived from bootstrap peers and richer
    /// topology groups. Ordering follows the upstream topology split: explicit
    /// bootstrap peers first, then trustable local roots, then other local
    /// roots, and finally public roots.
    pub fn ordered_fallback_peers(&self) -> Vec<SocketAddr> {
        ordered_peer_fallbacks(
            self.peer_addr,
            &self.bootstrap_peers,
            &self.local_roots,
            &self.public_roots,
        )
    }

    /// Returns currently eligible ledger-derived fallbacks, excluding peers
    /// already covered by the primary or static topology fallback set.
    pub fn eligible_ledger_fallback_peers(
        &self,
        snapshot: &LedgerPeerSnapshot,
        latest_slot: Option<u64>,
        ledger_state_judgement: LedgerStateJudgement,
        peer_snapshot_freshness: PeerSnapshotFreshness,
    ) -> (LedgerPeerUseDecision, Vec<SocketAddr>) {
        let mut blocked = self.ordered_fallback_peers();
        blocked.push(self.peer_addr);

        eligible_ledger_peer_candidates(
            snapshot,
            &blocked,
            self.use_ledger_peers_policy(),
            latest_slot,
            ledger_state_judgement,
            peer_snapshot_freshness,
        )
    }

    /// Derive snapshot freshness from the configured `peerSnapshotFile`, the
    /// snapshot slot, and the latest recovered slot known at startup.
    pub fn peer_snapshot_freshness(
        &self,
        snapshot_slot: Option<u64>,
        latest_slot: Option<u64>,
        snapshot_available: bool,
    ) -> PeerSnapshotFreshness {
        derive_peer_snapshot_freshness(
            self.use_ledger_peers_policy(),
            self.peer_snapshot_file.is_some(),
            snapshot_slot,
            latest_slot,
            snapshot_available,
        )
    }
}

/// Load a configured peer snapshot file from disk.
pub fn load_peer_snapshot_file(path: &Path) -> Result<LoadedPeerSnapshot, PeerSnapshotLoadError> {
    let snapshot_json = fs::read_to_string(path).map_err(|source| PeerSnapshotLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    parse_peer_snapshot_json(&snapshot_json).map_err(|source| PeerSnapshotLoadError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Parse a peer snapshot JSON document into a normalized snapshot.
pub fn parse_peer_snapshot_json(
    snapshot_json: &str,
) -> Result<LoadedPeerSnapshot, serde_json::Error> {
    let value: Value = serde_json::from_str(snapshot_json)?;
    let slot = value
        .get("slotNo")
        .and_then(Value::as_u64)
        .or_else(|| value.get("Point").and_then(extract_snapshot_point_slot));

    Ok(LoadedPeerSnapshot {
        slot,
        snapshot: LedgerPeerSnapshot::new(
            extract_snapshot_pool_peers(&value, "allLedgerPools"),
            extract_snapshot_pool_peers(&value, "bigLedgerPools"),
        ),
    })
}

fn extract_snapshot_pool_peers(root: &Value, pool_key: &str) -> Vec<SocketAddr> {
    let Some(pools) = root.get(pool_key).and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut peers = Vec::new();
    for pool in pools {
        let Some(relays) = pool.get("relays").and_then(Value::as_array) else {
            continue;
        };

        for relay in relays {
            let Some(access_point) = parse_snapshot_access_point(relay) else {
                continue;
            };

            for peer in resolve_peer_access_points(&access_point) {
                if !peers.contains(&peer) {
                    peers.push(peer);
                }
            }
        }
    }

    peers
}

fn parse_snapshot_access_point(value: &Value) -> Option<PeerAccessPoint> {
    let address = value.get("address")?.as_str()?.to_owned();
    let port = value
        .get("port")?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())?;

    Some(PeerAccessPoint { address, port })
}

fn extract_snapshot_point_slot(point: &Value) -> Option<u64> {
    match point {
        Value::Object(object) => ["slotNo", "blockPointSlot", "slot"]
            .into_iter()
            .find_map(|field| object.get(field).and_then(Value::as_u64))
            .or_else(|| object.values().find_map(extract_snapshot_point_slot)),
        Value::Array(values) => values.iter().find_map(extract_snapshot_point_slot),
        _ => None,
    }
}

fn default_slots_per_kes_period() -> u64 {
    129_600
}

fn default_max_kes_evolutions() -> u64 {
    62
}

/// Default for [`NodeConfigFile::max_concurrent_block_fetch_peers`].
///
/// `1` preserves the proven single-peer BlockFetch pipeline. See the
/// field rustdoc for the multi-peer follow-up roadmap.
fn default_max_concurrent_block_fetch_peers() -> u8 {
    1
}

fn default_epoch_length() -> u64 {
    432_000
}

fn default_byron_epoch_length() -> u64 {
    21_600
}

fn default_security_param_k() -> u64 {
    2160
}

fn default_active_slot_coeff() -> f64 {
    0.05
}

/// Conway-era `MaxMajorProtVer` — see [`CONWAY_MAJOR_PROTOCOL_VERSION`].
fn default_max_major_protocol_version() -> u64 {
    CONWAY_MAJOR_PROTOCOL_VERSION
}

fn default_governor_tick_interval_secs() -> u64 {
    5
}

fn default_peer_sharing() -> u8 {
    1
}

fn default_consensus_mode() -> ConsensusModeConfig {
    ConsensusModeConfig::PraosMode
}

fn default_governor_target_known() -> usize {
    20
}

fn default_governor_target_established() -> usize {
    10
}

fn default_governor_target_active() -> usize {
    5
}

fn default_governor_target_known_big_ledger() -> usize {
    0
}

fn default_governor_target_established_big_ledger() -> usize {
    0
}

fn default_governor_target_active_big_ledger() -> usize {
    0
}

fn default_turn_on_logging() -> bool {
    true
}

fn default_use_trace_dispatcher() -> bool {
    true
}

fn default_turn_on_log_metrics() -> bool {
    true
}

fn default_trace_option_metrics_prefix() -> String {
    "cardano.node.metrics.".to_owned()
}

fn default_trace_option_resource_frequency() -> u64 {
    1000
}

fn default_trace_forwarder_conn_queue_size() -> u64 {
    64
}

fn default_trace_forwarder_disconn_queue_size() -> u64 {
    128
}

fn default_trace_forwarder_max_reconnect_delay() -> u64 {
    30
}

fn default_trace_option_forwarder() -> TraceOptionForwarder {
    TraceOptionForwarder {
        socket_path: default_trace_forwarder_socket_path(),
        conn_queue_size: default_trace_forwarder_conn_queue_size(),
        disconn_queue_size: default_trace_forwarder_disconn_queue_size(),
        max_reconnect_delay: default_trace_forwarder_max_reconnect_delay(),
    }
}

fn default_trace_options() -> BTreeMap<String, TraceNamespaceConfig> {
    BTreeMap::from([
        (
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: Some("DNormal".to_owned()),
                backends: vec![
                    "EKGBackend".to_owned(),
                    "Forwarder".to_owned(),
                    "PrometheusSimple suffix 127.0.0.1 12798".to_owned(),
                    "Stdout HumanFormatColoured".to_owned(),
                ],
                max_frequency: None,
            },
        ),
        (
            "ChainSync.Client".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Warning".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        ),
        (
            "Net.PeerSelection".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Info".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        ),
        (
            "Startup.DiffusionInit".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Info".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        ),
        (
            "Node.Recovery.Checkpoint".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Info".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: Some(1.0),
            },
        ),
    ])
}

fn parse_topology_config(topology_json: &str) -> TopologyConfig {
    serde_json::from_str::<TopologyConfig>(topology_json).unwrap_or_default()
}

fn ordered_topology_peer_candidates(topology: &TopologyConfig) -> Vec<SocketAddr> {
    topology.resolved_root_providers().ordered_candidates()
}

#[cfg(test)]
fn parse_topology_bootstrap_peers(topology_json: &str) -> Vec<(String, u16)> {
    parse_topology_config(topology_json)
        .bootstrap_peers
        .configured_peers()
        .iter()
        .map(|peer| (peer.address.clone(), peer.port))
        .collect()
}

fn resolve_topology_peers(
    topology_json: &str,
    fallback_primary: SocketAddr,
) -> ResolvedTopologyPeers {
    let topology = parse_topology_config(topology_json);
    let mut ordered = ordered_topology_peer_candidates(&topology);

    if ordered.is_empty() {
        ordered.push(fallback_primary);
    }

    ResolvedTopologyPeers {
        primary_peer: ordered[0],
        fallback_peers: ordered.into_iter().skip(1).collect(),
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_peers.to_after_slot(),
        peer_snapshot_file: topology.peer_snapshot_file,
    }
}

/// Well-known Cardano network presets.
///
/// Each variant carries the genesis parameters (network magic, epoch length,
/// security parameter, etc.) and a default bootstrap relay address sourced
/// from the official Cardano Operations Book environment pages.
///
/// Reference:
/// - <https://book.world.dev.cardano.org/env-mainnet.html>
/// - <https://book.world.dev.cardano.org/env-preprod.html>
/// - <https://book.world.dev.cardano.org/env-preview.html>
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPreset {
    /// Production network.
    Mainnet,
    /// Pre-production testnet (mirrors mainnet parameters).
    Preprod,
    /// Preview testnet (shorter epochs, smaller k).
    Preview,
}

impl FromStr for NetworkPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Self::Mainnet),
            "preprod" => Ok(Self::Preprod),
            "preview" => Ok(Self::Preview),
            other => Err(format!(
                "unknown network: {other} (expected mainnet, preprod, or preview)"
            )),
        }
    }
}

impl std::fmt::Display for NetworkPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Preprod => write!(f, "preprod"),
            Self::Preview => write!(f, "preview"),
        }
    }
}

impl NetworkPreset {
    /// Build a [`NodeConfigFile`] with the genesis parameters and default
    /// bootstrap relay for this network.
    pub fn to_config(self) -> NodeConfigFile {
        match self {
            Self::Mainnet => mainnet_config(),
            Self::Preprod => preprod_config(),
            Self::Preview => preview_config(),
        }
    }

    /// Return the canonical NtN/NtC handshake network magic for this
    /// preset, without constructing the full [`NodeConfigFile`].
    ///
    /// Useful for preflight cross-checks and CLI defaults that only need
    /// the magic value (e.g. `cardano-cli query-tip --network mainnet`).
    /// Returns the same value `to_config().network_magic` would, but at
    /// `O(1)` cost without touching the topology / genesis loader paths.
    pub fn network_magic(self) -> u32 {
        match self {
            Self::Mainnet => MAINNET_NETWORK_MAGIC,
            Self::Preprod => PREPROD_NETWORK_MAGIC,
            Self::Preview => PREVIEW_NETWORK_MAGIC,
        }
    }

    /// All valid network presets in canonical declaration order
    /// (`Mainnet`, `Preprod`, `Preview`).
    ///
    /// Useful for exhaustive tests and iterate-over-all-presets
    /// scenarios. Returns a `'static` slice so callers can pattern the
    /// iteration as `for &preset in NetworkPreset::all()`. Adding a new
    /// variant to [`NetworkPreset`] MUST extend this list — the preset
    /// enumeration tests (`vendored_network_presets_produce_only_…`,
    /// `network_preset_network_magic_matches_to_config_for_all_presets`,
    /// etc.) rely on this as their source of truth.
    pub const fn all() -> &'static [Self] {
        &[Self::Mainnet, Self::Preprod, Self::Preview]
    }
}

/// Returns a sensible default configuration targeting Cardano mainnet
/// relay `backbone.cardano.iog.io:3001`.
pub fn default_config() -> NodeConfigFile {
    mainnet_config()
}

/// Mainnet configuration.
///
/// Genesis source: `shelley-genesis.json` from
/// <https://book.world.dev.cardano.org/environments/mainnet/>.
pub fn mainnet_config() -> NodeConfigFile {
    let fallback_primary = "3.125.94.58:3001".parse().expect("valid default addr");
    let topology = resolve_topology_peers(
        include_str!("../configuration/mainnet/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr: topology.primary_peer,
        inbound_listen_addr: None,
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/mainnet"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        max_concurrent_block_fetch_peers: default_max_concurrent_block_fetch_peers(),
        network_magic: MAINNET_NETWORK_MAGIC,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        byron_epoch_length: 21_600,
        byron_to_shelley_slot: Some(4_492_800),
        first_shelley_epoch: Some(208),
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        max_major_protocol_version: default_max_major_protocol_version(),
        keepalive_interval_secs: Some(60),
        peer_sharing: default_peer_sharing(),
        consensus_mode: default_consensus_mode(),
        governor_tick_interval_secs: default_governor_tick_interval_secs(),
        governor_target_known: default_governor_target_known(),
        governor_target_established: default_governor_target_established(),
        governor_target_active: default_governor_target_active(),
        governor_target_known_big_ledger: default_governor_target_known_big_ledger(),
        governor_target_established_big_ledger: default_governor_target_established_big_ledger(),
        governor_target_active_big_ledger: default_governor_target_active_big_ledger(),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-mainnet".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
        socket_path: None,
        requires_network_magic: None,
        min_node_version: None,
        protocol: None,
        checkpoints_file: None,
        checkpoints_file_hash: None,
        last_known_block_version_major: None,
        last_known_block_version_minor: None,
        last_known_block_version_alt: None,
        shelley_genesis_file: Some("shelley-genesis.json".to_owned()),
        shelley_genesis_hash: Some(
            "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81".to_owned(),
        ),
        byron_genesis_file: Some("byron-genesis.json".to_owned()),
        byron_genesis_hash: Some(
            "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb".to_owned(),
        ),
        alonzo_genesis_file: Some("alonzo-genesis.json".to_owned()),
        alonzo_genesis_hash: Some(
            "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874".to_owned(),
        ),
        conway_genesis_file: Some("conway-genesis.json".to_owned()),
        conway_genesis_hash: Some(
            "15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62".to_owned(),
        ),
        topology_file_path: None,
        shelley_kes_key: None,
        shelley_vrf_key: None,
        shelley_operational_certificate: None,
        shelley_operational_certificate_issuer_vkey: None,
    }
}

/// Pre-production testnet configuration.
///
/// Genesis source: `shelley-genesis.json` from
/// <https://book.world.dev.cardano.org/environments/preprod/>.
pub fn preprod_config() -> NodeConfigFile {
    let fallback_primary = "3.125.94.58:3001".parse().expect("fallback addr");
    let topology = resolve_topology_peers(
        include_str!("../configuration/preprod/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr: topology.primary_peer,
        inbound_listen_addr: None,
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/preprod"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        max_concurrent_block_fetch_peers: default_max_concurrent_block_fetch_peers(),
        network_magic: PREPROD_NETWORK_MAGIC,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        byron_epoch_length: 21_600,
        byron_to_shelley_slot: Some(86_400),
        first_shelley_epoch: Some(4),
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        max_major_protocol_version: default_max_major_protocol_version(),
        keepalive_interval_secs: Some(60),
        peer_sharing: default_peer_sharing(),
        consensus_mode: default_consensus_mode(),
        governor_tick_interval_secs: default_governor_tick_interval_secs(),
        governor_target_known: default_governor_target_known(),
        governor_target_established: default_governor_target_established(),
        governor_target_active: default_governor_target_active(),
        governor_target_known_big_ledger: default_governor_target_known_big_ledger(),
        governor_target_established_big_ledger: default_governor_target_established_big_ledger(),
        governor_target_active_big_ledger: default_governor_target_active_big_ledger(),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-preprod".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
        socket_path: None,
        requires_network_magic: None,
        min_node_version: None,
        protocol: None,
        checkpoints_file: None,
        checkpoints_file_hash: None,
        last_known_block_version_major: None,
        last_known_block_version_minor: None,
        last_known_block_version_alt: None,
        shelley_genesis_file: Some("shelley-genesis.json".to_owned()),
        shelley_genesis_hash: Some(
            "162d29c4e1cf6b8a84f2d692e67a3ac6bc7851bc3e6e4afe64d15778bed8bd86".to_owned(),
        ),
        byron_genesis_file: Some("byron-genesis.json".to_owned()),
        byron_genesis_hash: Some(
            "d4b8de7a11d929a323373cbab6c1a9bdc931beffff11db111cf9d57356ee1937".to_owned(),
        ),
        alonzo_genesis_file: Some("alonzo-genesis.json".to_owned()),
        alonzo_genesis_hash: Some(
            "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874".to_owned(),
        ),
        conway_genesis_file: Some("conway-genesis.json".to_owned()),
        conway_genesis_hash: Some(
            "0eb6adaec3fcb1fe286c1b4ae0da2a117eafc3add51e17577d36dd39eddfc3db".to_owned(),
        ),
        topology_file_path: None,
        shelley_kes_key: None,
        shelley_vrf_key: None,
        shelley_operational_certificate: None,
        shelley_operational_certificate_issuer_vkey: None,
    }
}

/// Preview testnet configuration.
///
/// Genesis source: `shelley-genesis.json` from
/// <https://book.world.dev.cardano.org/environments/preview/>.
pub fn preview_config() -> NodeConfigFile {
    let fallback_primary = "3.125.94.58:3001".parse().expect("fallback addr");
    let topology = resolve_topology_peers(
        include_str!("../configuration/preview/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr: topology.primary_peer,
        inbound_listen_addr: None,
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/preview"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        max_concurrent_block_fetch_peers: default_max_concurrent_block_fetch_peers(),
        network_magic: PREVIEW_NETWORK_MAGIC,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 86_400,
        byron_epoch_length: 21_600,
        // Preview launched directly into Shelley with no Byron prefix.
        byron_to_shelley_slot: None,
        first_shelley_epoch: None,
        security_param_k: 432,
        active_slot_coeff: 0.05,
        max_major_protocol_version: default_max_major_protocol_version(),
        keepalive_interval_secs: Some(60),
        peer_sharing: default_peer_sharing(),
        consensus_mode: default_consensus_mode(),
        governor_tick_interval_secs: default_governor_tick_interval_secs(),
        governor_target_known: default_governor_target_known(),
        governor_target_established: default_governor_target_established(),
        governor_target_active: default_governor_target_active(),
        governor_target_known_big_ledger: default_governor_target_known_big_ledger(),
        governor_target_established_big_ledger: default_governor_target_established_big_ledger(),
        governor_target_active_big_ledger: default_governor_target_active_big_ledger(),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-preview".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
        socket_path: None,
        requires_network_magic: None,
        min_node_version: None,
        protocol: None,
        checkpoints_file: None,
        checkpoints_file_hash: None,
        last_known_block_version_major: None,
        last_known_block_version_minor: None,
        last_known_block_version_alt: None,
        shelley_genesis_file: Some("shelley-genesis.json".to_owned()),
        shelley_genesis_hash: Some(
            "363498d1024f84bb39d3fa9593ce391483cb40d479b87233f868d6e57c3a400d".to_owned(),
        ),
        byron_genesis_file: Some("byron-genesis.json".to_owned()),
        byron_genesis_hash: Some(
            "83de1d7302569ad56cf9139a41e2e11346d4cb4a31c00142557b6ab3fa550761".to_owned(),
        ),
        alonzo_genesis_file: Some("alonzo-genesis.json".to_owned()),
        alonzo_genesis_hash: Some(
            "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874".to_owned(),
        ),
        conway_genesis_file: Some("conway-genesis.json".to_owned()),
        conway_genesis_hash: Some(
            "9cc5084f02e27210eacba47af0872e3dba8946ad9460b6072d793e1d2f3987ef".to_owned(),
        ),
        topology_file_path: None,
        shelley_kes_key: None,
        shelley_vrf_key: None,
        shelley_operational_certificate: None,
        shelley_operational_certificate_issuer_vkey: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_network_magic_constant_matches_upstream() {
        // Upstream `cardano-node` `Cardano.Chain.Genesis.Data`
        // `protocolMagicId = 764824073`. This constant MUST NOT drift —
        // every NtN / NtC handshake verifies it byte-for-byte, so any
        // other value produces a silently-incompatible client.
        assert_eq!(MAINNET_NETWORK_MAGIC, 764_824_073);
    }

    #[test]
    fn preprod_network_magic_constant_matches_upstream() {
        // Upstream `cardano-configurations` preprod
        // `shelley-genesis.json` `networkMagic = 1`.
        assert_eq!(PREPROD_NETWORK_MAGIC, 1);
    }

    #[test]
    fn preview_network_magic_constant_matches_upstream() {
        // Upstream `cardano-configurations` preview
        // `shelley-genesis.json` `networkMagic = 2`.
        assert_eq!(PREVIEW_NETWORK_MAGIC, 2);
    }

    #[test]
    fn all_three_network_magics_are_distinct() {
        // Defensive invariant: if any two preset magics ever collide,
        // handshake disambiguation breaks. Upstream guarantees all three
        // are distinct; pin it locally so a typo on any constant fails.
        assert_ne!(MAINNET_NETWORK_MAGIC, PREPROD_NETWORK_MAGIC);
        assert_ne!(MAINNET_NETWORK_MAGIC, PREVIEW_NETWORK_MAGIC);
        assert_ne!(PREPROD_NETWORK_MAGIC, PREVIEW_NETWORK_MAGIC);
    }

    #[test]
    fn preset_configs_use_canonical_magic_constants() {
        // Pin `preprod_config()` / `preview_config()` → their canonical
        // constants so a future refactor re-inlining the literals fails CI.
        assert_eq!(preprod_config().network_magic, PREPROD_NETWORK_MAGIC);
        assert_eq!(preview_config().network_magic, PREVIEW_NETWORK_MAGIC);
    }

    #[test]
    fn network_preset_network_magic_matches_to_config_for_all_presets() {
        // Cheap accessor (`network_magic()`) and full constructor
        // (`to_config().network_magic`) MUST agree for every preset.
        // A drift here would mean preflight code (which uses the cheap
        // accessor) and the actual node startup (which uses `to_config`)
        // disagree on the network — silently producing handshake
        // failures on real connections.
        for &preset in NetworkPreset::all() {
            assert_eq!(
                preset.network_magic(),
                preset.to_config().network_magic,
                "preset {preset:?}: cheap accessor disagrees with to_config()",
            );
        }
    }

    #[test]
    fn default_governor_target_fns_match_governor_targets_default() {
        // The six `default_governor_target_*` serde-default functions
        // MUST agree with the corresponding fields of
        // `GovernorTargets::default()`. Drift here would mean a freshly
        // parsed config (uses serde defaults) and a hand-constructed
        // `GovernorTargets::default()` (used internally by the governor)
        // disagree on peer-selection targets — silently producing
        // different peer-governor behavior. Pinning both sides here
        // turns the drift into a CI failure.
        use yggdrasil_network::GovernorTargets;

        let defaults = GovernorTargets::default();
        assert_eq!(default_governor_target_known(), defaults.target_known);
        assert_eq!(
            default_governor_target_established(),
            defaults.target_established,
        );
        assert_eq!(default_governor_target_active(), defaults.target_active);
        assert_eq!(
            default_governor_target_known_big_ledger(),
            defaults.target_known_big_ledger,
        );
        assert_eq!(
            default_governor_target_established_big_ledger(),
            defaults.target_established_big_ledger,
        );
        assert_eq!(
            default_governor_target_active_big_ledger(),
            defaults.target_active_big_ledger,
        );
    }

    #[test]
    fn default_governor_targets_are_sane() {
        // Pins that the config defaults themselves satisfy
        // `sanePeerSelectionTargets` — a preflight failure here would
        // mean every fresh install hits the slice-40 `insane governor
        // targets` warning right out of the box. Belt-and-braces next
        // to slice 60's direct unit coverage of `is_sane`.
        use yggdrasil_network::GovernorTargets;
        let defaults = GovernorTargets {
            target_known: default_governor_target_known(),
            target_established: default_governor_target_established(),
            target_active: default_governor_target_active(),
            target_known_big_ledger: default_governor_target_known_big_ledger(),
            target_established_big_ledger: default_governor_target_established_big_ledger(),
            target_active_big_ledger: default_governor_target_active_big_ledger(),
            ..Default::default()
        };
        assert!(
            defaults.is_sane(),
            "default governor targets must be sane — they are the fresh-install baseline",
        );
    }

    #[test]
    fn conway_major_protocol_version_constant_matches_upstream_default() {
        // Pin the Conway-era default `MaxMajorProtVer` against the
        // upstream value. Also pin the relationship to
        // `default_max_major_protocol_version()` so a refactor that
        // inlines the value in one place but not the other fails CI.
        assert_eq!(CONWAY_MAJOR_PROTOCOL_VERSION, 10);
        assert_eq!(
            default_max_major_protocol_version(),
            CONWAY_MAJOR_PROTOCOL_VERSION
        );
    }

    #[test]
    fn preset_configs_use_conway_major_protocol_version() {
        // All three presets default to Conway-era; each `max_major` must
        // route through the named constant.
        assert_eq!(
            mainnet_config().max_major_protocol_version,
            CONWAY_MAJOR_PROTOCOL_VERSION,
        );
        assert_eq!(
            preprod_config().max_major_protocol_version,
            CONWAY_MAJOR_PROTOCOL_VERSION,
        );
        assert_eq!(
            preview_config().max_major_protocol_version,
            CONWAY_MAJOR_PROTOCOL_VERSION,
        );
    }

    /// Pin the canonical default for `max_concurrent_block_fetch_peers`
    /// across every preset.
    ///
    /// The default `1` keeps the proven single-peer BlockFetch pipeline.
    /// Drift here would silently switch a preset onto a code path that
    /// doesn't yet exist (multi-peer dispatch is groundwork-only — see
    /// the field rustdoc and `docs/PARITY_PLAN.md` Phase 3 item 5).
    /// Iterates `NetworkPreset::all()` (slice 82) to catch the case
    /// where someone bumps mainnet's default but forgets preprod/preview.
    #[test]
    fn preset_configs_share_canonical_max_concurrent_block_fetch_peers() {
        for &preset in NetworkPreset::all() {
            let cfg = preset.to_config();
            assert_eq!(
                cfg.max_concurrent_block_fetch_peers, 1,
                "preset {preset:?}: max_concurrent_block_fetch_peers must default to 1 \
                 (single-peer pipeline; multi-peer is Phase 3 item 5 follow-up)",
            );
        }
    }

    /// Pin that the `default_max_concurrent_block_fetch_peers()`
    /// fallback used by serde matches the explicit per-preset default.
    /// Drift between serde-default (parsed-from-disk path) and the
    /// preset constructors (in-process default path) would silently
    /// produce different runtime behaviour for the same nominal preset.
    #[test]
    fn default_max_concurrent_block_fetch_peers_matches_preset_value() {
        assert_eq!(default_max_concurrent_block_fetch_peers(), 1);
        for &preset in NetworkPreset::all() {
            assert_eq!(
                preset.to_config().max_concurrent_block_fetch_peers,
                default_max_concurrent_block_fetch_peers(),
                "preset {preset:?}: in-process default must match serde-default",
            );
        }
    }

    #[test]
    fn preset_configs_share_canonical_protocol_versions() {
        // The three preset constructors (`mainnet_config`, `preprod_config`,
        // `preview_config`) each independently hand-code
        // `protocol_versions: vec![13, 14]`. Drift between them would mean
        // a freshly bootstrapped mainnet relay proposes a different NtN
        // version range than a preprod relay built from the same binary
        // — silently producing handshake mismatches that look like
        // peer-misbehaviour at the operator level. This test iterates
        // every preset via `NetworkPreset::all()` (slice 82) and asserts
        // each preset's `protocol_versions` is identical to mainnet's.
        // A divergent edit to ANY single preset fails CI naming the
        // offending preset.
        let mainnet = mainnet_config().protocol_versions.clone();
        for &preset in NetworkPreset::all() {
            let cfg = preset.to_config();
            assert_eq!(
                cfg.protocol_versions, mainnet,
                "preset {preset:?}: protocol_versions {:?} drifted from mainnet {:?}",
                cfg.protocol_versions, mainnet,
            );
        }
    }

    #[test]
    fn preset_configs_protocol_versions_match_named_handshake_constants() {
        // Every entry in the canonical `protocol_versions` vector MUST
        // correspond to one of the named NtN handshake constants
        // (`HandshakeVersion::V13`, `V14`, `V15` — see slice 88). A
        // typo like `vec![13, 41]` (transposed digits) would otherwise
        // pass the slice-82 cross-preset check (since all three would
        // share the same typo) but break real handshake negotiation
        // because tag 41 is not a known NtN protocol version.
        //
        // Pinning the literal `[13, 14]` AND cross-asserting against the
        // named constants composes the two slice-88 named constants with
        // the canonical preset content, so a future bump of the proposed
        // version range (e.g. adding V15 once Conway+1 is live) is a
        // single coordinated edit: update the preset constructors,
        // update this test's `expected` array, and the named constants
        // already exist.
        use yggdrasil_network::HandshakeVersion;

        let expected: Vec<u32> = vec![
            HandshakeVersion::V13.0 as u32,
            HandshakeVersion::V14.0 as u32,
        ];
        assert_eq!(
            expected,
            vec![13_u32, 14_u32],
            "named NtN constants must correspond to literal protocol versions 13/14",
        );
        assert_eq!(
            mainnet_config().protocol_versions,
            expected,
            "mainnet preset must propose exactly the named NtN constants",
        );
    }

    #[test]
    fn mainnet_network_id_constant_matches_upstream() {
        // Upstream `Cardano.Ledger.Api.Tx.Address`: `Network = Mainnet`
        // encodes to `1` in the high nibble of every reward / Shelley
        // address. Drift would silently misclassify mainnet addresses
        // as testnet (or vice versa) at value-preservation time.
        assert_eq!(MAINNET_NETWORK_ID, 1);
        assert_eq!(TESTNET_NETWORK_ID, 0);
    }

    #[test]
    fn expected_network_id_uses_named_constants_consistently() {
        // Mainnet config → MAINNET_NETWORK_ID, every test net → TESTNET_NETWORK_ID.
        // Pin both branches so a refactor that flips one fails CI.
        assert_eq!(mainnet_config().expected_network_id(), MAINNET_NETWORK_ID);
        assert_eq!(preprod_config().expected_network_id(), TESTNET_NETWORK_ID);
        assert_eq!(preview_config().expected_network_id(), TESTNET_NETWORK_ID);

        // Custom magic → testnet (defensive: any non-mainnet magic
        // including the canonical preprod/preview values must classify
        // as testnet).
        let mut cfg = mainnet_config();
        cfg.network_magic = 99_999;
        assert_eq!(cfg.expected_network_id(), TESTNET_NETWORK_ID);
    }

    #[test]
    fn network_preset_network_magic_returns_named_constants() {
        // Pin the cheap accessor's output to the named constants so a
        // refactor that inlines the literal in one branch (and not the
        // others) fails CI.
        assert_eq!(
            NetworkPreset::Mainnet.network_magic(),
            MAINNET_NETWORK_MAGIC
        );
        assert_eq!(
            NetworkPreset::Preprod.network_magic(),
            PREPROD_NETWORK_MAGIC
        );
        assert_eq!(
            NetworkPreset::Preview.network_magic(),
            PREVIEW_NETWORK_MAGIC
        );
    }

    #[test]
    fn mainnet_config_uses_canonical_magic_constant() {
        // Pin the `mainnet_config()` → `MAINNET_NETWORK_MAGIC` link so a
        // future refactor that accidentally hard-codes a different value
        // inline in the constructor fails CI.
        let cfg = mainnet_config();
        assert_eq!(cfg.network_magic, MAINNET_NETWORK_MAGIC);
    }

    #[test]
    fn requires_network_magic_default_pins_constant() {
        // Mainnet → RequiresNoMagic, anything else → RequiresMagic. Pin
        // both branches so a regression flipping the constant silently
        // loses mainnet Byron-header-decode compatibility.
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(MAINNET_NETWORK_MAGIC),
            RequiresNetworkMagic::RequiresNoMagic,
        );
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(MAINNET_NETWORK_MAGIC + 1),
            RequiresNetworkMagic::RequiresMagic,
        );
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(2),
            RequiresNetworkMagic::RequiresMagic,
        );
    }

    #[test]
    fn default_config_round_trips_json() {
        let cfg = default_config();
        let json = serde_json::to_string_pretty(&cfg).expect("serialize");
        let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.network_magic, cfg.network_magic);
        assert_eq!(parsed.peer_addr, cfg.peer_addr);
        assert_eq!(parsed.bootstrap_peers, cfg.bootstrap_peers);
        assert_eq!(parsed.local_roots, cfg.local_roots);
        assert_eq!(parsed.public_roots, cfg.public_roots);
        assert_eq!(parsed.use_ledger_after_slot, cfg.use_ledger_after_slot);
        assert_eq!(parsed.peer_snapshot_file, cfg.peer_snapshot_file);
        assert_eq!(parsed.storage_dir, cfg.storage_dir);
        assert_eq!(
            parsed.checkpoint_interval_slots,
            cfg.checkpoint_interval_slots
        );
        assert_eq!(parsed.max_ledger_snapshots, cfg.max_ledger_snapshots);
        assert_eq!(
            parsed.governor_tick_interval_secs,
            cfg.governor_tick_interval_secs
        );
        assert_eq!(parsed.governor_target_known, cfg.governor_target_known);
        assert_eq!(
            parsed.governor_target_established,
            cfg.governor_target_established
        );
        assert_eq!(parsed.governor_target_active, cfg.governor_target_active);
        assert_eq!(
            parsed.governor_target_known_big_ledger,
            cfg.governor_target_known_big_ledger
        );
        assert_eq!(
            parsed.governor_target_established_big_ledger,
            cfg.governor_target_established_big_ledger
        );
        assert_eq!(
            parsed.governor_target_active_big_ledger,
            cfg.governor_target_active_big_ledger
        );
        assert_eq!(parsed.peer_sharing, cfg.peer_sharing);
        assert_eq!(parsed.consensus_mode, cfg.consensus_mode);
        assert_eq!(parsed.turn_on_logging, cfg.turn_on_logging);
        assert_eq!(parsed.use_trace_dispatcher, cfg.use_trace_dispatcher);
        assert_eq!(parsed.trace_option_node_name, cfg.trace_option_node_name);
        assert_eq!(parsed.trace_options, cfg.trace_options);
        assert_eq!(parsed.shelley_genesis_file, cfg.shelley_genesis_file);
        assert_eq!(parsed.alonzo_genesis_file, cfg.alonzo_genesis_file);
        assert_eq!(parsed.conway_genesis_file, cfg.conway_genesis_file);
        assert_eq!(
            parsed.shelley_operational_certificate_issuer_vkey,
            cfg.shelley_operational_certificate_issuer_vkey
        );
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert!(cfg.bootstrap_peers.is_empty());
        assert!(cfg.local_roots.is_empty());
        assert!(cfg.public_roots.is_empty());
        assert!(cfg.use_ledger_after_slot.is_none());
        assert!(cfg.peer_snapshot_file.is_none());
        assert_eq!(cfg.storage_dir, PathBuf::from("data"));
        assert_eq!(cfg.checkpoint_interval_slots, 2160);
        assert_eq!(cfg.max_ledger_snapshots, 8);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert!(cfg.keepalive_interval_secs.is_none());
        assert_eq!(cfg.peer_sharing, 1);
        assert_eq!(cfg.consensus_mode, ConsensusModeConfig::PraosMode);
        assert_eq!(cfg.governor_tick_interval_secs, 5);
        assert_eq!(cfg.governor_target_known, 20);
        assert_eq!(cfg.governor_target_established, 10);
        assert_eq!(cfg.governor_target_active, 5);
        assert_eq!(cfg.governor_target_known_big_ledger, 0);
        assert_eq!(cfg.governor_target_established_big_ledger, 0);
        assert_eq!(cfg.governor_target_active_big_ledger, 0);
        assert!(cfg.turn_on_logging);
        assert!(cfg.use_trace_dispatcher);
        assert!(cfg.turn_on_log_metrics);
        assert!(cfg.trace_option_node_name.is_none());
        assert!(cfg.shelley_genesis_file.is_none());
        assert!(cfg.alonzo_genesis_file.is_none());
        assert!(cfg.conway_genesis_file.is_none());
        assert!(cfg.shelley_operational_certificate_issuer_vkey.is_none());
        assert!(cfg.trace_options.contains_key(""));
        assert!(cfg.trace_options.contains_key("Node.Recovery.Checkpoint"));
        assert_eq!(
            cfg.trace_options
                .get("Node.Recovery.Checkpoint")
                .expect("checkpoint trace options")
                .max_frequency,
            Some(1.0)
        );
    }

    #[test]
    fn config_parses_big_ledger_governor_targets() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "governor_target_known_big_ledger": 8,
            "governor_target_established_big_ledger": 3,
            "governor_target_active_big_ledger": 1
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

        assert_eq!(cfg.governor_target_known_big_ledger, 8);
        assert_eq!(cfg.governor_target_established_big_ledger, 3);
        assert_eq!(cfg.governor_target_active_big_ledger, 1);
    }

    #[test]
    fn config_parses_upstream_genesis_hash_aliases() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "ShelleyGenesisFile": "shelley-genesis.json",
            "ShelleyGenesisHash": "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
            "AlonzoGenesisFile": "alonzo-genesis.json",
            "AlonzoGenesisHash": "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874",
            "ConwayGenesisFile": "conway-genesis.json",
            "ConwayGenesisHash": "15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62",
            "ByronGenesisFile": "byron-genesis.json",
            "ByronGenesisHash": "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(
            cfg.shelley_genesis_hash.as_deref(),
            Some("1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81")
        );
        assert_eq!(
            cfg.alonzo_genesis_hash.as_deref(),
            Some("7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874")
        );
        assert_eq!(
            cfg.conway_genesis_hash.as_deref(),
            Some("15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62")
        );
        assert_eq!(
            cfg.byron_genesis_hash.as_deref(),
            Some("5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb")
        );
    }

    #[test]
    fn verify_known_genesis_hashes_passes_when_files_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"{\"k\":1}";
        std::fs::write(dir.path().join("shelley.json"), body).expect("write");
        std::fs::write(dir.path().join("alonzo.json"), body).expect("write");
        std::fs::write(dir.path().join("conway.json"), body).expect("write");
        let expected_hex = hex::encode(yggdrasil_crypto::blake2b::hash_bytes_256(body).0);

        let mut cfg = mainnet_config();
        cfg.shelley_genesis_file = Some("shelley.json".to_owned());
        cfg.shelley_genesis_hash = Some(expected_hex.clone());
        cfg.alonzo_genesis_file = Some("alonzo.json".to_owned());
        cfg.alonzo_genesis_hash = Some(expected_hex.clone());
        cfg.conway_genesis_file = Some("conway.json".to_owned());
        cfg.conway_genesis_hash = Some(expected_hex);

        cfg.verify_known_genesis_hashes(Some(dir.path()))
            .expect("matching hashes should pass");
    }

    #[test]
    fn vendored_preset_hashes_match_vendored_genesis_files_end_to_end() {
        // Exercises the full path that runs on every `--network <preset>`
        // startup: each preset's preset constructor declares the
        // canonical *GenesisHash values, and `verify_known_genesis_hashes`
        // reads the vendored genesis files from `node/configuration/<network>/`
        // and compares Blake2b-256 of the file bytes. If a vendored file
        // is updated without bumping the in-code hash (or vice versa),
        // this test fails immediately so the drift is caught at CI time.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for &preset in NetworkPreset::all() {
            let cfg = preset.to_config();
            let base = manifest_dir.join("configuration").join(preset.to_string());
            cfg.verify_known_genesis_hashes(Some(&base))
                .unwrap_or_else(|err| {
                    panic!(
                        "preset {preset:?} hashes drifted from vendored files at {}: {err}",
                        base.display(),
                    );
                });
        }
    }

    #[test]
    fn verify_known_genesis_hashes_short_circuits_on_first_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("shelley.json"), b"{}").expect("write");

        let mut cfg = mainnet_config();
        cfg.shelley_genesis_file = Some("shelley.json".to_owned());
        cfg.shelley_genesis_hash = Some("0".repeat(64));
        // Other genesis paths intentionally point at non-existent files
        // so we can prove short-circuit: if the Shelley check did not fire
        // first, the loader for the next file would surface a different
        // error variant.
        cfg.alonzo_genesis_file = Some("missing-alonzo.json".to_owned());
        cfg.alonzo_genesis_hash = Some("0".repeat(64));
        cfg.conway_genesis_file = Some("missing-conway.json".to_owned());
        cfg.conway_genesis_hash = Some("0".repeat(64));

        let err = cfg
            .verify_known_genesis_hashes(Some(dir.path()))
            .expect_err("Shelley mismatch must surface");
        assert!(
            matches!(err, crate::genesis::GenesisLoadError::HashMismatch { .. }),
            "expected HashMismatch first, got {err:?}",
        );
    }

    #[test]
    fn config_parses_requires_network_magic_and_min_node_version() {
        // Mainnet uses RequiresNoMagic; preprod/preview use RequiresMagic.
        // Both keys parse into our typed fields and the operator-supplied
        // MinNodeVersion string round-trips verbatim.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "RequiresNetworkMagic": "RequiresNoMagic",
            "MinNodeVersion": "10.6.2"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(
            cfg.requires_network_magic,
            Some(RequiresNetworkMagic::RequiresNoMagic)
        );
        assert_eq!(cfg.min_node_version.as_deref(), Some("10.6.2"));

        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 1,
            "protocol_versions": [13],
            "RequiresNetworkMagic": "RequiresMagic"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(
            cfg.requires_network_magic,
            Some(RequiresNetworkMagic::RequiresMagic)
        );
        assert_eq!(cfg.min_node_version, None);
    }

    #[test]
    fn requires_network_magic_default_for_magic_matches_upstream() {
        // Canonical mainnet magic → RequiresNoMagic.
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(764_824_073),
            RequiresNetworkMagic::RequiresNoMagic,
        );
        // Anything else → RequiresMagic (preprod is 1, preview is 2,
        // sancho/scratchpad networks have arbitrary magics).
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(1),
            RequiresNetworkMagic::RequiresMagic,
        );
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(2),
            RequiresNetworkMagic::RequiresMagic,
        );
        assert_eq!(
            RequiresNetworkMagic::default_for_magic(0),
            RequiresNetworkMagic::RequiresMagic,
        );
    }

    #[test]
    fn config_parses_checkpoints_file_upstream_keys() {
        // Vendored mainnet config ships these alongside the genesis hash
        // declarations. We currently parse them for byte-for-byte
        // upstream-config compat; the underlying checkpoint-pinning
        // feature is a separate slice.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "CheckpointsFile": "checkpoints.json",
            "CheckpointsFileHash": "3e6dee5bae7acc6d870187e72674b37c929be8c66e62a552cf6a876b1af31ade"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.checkpoints_file.as_deref(), Some("checkpoints.json"));
        assert_eq!(
            cfg.checkpoints_file_hash.as_deref(),
            Some("3e6dee5bae7acc6d870187e72674b37c929be8c66e62a552cf6a876b1af31ade")
        );
    }

    #[test]
    fn config_parses_last_known_block_version_and_protocol_upstream_keys() {
        // The hyphenated `LastKnownBlockVersion-*` keys round-trip into
        // distinct typed fields and the literal `Protocol` string is
        // preserved, matching upstream `cardano-node`'s mainnet config.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 764824073,
            "protocol_versions": [13],
            "Protocol": "Cardano",
            "LastKnownBlockVersion-Major": 3,
            "LastKnownBlockVersion-Minor": 0,
            "LastKnownBlockVersion-Alt": 0
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.protocol.as_deref(), Some("Cardano"));
        assert_eq!(cfg.last_known_block_version_major, Some(3));
        assert_eq!(cfg.last_known_block_version_minor, Some(0));
        assert_eq!(cfg.last_known_block_version_alt, Some(0));
    }

    #[test]
    fn config_parses_max_known_major_protocol_version_upstream_alias() {
        // Upstream `cardano-node` ships `MaxKnownMajorProtocolVersion` in
        // `config.json`; vendored configs that use this key must parse
        // straight into our `max_major_protocol_version` field.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "MaxKnownMajorProtocolVersion": 11
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.max_major_protocol_version, 11);
    }

    #[test]
    fn config_parses_upstream_target_peer_count_aliases() {
        // The official cardano-node config uses PascalCase keys
        // `TargetNumberOfKnownPeers` etc.; vendored / operator-supplied
        // configs that use those names must parse without translation.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TargetNumberOfKnownPeers": 150,
            "TargetNumberOfEstablishedPeers": 60,
            "TargetNumberOfActivePeers": 30,
            "TargetNumberOfKnownBigLedgerPeers": 20,
            "TargetNumberOfEstablishedBigLedgerPeers": 10,
            "TargetNumberOfActiveBigLedgerPeers": 4
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

        assert_eq!(cfg.governor_target_known, 150);
        assert_eq!(cfg.governor_target_established, 60);
        assert_eq!(cfg.governor_target_active, 30);
        assert_eq!(cfg.governor_target_known_big_ledger, 20);
        assert_eq!(cfg.governor_target_established_big_ledger, 10);
        assert_eq!(cfg.governor_target_active_big_ledger, 4);
    }

    #[test]
    fn config_parses_peer_sharing_and_consensus_mode_aliases() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "PeerSharing": 0,
            "ConsensusMode": "GenesisMode"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");

        assert_eq!(cfg.peer_sharing, 0);
        assert_eq!(cfg.consensus_mode, ConsensusModeConfig::GenesisMode);
    }

    #[test]
    fn tracing_config_parses_with_upstream_field_names() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TurnOnLogging": true,
            "UseTraceDispatcher": true,
            "TurnOnLogMetrics": false,
            "TraceOptionNodeName": "yggdrasil-local",
            "TraceOptionMetricsPrefix": "cardano.node.metrics.",
            "TraceOptionResourceFrequency": 500,
            "TraceOptionForwarder": {
                "connQueueSize": 16,
                "disconnQueueSize": 32,
                "maxReconnectDelay": 5
            },
            "TraceOptions": {
                "": {
                    "severity": "Notice",
                    "detail": "DNormal",
                    "backends": ["Stdout MachineFormat"]
                },
                "Net.PeerSelection": {
                    "severity": "Info"
                }
            }
        }"#;

        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert!(cfg.turn_on_logging);
        assert!(cfg.use_trace_dispatcher);
        assert!(!cfg.turn_on_log_metrics);
        assert_eq!(
            cfg.trace_option_node_name.as_deref(),
            Some("yggdrasil-local")
        );
        assert_eq!(cfg.trace_option_resource_frequency, 500);
        assert_eq!(cfg.trace_option_forwarder.conn_queue_size, 16);
        assert_eq!(
            cfg.trace_options
                .get("")
                .expect("root trace options")
                .backends,
            vec!["Stdout MachineFormat".to_owned()]
        );
        assert_eq!(
            cfg.trace_options
                .get("Net.PeerSelection")
                .expect("peer selection trace options")
                .severity
                .as_deref(),
            Some("Info")
        );
    }

    #[test]
    fn mainnet_stability_window() {
        let cfg = default_config();
        // stability_window = 3k/f = 3 * 2160 / 0.05 = 129600
        let stability_window = (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
        assert_eq!(stability_window, 129_600);
    }

    #[test]
    fn mainnet_preset_matches_genesis() {
        let cfg = NetworkPreset::Mainnet.to_config();
        let mut candidates = vec![cfg.peer_addr];
        candidates.extend(cfg.bootstrap_peers.iter().copied());
        assert_eq!(cfg.network_magic, 764_824_073);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.use_ledger_after_slot, Some(177_724_800));
        assert_eq!(
            cfg.peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
        assert_eq!(cfg.storage_dir, PathBuf::from("data/mainnet"));
        assert_eq!(cfg.expected_network_id(), 1);
        assert_eq!(cfg.checkpoint_interval_slots, 2160);
        assert_eq!(cfg.max_ledger_snapshots, 8);
        assert_eq!(
            cfg.shelley_genesis_file.as_deref(),
            Some("shelley-genesis.json")
        );
        assert_eq!(
            cfg.alonzo_genesis_file.as_deref(),
            Some("alonzo-genesis.json")
        );
        assert_eq!(
            cfg.conway_genesis_file.as_deref(),
            Some("conway-genesis.json")
        );
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= 3);
    }

    #[test]
    fn mainnet_preset_loads_plutus_cost_model() {
        let cfg = NetworkPreset::Mainnet.to_config();
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("configuration/mainnet");
        let model = cfg
            .load_plutus_cost_model(Some(base_dir.as_path()))
            .expect("load plutus cost model")
            .expect("mainnet plutus cost model");
        assert_eq!(model.step_costs.var_cpu, 29_773);
        assert_eq!(model.step_costs.var_mem, 100);
        assert_eq!(model.builtin_cpu, 29_773);
        assert_eq!(model.builtin_mem, 100);
    }

    #[test]
    fn preprod_preset_matches_genesis() {
        let cfg = NetworkPreset::Preprod.to_config();
        assert_eq!(cfg.network_magic, 1);
        assert_eq!(cfg.expected_network_id(), 0);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.use_ledger_after_slot, Some(112_406_400));
        assert_eq!(
            cfg.peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
        assert_eq!(cfg.storage_dir, PathBuf::from("data/preprod"));
        assert_eq!(cfg.checkpoint_interval_slots, 2160);
        assert_eq!(cfg.max_ledger_snapshots, 8);
        assert!(cfg.bootstrap_peers.is_empty());
    }

    #[test]
    fn preview_preset_matches_genesis() {
        let cfg = NetworkPreset::Preview.to_config();
        assert_eq!(cfg.network_magic, 2);
        assert_eq!(cfg.expected_network_id(), 0);
        assert_eq!(cfg.epoch_length, 86_400);
        assert_eq!(cfg.security_param_k, 432);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.use_ledger_after_slot, Some(102_729_600));
        assert_eq!(
            cfg.peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
        assert_eq!(cfg.storage_dir, PathBuf::from("data/preview"));
        // stability_window = 3*432/0.05 = 25920
        let stability_window = (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
        assert_eq!(stability_window, 25_920);
        assert!(cfg.bootstrap_peers.is_empty());
    }

    #[test]
    fn explicit_bootstrap_peers_parse_from_json() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "bootstrap_peers": ["127.0.0.2:3001", "127.0.0.3:3001"],
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse with bootstrap peers");
        assert_eq!(cfg.peer_addr, "127.0.0.1:3001".parse().expect("addr"));
        assert_eq!(cfg.bootstrap_peers.len(), 2);
    }

    #[test]
    fn topology_parser_reads_bootstrap_peers() {
        let peers =
            parse_topology_bootstrap_peers(include_str!("../configuration/mainnet/topology.json"));
        assert_eq!(peers.len(), 3);
        assert_eq!(peers[0].0, "backbone.cardano.iog.io");
        assert_eq!(peers[0].1, 3001);
    }

    #[test]
    fn topology_resolution_falls_back_when_json_has_no_bootstrap_peers() {
        let fallback: SocketAddr = "127.0.0.1:3001".parse().expect("fallback");
        let topology = resolve_topology_peers("{\"bootstrapPeers\":[]}", fallback);
        assert_eq!(topology.primary_peer, fallback);
        assert!(topology.fallback_peers.is_empty());
    }

    #[test]
    fn topology_resolution_prefers_bootstrap_then_trustable_local_then_public_roots() {
        let fallback: SocketAddr = "127.0.0.99:3001".parse().expect("fallback");
        let topology = resolve_topology_peers(
            r#"{
                "bootstrapPeers": [
                    { "address": "127.0.0.10", "port": 3001 },
                    { "address": "127.0.0.11", "port": 3001 }
                ],
                "localRoots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.12", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": false,
                        "valency": 1
                    },
                    {
                        "accessPoints": [
                            { "address": "127.0.0.13", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": true,
                        "valency": 1
                    }
                ],
                "publicRoots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.14", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ]
            }"#,
            fallback,
        );

        assert_eq!(
            topology.primary_peer,
            "127.0.0.10:3001".parse().expect("addr")
        );
        assert_eq!(
            topology.fallback_peers,
            vec![
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
                "127.0.0.14:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn ordered_fallback_peers_include_resolved_topology_groups() {
        let cfg: NodeConfigFile = serde_json::from_str(
            r#"{
                "peer_addr": "127.0.0.10:3001",
                "bootstrap_peers": ["127.0.0.11:3001"],
                "local_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.13", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": true,
                        "valency": 1
                    },
                    {
                        "accessPoints": [
                            { "address": "127.0.0.12", "port": 3001 }
                        ],
                        "advertise": false,
                        "trustable": false,
                        "valency": 1
                    }
                ],
                "public_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.14", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ],
                "network_magic": 42,
                "protocol_versions": [13]
            }"#,
        )
        .expect("parse with topology groups");

        assert_eq!(
            cfg.ordered_fallback_peers(),
            vec![
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
                "127.0.0.14:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn use_ledger_peers_policy_preserves_legacy_option_semantics() {
        let mut cfg = default_config();

        cfg.use_ledger_after_slot = None;
        assert_eq!(
            cfg.use_ledger_peers_policy(),
            UseLedgerPeers::DontUseLedgerPeers
        );

        cfg.use_ledger_after_slot = Some(0);
        assert_eq!(
            cfg.use_ledger_peers_policy(),
            UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::Always)
        );

        cfg.use_ledger_after_slot = Some(42);
        assert_eq!(
            cfg.use_ledger_peers_policy(),
            UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::After(42))
        );
    }

    #[test]
    fn topology_config_round_trips_network_owned_fields() {
        let cfg = NetworkPreset::Mainnet.to_config();
        let topology = cfg.topology_config();

        assert_eq!(topology.local_roots, cfg.local_roots);
        assert_eq!(topology.public_roots, cfg.public_roots);
        assert_eq!(topology.use_ledger_peers, cfg.use_ledger_peers_policy());
        assert_eq!(topology.peer_snapshot_file, cfg.peer_snapshot_file);
    }

    #[test]
    fn eligible_ledger_fallback_peers_returns_empty_when_policy_blocks_use() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(100);

        let snapshot = LedgerPeerSnapshot::new(
            ["127.0.0.20:3001".parse().expect("ledger")],
            ["127.0.0.21:3001".parse().expect("big")],
        );

        let (decision, peers) = cfg.eligible_ledger_fallback_peers(
            &snapshot,
            Some(99),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::Fresh,
        );

        assert_eq!(
            decision,
            LedgerPeerUseDecision::BeforeUseLedgerAfterSlot {
                after_slot: 100,
                latest_slot: 99,
            }
        );
        assert!(peers.is_empty());
    }

    #[test]
    fn eligible_ledger_fallback_peers_filters_primary_and_static_fallbacks() {
        let cfg: NodeConfigFile = serde_json::from_str(
            r#"{
                "peer_addr": "127.0.0.1:3001",
                "bootstrap_peers": ["127.0.0.2:3001"],
                "public_roots": [
                    {
                        "accessPoints": [
                            { "address": "127.0.0.3", "port": 3001 }
                        ],
                        "advertise": false
                    }
                ],
                "use_ledger_after_slot": 0,
                "peer_snapshot_file": "peer-snapshot.json",
                "network_magic": 42,
                "protocol_versions": [13]
            }"#,
        )
        .expect("parse config");

        let snapshot = LedgerPeerSnapshot::new(
            [
                "127.0.0.1:3001".parse().expect("primary overlap"),
                "127.0.0.2:3001".parse().expect("bootstrap overlap"),
                "127.0.0.4:3001".parse().expect("new ledger"),
            ],
            [
                "127.0.0.3:3001".parse().expect("public overlap"),
                "127.0.0.5:3001".parse().expect("new big ledger"),
            ],
        );

        let (decision, peers) = cfg.eligible_ledger_fallback_peers(
            &snapshot,
            Some(1),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::Fresh,
        );

        assert_eq!(decision, LedgerPeerUseDecision::Eligible);
        assert_eq!(
            peers,
            vec![
                "127.0.0.4:3001".parse().expect("ledger fallback"),
                "127.0.0.5:3001".parse().expect("big ledger fallback"),
            ]
        );
    }

    #[test]
    fn eligible_ledger_fallback_peers_returns_empty_when_snapshot_is_not_fresh() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(0);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        let snapshot = LedgerPeerSnapshot::new(
            ["127.0.0.20:3001".parse().expect("ledger")],
            ["127.0.0.21:3001".parse().expect("big")],
        );

        let (decision, peers) = cfg.eligible_ledger_fallback_peers(
            &snapshot,
            Some(100),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::Stale,
        );

        assert_eq!(
            decision,
            LedgerPeerUseDecision::BlockedByPeerSnapshot {
                freshness: PeerSnapshotFreshness::Stale,
            }
        );
        assert!(peers.is_empty());
    }

    #[test]
    fn peer_snapshot_freshness_waits_for_latest_slot_before_gate() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(100);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        assert_eq!(
            cfg.peer_snapshot_freshness(Some(100), None, true),
            PeerSnapshotFreshness::Awaiting
        );
        assert_eq!(
            cfg.peer_snapshot_freshness(Some(100), Some(99), true),
            PeerSnapshotFreshness::Awaiting
        );
    }

    #[test]
    fn peer_snapshot_freshness_marks_old_snapshot_stale_after_gate() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(100);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        assert_eq!(
            cfg.peer_snapshot_freshness(Some(99), Some(100), true),
            PeerSnapshotFreshness::Stale
        );
        assert_eq!(
            cfg.peer_snapshot_freshness(Some(100), Some(100), true),
            PeerSnapshotFreshness::Fresh
        );
    }

    #[test]
    fn derive_peer_snapshot_freshness_matches_node_config_helper() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(100);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        assert_eq!(
            derive_peer_snapshot_freshness(
                cfg.use_ledger_peers_policy(),
                true,
                Some(100),
                Some(100),
                true,
            ),
            cfg.peer_snapshot_freshness(Some(100), Some(100), true)
        );
    }

    #[test]
    fn parse_peer_snapshot_json_supports_v2_big_ledger_snapshots() {
        let loaded = parse_peer_snapshot_json(
            r#"{
                "version": 2,
                "slotNo": 42,
                "bigLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.20", "port": 3001 },
                            { "address": "127.0.0.21", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("parse v2 snapshot");

        assert_eq!(loaded.slot, Some(42));
        assert!(loaded.snapshot.ledger_peers.is_empty());
        assert_eq!(
            loaded.snapshot.big_ledger_peers,
            vec![
                "127.0.0.20:3001".parse().expect("peer"),
                "127.0.0.21:3001".parse().expect("peer"),
            ]
        );
    }

    #[test]
    fn parse_peer_snapshot_json_supports_v23_all_ledger_snapshots() {
        let loaded = parse_peer_snapshot_json(
            r#"{
                "NodeToClientVersion": 23,
                "Point": {
                    "slot": 84,
                    "hash": "00"
                },
                "NetworkMagic": 1,
                "allLedgerPools": [
                    {
                        "relativeStake": 0.25,
                        "relays": [
                            { "address": "127.0.0.30", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("parse v23 snapshot");

        assert_eq!(loaded.slot, Some(84));
        assert_eq!(
            loaded.snapshot.ledger_peers,
            vec!["127.0.0.30:3001".parse().expect("peer")]
        );
        assert!(loaded.snapshot.big_ledger_peers.is_empty());
    }

    #[test]
    fn network_preset_from_str() {
        assert_eq!(
            "mainnet".parse::<NetworkPreset>().expect("mainnet"),
            NetworkPreset::Mainnet
        );
        assert_eq!(
            "Preprod".parse::<NetworkPreset>().expect("preprod"),
            NetworkPreset::Preprod
        );
        assert_eq!(
            "PREVIEW".parse::<NetworkPreset>().expect("preview"),
            NetworkPreset::Preview
        );
        assert!("unknown".parse::<NetworkPreset>().is_err());
    }

    #[test]
    fn network_preset_all_returns_every_variant_exactly_once() {
        // Pins `NetworkPreset::all()` content: exactly the three variants
        // in canonical declaration order. Extends the slice-80-era guard
        // that every preset's constants match upstream by ensuring the
        // iteration helper's set itself can't silently drift — a new
        // enum variant must be explicitly added to `all()` or this test
        // (and the downstream `.all()` callers) fail immediately.
        let all = NetworkPreset::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], NetworkPreset::Mainnet);
        assert_eq!(all[1], NetworkPreset::Preprod);
        assert_eq!(all[2], NetworkPreset::Preview);

        // And every variant must be distinct — catches a duplicated
        // entry from a copy-paste refactor.
        assert_ne!(all[0], all[1]);
        assert_ne!(all[0], all[2]);
        assert_ne!(all[1], all[2]);
    }

    #[test]
    fn network_preset_display_round_trips() {
        for &preset in NetworkPreset::all() {
            let s = preset.to_string();
            let parsed: NetworkPreset = s.parse().expect("display should round-trip");
            assert_eq!(parsed, preset);
        }
    }

    #[test]
    fn default_config_is_mainnet() {
        let def = default_config();
        let mainnet = mainnet_config();
        assert_eq!(def.network_magic, mainnet.network_magic);
        assert_eq!(def.epoch_length, mainnet.epoch_length);
        assert_eq!(def.security_param_k, mainnet.security_param_k);
        assert_eq!(def.expected_network_id(), 1);
    }

    #[test]
    fn topology_file_path_config_parses() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13],
            "TopologyFilePath": "topology.json"
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.topology_file_path.as_deref(), Some("topology.json"));
    }

    #[test]
    fn topology_file_path_absent_defaults_to_none() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
        assert!(cfg.topology_file_path.is_none());
    }

    #[test]
    fn load_topology_file_reads_upstream_format() {
        let dir = std::env::temp_dir().join(format!(
            "yggdrasil-topology-load-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let topo_path = dir.join("topology.json");
        std::fs::write(
            &topo_path,
            r#"{
                "bootstrapPeers": [
                    {"address": "127.0.0.20", "port": 3001}
                ],
                "localRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.21", "port": 3002}
                        ],
                        "advertise": false,
                        "valency": 1,
                        "trustable": true
                    }
                ],
                "publicRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.22", "port": 3003}
                        ],
                        "advertise": false
                    }
                ],
                "useLedgerAfterSlot": 42000,
                "peerSnapshotFile": "snap.json"
            }"#,
        )
        .expect("write topology file");

        let topology = load_topology_file(&topo_path).expect("load topology");
        assert_eq!(topology.local_roots.len(), 1);
        assert_eq!(topology.public_roots.len(), 1);
        assert_eq!(topology.use_ledger_peers.to_after_slot(), Some(42000));
        assert_eq!(topology.peer_snapshot_file.as_deref(), Some("snap.json"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_topology_file_returns_error_on_missing_file() {
        let result = load_topology_file(std::path::Path::new("/tmp/nonexistent-topology.json"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, TopologyFileError::Io { .. }));
    }

    #[test]
    fn apply_topology_to_config_overrides_inline_topology() {
        use yggdrasil_network::TopologyConfig;
        let mut cfg = default_config();
        cfg.local_roots = Vec::new();
        cfg.public_roots = Vec::new();
        cfg.use_ledger_after_slot = None;
        cfg.peer_snapshot_file = None;

        let topology = TopologyConfig {
            local_roots: vec![yggdrasil_network::LocalRootConfig {
                access_points: vec![yggdrasil_network::PeerAccessPoint {
                    address: "127.0.0.30".to_owned(),
                    port: 3001,
                }],
                advertise: false,
                trustable: true,
                hot_valency: 1,
                warm_valency: None,
                diffusion_mode: Default::default(),
            }],
            public_roots: vec![yggdrasil_network::PublicRootConfig {
                access_points: vec![yggdrasil_network::PeerAccessPoint {
                    address: "127.0.0.31".to_owned(),
                    port: 3002,
                }],
                advertise: false,
            }],
            use_ledger_peers: yggdrasil_network::UseLedgerPeers::UseLedgerPeers(
                yggdrasil_network::AfterSlot::After(99000),
            ),
            peer_snapshot_file: Some("my-snap.json".to_owned()),
            ..Default::default()
        };

        apply_topology_to_config(&mut cfg, &topology);

        assert_eq!(cfg.local_roots.len(), 1);
        assert_eq!(cfg.public_roots.len(), 1);
        assert_eq!(cfg.use_ledger_after_slot, Some(99000));
        assert_eq!(cfg.peer_snapshot_file.as_deref(), Some("my-snap.json"));
    }

    #[test]
    fn topology_file_path_round_trips_json() {
        let mut cfg = default_config();
        cfg.topology_file_path = Some("my-topology.json".to_owned());
        let json = serde_json::to_string_pretty(&cfg).expect("serialize");
        let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            parsed.topology_file_path.as_deref(),
            Some("my-topology.json")
        );
    }

    /// Default `max_major_protocol_version` matches Conway-era `MaxMajorProtVer`
    /// (upstream value: 10).
    #[test]
    fn max_major_protocol_version_default_is_conway_era() {
        let cfg = default_config();
        assert_eq!(cfg.max_major_protocol_version, 10);
    }

    /// `max_major_protocol_version` round-trips through JSON serialization and
    /// deserializes to the default when absent from the input.
    #[test]
    fn max_major_protocol_version_round_trips_and_defaults() {
        // Explicit value round-trips.
        let mut cfg = default_config();
        cfg.max_major_protocol_version = 12;
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: NodeConfigFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.max_major_protocol_version, 12);

        // Missing from JSON → defaults to 10.
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
        let parsed: NodeConfigFile = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.max_major_protocol_version, 10);
    }
}
