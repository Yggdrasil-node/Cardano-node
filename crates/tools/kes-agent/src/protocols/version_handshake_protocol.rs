//! Protocol for negotiating KES-agent protocol versions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/VersionHandshake/Protocol.hs.
//!
//! Direct data-level mirror of upstream
//! `Cardano.KESAgent.Protocols.VersionHandshake.Protocol`.
//! Socket driver I/O remains deferred; this module pins states,
//! messages, state tokens, and the version identifier used by that
//! future driver.

use super::types::{VersionIdentifier, mk_version_identifier};

/// Protocol state kind. Mirrors upstream `VersionHandshakeProtocol`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum VersionHandshakeProtocol {
    /// `InitialState`.
    InitialState,
    /// `VersionsOfferedState`.
    VersionsOfferedState,
    /// `EndState`.
    EndState,
}

/// Messages in the version-handshake protocol. Mirrors upstream
/// `Message VersionHandshakeProtocol`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum VersionHandshakeMessage {
    /// `VersionOfferMessage`.
    VersionOfferMessage(Vec<VersionIdentifier>),
    /// `VersionAcceptMessage`.
    VersionAcceptMessage(VersionIdentifier),
    /// `VersionRejectedMessage`.
    VersionRejectedMessage,
}

/// Singleton state tokens used by the upstream typed protocol.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SVersionHandshakeProtocol {
    /// `SInitialState`.
    SInitialState,
    /// `SVersionsOfferedState`.
    SVersionsOfferedState,
    /// `SEndState`.
    SEndState,
}

/// Text tag passed to upstream `mkVersionIdentifier`.
pub const VP_VERSION_IDENTIFIER_TEXT: &str = "VersionHandshake:0.1";

/// Idiomatic Rust casing for upstream `vpVersionIdentifier`.
pub fn vp_version_identifier() -> VersionIdentifier {
    mk_version_identifier(VP_VERSION_IDENTIFIER_TEXT)
}

impl VersionHandshakeProtocol {
    /// State token mirror for upstream `StateTokenI` instances.
    pub const fn state_token(self) -> SVersionHandshakeProtocol {
        match self {
            Self::InitialState => SVersionHandshakeProtocol::SInitialState,
            Self::VersionsOfferedState => SVersionHandshakeProtocol::SVersionsOfferedState,
            Self::EndState => SVersionHandshakeProtocol::SEndState,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vp_version_identifier_matches_upstream_text() {
        assert_eq!(VP_VERSION_IDENTIFIER_TEXT, "VersionHandshake:0.1");
        assert_eq!(vp_version_identifier().as_str(), "VersionHandshake:0.1");
    }

    #[test]
    fn state_tokens_match_upstream_state_names() {
        assert_eq!(
            VersionHandshakeProtocol::InitialState.state_token(),
            SVersionHandshakeProtocol::SInitialState
        );
        assert_eq!(
            VersionHandshakeProtocol::VersionsOfferedState.state_token(),
            SVersionHandshakeProtocol::SVersionsOfferedState
        );
        assert_eq!(
            VersionHandshakeProtocol::EndState.state_token(),
            SVersionHandshakeProtocol::SEndState
        );
    }

    #[test]
    fn message_constructors_preserve_payloads() {
        let v = vp_version_identifier();
        assert_eq!(
            VersionHandshakeMessage::VersionOfferMessage(vec![v.clone()]),
            VersionHandshakeMessage::VersionOfferMessage(vec![v.clone()])
        );
        assert_eq!(
            VersionHandshakeMessage::VersionAcceptMessage(v.clone()),
            VersionHandshakeMessage::VersionAcceptMessage(v)
        );
        assert_eq!(
            VersionHandshakeMessage::VersionRejectedMessage,
            VersionHandshakeMessage::VersionRejectedMessage
        );
    }
}
