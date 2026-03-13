//! Node configuration file types.
//!
//! The configuration format follows the same JSON convention used by the
//! official Cardano node runtime. A config file is a JSON object with
//! peer address, network magic, protocol versions, and consensus parameters.
//!
//! Reference: `cardano-node/configuration/` in the IntersectMBO repository.

use std::net::{SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// On-disk node configuration parsed from a JSON file.
///
/// CLI flags can override individual fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfigFile {
    /// Peer address in `host:port` form.
    pub peer_addr: SocketAddr,
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
    NodeConfigFile {
        peer_addr: "3.125.94.58:3001".parse().expect("valid default addr"),
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
    NodeConfigFile {
        peer_addr: "preprod-node.play.dev.cardano.org:3001"
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .unwrap_or_else(|| "3.125.94.58:3001".parse().expect("fallback addr")),
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
    NodeConfigFile {
        peer_addr: "preview-node.play.dev.cardano.org:3001"
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .unwrap_or_else(|| "3.125.94.58:3001".parse().expect("fallback addr")),
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
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let json = r#"{
            "peer_addr": "127.0.0.1:3001",
            "network_magic": 42,
            "protocol_versions": [13]
        }"#;
        let cfg: NodeConfigFile = serde_json::from_str(json).expect("parse");
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
        assert_eq!(cfg.network_magic, 764_824_073);
        assert_eq!(cfg.epoch_length, 432_000);
        assert_eq!(cfg.security_param_k, 2160);
        assert!((cfg.active_slot_coeff - 0.05).abs() < f64::EPSILON);
        assert_eq!(cfg.slots_per_kes_period, 129_600);
        assert_eq!(cfg.max_kes_evolutions, 62);
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
