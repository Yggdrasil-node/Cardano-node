#![cfg_attr(test, allow(clippy::unwrap_used))]
//! yggdrasil-node-config — node configuration file types + path resolution + upstream pin metadata.
//!
//! Wave 5 PR 7 split this crate out of the monolithic `yggdrasil-node`
//! binary. Three logical surfaces now live here as a single leaf-of-
//! the-build-graph crate:
//!
//!  * Configuration parsing: `NodeConfigFile`, `NetworkPreset`,
//!    `TraceNamespaceConfig`, and the JSON-first deserializer with
//!    PascalCase upstream key aliases (this file). Source-of-truth
//!    for the operator-stable Tier-1 config surface declared in
//!    `docs/COMPATIBILITY.md`.
//!  * [`path_resolve`]: `$XDG_CONFIG_HOME` / per-network preset path
//!    resolution helpers (pure `std::path`, no I/O).
//!  * [`upstream_pins`]: cardano-base / ouroboros-consensus / etc.
//!    git SHA constants pinned at the policy tag from
//!    `docs/parity-matrix.json::reference.tag` (currently 11.0.1).
//!    Cross-checked by `scripts/check-fixture-manifest.py`.
//!
//! The `yggdrasil-node` binary re-exports this crate via
//! `pub use yggdrasil_node_config as config;` so the public surface
//! `yggdrasil_node_config::NetworkPreset` stays stable for every
//! external consumer (sister tools, downstream embedders, the
//! `cardano-cli` subcommand registered at `crates/tools/cardano-cli/`).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side binary `NodeConfigFile` struct + JSON-first deserializer (with YAML fallback) + PascalCase upstream key aliases (`TargetNumberOfKnownPeers`, `MaxKnownMajorProtocolVersion`, `ShelleyGenesisHash`, etc.). Upstream's equivalent is split across `Cardano.Node.Configuration.POM` + `Cardano.Tracing.Config` + per-subsystem configs; Yggdrasil unifies into a single struct that the binary's CLI override layer applies on top of.

