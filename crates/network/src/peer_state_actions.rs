//! Governor-to-runtime peer state action bridge helpers.
//!
//! This module keeps the outbound governor pure while providing a small,
//! testable seam that runtime code can implement. It mirrors the upstream
//! `PeerStateActions` record shape:
//! - map governor decisions into connection-lifecycle actions
//! - dispatch those actions through a runtime-provided executor
//!
//! Reference:
//! `ouroboros-network/src/Ouroboros/Network/PeerSelection/PeerStateActions.hs`

use std::net::SocketAddr;

use crate::diffusion::PeerStateAction;
use crate::governor::GovernorAction;

/// Runtime-side executor for peer connection lifecycle actions.
///
/// The network crate owns only pure decision logic. Runtime implementations
/// (for example in `node/`) provide effectful behavior for each action.
pub trait PeerStateActions {
    /// Runtime-specific error type for action execution.
    type Error;

    /// Establish an outbound connection to a cold peer.
    fn establish_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error>;

    /// Activate hot protocols on an existing warm connection.
    fn activate_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error>;

    /// Deactivate hot protocols on an existing hot connection.
    fn deactivate_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error>;

    /// Close an established connection and demote the peer to cold.
    fn close_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error>;

    /// Execute a single translated peer state action.
    fn execute_peer_state_action(&mut self, action: PeerStateAction) -> Result<(), Self::Error> {
        match action {
            PeerStateAction::EstablishConnection(peer) => self.establish_peer_connection(peer),
            PeerStateAction::ActivateConnection(peer) => self.activate_peer_connection(peer),
            PeerStateAction::DeactivateConnection(peer) => self.deactivate_peer_connection(peer),
            PeerStateAction::CloseConnection(peer) => self.close_peer_connection(peer),
        }
    }

