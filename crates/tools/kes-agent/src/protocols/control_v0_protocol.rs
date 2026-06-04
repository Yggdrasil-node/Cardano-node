//! Version 0 control protocol for KES-agent control commands.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Control/V0/Protocol.hs.
//!
//! Direct data-level mirror of upstream
//! `Cardano.KESAgent.Protocols.Control.V0.Protocol`. Concrete KES
//! verification keys, operational certificates, and raw socket codecs
//! remain deferred to the daemon/socket follow-on.

use super::recv_result::RecvResult;
use super::types::{VersionIdentifier, mk_version_identifier};

/// Upstream crypto name used by the currently shipped `StandardCrypto`
/// control driver registration.
pub const STANDARD_CRYPTO_NAME: &str = "StandardCrypto";

/// KES period number. Mirrors upstream `KESPeriod` usage.
pub type KESPeriod = u64;

/// Opaque placeholder for upstream `VerKeyKES (KES c)`.
pub type VerKeyKES = Vec<u8>;

/// Opaque placeholder for upstream `SignedDSIGN ... OCertSignable`.
pub type SignedDSIGN = Vec<u8>;

/// Opaque placeholder for upstream `OCert c`.
pub type OCert = Vec<u8>;

/// UTC timestamp rendered in upstream textual form for the pure
/// vocabulary layer.
pub type UTCTime = String;

/// Agent state information returned by Control V0 `InfoMessage`.
/// Mirrors upstream `AgentInfo c`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AgentInfo {
    /// `agentInfoCurrentBundle`.
    pub agent_info_current_bundle: Option<BundleInfo>,
    /// `agentInfoStagedKey`.
    pub agent_info_staged_key: Option<KeyInfo>,
    /// `agentInfoCurrentTime`.
    pub agent_info_current_time: UTCTime,
    /// `agentInfoCurrentKESPeriod`.
    pub agent_info_current_kes_period: KESPeriod,
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

/// Information about an installed key bundle. Mirrors upstream
/// `BundleInfo c`.
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
/// `KeyInfo c`.
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
    /// `WaitForInfoState`.
    WaitForInfoState,
    /// `WaitForConfirmationState`.
    WaitForConfirmationState,
    /// `EndState`.
    EndState,
}

/// Messages in Control V0. Mirrors upstream
/// `Message (ControlProtocol m c)`.
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
    /// `SWaitForConfirmationState`.
    SWaitForConfirmationState,
    /// `SWaitForPublicKeyState`.
    SWaitForPublicKeyState,
    /// `SWaitForInfoState`.
    SWaitForInfoState,
    /// `SEndState`.
    SEndState,
}

/// Prefix passed to upstream `mkVersionIdentifier`.
pub const CP_VERSION_IDENTIFIER_PREFIX: &str = "Control:";

/// Suffix passed to upstream `mkVersionIdentifier`.
pub const CP_VERSION_IDENTIFIER_SUFFIX: &str = ":0.5";

/// Version identifier text for an upstream Control V0 crypto name.
pub fn cp_version_identifier_text(crypto_name: &str) -> String {
    format!("{CP_VERSION_IDENTIFIER_PREFIX}{crypto_name}{CP_VERSION_IDENTIFIER_SUFFIX}")
}

/// Version identifier for upstream Control V0 `ControlProtocol`.
pub fn cp_version_identifier(crypto_name: &str) -> VersionIdentifier {
    mk_version_identifier(cp_version_identifier_text(crypto_name))
}

/// Version identifier for the current upstream `StandardCrypto`
/// registration.
pub fn standard_crypto_cp_version_identifier() -> VersionIdentifier {
    cp_version_identifier(STANDARD_CRYPTO_NAME)
}

impl ControlProtocol {
    /// State token mirror for upstream `StateTokenI` instances.
    pub const fn state_token(self) -> SControlProtocol {
        match self {
            Self::InitialState => SControlProtocol::SInitialState,
            Self::IdleState => SControlProtocol::SIdleState,
            Self::WaitForConfirmationState => SControlProtocol::SWaitForConfirmationState,
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
    fn control_v0_version_identifier_matches_upstream_formula() {
        assert_eq!(CP_VERSION_IDENTIFIER_PREFIX, "Control:");
        assert_eq!(CP_VERSION_IDENTIFIER_SUFFIX, ":0.5");
        assert_eq!(
            cp_version_identifier_text("StandardCrypto"),
            "Control:StandardCrypto:0.5"
        );
        assert_eq!(
            standard_crypto_cp_version_identifier().as_str(),
            "Control:StandardCrypto:0.5"
        );
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
            ControlProtocol::WaitForConfirmationState.state_token(),
            SControlProtocol::SWaitForConfirmationState
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
    fn agent_info_record_preserves_control_v0_required_fields() {
        let info = AgentInfo {
            agent_info_current_bundle: Some(BundleInfo {
                bundle_info_evolution: 4,
                bundle_info_start_kes_period: 10,
                bundle_info_ocert_n: 99,
                bundle_info_vk: vec![1, 2, 3],
                bundle_info_sigma: vec![4, 5, 6],
            }),
            agent_info_staged_key: Some(KeyInfo {
                key_info_vk: vec![7, 8],
            }),
            agent_info_current_time: "2026-06-04T00:00:01Z".to_string(),
            agent_info_current_kes_period: 11,
            agent_info_bootstrap_connections: vec![BootstrapInfo {
                bootstrap_info_address: "/tmp/peer.socket".to_string(),
                bootstrap_info_status: ConnectionStatus::ConnectionConnecting,
            }],
        };

        assert_eq!(info.agent_info_current_kes_period, 11);
        let bundle = info
            .agent_info_current_bundle
            .as_ref()
            .expect("Control V0 current bundle fixture should exist");
        assert_eq!(bundle.bundle_info_ocert_n, 99);
    }
}
