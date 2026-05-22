//! DMQ node-to-client (NtC) protocol surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `DMQ/NodeToClient/` directory, plus the self-contained constants of
//! `DMQ/NodeToClient.hs` (the mux mini-protocol numbers and the
//! reply-batch cap) and the node-to-client mux mini-protocol bundle.
//! The runnable `ntcApps` / `Apps` closures of `DMQ/NodeToClient.hs`
//! land with the `run()` loop.

pub mod version;

use yggdrasil_network::{
    MiniProtocolDescriptor, MiniProtocolLimits, MiniProtocolNum, MiniProtocolStart,
    OuroborosBundle, ProtocolTemperature,
};

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

/// The DMQ node-to-client mux mini-protocol bundle.
///
/// Mirror of the DMQ NtC protocol assignment (`DMQ/NodeToClient.hs`):
/// the `LocalMsgSubmission` (14) and `LocalMsgNotification` (15)
/// mini-protocols. Node-to-client connections are responder-only —
/// every protocol is established-tier, with no hot or warm tier.
pub fn dmq_ntc_bundle() -> OuroborosBundle {
    let descriptor = |num: MiniProtocolNum| MiniProtocolDescriptor {
        num,
        temperature: ProtocolTemperature::Established,
        start_mode: MiniProtocolStart::StartEagerly,
        limits: MiniProtocolLimits::default(),
    };
    OuroborosBundle {
        hot: Vec::new(),
        warm: Vec::new(),
        established: vec![
            descriptor(LOCAL_MSG_SUBMISSION_MINI_PROTOCOL_NUM),
            descriptor(LOCAL_MSG_NOTIFICATION_MINI_PROTOCOL_NUM),
        ],
    }
}

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
    fn dmq_ntc_bundle_is_established_only() {
        let bundle = dmq_ntc_bundle();
        // NtC connections are responder-only — no hot or warm tier.
        assert!(bundle.hot.is_empty());
        assert!(bundle.warm.is_empty());
        let established: Vec<MiniProtocolNum> = bundle.established.iter().map(|d| d.num).collect();
        assert_eq!(
            established,
            vec![
                LOCAL_MSG_SUBMISSION_MINI_PROTOCOL_NUM,
                LOCAL_MSG_NOTIFICATION_MINI_PROTOCOL_NUM,
            ]
        );
    }

    #[test]
    fn ntc_max_sigs_to_ack_matches_upstream() {
        assert_eq!(NTC_MAX_SIGS_TO_ACK, 1000);
    }
}