    /// Execute a sequence of translated actions in order.
    ///
    /// Execution stops at the first error and returns that error.
    fn execute_peer_state_actions<I>(&mut self, actions: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = PeerStateAction>,
    {
        for action in actions {
            self.execute_peer_state_action(action)?;
        }
        Ok(())
    }
}

/// Map one governor action to a peer-state action when it is a
/// connection-lifecycle transition.
///
/// Returns `None` for non-connection actions that runtime code handles
/// through other paths (`ForgetPeer`, `ShareRequest`, root refresh,
/// inbound adoption).
pub fn governor_action_to_peer_state_action(action: &GovernorAction) -> Option<PeerStateAction> {
    match action {
        GovernorAction::PromoteToWarm(peer) => Some(PeerStateAction::EstablishConnection(*peer)),
        GovernorAction::PromoteToHot(peer) => Some(PeerStateAction::ActivateConnection(*peer)),
        GovernorAction::DemoteToWarm(peer) => Some(PeerStateAction::DeactivateConnection(*peer)),
        GovernorAction::DemoteToCold(peer) => Some(PeerStateAction::CloseConnection(*peer)),
        GovernorAction::ForgetPeer(_)
        | GovernorAction::ShareRequest(_)
        | GovernorAction::RequestPublicRoots
        | GovernorAction::RequestBigLedgerPeers
        | GovernorAction::AdoptInboundPeer(_) => None,
    }
}

/// Map a sequence of governor actions into peer-state actions,
/// preserving order and dropping non-connection actions.
pub fn governor_actions_to_peer_state_actions<'a, I>(actions: I) -> Vec<PeerStateAction>
where
    I: IntoIterator<Item = &'a GovernorAction>,
{
    actions
        .into_iter()
        .filter_map(governor_action_to_peer_state_action)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    #[test]
    fn maps_connection_lifecycle_governor_actions() {
        assert_eq!(
            governor_action_to_peer_state_action(&GovernorAction::PromoteToWarm(addr(1))),
            Some(PeerStateAction::EstablishConnection(addr(1)))
        );
        assert_eq!(
            governor_action_to_peer_state_action(&GovernorAction::PromoteToHot(addr(2))),
            Some(PeerStateAction::ActivateConnection(addr(2)))
        );
        assert_eq!(
            governor_action_to_peer_state_action(&GovernorAction::DemoteToWarm(addr(3))),
            Some(PeerStateAction::DeactivateConnection(addr(3)))
        );
        assert_eq!(
            governor_action_to_peer_state_action(&GovernorAction::DemoteToCold(addr(4))),
            Some(PeerStateAction::CloseConnection(addr(4)))
        );
    }

    #[test]
    fn ignores_non_connection_governor_actions() {
        let actions = [
            GovernorAction::ForgetPeer(addr(1)),
            GovernorAction::ShareRequest(addr(2)),
            GovernorAction::RequestPublicRoots,
            GovernorAction::RequestBigLedgerPeers,
            GovernorAction::AdoptInboundPeer(addr(3)),
        ];

        for action in actions {
            assert!(governor_action_to_peer_state_action(&action).is_none());
        }
    }

    #[test]
    fn maps_action_sequence_preserving_order() {
        let input = vec![
            GovernorAction::PromoteToWarm(addr(10)),
            GovernorAction::RequestPublicRoots,
            GovernorAction::PromoteToHot(addr(11)),
            GovernorAction::DemoteToWarm(addr(12)),
            GovernorAction::ForgetPeer(addr(13)),
            GovernorAction::DemoteToCold(addr(14)),
        ];

        let mapped = governor_actions_to_peer_state_actions(input.iter());

        assert_eq!(
            mapped,
            vec![
                PeerStateAction::EstablishConnection(addr(10)),
                PeerStateAction::ActivateConnection(addr(11)),
                PeerStateAction::DeactivateConnection(addr(12)),
                PeerStateAction::CloseConnection(addr(14)),
            ]
        );
    }

    #[derive(Default)]
    struct RecordingPeerStateActions {
        calls: Vec<PeerStateAction>,
        fail_on: Option<PeerStateAction>,
    }

    impl PeerStateActions for RecordingPeerStateActions {
        type Error = &'static str;

        fn establish_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error> {
            let action = PeerStateAction::EstablishConnection(peer);
            if self.fail_on == Some(action.clone()) {
                return Err("establish failure");
            }
            self.calls.push(action);
            Ok(())
        }

        fn activate_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error> {
            let action = PeerStateAction::ActivateConnection(peer);
            if self.fail_on == Some(action.clone()) {
                return Err("activate failure");
            }
            self.calls.push(action);
            Ok(())
        }

        fn deactivate_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error> {
            let action = PeerStateAction::DeactivateConnection(peer);
            if self.fail_on == Some(action.clone()) {
                return Err("deactivate failure");
            }
            self.calls.push(action);
            Ok(())
        }

        fn close_peer_connection(&mut self, peer: SocketAddr) -> Result<(), Self::Error> {
            let action = PeerStateAction::CloseConnection(peer);
            if self.fail_on == Some(action.clone()) {
                return Err("close failure");
            }
            self.calls.push(action);
            Ok(())
        }
    }

    #[test]
    fn execute_single_action_dispatches_to_trait_methods() {
        let mut recorder = RecordingPeerStateActions::default();

        let result =
            recorder.execute_peer_state_action(PeerStateAction::EstablishConnection(addr(2000)));

        assert!(result.is_ok());
        assert_eq!(
            recorder.calls,
            vec![PeerStateAction::EstablishConnection(addr(2000))]
        );
    }

    #[test]
    fn execute_actions_runs_in_order() {
        let mut recorder = RecordingPeerStateActions::default();
        let actions = vec![
            PeerStateAction::EstablishConnection(addr(2000)),
            PeerStateAction::ActivateConnection(addr(2001)),
            PeerStateAction::DeactivateConnection(addr(2002)),
            PeerStateAction::CloseConnection(addr(2003)),
        ];

        let result = recorder.execute_peer_state_actions(actions.clone());

        assert!(result.is_ok());
        assert_eq!(recorder.calls, actions);
    }

    #[test]
    fn execute_actions_stops_on_first_error() {
        let mut recorder = RecordingPeerStateActions {
            calls: Vec::new(),
            fail_on: Some(PeerStateAction::ActivateConnection(addr(2001))),
        };
        let actions = vec![
            PeerStateAction::EstablishConnection(addr(2000)),
            PeerStateAction::ActivateConnection(addr(2001)),
            PeerStateAction::CloseConnection(addr(2002)),
        ];

        let result = recorder.execute_peer_state_actions(actions);

        assert_eq!(result, Err("activate failure"));
        assert_eq!(
            recorder.calls,
            vec![PeerStateAction::EstablishConnection(addr(2000))]
        );
    }
}
