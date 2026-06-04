//! Version 2 service driver vocabulary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Service/V2/Driver.hs.
//!
//! Mirrors the pure discriminator and read-error mapping surface of
//! upstream `Service.V2.Driver`. Raw bearer send/receive and direct
//! serialization remain deferred to the daemon/socket follow-on.

use super::service_v2_protocol::ServiceMessage;
use super::types::{ServiceDriverTrace, VersionIdentifier};

/// Message discriminator for key-push payloads. Mirrors upstream
/// `KeyMessageTypeID`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[repr(u8)]
pub enum KeyMessageTypeID {
    /// `KeyMessageID`.
    KeyMessageID = 0,
    /// `DropKeyMessageID`.
    DropKeyMessageID = 1,
}

impl KeyMessageTypeID {
    /// Discriminants in upstream declaration order.
    pub const ALL: [Self; 2] = [Self::KeyMessageID, Self::DropKeyMessageID];

    /// Upstream enum ordinal used by `encodeEnum`.
    pub const fn ordinal(self) -> u8 {
        self as u8
    }

    /// Decode an upstream enum ordinal.
    pub const fn from_ordinal(ordinal: u8) -> Option<Self> {
        match ordinal {
            0 => Some(Self::KeyMessageID),
            1 => Some(Self::DropKeyMessageID),
            _ => None,
        }
    }
}

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

/// Message discriminator selected by upstream `sendMessage` for idle
/// key-push messages.
pub const fn key_message_type_id(message: &ServiceMessage) -> Option<KeyMessageTypeID> {
    match message {
        ServiceMessage::KeyMessage(_, _) => Some(KeyMessageTypeID::KeyMessageID),
        ServiceMessage::DropKeyMessage(_) => Some(KeyMessageTypeID::DropKeyMessageID),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::types::mk_version_identifier;

    #[test]
    fn key_message_type_ordinals_match_upstream_declaration_order() {
        for (idx, tag) in KeyMessageTypeID::ALL.iter().copied().enumerate() {
            let ordinal = idx as u8;
            assert_eq!(tag.ordinal(), ordinal);
            assert_eq!(KeyMessageTypeID::from_ordinal(ordinal), Some(tag));
        }
        assert_eq!(KeyMessageTypeID::from_ordinal(2), None);
    }

    #[test]
    fn key_message_type_id_matches_upstream_send_message_cases() {
        assert_eq!(
            key_message_type_id(&ServiceMessage::KeyMessage(vec![1], 1)),
            Some(KeyMessageTypeID::KeyMessageID)
        );
        assert_eq!(
            key_message_type_id(&ServiceMessage::DropKeyMessage(1)),
            Some(KeyMessageTypeID::DropKeyMessageID)
        );
        assert_eq!(key_message_type_id(&ServiceMessage::VersionMessage), None);
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
                expected: mk_version_identifier("Service:2.0"),
                actual: mk_version_identifier("Service:1.0"),
            }),
            ServiceDriverTrace::ServiceDriverInvalidVersion(
                mk_version_identifier("Service:2.0"),
                mk_version_identifier("Service:1.0")
            )
        );
    }
}
