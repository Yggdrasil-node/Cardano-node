//! DMQ node-to-client (NtC) protocol surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `DMQ/NodeToClient/` directory, plus the self-contained constants of
//! `DMQ/NodeToClient.hs` (the mux mini-protocol numbers and the
//! reply-batch cap). The `ntcApps` / `Apps` mux-application wiring of
//! `DMQ/NodeToClient.hs` is runtime integration that lands with the
//! `run()` loop.

pub mod version;

use yggdrasil_network::MiniProtocolNum;

/// The mux mini-protocol number for `LocalMsgSubmission` (node-to-client).
///
/// Mirror of upstream `DMQ/NodeToClient.hs`'s `localMsgSubmission`
/// `miniProtocolNum = MiniProtocolNum 14`.
pub const LOCAL_MSG_SUBMISSION_MINI_PROTOCOL_NUM: MiniProtocolNum = MiniProtocolNum(14);

/// The mux mini-protocol number for `LocalMsgNotification` (node-to-client).
///
/// Mirror of upstream `DMQ/NodeToClient.hs`'s `localMsgNotification`
/// `miniProtocolNum = MiniProtocolNum 15`.
pub const LOCAL_MSG_NOTIFICATION_MINI_PROTOCOL_NUM: MiniProtocolNum = MiniProtocolNum(15);

/// The maximum number of messages `LocalMsgNotification` provides in a
/// single reply.
///
/// Mirror of upstream `DMQ/NodeToClient.hs`'s `_ntc_MAX_SIGS_TO_ACK`.
pub const NTC_MAX_SIGS_TO_ACK: u16 = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntc_mini_protocol_numbers_match_upstream() {
        assert_eq!(LOCAL_MSG_SUBMISSION_MINI_PROTOCOL_NUM, MiniProtocolNum(14));
        assert_eq!(
            LOCAL_MSG_NOTIFICATION_MINI_PROTOCOL_NUM,
            MiniProtocolNum(15)
        );
    }

    #[test]
    fn ntc_max_sigs_to_ack_matches_upstream() {
        assert_eq!(NTC_MAX_SIGS_TO_ACK, 1000);
    }
}