pub mod path_resolve;
pub mod upstream_pins;

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
    UseLedgerPeers, always_eligible_snapshot_peers, eligible_ledger_peer_candidates,
    ordered_peer_fallbacks, resolve_peer_access_points,
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
    /// Default `2` matches upstream `Ouroboros.Network.BlockFetch.Decision`
    /// `bfcMaxConcurrencyBulkSync = 2` — the canonical initial-sync
    /// concurrency cap. The runtime path is fully wired:
    /// `runtime.rs::handle_cm_actions` calls
    /// [`OutboundPeerManager::migrate_session_to_worker`] on every
    /// `StartConnect → promote_to_warm` when this knob is `> 1`,
    /// registering the peer's `BlockFetchClient` in the shared
    /// `FetchWorkerPool`. The sync loop's
    /// `execute_multi_peer_blockfetch_plan` then dispatches
    /// fetch ranges across the registered workers in parallel.
    ///
    /// R218 operational verification on mainnet
    /// (`docs/operational-runs/2026-04-30-round-218-*.md`) measured
    /// a **67% throughput gain** (3.33 → 5.55 blk/s) with knob=4
    /// settling at 2 active workers (per the
    /// `bfcMaxConcurrencyBulkSync = 2` upstream cap).
    ///
    /// Operators wanting strict single-peer behaviour for replay or
    /// byte-for-byte audit comparison can override to `1`; operators
    /// with rich topologies who want to push beyond the BulkSync cap
    /// can set `> 2` (the runtime scales linearly per additional
    /// warm peer).
    ///
    /// Reference: `Ouroboros.Network.BlockFetch.Decision` —
    /// `bfcMaxConcurrencyDeadline = 1`, `bfcMaxConcurrencyBulkSync = 2`.
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
    /// [`yggdrasil_node_genesis::verify_genesis_file_hash`]. Enforcement at the
    /// loader level will land alongside the checkpoint-pinning feature.
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
    /// When set, [`yggdrasil_node_genesis::verify_genesis_file_hash`] is invoked
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
    /// Expected Byron genesis hash.
    ///
    /// Unlike Shelley-family genesis hashes, upstream Byron hashes are
    /// computed over `Text.JSON.Canonical.renderCanonicalJSON` of the parsed
    /// JSON value. Startup verifies this with
    /// [`yggdrasil_node_genesis::verify_byron_genesis_file_hash`].
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
    ) -> Result<Option<ProtocolParameters>, yggdrasil_node_genesis::GenesisLoadError> {
        use yggdrasil_node_genesis::{
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
                )?))
            }
            _ => Ok(None),
        }
    }

    /// Load the parsed Shelley bootstrap bundle from the configured genesis
    /// file when one is present.
    pub fn load_shelley_genesis_bootstrap(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<
        Option<yggdrasil_node_genesis::ShelleyGenesisBootstrap>,
        yggdrasil_node_genesis::GenesisLoadError,
    > {
        use yggdrasil_node_genesis::load_shelley_genesis_bootstrap;

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
    ) -> Result<
        Vec<yggdrasil_node_genesis::ByronGenesisUtxoEntry>,
        yggdrasil_node_genesis::GenesisLoadError,
    > {
        use yggdrasil_node_genesis::load_byron_genesis_utxo;

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

    /// Verify the Blake2b-256 hashes of the configured genesis files against
    /// the operator-supplied `*GenesisHash` declarations.
    ///
    /// For each `(file_path, expected_hash)` pair where both sides are
    /// present, this method invokes the era-appropriate hash verifier and
    /// short-circuits on the first mismatch. Each configured file must have
    /// its paired hash and vice versa. Byron uses canonical JSON rendering;
    /// Shelley / Alonzo / Conway use raw file bytes.
    ///
    /// Returns `Ok(())` when every checked file matches, or
    /// [`yggdrasil_node_genesis::GenesisLoadError::HashMismatch`] /
    /// [`yggdrasil_node_genesis::GenesisLoadError::InvalidHashHex`] on the first
    /// failure.
    ///
    /// Reference: `cardano-node` `Cardano.Node.Configuration.POM` —
    /// `parseGenesisHash` startup verification.
    pub fn verify_known_genesis_hashes(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<(), yggdrasil_node_genesis::GenesisLoadError> {
        use yggdrasil_node_genesis::{verify_byron_genesis_file_hash, verify_genesis_file_hash};

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
                self.byron_genesis_file.as_deref(),
                self.byron_genesis_hash.as_deref(),
                "ByronGenesisHash",
                true,
            ),
            (
                self.shelley_genesis_file.as_deref(),
                self.shelley_genesis_hash.as_deref(),
                "ShelleyGenesisHash",
                false,
            ),
            (
                self.alonzo_genesis_file.as_deref(),
                self.alonzo_genesis_hash.as_deref(),
                "AlonzoGenesisHash",
                false,
            ),
            (
                self.conway_genesis_file.as_deref(),
                self.conway_genesis_hash.as_deref(),
                "ConwayGenesisHash",
                false,
            ),
        ];

        for (file, expected, field, byron) in pairs {
            // Hard-fail on unpaired (file, hash). A configured genesis file
            // without a matching `*GenesisHash` would otherwise be loaded
            // unverified — that is the path that lets an operator silently
            // substitute a tampered or wrong-network genesis file.  Audit
            // finding M-8.
            match (file, expected) {
                (Some(file), Some(expected)) => {
                    let path = resolve(file);
                    if byron {
                        verify_byron_genesis_file_hash(&path, expected)?;
                    } else {
                        verify_genesis_file_hash(&path, expected, field)?;
                    }
                }
                (Some(_), None) => {
                    return Err(yggdrasil_node_genesis::GenesisLoadError::MissingHash { field });
                }
                (None, Some(_)) => {
                    return Err(yggdrasil_node_genesis::GenesisLoadError::MissingFile { field });
                }
                (None, None) => {} // optional era genesis (e.g. Conway pre-Conway)
            }
        }
        Ok(())
    }

    /// Load the genesis [`yggdrasil_ledger::EnactState`] from the configured
    /// Conway genesis file when a `constitution` section is present.
    pub fn load_genesis_enact_state(
        &self,
        config_base_dir: Option<&Path>,
    ) -> Result<Option<yggdrasil_ledger::EnactState>, yggdrasil_node_genesis::GenesisLoadError>
    {
        use yggdrasil_node_genesis::{build_genesis_enact_state, load_conway_genesis};

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
    ) -> Result<Option<CostModel>, yggdrasil_node_genesis::GenesisCostModelError> {
        use yggdrasil_node_genesis::{
            build_plutus_cost_model, load_alonzo_genesis, load_conway_genesis,
        };

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

    /// Returns peers loaded from the configured `peerSnapshotFile` that are
    /// **always eligible** regardless of the `useLedgerAfterSlot` gate.
    ///
    /// Upstream `Ouroboros.Network.PeerSelection.LedgerPeers` populates
    /// `bigLedgerPeers` from the snapshot file at process start; only LIVE
    /// chain-derived ledger peers wait for the gate. Yggdrasil R250 splits the
    /// snapshot path from the live-ledger path so initial sync can multi-peer-
    /// fetch from genesis (closing the dominant perf gap surfaced by the R249
    /// side-by-side preview soak).
    ///
    /// Excludes peers already covered by the primary or static topology
    /// fallback set, mirroring `eligible_ledger_fallback_peers` filtering.
    pub fn always_eligible_snapshot_fallbacks(
        &self,
        snapshot_overlay: Option<&LedgerPeerSnapshot>,
    ) -> Vec<SocketAddr> {
        let mut blocked = self.ordered_fallback_peers();
        blocked.push(self.peer_addr);
        always_eligible_snapshot_peers(snapshot_overlay, &blocked)
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
/// `2` matches upstream `Ouroboros.Network.BlockFetch.Decision`'s
/// `bfcMaxConcurrencyBulkSync = 2` — the canonical initial-sync
/// concurrency cap.  R218 (`docs/operational-runs/2026-04-30-round-218-
/// mainnet-multipeer-fetch-rate.md`) measured a 67% throughput gain
/// (3.33 → 5.55 blk/s on mainnet) with knob=4 actually saturating at
/// 2 workers because `bfcMaxConcurrencyBulkSync = 2` is the upstream
/// ceiling for BulkSync mode.  Operators who want single-peer
/// behaviour for replay/audit can still set `1`; operators with rich
/// topologies who want to saturate beyond the BulkSync cap can set
/// `> 2` (Yggdrasil's runtime honours up to N warm-peer workers).
fn default_max_concurrent_block_fetch_peers() -> u8 {
    2
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
        include_str!("../../yggdrasil-node/configuration/mainnet/topology.json"),
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
        include_str!("../../yggdrasil-node/configuration/preprod/topology.json"),
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
        include_str!("../../yggdrasil-node/configuration/preview/topology.json"),
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
mod tests;
