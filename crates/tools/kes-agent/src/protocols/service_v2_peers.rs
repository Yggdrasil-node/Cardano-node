//! Pure peers for the version 2 service protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V2/Peers.hs.
//!
//! Mirrors the pure control-flow decisions in upstream
//! `serviceReceiver` and `servicePusher`. Runtime typed-protocol
//! peers, async waits, and socket I/O remain deferred.

use super::recv_result::RecvResult;
use super::service_v2_protocol::{ServiceBundle, ServiceMessage};
use super::types::Timestamp;

/// Rust mirror of upstream `TaggedBundle m StandardCrypto`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TaggedBundle {
    /// `TaggedBundle` bundle payload.
    pub bundle: Option<ServiceBundle>,
    /// `TaggedBundle` timestamp.
    pub timestamp: Timestamp,
}

/// Receive-side response to one service message. Mirrors the decision
/// path inside upstream `serviceReceiver`.
pub fn service_receiver<F>(message: &ServiceMessage, receive_bundle: F) -> Option<ServiceMessage>
where
    F: FnOnce(TaggedBundle) -> RecvResult,
{
    match message {
        ServiceMessage::VersionMessage => None,
        ServiceMessage::KeyMessage(bundle, timestamp) => {
            let result = receive_bundle(TaggedBundle {
                bundle: Some(bundle.clone()),
                timestamp: *timestamp,
            });
            Some(ServiceMessage::RecvResultMessage(result))
        }
        ServiceMessage::DropKeyMessage(timestamp) => {
            let result = receive_bundle(TaggedBundle {
                bundle: None,
                timestamp: *timestamp,
            });
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

/// Key/drop message produced by upstream `servicePusher`'s `goKey`.
pub fn service_pusher_key_message(tagged_bundle: TaggedBundle) -> ServiceMessage {
    match tagged_bundle.bundle {
        Some(bundle) => ServiceMessage::KeyMessage(bundle, tagged_bundle.timestamp),
        None => ServiceMessage::DropKeyMessage(tagged_bundle.timestamp),
    }
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
        let response = service_receiver(&ServiceMessage::KeyMessage(vec![1, 2, 3], 42), |tagged| {
            assert_eq!(tagged.bundle, Some(vec![1, 2, 3]));
            assert_eq!(tagged.timestamp, 42);
            RecvResult::RecvOK
        });
        assert_eq!(
            response,
            Some(ServiceMessage::RecvResultMessage(RecvResult::RecvOK))
        );
    }

    #[test]
    fn service_receiver_confirms_drop_key_messages() {
        let response = service_receiver(&ServiceMessage::DropKeyMessage(7), |tagged| {
            assert_eq!(tagged.bundle, None);
            assert_eq!(tagged.timestamp, 7);
            RecvResult::RecvErrorNoKey
        });
        assert_eq!(
            response,
            Some(ServiceMessage::RecvResultMessage(
                RecvResult::RecvErrorNoKey
            ))
        );
    }

    #[test]
    fn service_pusher_messages_match_upstream_shape() {
        assert_eq!(
            service_pusher_initial_message(),
            ServiceMessage::VersionMessage
        );
        assert_eq!(
            service_pusher_key_message(TaggedBundle {
                bundle: Some(vec![4, 5]),
                timestamp: 11,
            }),
            ServiceMessage::KeyMessage(vec![4, 5], 11)
        );
        assert_eq!(
            service_pusher_key_message(TaggedBundle {
                bundle: None,
                timestamp: 12,
            }),
            ServiceMessage::DropKeyMessage(12)
        );
        assert_eq!(
            service_pusher_result(&ServiceMessage::RecvResultMessage(RecvResult::RecvOK)),
            Some(RecvResult::RecvOK)
        );
    }
}
