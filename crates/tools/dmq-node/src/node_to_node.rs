//! DMQ node-to-node (NtN) protocol surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `DMQ/NodeToNode/` directory, plus the self-contained constants of
//! `DMQ/NodeToNode.hs` (the mux mini-protocol numbers). The `ntnApps`
//! / `Apps` mux-application wiring of `DMQ/NodeToNode.hs` is runtime
//! integration that lands with the `run()` loop.

pub mod version;

use yggdrasil_network::MiniProtocolNum;

/// The mux mini-protocol number for `SigSubmission` (node-to-node).
///
/// Mirror of upstream `sigSubmissionMiniProtocolNum = MiniProtocolNum 11`.
pub const SIG_SUBMISSION_MINI_PROTOCOL_NUM: MiniProtocolNum = MiniProtocolNum(11);

/// The mux mini-protocol number for `KeepAlive` (node-to-node).
///
/// Mirror of upstream `keepAliveMiniProtocolNum = MiniProtocolNum 12`.
pub const KEEP_ALIVE_MINI_PROTOCOL_NUM: MiniProtocolNum = MiniProtocolNum(12);

/// The mux mini-protocol number for `PeerSharing` (node-to-node).
///
/// Mirror of upstream `peerSharingMiniProtocolNum = MiniProtocolNum 13`.
pub const PEER_SHARING_MINI_PROTOCOL_NUM: MiniProtocolNum = MiniProtocolNum(13);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntn_mini_protocol_numbers_match_upstream() {
        assert_eq!(SIG_SUBMISSION_MINI_PROTOCOL_NUM, MiniProtocolNum(11));
        assert_eq!(KEEP_ALIVE_MINI_PROTOCOL_NUM, MiniProtocolNum(12));
        assert_eq!(PEER_SHARING_MINI_PROTOCOL_NUM, MiniProtocolNum(13));
    }
}
