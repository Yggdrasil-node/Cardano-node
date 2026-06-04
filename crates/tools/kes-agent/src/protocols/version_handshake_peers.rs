//! Pure peers for version negotiation.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/VersionHandshake/Peers.hs.
//!
//! Mirrors the pure decision logic in upstream
//! `versionHandshakeClient` and `versionHandshakeServer` without
//! wiring the typed-protocol runtime.

use super::types::VersionIdentifier;
use super::version_handshake_protocol::VersionHandshakeMessage;

/// Client-side version choice. Mirrors upstream `versionHandshakeClient`.
pub fn version_handshake_client(
    acceptable_versions: &[VersionIdentifier],
    available_versions: &[VersionIdentifier],
) -> Option<VersionIdentifier> {
    acceptable_versions
        .iter()
        .find(|acceptable| {
            available_versions
                .iter()
                .any(|available| available == *acceptable)
        })
        .cloned()
}

/// Client response message for the server's offered versions.
pub fn version_handshake_client_response(
    acceptable_versions: &[VersionIdentifier],
    available_versions: &[VersionIdentifier],
) -> VersionHandshakeMessage {
    match version_handshake_client(acceptable_versions, available_versions) {
        Some(version) => VersionHandshakeMessage::VersionAcceptMessage(version),
        None => VersionHandshakeMessage::VersionRejectedMessage,
    }
}

/// Server-side initial offer. Mirrors upstream `versionHandshakeServer`.
pub fn version_handshake_server(
    available_versions: Vec<VersionIdentifier>,
) -> VersionHandshakeMessage {
    VersionHandshakeMessage::VersionOfferMessage(available_versions)
}

/// Server result after the client accepts or rejects the offered list.
pub fn version_handshake_server_result(
    response: &VersionHandshakeMessage,
) -> Option<VersionIdentifier> {
    match response {
        VersionHandshakeMessage::VersionAcceptMessage(version) => Some(version.clone()),
        VersionHandshakeMessage::VersionRejectedMessage => None,
        VersionHandshakeMessage::VersionOfferMessage(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::types::mk_version_identifier;

    #[test]
    fn client_picks_first_acceptable_common_version() {
        let acceptable = [
            mk_version_identifier("Control:2.0"),
            mk_version_identifier("Control:3.0"),
        ];
        let available = [
            mk_version_identifier("Control:3.0"),
            mk_version_identifier("Control:2.0"),
        ];

        assert_eq!(
            version_handshake_client(&acceptable, &available),
            Some(mk_version_identifier("Control:2.0"))
        );
    }

    #[test]
    fn client_rejects_when_no_common_version_exists() {
        let acceptable = [mk_version_identifier("Control:2.0")];
        let available = [mk_version_identifier("Service:2.0")];

        assert_eq!(version_handshake_client(&acceptable, &available), None);
        assert_eq!(
            version_handshake_client_response(&acceptable, &available),
            VersionHandshakeMessage::VersionRejectedMessage
        );
    }

    #[test]
    fn server_offer_and_result_match_upstream_peer_shape() {
        let offered = vec![mk_version_identifier("VersionHandshake:0.1")];
        assert_eq!(
            version_handshake_server(offered.clone()),
            VersionHandshakeMessage::VersionOfferMessage(offered.clone())
        );
        assert_eq!(
            version_handshake_server_result(&VersionHandshakeMessage::VersionAcceptMessage(
                offered[0].clone()
            )),
            Some(offered[0].clone())
        );
        assert_eq!(
            version_handshake_server_result(&VersionHandshakeMessage::VersionRejectedMessage),
            None
        );
    }
}
