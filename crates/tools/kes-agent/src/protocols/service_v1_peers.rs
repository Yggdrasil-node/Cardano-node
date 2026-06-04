//! Pure peers for the version 1 service protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V1/Peers.hs.
//!
//! Mirrors the pure control-flow decisions in upstream
//! `serviceReceiver` and `servicePusher`. Runtime typed-protocol
//! peers, async waits, and socket I/O remain deferred.

use super::recv_result::RecvResult;
use super::service_v1_protocol::{ServiceBundle, ServiceMessage};

/// Receive-side response to one service message. Mirrors the decision
/// path inside upstream `serviceReceiver`.
pub fn service_receiver<F>(message: &ServiceMessage, receive_bundle: F) -> Option<ServiceMessage>
where
    F: FnOnce(ServiceBundle) -> RecvResult,
{
    match message {
        ServiceMessage::VersionMessage => None,
        ServiceMessage::KeyMessage(bundle) => {
            let result = receive_bundle(bundle.clone());
            Some(ServiceMessage::RecvResultMessage(result))
        }
        ServiceMessage::AbortMessage
        | ServiceMessage::ProtocolErrorMessage
        | ServiceMessage::ServerDisconnectMessage => None,
        ServiceMessage::RecvResultMessage(_) | ServiceMessage::ClientDisconnectMessage => None,
    }
}

/// Initial message sent by upstream `servicePusher`.
pub const fn service_pusher_initial_message() -> ServiceMessage {
    ServiceMessage::VersionMessage
}

/// Key message produced by upstream `servicePusher` after a current or
/// next bundle is available.
pub fn service_pusher_key_message(bundle: ServiceBundle) -> ServiceMessage {
    ServiceMessage::KeyMessage(bundle)
}

/// Synthetic result used by upstream `servicePusher` when no next key
/// is currently available.
pub const fn service_pusher_no_next_key_result() -> RecvResult {
    RecvResult::RecvErrorUnsupportedOperation
}

/// Extract the receive result handled by upstream `servicePusher`.
pub const fn service_pusher_result(message: &ServiceMessage) -> Option<RecvResult> {
    match message {
        ServiceMessage::RecvResultMessage(result) => Some(*result),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_receiver_confirms_key_messages() {
        let response = service_receiver(&ServiceMessage::KeyMessage(vec![1, 2, 3]), |bundle| {
            assert_eq!(bundle, vec![1, 2, 3]);
            RecvResult::RecvOK
        });
        assert_eq!(
            response,
            Some(ServiceMessage::RecvResultMessage(RecvResult::RecvOK))
        );
    }

    #[test]
    fn service_receiver_ignores_disconnect_and_terminal_messages() {
        assert_eq!(
            service_receiver(&ServiceMessage::ServerDisconnectMessage, |_| {
                RecvResult::RecvOK
            }),
            None
        );
        assert_eq!(
            service_receiver(&ServiceMessage::ProtocolErrorMessage, |_| {
                RecvResult::RecvOK
            }),
            None
        );
    }

    #[test]
    fn service_pusher_messages_match_upstream_shape() {
        assert_eq!(
            service_pusher_initial_message(),
            ServiceMessage::VersionMessage
        );
        assert_eq!(
            service_pusher_key_message(vec![4, 5]),
            ServiceMessage::KeyMessage(vec![4, 5])
        );
        assert_eq!(
            service_pusher_no_next_key_result(),
            RecvResult::RecvErrorUnsupportedOperation
        );
        assert_eq!(
            service_pusher_result(&ServiceMessage::RecvResultMessage(RecvResult::RecvOK)),
            Some(RecvResult::RecvOK)
        );
    }
}
