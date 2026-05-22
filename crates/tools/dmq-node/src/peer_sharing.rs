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
//! Slices of the Option A `run()` integration arc (see the
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry): the peer-sharing
//! policy constants, the `PublicPeerSelectionState`, and the
//! `PeerSharingAPI` record.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
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

/// The public peer-selection state — the set of peer addresses this
/// node is willing to share via the `PeerSharing` protocol.
///
/// Mirror of upstream `newtype PublicPeerSelectionState peeraddr`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicPeerSelectionState<PeerAddr: Ord> {
    /// The peers available to gossip about (`availableToShare`).
    pub available_to_share: BTreeSet<PeerAddr>,
}

impl<PeerAddr: Ord> Default for PublicPeerSelectionState<PeerAddr> {
    fn default() -> Self {
        PublicPeerSelectionState {
            available_to_share: BTreeSet::new(),
        }
    }
}

impl<PeerAddr: Ord> PublicPeerSelectionState<PeerAddr> {
    /// The empty public peer-selection state.
    ///
    /// Mirror of upstream `emptyPublicPeerSelectionState`.
    pub fn empty() -> PublicPeerSelectionState<PeerAddr> {
        PublicPeerSelectionState::default()
    }
}

/// The peer-sharing API the DMQ `NodeKernel` holds for the
/// `PeerSharing` mini-protocol.
///
/// Mirror of upstream `data PeerSharingAPI addr s m`. The RNG state
/// `s` (upstream `StdGen`) is modelled as a `u64` seed; upstream's
/// `Time` re-salt deadline is a `Duration` since the monotonic
/// origin.
#[derive(Clone, Debug)]
pub struct PeerSharingAPI<PeerAddr: Ord> {
    /// The shared public peer-selection state
    /// (`psPublicPeerSelectionStateVar`).
    pub public_peer_selection_state_var: Arc<Mutex<PublicPeerSelectionState<PeerAddr>>>,
    /// The peer-pick PRNG state (`psGenVar`).
    pub gen_var: Arc<Mutex<u64>>,
    /// The deadline of the next salt rotation (`psReSaltAtVar`).
    pub re_salt_at_var: Arc<Mutex<Duration>>,
    /// The salt-rotation interval (`psPolicyPeerShareStickyTime`).
    pub policy_peer_share_sticky_time: Duration,
    /// The maximum peers per `PeerSharing` reply
    /// (`psPolicyPeerShareMaxPeers`).
    pub policy_peer_share_max_peers: PeerSharingAmount,
}

/// Construct a [`PeerSharingAPI`] over a shared public
/// peer-selection state, a PRNG seed, and the peer-share policy.
///
/// Mirror of upstream `newPeerSharingAPI`.
pub fn new_peer_sharing_api<PeerAddr: Ord>(
    public_peer_selection_state_var: Arc<Mutex<PublicPeerSelectionState<PeerAddr>>>,
    rng: u64,
    policy_peer_share_sticky_time: Duration,
    policy_peer_share_max_peers: PeerSharingAmount,
) -> PeerSharingAPI<PeerAddr> {
    PeerSharingAPI {
        public_peer_selection_state_var,
        gen_var: Arc::new(Mutex::new(rng)),
        re_salt_at_var: Arc::new(Mutex::new(Duration::ZERO)),
        policy_peer_share_sticky_time,
        policy_peer_share_max_peers,
    }
}

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

    #[test]
    fn public_peer_selection_state_empty_has_no_peers() {
        let state: PublicPeerSelectionState<String> = PublicPeerSelectionState::empty();
        assert!(state.available_to_share.is_empty());
    }

    #[test]
    fn new_peer_sharing_api_carries_the_policy_and_state() {
        let state_var = Arc::new(Mutex::new(PublicPeerSelectionState::<String>::empty()));
        let api = new_peer_sharing_api(
            Arc::clone(&state_var),
            0x1234,
            PS_POLICY_PEER_SHARE_STICKY_TIME,
            PS_POLICY_PEER_SHARE_MAX_PEERS,
        );
        assert_eq!(api.policy_peer_share_sticky_time, Duration::from_secs(823));
        assert_eq!(api.policy_peer_share_max_peers, PeerSharingAmount(10));
        assert_eq!(
            *api.gen_var.lock().unwrap_or_else(|e| e.into_inner()),
            0x1234
        );
        // The state var is shared: a peer added via the handle is visible
        // through the API.
        state_var
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .available_to_share
            .insert("peer-x".to_string());
        assert!(
            api.public_peer_selection_state_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .available_to_share
                .contains("peer-x")
        );
    }
}
