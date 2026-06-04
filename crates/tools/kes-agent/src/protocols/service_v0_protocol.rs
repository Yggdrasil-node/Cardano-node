//! Version 0 service protocol for pushing KES keys.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V0/Protocol.hs.
//!
//! Direct data-level mirror of upstream
//! `Cardano.KESAgent.Protocols.Service.V0.Protocol`. Concrete
//! crypto-specific KES bundle codecs and raw socket driver I/O remain
//! deferred to the daemon/socket follow-on.

use super::recv_result::RecvResult;
use super::types::{VersionIdentifier, mk_version_identifier};

/// Upstream crypto name used by the currently shipped `StandardCrypto`
/// service-driver registration.
pub const STANDARD_CRYPTO_NAME: &str = "StandardCrypto";

/// Opaque placeholder for upstream `Bundle m c`.
pub type ServiceBundle = Vec<u8>;

/// Protocol state kind. Mirrors upstream `ServiceProtocol`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ServiceProtocol {
    /// `InitialState`.
    InitialState,
    /// `IdleState`.
    IdleState,
    /// `WaitForConfirmationState`.
    WaitForConfirmationState,
    /// `EndState`.
    EndState,
}

/// Messages in the service protocol. Mirrors upstream
/// `Message (ServiceProtocol m c)`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ServiceMessage {
    /// `VersionMessage`.
    VersionMessage,
    /// `KeyMessage`.
    KeyMessage(ServiceBundle),
    /// `RecvResultMessage`.
    RecvResultMessage(RecvResult),
    /// `AbortMessage`.
    AbortMessage,
    /// `ServerDisconnectMessage`.
    ServerDisconnectMessage,
    /// `ClientDisconnectMessage`.
    ClientDisconnectMessage,
    /// `ProtocolErrorMessage`.
    ProtocolErrorMessage,
}

/// Singleton state tokens used by the upstream typed protocol.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SServiceProtocol {
    /// `SInitialState`.
    SInitialState,
    /// `SIdleState`.
    SIdleState,
    /// `SWaitForConfirmationState`.
    SWaitForConfirmationState,
    /// `SEndState`.
    SEndState,
}

/// Prefix passed to upstream `mkVersionIdentifier`.
pub const SP_VERSION_IDENTIFIER_PREFIX: &str = "Service:";

/// Suffix passed to upstream `mkVersionIdentifier`.
pub const SP_VERSION_IDENTIFIER_SUFFIX: &str = ":0.4";

/// Version identifier text for an upstream Service V0 crypto name.
pub fn sp_version_identifier_text(crypto_name: &str) -> String {
    format!("{SP_VERSION_IDENTIFIER_PREFIX}{crypto_name}{SP_VERSION_IDENTIFIER_SUFFIX}")
}

/// Version identifier for upstream Service V0 `ServiceProtocol`.
pub fn sp_version_identifier(crypto_name: &str) -> VersionIdentifier {
    mk_version_identifier(sp_version_identifier_text(crypto_name))
}

/// Version identifier for the current upstream `StandardCrypto`
/// registration.
pub fn standard_crypto_sp_version_identifier() -> VersionIdentifier {
    sp_version_identifier(STANDARD_CRYPTO_NAME)
}

impl ServiceProtocol {
    /// State token mirror for upstream `StateTokenI` instances.
    pub const fn state_token(self) -> SServiceProtocol {
        match self {
            Self::InitialState => SServiceProtocol::SInitialState,
            Self::IdleState => SServiceProtocol::SIdleState,
            Self::WaitForConfirmationState => SServiceProtocol::SWaitForConfirmationState,
            Self::EndState => SServiceProtocol::SEndState,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_v0_version_identifier_matches_upstream_formula() {
        assert_eq!(
            sp_version_identifier_text("StandardCrypto"),
            "Service:StandardCrypto:0.4"
        );
        assert_eq!(
            standard_crypto_sp_version_identifier().as_str(),
            "Service:StandardCrypto:0.4"
        );
    }

    #[test]
    fn service_state_tokens_match_upstream_state_names() {
        assert_eq!(
            ServiceProtocol::InitialState.state_token(),
            SServiceProtocol::SInitialState
        );
        assert_eq!(
            ServiceProtocol::IdleState.state_token(),
            SServiceProtocol::SIdleState
        );
        assert_eq!(
            ServiceProtocol::WaitForConfirmationState.state_token(),
            SServiceProtocol::SWaitForConfirmationState
        );
        assert_eq!(
            ServiceProtocol::EndState.state_token(),
            SServiceProtocol::SEndState
        );
    }

    #[test]
    fn service_message_constructors_preserve_payloads() {
        assert_eq!(
            ServiceMessage::KeyMessage(vec![1, 2, 3]),
            ServiceMessage::KeyMessage(vec![1, 2, 3])
        );
        assert_eq!(
            ServiceMessage::RecvResultMessage(RecvResult::RecvOK),
            ServiceMessage::RecvResultMessage(RecvResult::RecvOK)
        );
    }
}
