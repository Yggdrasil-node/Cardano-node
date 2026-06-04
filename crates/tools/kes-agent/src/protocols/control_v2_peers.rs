//! Pure peers for the version 2 control protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/kes-agent/kes-agent/src/Cardano/KESAgent/Protocols/Control/V2/Peers.hs.
//!
//! Mirrors the pure control-flow decisions in upstream
//! `controlReceiver` and the `control*` server peers. Runtime
//! typed-protocol peers, STM waits, and socket I/O remain deferred.

use super::control_v2_protocol::{AgentInfo, ControlMessage, OCert, VerKeyKES};
use super::recv_result::RecvResult;

/// Operation requested from the receive-side handler. Mirrors the
/// branches in upstream `controlReceiver`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlReceiverRequest {
    /// `GenStagedKeyMessage`.
    GenStagedKey,
    /// `DropStagedKeyMessage`.
    DropStagedKey,
    /// `QueryStagedKeyMessage`.
    QueryStagedKey,
    /// `InstallKeyMessage`.
    InstallKey(OCert),
    /// `DropKeyMessage`.
    DropKey,
    /// `RequestInfoMessage`.
    RequestInfo,
}

/// Response emitted by the pure receive-side handler.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlReceiverResponse {
    /// `PublicKeyMessage`.
    PublicKey(Option<VerKeyKES>),
    /// `InstallResultMessage`.
    InstallResult(RecvResult),
    /// `DropKeyResultMessage`.
    DropKeyResult(RecvResult),
    /// `InfoMessage`.
    Info(AgentInfo),
}

impl ControlReceiverResponse {
    /// Convert the response into the upstream message constructor.
    pub fn into_message(self) -> ControlMessage {
        match self {
            Self::PublicKey(vkey) => ControlMessage::PublicKeyMessage(vkey),
            Self::InstallResult(result) => ControlMessage::InstallResultMessage(result),
            Self::DropKeyResult(result) => ControlMessage::DropKeyResultMessage(result),
            Self::Info(info) => ControlMessage::InfoMessage(info),
        }
    }
}

