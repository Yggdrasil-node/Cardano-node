//! Version 0 service driver vocabulary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V0/Driver.hs.
//!
//! Mirrors the pure read-error mapping and key-message surface of
//! upstream `Service.V0.Driver`. Raw bearer send/receive and direct
//! serialization remain deferred to the daemon/socket follow-on.

use super::service_v0_protocol::{ServiceBundle, ServiceMessage};
use super::types::{ServiceDriverTrace, VersionIdentifier};

/// Read result categories used by upstream `ReadResult`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ReadResult {
    /// `ReadOK`.
    ReadOK,
    /// `ReadEOF`.
    ReadEOF,
    /// `ReadMalformed`.
    ReadMalformed(String),
    /// `ReadVersionMismatch`.
    ReadVersionMismatch {
        /// Expected version.
        expected: VersionIdentifier,
        /// Actual version.
        actual: VersionIdentifier,
    },
}

/// Idiomatic Rust casing for upstream `readErrorToServiceDriverTrace`.
pub fn read_error_to_service_driver_trace(result: ReadResult) -> ServiceDriverTrace {
    match result {
        ReadResult::ReadOK => {
            ServiceDriverTrace::ServiceDriverMisc("This should not happen".to_string())
        }
        ReadResult::ReadEOF => ServiceDriverTrace::ServiceDriverConnectionClosed,
        ReadResult::ReadMalformed(what) => ServiceDriverTrace::ServiceDriverProtocolError(what),
        ReadResult::ReadVersionMismatch { expected, actual } => {
            ServiceDriverTrace::ServiceDriverInvalidVersion(expected, actual)
        }
    }
}

/// Payload selected by upstream `sendMessage` for idle-state
/// `KeyMessage` values.
pub fn key_message_payload(message: &ServiceMessage) -> Option<&ServiceBundle> {
    match message {
        ServiceMessage::KeyMessage(bundle) => Some(bundle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::types::mk_version_identifier;

    #[test]
    fn key_message_payload_matches_upstream_send_message_cases() {
        assert_eq!(
            key_message_payload(&ServiceMessage::KeyMessage(vec![1])),
            Some(&vec![1])
        );
        assert_eq!(key_message_payload(&ServiceMessage::VersionMessage), None);
        assert_eq!(
            key_message_payload(&ServiceMessage::RecvResultMessage(
                crate::protocols::recv_result::RecvResult::RecvOK
            )),
            None
        );
    }

    #[test]
    fn read_error_to_service_driver_trace_matches_upstream_mapping() {
        assert_eq!(
            read_error_to_service_driver_trace(ReadResult::ReadOK),
            ServiceDriverTrace::ServiceDriverMisc("This should not happen".to_string())
        );
        assert_eq!(
            read_error_to_service_driver_trace(ReadResult::ReadEOF),
            ServiceDriverTrace::ServiceDriverConnectionClosed
        );
        assert_eq!(
            read_error_to_service_driver_trace(ReadResult::ReadMalformed("bad".to_string())),
            ServiceDriverTrace::ServiceDriverProtocolError("bad".to_string())
        );
        assert_eq!(
            read_error_to_service_driver_trace(ReadResult::ReadVersionMismatch {
                expected: mk_version_identifier("Service:StandardCrypto:0.4"),
                actual: mk_version_identifier("Service:1.0"),
            }),
            ServiceDriverTrace::ServiceDriverInvalidVersion(
                mk_version_identifier("Service:StandardCrypto:0.4"),
                mk_version_identifier("Service:1.0")
            )
        );
    }
}
