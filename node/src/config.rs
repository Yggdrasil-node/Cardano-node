//! Node configuration file types.
//!
//! The configuration format follows the same JSON convention used by the
//! official Cardano node runtime. A config file is a JSON object with
//! a primary peer address, optional ordered bootstrap relays, network magic,
//! protocol versions, and consensus parameters.
//!
//! Reference: `cardano-node/configuration/` in the IntersectMBO repository.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use yggdrasil_network::{
    LocalRootConfig, PublicRootConfig, TopologyConfig,
    ordered_peer_fallbacks,
};

#[derive(Debug)]
struct ResolvedTopologyPeers {
    primary_peer: SocketAddr,
    fallback_peers: Vec<SocketAddr>,
    local_roots: Vec<LocalRootConfig>,
    public_roots: Vec<PublicRootConfig>,
    use_ledger_after_slot: Option<u64>,
    peer_snapshot_file: Option<String>,
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
    #[serde(default, rename = "maxFrequency", skip_serializing_if = "Option::is_none")]
    pub max_frequency: Option<f64>,
}

/// Forwarder queue sizing aligned with the upstream node tracing config.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TraceOptionForwarder {
    /// Maximum buffered connection events.
    #[serde(default = "default_trace_forwarder_conn_queue_size", rename = "connQueueSize")]
    pub conn_queue_size: u64,
    /// Maximum buffered disconnection events.
    #[serde(default = "default_trace_forwarder_disconn_queue_size", rename = "disconnQueueSize")]
    pub disconn_queue_size: u64,
    /// Maximum reconnect delay in seconds.
    #[serde(default = "default_trace_forwarder_max_reconnect_delay", rename = "maxReconnectDelay")]
    pub max_reconnect_delay: u64,
}

/// On-disk node configuration parsed from a JSON file.
///
/// CLI flags can override individual fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfigFile {
    /// Peer address in `host:port` form.
    pub peer_addr: SocketAddr,
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
    /// The network magic for handshake (mainnet = 764824073).
    pub network_magic: u32,
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
    /// Security parameter `k` (mainnet: 2160).
    #[serde(default = "default_security_param_k")]
    pub security_param_k: u64,
    /// Active slot coefficient `f` (mainnet: 0.05).
    #[serde(default = "default_active_slot_coeff")]
    pub active_slot_coeff: f64,
    /// KeepAlive heartbeat interval in seconds. `null` disables heartbeats.
    #[serde(default)]
    pub keepalive_interval_secs: Option<u64>,
    /// Whether local logging output is enabled.
    #[serde(rename = "TurnOnLogging", default = "default_turn_on_logging")]
    pub turn_on_logging: bool,
    /// Whether namespace-based trace dispatch is enabled.
    #[serde(rename = "UseTraceDispatcher", default = "default_use_trace_dispatcher")]
    pub use_trace_dispatcher: bool,
    /// Whether metrics production is enabled for tracing backends.
    #[serde(rename = "TurnOnLogMetrics", default = "default_turn_on_log_metrics")]
    pub turn_on_log_metrics: bool,
    /// Optional node name carried in trace objects and metrics labels.
    #[serde(rename = "TraceOptionNodeName", default, skip_serializing_if = "Option::is_none")]
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
}

fn default_slots_per_kes_period() -> u64 {
    129_600
}

fn default_max_kes_evolutions() -> u64 {
    62
}

fn default_epoch_length() -> u64 {
    432_000
}

fn default_security_param_k() -> u64 {
    2160
}

