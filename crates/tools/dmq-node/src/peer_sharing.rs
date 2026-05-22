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
//! policy constants, `PublicPeerSelectionState`, the `PeerSharingAPI`
//! record, and the `PeerSharingController` / `PeerSharingRegistry`.

use std::collections::{BTreeMap, BTreeSet};
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

/// A pending peer-sharing request — how many peers are wanted plus
/// the slot the shared peers will be delivered into.
///
/// Models the upstream `(PeerSharingAmount, MVar m [peer])` payload of
/// a `PeerSharingController`'s request mailbox.
#[derive(Clone, Debug)]
pub struct PeerSharingRequest<Peer> {
    /// How many peers are being requested.
    pub amount: PeerSharingAmount,
    /// The result slot — filled with the shared peers once available.
    pub result: Arc<Mutex<Option<Vec<Peer>>>>,
}

/// A depth-1 request mailbox for one peer's peer-sharing exchange.
///
/// Mirror of upstream `newtype PeerSharingController peer m`. The
/// upstream `StrictTMVar m (PeerSharingAmount, MVar m [peer])` mailbox
/// is modelled as a `Mutex`-guarded optional slot — `None` empty,
/// `Some` one pending request.
#[derive(Clone, Debug)]
pub struct PeerSharingController<Peer> {
    /// The pending peer-sharing request, if any (`requestQueue`).
    pub request_queue: Arc<Mutex<Option<PeerSharingRequest<Peer>>>>,
}

impl<Peer> Default for PeerSharingController<Peer> {
    fn default() -> Self {
        PeerSharingController {
            request_queue: Arc::new(Mutex::new(None)),
        }
    }
}

impl<Peer> PeerSharingController<Peer> {
    /// A fresh controller with an empty request mailbox.
    pub fn new() -> PeerSharingController<Peer> {
        PeerSharingController::default()
    }
}

/// A registry of per-peer peer-sharing controllers.
///
/// Mirror of upstream `newtype PeerSharingRegistry peer m`.
#[derive(Clone, Debug)]
pub struct PeerSharingRegistry<Peer: Ord> {
    /// One [`PeerSharingController`] per registered peer
    /// (`getPeerSharingRegistry`).
    pub get_peer_sharing_registry: Arc<Mutex<BTreeMap<Peer, PeerSharingController<Peer>>>>,
}

impl<Peer: Ord> Default for PeerSharingRegistry<Peer> {
    fn default() -> Self {
        PeerSharingRegistry {
            get_peer_sharing_registry: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

/// Construct an empty [`PeerSharingRegistry`].
///
/// Mirror of upstream `newPeerSharingRegistry`.
pub fn new_peer_sharing_registry<Peer: Ord>() -> PeerSharingRegistry<Peer> {
    PeerSharingRegistry::default()
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

    #[test]
    fn peer_sharing_registry_registers_controllers() {
        let registry = new_peer_sharing_registry::<String>();
        assert!(
            registry
                .get_peer_sharing_registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
        registry
            .get_peer_sharing_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert("peer-a".to_string(), PeerSharingController::new());
        let guard = registry
            .get_peer_sharing_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let controller = guard.get("peer-a").expect("registered");
        // A fresh controller has an empty request mailbox.
        assert!(
            controller
                .request_queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
    }

    #[test]
    fn peer_sharing_controller_holds_a_request() {
        let controller: PeerSharingController<String> = PeerSharingController::new();
        let result: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
        *controller
            .request_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(PeerSharingRequest {
            amount: PeerSharingAmount(5),
            result: Arc::clone(&result),
        });
        let guard = controller
            .request_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.as_ref().map(|r| r.amount), Some(PeerSharingAmount(5)));
    }
}
