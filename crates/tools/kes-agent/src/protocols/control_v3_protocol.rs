//! Version 3 control protocol for KES-agent control commands.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Control/V3/Protocol.hs.
//!
//! Direct data-level mirror of upstream
//! `Cardano.KESAgent.Protocols.Control.V3.Protocol`. Concrete KES
//! verification keys, operational certificates, and raw socket codecs
//! remain deferred to the daemon/socket follow-on.

use super::recv_result::RecvResult;
use super::types::{VersionIdentifier, mk_version_identifier};

/// KES period number. Mirrors upstream `KESPeriod` usage.
pub type KESPeriod = u64;

/// Opaque placeholder for upstream `VerKeyKES (KES StandardCrypto)`.
pub type VerKeyKES = Vec<u8>;

/// Opaque placeholder for upstream `SignedDSIGN ... OCertSignable`.
pub type SignedDSIGN = Vec<u8>;

/// Opaque placeholder for upstream `OCert StandardCrypto`.
pub type OCert = Vec<u8>;

/// UTC timestamp rendered in upstream textual form for the pure
/// vocabulary layer.
pub type UTCTime = String;

/// Agent state information returned by Control V3 `InfoMessage`.
/// Mirrors upstream `AgentInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AgentInfo {
    /// `agentInfoProgramVersion`.
    pub agent_info_program_version: String,
    /// `agentInfoCurrentBundle`.
    pub agent_info_current_bundle: Option<TaggedBundleInfo>,
    /// `agentInfoStagedKey`.
    pub agent_info_staged_key: Option<KeyInfo>,
    /// `agentInfoCurrentTime`.
    pub agent_info_current_time: UTCTime,
    /// `agentInfoCurrentKESPeriod`.
    pub agent_info_current_kes_period: KESPeriod,
    /// `agentInfoCurrentKESPeriodStart`.
    pub agent_info_current_kes_period_start: UTCTime,
    /// `agentInfoCurrentKESPeriodEnd`.
    pub agent_info_current_kes_period_end: UTCTime,
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

    /// Upstream enum ordinal used by `encodeEnum`.
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

/// Information about a timestamped installed key bundle. Mirrors
/// upstream `TaggedBundleInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TaggedBundleInfo {
    /// `taggedBundleInfo`.
    pub tagged_bundle_info: Option<BundleInfo>,
    /// `taggedBundleInfoTimestamp`.
    pub tagged_bundle_info_timestamp: Option<UTCTime>,
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
    pub bundle_info_vk: VerKeyKES,
    /// `bundleInfoSigma`.
    pub bundle_info_sigma: SignedDSIGN,
}

/// Information about a staged KES verification key. Mirrors upstream
/// `KeyInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyInfo {
    /// `keyInfoVK`.
    pub key_info_vk: VerKeyKES,
}

/// Protocol state kind. Mirrors upstream `ControlProtocol`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ControlProtocol {
    /// `InitialState`.
    InitialState,
    /// `IdleState`.
    IdleState,
    /// `WaitForPublicKeyState`.
    WaitForPublicKeyState,
    /// `WaitForDropConfirmationState`.
    WaitForDropConfirmationState,
    /// `WaitForInfoState`.
    WaitForInfoState,
    /// `WaitForKeyConfirmationState`.
    WaitForKeyConfirmationState,
    /// `EndState`.
    EndState,
}

/// Messages in Control V3. Mirrors upstream
/// `Message (ControlProtocol m)`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlMessage {
    /// `VersionMessage`.
    VersionMessage,
    /// `GenStagedKeyMessage`.
    GenStagedKeyMessage,
    /// `QueryStagedKeyMessage`.
    QueryStagedKeyMessage,
    /// `DropStagedKeyMessage`.
    DropStagedKeyMessage,
    /// `PublicKeyMessage`.
    PublicKeyMessage(Option<VerKeyKES>),
    /// `InstallKeyMessage`.
    InstallKeyMessage(OCert),
    /// `InstallResultMessage`.
    InstallResultMessage(RecvResult),
    /// `DropKeyMessage`.
    DropKeyMessage,
    /// `DropKeyResultMessage`.
    DropKeyResultMessage(RecvResult),
    /// `RequestInfoMessage`.
    RequestInfoMessage,
    /// `InfoMessage`.
    InfoMessage(AgentInfo),
    /// `AbortMessage`.
    AbortMessage,
    /// `EndMessage`.
    EndMessage,
    /// `ProtocolErrorMessage`.
    ProtocolErrorMessage,
}

