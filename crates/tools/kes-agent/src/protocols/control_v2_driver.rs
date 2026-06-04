//! Version 2 control driver vocabulary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Control/V2/Driver.hs.
//!
//! Mirrors the pure discriminator and read-error mapping surface of
//! upstream `Control.V2.Driver`. Raw bearer send/receive and direct
//! serialization remain deferred to the daemon/socket follow-on.

use super::control_v2_protocol::ControlMessage;
use super::types::{Command, ControlDriverTrace, VersionIdentifier};

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

/// Idiomatic Rust casing for upstream `readErrorToControlDriverTrace`.
pub fn read_error_to_control_driver_trace(result: ReadResult) -> ControlDriverTrace {
    match result {
        ReadResult::ReadOK => {
            ControlDriverTrace::ControlDriverMisc("This should not happen".to_string())
        }
        ReadResult::ReadEOF => ControlDriverTrace::ControlDriverConnectionClosed,
        ReadResult::ReadMalformed(what) => ControlDriverTrace::ControlDriverProtocolError(what),
        ReadResult::ReadVersionMismatch { expected, actual } => {
            ControlDriverTrace::ControlDriverInvalidVersion(expected, actual)
        }
    }
}

/// Command discriminator selected by upstream `sendMessage` for
/// idle-state Control V2 command messages.
pub const fn command_for_control_message(message: &ControlMessage) -> Option<Command> {
    match message {
        ControlMessage::GenStagedKeyMessage => Some(Command::GenStagedKeyCmd),
        ControlMessage::QueryStagedKeyMessage => Some(Command::QueryStagedKeyCmd),
        ControlMessage::DropStagedKeyMessage => Some(Command::DropStagedKeyCmd),
        ControlMessage::DropKeyMessage => Some(Command::DropKeyCmd),
        ControlMessage::RequestInfoMessage => Some(Command::RequestInfoCmd),
        ControlMessage::InstallKeyMessage(_) => Some(Command::InstallKeyCmd),
        ControlMessage::VersionMessage
        | ControlMessage::PublicKeyMessage(_)
        | ControlMessage::InstallResultMessage(_)
        | ControlMessage::DropKeyResultMessage(_)
        | ControlMessage::InfoMessage(_)
        | ControlMessage::AbortMessage
        | ControlMessage::EndMessage
        | ControlMessage::ProtocolErrorMessage => None,
    }
}

/// Message selected by upstream `recvMessage` after reading a
/// Control V2 command discriminator.
pub fn control_message_for_command(command: Command, ocert: Option<Vec<u8>>) -> ControlMessage {
    match command {
        Command::GenStagedKeyCmd => ControlMessage::GenStagedKeyMessage,
        Command::QueryStagedKeyCmd => ControlMessage::QueryStagedKeyMessage,
        Command::DropStagedKeyCmd => ControlMessage::DropStagedKeyMessage,
        Command::DropKeyCmd => ControlMessage::DropKeyMessage,
        Command::RequestInfoCmd => ControlMessage::RequestInfoMessage,
        Command::InstallKeyCmd => ControlMessage::InstallKeyMessage(ocert.unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::recv_result::RecvResult;
    use crate::protocols::types::mk_version_identifier;

    #[test]
    fn read_error_to_control_driver_trace_matches_upstream_mapping() {
        assert_eq!(
            read_error_to_control_driver_trace(ReadResult::ReadOK),
            ControlDriverTrace::ControlDriverMisc("This should not happen".to_string())
        );
        assert_eq!(
            read_error_to_control_driver_trace(ReadResult::ReadEOF),
            ControlDriverTrace::ControlDriverConnectionClosed
        );
        assert_eq!(
            read_error_to_control_driver_trace(ReadResult::ReadMalformed("bad".to_string())),
            ControlDriverTrace::ControlDriverProtocolError("bad".to_string())
        );
        assert_eq!(
            read_error_to_control_driver_trace(ReadResult::ReadVersionMismatch {
                expected: mk_version_identifier("Control:2.0"),
                actual: mk_version_identifier("Control:1.0"),
            }),
            ControlDriverTrace::ControlDriverInvalidVersion(
                mk_version_identifier("Control:2.0"),
                mk_version_identifier("Control:1.0")
            )
        );
    }

    #[test]
    fn command_for_control_message_matches_upstream_send_message_cases() {
        assert_eq!(
            command_for_control_message(&ControlMessage::GenStagedKeyMessage),
            Some(Command::GenStagedKeyCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::QueryStagedKeyMessage),
            Some(Command::QueryStagedKeyCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::DropStagedKeyMessage),
            Some(Command::DropStagedKeyCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::DropKeyMessage),
            Some(Command::DropKeyCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::RequestInfoMessage),
            Some(Command::RequestInfoCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::InstallKeyMessage(vec![1])),
            Some(Command::InstallKeyCmd)
        );
        assert_eq!(
            command_for_control_message(&ControlMessage::InstallResultMessage(RecvResult::RecvOK)),
            None
        );
    }

    #[test]
    fn control_message_for_command_matches_upstream_recv_message_cases() {
        assert_eq!(
            control_message_for_command(Command::GenStagedKeyCmd, None),
            ControlMessage::GenStagedKeyMessage
        );
        assert_eq!(
            control_message_for_command(Command::QueryStagedKeyCmd, None),
            ControlMessage::QueryStagedKeyMessage
        );
        assert_eq!(
            control_message_for_command(Command::DropStagedKeyCmd, None),
            ControlMessage::DropStagedKeyMessage
        );
        assert_eq!(
            control_message_for_command(Command::DropKeyCmd, None),
            ControlMessage::DropKeyMessage
        );
        assert_eq!(
            control_message_for_command(Command::RequestInfoCmd, None),
            ControlMessage::RequestInfoMessage
        );
        assert_eq!(
            control_message_for_command(Command::InstallKeyCmd, Some(vec![9])),
            ControlMessage::InstallKeyMessage(vec![9])
        );
    }
}