/// Decode the receive-side request for one upstream `controlReceiver`
/// idle-state message.
pub fn control_receiver_request(message: &ControlMessage) -> Option<ControlReceiverRequest> {
    match message {
        ControlMessage::GenStagedKeyMessage => Some(ControlReceiverRequest::GenStagedKey),
        ControlMessage::DropStagedKeyMessage => Some(ControlReceiverRequest::DropStagedKey),
        ControlMessage::QueryStagedKeyMessage => Some(ControlReceiverRequest::QueryStagedKey),
        ControlMessage::InstallKeyMessage(ocert) => {
            Some(ControlReceiverRequest::InstallKey(ocert.clone()))
        }
        ControlMessage::DropKeyMessage => Some(ControlReceiverRequest::DropKey),
        ControlMessage::RequestInfoMessage => Some(ControlReceiverRequest::RequestInfo),
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

/// Initial message sent by every upstream Control V2 server peer.
pub const fn control_server_initial_message() -> ControlMessage {
    ControlMessage::VersionMessage
}

/// Command message emitted by upstream `controlGenKey`.
pub const fn control_gen_key_command() -> ControlMessage {
    ControlMessage::GenStagedKeyMessage
}

/// Extract the result handled by upstream `controlGenKey`.
pub fn control_gen_key_result(message: &ControlMessage) -> Option<Option<VerKeyKES>> {
    match message {
        ControlMessage::PublicKeyMessage(vkey) => Some(vkey.clone()),
        _ => None,
    }
}

/// Command message emitted by upstream `controlQueryKey`.
pub const fn control_query_key_command() -> ControlMessage {
    ControlMessage::QueryStagedKeyMessage
}

/// Extract the result handled by upstream `controlQueryKey`.
pub fn control_query_key_result(message: &ControlMessage) -> Option<Option<VerKeyKES>> {
    control_gen_key_result(message)
}

/// Command message emitted by upstream `controlDropStagedKey`.
pub const fn control_drop_staged_key_command() -> ControlMessage {
    ControlMessage::DropStagedKeyMessage
}

/// Extract the result handled by upstream `controlDropStagedKey`.
pub fn control_drop_staged_key_result(message: &ControlMessage) -> Option<Option<VerKeyKES>> {
    control_gen_key_result(message)
}

/// Command message emitted by upstream `controlInstallKey`.
pub fn control_install_key_command(ocert: OCert) -> ControlMessage {
    ControlMessage::InstallKeyMessage(ocert)
}

/// Extract the result handled by upstream `controlInstallKey`.
pub const fn control_install_key_result(message: &ControlMessage) -> Option<RecvResult> {
    match message {
        ControlMessage::InstallResultMessage(result) => Some(*result),
        _ => None,
    }
}

/// Command message emitted by upstream `controlDropKey`.
pub const fn control_drop_key_command() -> ControlMessage {
    ControlMessage::DropKeyMessage
}

/// Extract the result handled by upstream `controlDropKey`.
pub const fn control_drop_key_result(message: &ControlMessage) -> Option<RecvResult> {
    match message {
        ControlMessage::DropKeyResultMessage(result) => Some(*result),
        _ => None,
    }
}

/// Command message emitted by upstream `controlGetInfo`.
pub const fn control_get_info_command() -> ControlMessage {
    ControlMessage::RequestInfoMessage
}

/// Extract the result handled by upstream `controlGetInfo`.
pub fn control_get_info_result(message: &ControlMessage) -> Option<&AgentInfo> {
    match message {
        ControlMessage::InfoMessage(info) => Some(info),
        _ => None,
    }
}

/// Final orderly close message sent by upstream server peers after a
/// successful response.
pub const fn control_server_end_message() -> ControlMessage {
    ControlMessage::EndMessage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::control_v2_protocol::ConnectionStatus;

    fn sample_agent_info() -> AgentInfo {
        AgentInfo {
            agent_info_current_bundle: None,
            agent_info_staged_key: None,
            agent_info_current_time: "2026-06-04T00:00:01Z".to_string(),
            agent_info_current_kes_period: 11,
            agent_info_bootstrap_connections: vec![
                crate::protocols::control_v2_protocol::BootstrapInfo {
                    bootstrap_info_address: "/tmp/peer.socket".to_string(),
                    bootstrap_info_status: ConnectionStatus::ConnectionUp,
                },
            ],
        }
    }

    #[test]
    fn control_receiver_requests_match_upstream_idle_branches() {
        assert_eq!(
            control_receiver_request(&ControlMessage::GenStagedKeyMessage),
            Some(ControlReceiverRequest::GenStagedKey)
        );
        assert_eq!(
            control_receiver_request(&ControlMessage::QueryStagedKeyMessage),
            Some(ControlReceiverRequest::QueryStagedKey)
        );
        assert_eq!(
            control_receiver_request(&ControlMessage::DropStagedKeyMessage),
            Some(ControlReceiverRequest::DropStagedKey)
        );
        assert_eq!(
            control_receiver_request(&ControlMessage::InstallKeyMessage(vec![1, 2])),
            Some(ControlReceiverRequest::InstallKey(vec![1, 2]))
        );
        assert_eq!(
            control_receiver_request(&ControlMessage::DropKeyMessage),
            Some(ControlReceiverRequest::DropKey)
        );
        assert_eq!(
            control_receiver_request(&ControlMessage::RequestInfoMessage),
            Some(ControlReceiverRequest::RequestInfo)
        );
        assert_eq!(control_receiver_request(&ControlMessage::EndMessage), None);
    }

    #[test]
    fn control_server_commands_match_upstream_peer_sequence_heads() {
        assert_eq!(
            control_server_initial_message(),
            ControlMessage::VersionMessage
        );
        assert_eq!(
            control_gen_key_command(),
            ControlMessage::GenStagedKeyMessage
        );
        assert_eq!(
            control_query_key_command(),
            ControlMessage::QueryStagedKeyMessage
        );
        assert_eq!(
            control_drop_staged_key_command(),
            ControlMessage::DropStagedKeyMessage
        );
        assert_eq!(
            control_install_key_command(vec![9, 9]),
            ControlMessage::InstallKeyMessage(vec![9, 9])
        );
        assert_eq!(control_drop_key_command(), ControlMessage::DropKeyMessage);
        assert_eq!(
            control_get_info_command(),
            ControlMessage::RequestInfoMessage
        );
        assert_eq!(control_server_end_message(), ControlMessage::EndMessage);
    }

    #[test]
    fn control_server_result_extractors_match_upstream_awaits() {
        assert_eq!(
            control_gen_key_result(&ControlMessage::PublicKeyMessage(Some(vec![7]))),
            Some(Some(vec![7]))
        );
        assert_eq!(
            control_install_key_result(&ControlMessage::InstallResultMessage(RecvResult::RecvOK)),
            Some(RecvResult::RecvOK)
        );
        assert_eq!(
            control_drop_key_result(&ControlMessage::DropKeyResultMessage(
                RecvResult::RecvErrorNoKey
            )),
            Some(RecvResult::RecvErrorNoKey)
        );

        let info = sample_agent_info();
        assert_eq!(
            control_get_info_result(&ControlMessage::InfoMessage(info.clone())),
            Some(&info)
        );
    }

    #[test]
    fn control_receiver_response_maps_to_upstream_message_constructors() {
        assert_eq!(
            ControlReceiverResponse::PublicKey(Some(vec![1])).into_message(),
            ControlMessage::PublicKeyMessage(Some(vec![1]))
        );
        assert_eq!(
            ControlReceiverResponse::InstallResult(RecvResult::RecvOK).into_message(),
            ControlMessage::InstallResultMessage(RecvResult::RecvOK)
        );
        assert_eq!(
            ControlReceiverResponse::DropKeyResult(RecvResult::RecvErrorNoKey).into_message(),
            ControlMessage::DropKeyResultMessage(RecvResult::RecvErrorNoKey)
        );
        let info = sample_agent_info();
        assert_eq!(
            ControlReceiverResponse::Info(info.clone()).into_message(),
            ControlMessage::InfoMessage(info)
        );
    }
}