fn default_active_slot_coeff() -> f64 {
    0.05
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
            other => Err(format!("unknown network: {other} (expected mainnet, preprod, or preview)")),
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
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/mainnet"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        network_magic: 764_824_073,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-mainnet".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
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
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/preprod"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        network_magic: 1,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-preprod".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
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
        bootstrap_peers: topology.fallback_peers,
        local_roots: topology.local_roots,
        public_roots: topology.public_roots,
        use_ledger_after_slot: topology.use_ledger_after_slot,
        peer_snapshot_file: topology.peer_snapshot_file,
        storage_dir: PathBuf::from("data/preview"),
        checkpoint_interval_slots: default_checkpoint_interval_slots(),
        max_ledger_snapshots: default_max_ledger_snapshots(),
        network_magic: 2,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 86_400,
        security_param_k: 432,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
        turn_on_logging: default_turn_on_logging(),
        use_trace_dispatcher: default_use_trace_dispatcher(),
        turn_on_log_metrics: default_turn_on_log_metrics(),
        trace_option_node_name: Some("yggdrasil-preview".to_owned()),
        trace_option_metrics_prefix: default_trace_option_metrics_prefix(),
        trace_option_resource_frequency: default_trace_option_resource_frequency(),
        trace_option_forwarder: default_trace_option_forwarder(),
        trace_options: default_trace_options(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(parsed.checkpoint_interval_slots, cfg.checkpoint_interval_slots);
        assert_eq!(parsed.max_ledger_snapshots, cfg.max_ledger_snapshots);
        assert_eq!(parsed.turn_on_logging, cfg.turn_on_logging);
        assert_eq!(parsed.use_trace_dispatcher, cfg.use_trace_dispatcher);
        assert_eq!(parsed.trace_option_node_name, cfg.trace_option_node_name);
        assert_eq!(parsed.trace_options, cfg.trace_options);
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
        assert!(cfg.turn_on_logging);
        assert!(cfg.use_trace_dispatcher);
        assert!(cfg.turn_on_log_metrics);
        assert!(cfg.trace_option_node_name.is_none());
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
        assert_eq!(cfg.trace_option_node_name.as_deref(), Some("yggdrasil-local"));
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
        let stability_window =
            (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
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
        assert_eq!(cfg.peer_snapshot_file.as_deref(), Some("peer-snapshot.json"));
        assert_eq!(cfg.storage_dir, PathBuf::from("data/mainnet"));
        assert_eq!(cfg.checkpoint_interval_slots, 2160);
        assert_eq!(cfg.max_ledger_snapshots, 8);
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= 3);
    }

    #[test]
    fn preprod_preset_matches_genesis() {
        let cfg = NetworkPreset::Preprod.to_config();
        assert_eq!(cfg.network_magic, 1);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.use_ledger_after_slot, Some(112_406_400));
        assert_eq!(cfg.peer_snapshot_file.as_deref(), Some("peer-snapshot.json"));
        assert_eq!(cfg.storage_dir, PathBuf::from("data/preprod"));
        assert_eq!(cfg.checkpoint_interval_slots, 2160);
        assert_eq!(cfg.max_ledger_snapshots, 8);
        assert!(cfg.bootstrap_peers.is_empty());
    }

    #[test]
    fn preview_preset_matches_genesis() {
        let cfg = NetworkPreset::Preview.to_config();
        assert_eq!(cfg.network_magic, 2);
        assert_eq!(cfg.epoch_length, 86_400);
        assert_eq!(cfg.security_param_k, 432);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.use_ledger_after_slot, Some(102_729_600));
        assert_eq!(cfg.peer_snapshot_file.as_deref(), Some("peer-snapshot.json"));
        assert_eq!(cfg.storage_dir, PathBuf::from("data/preview"));
        // stability_window = 3*432/0.05 = 25920
        let stability_window =
            (3.0 * cfg.security_param_k as f64 / cfg.active_slot_coeff) as u64;
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
        let peers = parse_topology_bootstrap_peers(
            include_str!("../configuration/mainnet/topology.json"),
        );
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

        assert_eq!(topology.primary_peer, "127.0.0.10:3001".parse().expect("addr"));
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
    fn network_preset_from_str() {
        assert_eq!("mainnet".parse::<NetworkPreset>().expect("mainnet"), NetworkPreset::Mainnet);
        assert_eq!("Preprod".parse::<NetworkPreset>().expect("preprod"), NetworkPreset::Preprod);
        assert_eq!("PREVIEW".parse::<NetworkPreset>().expect("preview"), NetworkPreset::Preview);
        assert!("unknown".parse::<NetworkPreset>().is_err());
    }

    #[test]
    fn network_preset_display_round_trips() {
        for preset in [NetworkPreset::Mainnet, NetworkPreset::Preprod, NetworkPreset::Preview] {
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
    }
}
