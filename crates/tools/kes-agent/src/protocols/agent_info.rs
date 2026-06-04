//! Agent state information displayed by control clients.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/AgentInfo.hs.
//!
//! This module mirrors the upstream information records with owned,
//! pure-Rust data. Cryptographic payloads are byte vectors until the
//! daemon/socket follow-on wires the concrete KES and DSIGN types.

/// KES period number. Mirrors upstream `KESPeriod` usage in
/// `AgentInfo`.
pub type KESPeriod = u64;

/// Agent state information. Mirrors upstream `AgentInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AgentInfo {
    /// `agentInfoProgramVersion`.
    pub agent_info_program_version: Option<String>,
    /// `agentInfoCurrentBundle`.
    pub agent_info_current_bundle: Option<TaggedBundleInfo>,
    /// `agentInfoStagedKey`.
    pub agent_info_staged_key: Option<KeyInfo>,
    /// `agentInfoCurrentTime`.
    pub agent_info_current_time: String,
    /// `agentInfoCurrentKESPeriod`.
    pub agent_info_current_kes_period: KESPeriod,
    /// `agentInfoCurrentKESPeriodStart`.
    pub agent_info_current_kes_period_start: Option<String>,
    /// `agentInfoCurrentKESPeriodEnd`.
    pub agent_info_current_kes_period_end: Option<String>,
    /// `agentInfoBootstrapConnections`.
    pub agent_info_bootstrap_connections: Vec<BootstrapInfo>,
}

/// Information about a bootstrapping connection. Mirrors upstream
/// `BootstrapInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BootstrapInfo {
    /// `bootstrapInfoAddress`.
    pub bootstrap_info_address: String,
    /// `bootstrapInfoStatus`.
    pub bootstrap_info_status: ConnectionStatus,
}

/// Information about an installed key bundle. Mirrors upstream
/// `BundleInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BundleInfo {
    /// `bundleInfoEvolution`.
    pub bundle_info_evolution: u32,
    /// `bundleInfoStartKESPeriod`.
    pub bundle_info_start_kes_period: KESPeriod,
    /// `bundleInfoOCertN`.
    pub bundle_info_ocert_n: u64,
    /// `bundleInfoVK`.
    pub bundle_info_vk: Vec<u8>,
    /// `bundleInfoSigma`.
    pub bundle_info_sigma: Vec<u8>,
}

/// Information about a timestamped bundle mutation. Mirrors upstream
/// `TaggedBundleInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TaggedBundleInfo {
    /// `taggedBundleInfo`.
    pub tagged_bundle_info: Option<BundleInfo>,
    /// `taggedBundleInfoTimestamp`.
    pub tagged_bundle_info_timestamp: Option<String>,
}

/// Information about a staged KES verification key. Mirrors upstream
/// `KeyInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyInfo {
    /// `keyInfoVK`.
    pub key_info_vk: Vec<u8>,
}

/// Status of a bootstrapping connection. Mirrors upstream
/// `ConnectionStatus`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[repr(u8)]
pub enum ConnectionStatus {
    /// `ConnectionUp`.
    ConnectionUp = 0,
    /// `ConnectionConnecting`.
    ConnectionConnecting = 1,
    /// `ConnectionDown`.
    ConnectionDown = 2,
}

impl ConnectionStatus {
    /// Discriminants in upstream declaration order.
    pub const ALL: [Self; 3] = [
        Self::ConnectionUp,
        Self::ConnectionConnecting,
        Self::ConnectionDown,
    ];

    /// Upstream enum ordinal.
    pub const fn ordinal(self) -> u8 {
        self as u8
    }

    /// Decode an upstream enum ordinal.
    pub const fn from_ordinal(ordinal: u8) -> Option<Self> {
        match ordinal {
            0 => Some(Self::ConnectionUp),
            1 => Some(Self::ConnectionConnecting),
            2 => Some(Self::ConnectionDown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_status_ordinals_match_upstream_declaration_order() {
        for (idx, status) in ConnectionStatus::ALL.iter().copied().enumerate() {
            let ordinal = idx as u8;
            assert_eq!(status.ordinal(), ordinal);
            assert_eq!(ConnectionStatus::from_ordinal(ordinal), Some(status));
        }
        assert_eq!(ConnectionStatus::from_ordinal(3), None);
    }

    #[test]
    fn agent_info_records_preserve_upstream_field_shapes() {
        let info = AgentInfo {
            agent_info_program_version: Some("kes-agent-1.0".to_string()),
            agent_info_current_bundle: Some(TaggedBundleInfo {
                tagged_bundle_info: Some(BundleInfo {
                    bundle_info_evolution: 4,
                    bundle_info_start_kes_period: 10,
                    bundle_info_ocert_n: 99,
                    bundle_info_vk: vec![1, 2, 3],
                    bundle_info_sigma: vec![4, 5, 6],
                }),
                tagged_bundle_info_timestamp: Some("2026-06-04T00:00:00Z".to_string()),
            }),
            agent_info_staged_key: Some(KeyInfo {
                key_info_vk: vec![7, 8],
            }),
            agent_info_current_time: "2026-06-04T00:00:01Z".to_string(),
            agent_info_current_kes_period: 11,
            agent_info_current_kes_period_start: Some("2026-06-04T00:00:00Z".to_string()),
            agent_info_current_kes_period_end: Some("2026-06-05T00:00:00Z".to_string()),
            agent_info_bootstrap_connections: vec![BootstrapInfo {
                bootstrap_info_address: "/tmp/peer.socket".to_string(),
                bootstrap_info_status: ConnectionStatus::ConnectionConnecting,
            }],
        };

        assert_eq!(info.agent_info_current_kes_period, 11);
        assert_eq!(
            info.agent_info_bootstrap_connections[0].bootstrap_info_status,
            ConnectionStatus::ConnectionConnecting
        );
        let Some(current_bundle) = info.agent_info_current_bundle.as_ref() else {
            panic!("test fixture should include a current bundle");
        };
        let Some(bundle) = current_bundle.tagged_bundle_info.as_ref() else {
            panic!("test fixture should include bundle information");
        };
        assert_eq!(bundle.bundle_info_ocert_n, 99);
    }
}
