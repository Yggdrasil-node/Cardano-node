//! Version 2 service protocol for pushing KES keys.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V2/Protocol.hs.
//!
//! Direct data-level mirror of upstream
//! `Cardano.KESAgent.Protocols.Service.V2.Protocol`. Concrete
//! KES bundle codecs and raw socket driver I/O remain deferred to the
//! daemon/socket follow-on.

use super::recv_result::RecvResult;
use super::types::{Timestamp, VersionIdentifier, mk_version_identifier};

/// Opaque placeholder for upstream `Bundle m StandardCrypto`.
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
/// `Message (ServiceProtocol m)`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ServiceMessage {
    /// `VersionMessage`.
    VersionMessage,
    /// `KeyMessage`.
    KeyMessage(ServiceBundle, Timestamp),
    /// `DropKeyMessage`.
    DropKeyMessage(Timestamp),
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

/// Text tag passed to upstream `mkVersionIdentifier`.
pub const SERVICE_V2_VERSION_IDENTIFIER_TEXT: &str = "Service:2.0";

/// Version identifier for upstream `ServiceProtocol` V2.
pub fn service_v2_version_identifier() -> VersionIdentifier {
    mk_version_identifier(SERVICE_V2_VERSION_IDENTIFIER_TEXT)
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
    fn service_v2_version_identifier_matches_upstream_text() {
        assert_eq!(SERVICE_V2_VERSION_IDENTIFIER_TEXT, "Service:2.0");
        assert_eq!(service_v2_version_identifier().as_str(), "Service:2.0");
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
            ServiceMessage::KeyMessage(vec![1, 2, 3], 42),
            ServiceMessage::KeyMessage(vec![1, 2, 3], 42)
        );
        assert_eq!(
            ServiceMessage::DropKeyMessage(99),
            ServiceMessage::DropKeyMessage(99)
        );
        assert_eq!(
            ServiceMessage::RecvResultMessage(RecvResult::RecvOK),
            ServiceMessage::RecvResultMessage(RecvResult::RecvOK)
        );
    }
}