/// Singleton state tokens used by the upstream typed protocol.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SControlProtocol {
    /// `SInitialState`.
    SInitialState,
    /// `SIdleState`.
    SIdleState,
    /// `SWaitForKeyConfirmationState`.
    SWaitForKeyConfirmationState,
    /// `SWaitForDropConfirmationState`.
    SWaitForDropConfirmationState,
    /// `SWaitForPublicKeyState`.
    SWaitForPublicKeyState,
    /// `SWaitForInfoState`.
    SWaitForInfoState,
    /// `SEndState`.
    SEndState,
}

/// Text tag passed to upstream `mkVersionIdentifier`.
pub const CP_VERSION_IDENTIFIER_TEXT: &str = "Control:3.0";

/// Version identifier for upstream Control V3 `ControlProtocol`.
pub fn cp_version_identifier() -> VersionIdentifier {
    mk_version_identifier(CP_VERSION_IDENTIFIER_TEXT)
}

impl ControlProtocol {
    /// State token mirror for upstream `StateTokenI` instances.
    pub const fn state_token(self) -> SControlProtocol {
        match self {
            Self::InitialState => SControlProtocol::SInitialState,
            Self::IdleState => SControlProtocol::SIdleState,
            Self::WaitForKeyConfirmationState => SControlProtocol::SWaitForKeyConfirmationState,
            Self::WaitForDropConfirmationState => SControlProtocol::SWaitForDropConfirmationState,
            Self::WaitForPublicKeyState => SControlProtocol::SWaitForPublicKeyState,
            Self::WaitForInfoState => SControlProtocol::SWaitForInfoState,
            Self::EndState => SControlProtocol::SEndState,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_v3_version_identifier_matches_upstream_text() {
        assert_eq!(CP_VERSION_IDENTIFIER_TEXT, "Control:3.0");
        assert_eq!(cp_version_identifier().as_str(), "Control:3.0");
    }

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
    fn control_state_tokens_match_upstream_state_names() {
        assert_eq!(
            ControlProtocol::InitialState.state_token(),
            SControlProtocol::SInitialState
        );
        assert_eq!(
            ControlProtocol::IdleState.state_token(),
            SControlProtocol::SIdleState
        );
        assert_eq!(
            ControlProtocol::WaitForKeyConfirmationState.state_token(),
            SControlProtocol::SWaitForKeyConfirmationState
        );
        assert_eq!(
            ControlProtocol::WaitForDropConfirmationState.state_token(),
            SControlProtocol::SWaitForDropConfirmationState
        );
        assert_eq!(
            ControlProtocol::WaitForPublicKeyState.state_token(),
            SControlProtocol::SWaitForPublicKeyState
        );
        assert_eq!(
            ControlProtocol::WaitForInfoState.state_token(),
            SControlProtocol::SWaitForInfoState
        );
        assert_eq!(
            ControlProtocol::EndState.state_token(),
            SControlProtocol::SEndState
        );
    }

    #[test]
    fn control_messages_preserve_payload_shapes() {
        assert_eq!(
            ControlMessage::PublicKeyMessage(Some(vec![1, 2, 3])),
            ControlMessage::PublicKeyMessage(Some(vec![1, 2, 3]))
        );
        assert_eq!(
            ControlMessage::InstallKeyMessage(vec![4, 5]),
            ControlMessage::InstallKeyMessage(vec![4, 5])
        );
        assert_eq!(
            ControlMessage::InstallResultMessage(RecvResult::RecvOK),
            ControlMessage::InstallResultMessage(RecvResult::RecvOK)
        );
    }

    #[test]
    fn agent_info_record_preserves_control_v3_required_fields() {
        let info = AgentInfo {
            agent_info_program_version: "kes-agent-1.0".to_string(),
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
            agent_info_current_kes_period_start: "2026-06-04T00:00:00Z".to_string(),
            agent_info_current_kes_period_end: "2026-06-05T00:00:00Z".to_string(),
            agent_info_bootstrap_connections: vec![BootstrapInfo {
                bootstrap_info_address: "/tmp/peer.socket".to_string(),
                bootstrap_info_status: ConnectionStatus::ConnectionConnecting,
            }],
        };

        assert_eq!(info.agent_info_program_version, "kes-agent-1.0");
        assert_eq!(info.agent_info_current_kes_period, 11);
        assert_eq!(
            info.agent_info_bootstrap_connections[0].bootstrap_info_status,
            ConnectionStatus::ConnectionConnecting
        );
    }
}
