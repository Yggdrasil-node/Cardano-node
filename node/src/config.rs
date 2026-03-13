//! Node configuration file types.
//!
//! The configuration format follows the same JSON convention used by the
//! official Cardano node runtime. A config file is a JSON object with
//! a primary peer address, optional ordered bootstrap relays, network magic,
//! protocol versions, and consensus parameters.
//!
//! Reference: `cardano-node/configuration/` in the IntersectMBO repository.

use std::net::{SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TopologyConfigFile {
    #[serde(default)]
    bootstrap_peers: Vec<TopologyBootstrapPeer>,
}

#[derive(Debug, Deserialize)]
struct TopologyBootstrapPeer {
    address: String,
    port: u16,
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

fn resolve_bootstrap_peer(host: &str, port: u16) -> Option<SocketAddr> {
    format!("{host}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
}

fn resolve_bootstrap_peers(
    entries: &[(&str, u16)],
    fallback_primary: SocketAddr,
) -> (SocketAddr, Vec<SocketAddr>) {
    let mut resolved = Vec::new();
    for (host, port) in entries {
        if let Some(addr) = resolve_bootstrap_peer(host, *port) {
            if !resolved.contains(&addr) {
                resolved.push(addr);
            }
        }
    }

    if resolved.is_empty() {
        resolved.push(fallback_primary);
    }

    let primary = resolved[0];
    let fallbacks = resolved.into_iter().skip(1).collect();
    (primary, fallbacks)
}

fn parse_topology_bootstrap_peers(topology_json: &str) -> Vec<(String, u16)> {
    serde_json::from_str::<TopologyConfigFile>(topology_json)
        .map(|cfg| {
            cfg.bootstrap_peers
                .into_iter()
                .map(|peer| (peer.address, peer.port))
                .collect()
        })
        .unwrap_or_default()
}

fn resolve_bootstrap_peers_from_topology(
    topology_json: &str,
    fallback_primary: SocketAddr,
) -> (SocketAddr, Vec<SocketAddr>) {
    let entries = parse_topology_bootstrap_peers(topology_json);
    let entry_refs: Vec<(&str, u16)> = entries.iter().map(|(host, port)| (host.as_str(), *port)).collect();
    resolve_bootstrap_peers(&entry_refs, fallback_primary)
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
    let (peer_addr, bootstrap_peers) = resolve_bootstrap_peers_from_topology(
        include_str!("../configuration/mainnet/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr,
        bootstrap_peers,
        network_magic: 764_824_073,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
    }
}

/// Pre-production testnet configuration.
///
/// Genesis source: `shelley-genesis.json` from
/// <https://book.world.dev.cardano.org/environments/preprod/>.
pub fn preprod_config() -> NodeConfigFile {
    let fallback_primary = "3.125.94.58:3001".parse().expect("fallback addr");
    let (peer_addr, bootstrap_peers) = resolve_bootstrap_peers_from_topology(
        include_str!("../configuration/preprod/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr,
        bootstrap_peers,
        network_magic: 1,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 432_000,
        security_param_k: 2160,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
    }
}

/// Preview testnet configuration.
///
/// Genesis source: `shelley-genesis.json` from
/// <https://book.world.dev.cardano.org/environments/preview/>.
pub fn preview_config() -> NodeConfigFile {
    let fallback_primary = "3.125.94.58:3001".parse().expect("fallback addr");
    let (peer_addr, bootstrap_peers) = resolve_bootstrap_peers_from_topology(
        include_str!("../configuration/preview/topology.json"),
        fallback_primary,
    );

    NodeConfigFile {
        peer_addr,
        bootstrap_peers,
        network_magic: 2,
        protocol_versions: vec![13, 14],
        slots_per_kes_period: 129_600,
        max_kes_evolutions: 62,
        epoch_length: 86_400,
        security_param_k: 432,
        active_slot_coeff: 0.05,
        keepalive_interval_secs: Some(60),
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
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert!(cfg.keepalive_interval_secs.is_none());
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
        let (peer_addr, bootstrap_peers) =
            resolve_bootstrap_peers_from_topology("{\"bootstrapPeers\":[]}", fallback);
        assert_eq!(peer_addr, fallback);
        assert!(bootstrap_peers.is_empty());
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
