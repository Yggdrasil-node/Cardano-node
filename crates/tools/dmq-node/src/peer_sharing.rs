//! dmq-node peer-sharing API infrastructure.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of the peer-sharing API types of
//! upstream `Ouroboros.Network.PeerSharing` — the `PeerSharingAPI`
//! the DMQ `NodeKernel` (`Diffusion/NodeKernel.hs`) holds for the
//! `PeerSharing` mini-protocol — and the peer-sharing policy
//! constants. dmq-node carries its own copy (the R732 dmq-node-local
//! decision).
//!
//! Slice of the Option A `run()` integration arc (see the
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry); this slice ports
//! the self-contained peer-sharing policy foundations.

use std::time::Duration;

/// The number of peers requested in, or returned by, one `PeerSharing`
/// exchange.
///
/// Mirror of upstream `newtype PeerSharingAmount = PeerSharingAmount
/// Word8` (`Protocol/PeerSharing/Type.hs`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PeerSharingAmount(pub u8);

/// The time between changes to the salt used to pick peers to gossip
/// about — 823 seconds.
///
/// Mirror of upstream `ps_POLICY_PEER_SHARE_STICKY_TIME`.
pub const PS_POLICY_PEER_SHARE_STICKY_TIME: Duration = Duration::from_secs(823);

/// The maximum number of peers to respond with in a single
/// `PeerSharing` request — 10.
///
/// Mirror of upstream `ps_POLICY_PEER_SHARE_MAX_PEERS`.
pub const PS_POLICY_PEER_SHARE_MAX_PEERS: PeerSharingAmount = PeerSharingAmount(10);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_sharing_amount_wraps_word8() {
        assert_eq!(PeerSharingAmount(7).0, 7);
        assert_eq!(PeerSharingAmount::default(), PeerSharingAmount(0));
        assert!(PeerSharingAmount(3) < PeerSharingAmount(9));
    }

    #[test]
    fn peer_share_policy_constants_match_upstream() {
        assert_eq!(PS_POLICY_PEER_SHARE_STICKY_TIME, Duration::from_secs(823));
        assert_eq!(PS_POLICY_PEER_SHARE_MAX_PEERS, PeerSharingAmount(10));
    }
}
