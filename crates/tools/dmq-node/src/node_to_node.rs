//! DMQ node-to-node (NtN) protocol surface.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Module-tree parent for the upstream
//! `DMQ/NodeToNode/` directory, plus the self-contained constants of
//! `DMQ/NodeToNode.hs` (the mux mini-protocol numbers) and the
//! node-to-node mux mini-protocol bundle. The runnable `ntnApps` /
//! `Apps` closures of `DMQ/NodeToNode.hs` land with the `run()` loop.

pub mod version;

use yggdrasil_network::{
    MiniProtocolDescriptor, MiniProtocolLimits, MiniProtocolNum, MiniProtocolStart,
    OuroborosBundle, ProtocolTemperature,
};

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

/// A mini-protocol descriptor with the standard eager start and the
/// default ingress-queue limit.
fn dmq_descriptor(
    num: MiniProtocolNum,
    temperature: ProtocolTemperature,
) -> MiniProtocolDescriptor {
    MiniProtocolDescriptor {
        num,
        temperature,
        start_mode: MiniProtocolStart::StartEagerly,
        limits: MiniProtocolLimits::default(),
    }
}

/// The DMQ node-to-node mux mini-protocol bundle.
///
/// Mirror of the DMQ NtN protocol assignment (`DMQ/NodeToNode.hs` /
/// `Diffusion/Applications.hs`): the warm-tier `SigSubmission` and
/// `KeepAlive`, the established-tier `PeerSharing`. dmq-node runs no
/// block sync, so there is no hot tier.
pub fn dmq_ntn_bundle() -> OuroborosBundle {
    OuroborosBundle {
        hot: Vec::new(),
        warm: vec![
            dmq_descriptor(SIG_SUBMISSION_MINI_PROTOCOL_NUM, ProtocolTemperature::Warm),
            dmq_descriptor(KEEP_ALIVE_MINI_PROTOCOL_NUM, ProtocolTemperature::Warm),
        ],
        established: vec![dmq_descriptor(
            PEER_SHARING_MINI_PROTOCOL_NUM,
            ProtocolTemperature::Established,
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntn_mini_protocol_numbers_match_upstream() {
        assert_eq!(SIG_SUBMISSION_MINI_PROTOCOL_NUM, MiniProtocolNum(11));
        assert_eq!(KEEP_ALIVE_MINI_PROTOCOL_NUM, MiniProtocolNum(12));
        assert_eq!(PEER_SHARING_MINI_PROTOCOL_NUM, MiniProtocolNum(13));
    }

    #[test]
    fn dmq_ntn_bundle_assigns_protocols_to_tiers() {
        let bundle = dmq_ntn_bundle();
        // No hot tier — dmq-node does not sync blocks.
        assert!(bundle.hot.is_empty());
        // SigSubmission and KeepAlive are warm.
        let warm: Vec<MiniProtocolNum> = bundle.warm.iter().map(|d| d.num).collect();
        assert_eq!(
            warm,
            vec![
                SIG_SUBMISSION_MINI_PROTOCOL_NUM,
                KEEP_ALIVE_MINI_PROTOCOL_NUM
            ]
        );
        // PeerSharing is established.
        assert_eq!(bundle.established.len(), 1);
        assert_eq!(bundle.established[0].num, PEER_SHARING_MINI_PROTOCOL_NUM);
        assert_eq!(
            bundle.established[0].temperature,
            ProtocolTemperature::Established
        );
    }
}
