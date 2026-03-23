/// States of the LocalTxSubmission mini-protocol state machine.
///
/// The LocalTxSubmission protocol is a simple request-response exchange used
/// by local clients (wallets, tooling) to submit signed transactions to the
/// node over the Node-to-Client socket.
///
/// ```text
///  MsgSubmitTx               MsgAcceptTx
///  StIdle ──────────► StBusy ──────────► StIdle
///                      │
///                      │ MsgRejectTx(reason)
///                      └─────────────────────► StIdle
///
///  StIdle ──MsgDone──► StDone
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTxSubmissionState {
    /// Client agency — may send `MsgSubmitTx` or `MsgDone`.
    StIdle,
    /// Server agency — must reply with `MsgAcceptTx` or `MsgRejectTx`.
    StBusy,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalTxSubmission mini-protocol.
///
/// CDDL wire tags (from upstream `local-tx-submission.cddl`):
///
/// | Tag | Message        |
/// |-----|----------------|
/// |  0  | `MsgSubmitTx`  |
/// |  1  | `MsgAcceptTx`  |
/// |  2  | `MsgRejectTx`  |
/// |  3  | `MsgDone`      |
///
/// Transaction bytes and rejection reasons remain opaque at this layer;
/// the node layer decodes them per-era.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxSubmissionMessage {
    /// `[0, tx_bytes]` — client submits a serialized transaction.
    ///
    /// Transition: `StIdle → StBusy`.
    MsgSubmitTx {
        /// Raw CBOR-encoded transaction bytes.
        tx: Vec<u8>,
    },

    /// `[1]` — server accepted the transaction.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgAcceptTx,

    /// `[2, reason_bytes]` — server rejected the transaction.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgRejectTx {
        /// Opaque rejection reason (era-specific CBOR).
        reason: Vec<u8>,
    },

    /// `[3]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal LocalTxSubmission state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalTxSubmissionTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal local-tx-submission transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: LocalTxSubmissionState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl LocalTxSubmissionState {
    /// Compute the next state given a message, or return an error if the
    /// transition is illegal in the current state.
    pub fn transition(
        self,
        msg: &LocalTxSubmissionMessage,
    ) -> Result<Self, LocalTxSubmissionTransitionError> {
        match (self, msg) {
            (Self::StIdle, LocalTxSubmissionMessage::MsgSubmitTx { .. }) => Ok(Self::StBusy),
            (Self::StIdle, LocalTxSubmissionMessage::MsgDone) => Ok(Self::StDone),
            (Self::StBusy, LocalTxSubmissionMessage::MsgAcceptTx) => Ok(Self::StIdle),
            (Self::StBusy, LocalTxSubmissionMessage::MsgRejectTx { .. }) => Ok(Self::StIdle),
            (from, msg) => Err(LocalTxSubmissionTransitionError::IllegalTransition {
                from,
                msg_tag: match msg {
                    LocalTxSubmissionMessage::MsgSubmitTx { .. } => "MsgSubmitTx",
                    LocalTxSubmissionMessage::MsgAcceptTx => "MsgAcceptTx",
                    LocalTxSubmissionMessage::MsgRejectTx { .. } => "MsgRejectTx",
                    LocalTxSubmissionMessage::MsgDone => "MsgDone",
                },
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::LedgerError;

impl LocalTxSubmissionMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream CDDL):
    /// - `MsgSubmitTx`  → `[0, bytes]`
    /// - `MsgAcceptTx`  → `[1]`
    /// - `MsgRejectTx`  → `[2, bytes]`
    /// - `MsgDone`      → `[3]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgSubmitTx { tx } => {
                enc.array(2);
                enc.unsigned(0);
                enc.bytes(tx);
            }
            Self::MsgAcceptTx => {
                enc.array(1);
                enc.unsigned(1);
            }
            Self::MsgRejectTx { reason } => {
                enc.array(2);
                enc.unsigned(2);
                enc.bytes(reason);
            }
            Self::MsgDone => {
                enc.array(1);
                enc.unsigned(3);
            }
        }
        enc.finish()
    }

    /// Decode a CBOR-encoded message from wire bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let _len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 => {
                let tx = dec.bytes()?.to_vec();
                Ok(Self::MsgSubmitTx { tx })
            }
            1 => Ok(Self::MsgAcceptTx),
            2 => {
                let reason = dec.bytes()?.to_vec();
                Ok(Self::MsgRejectTx { reason })
            }
            3 => Ok(Self::MsgDone),
            tag => Err(LedgerError::CborDecodeError(format!(
                "unknown LocalTxSubmission message tag: {tag}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_accept_roundtrip() {
        let tx = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let msg = LocalTxSubmissionMessage::MsgSubmitTx { tx: tx.clone() };
        let encoded = msg.to_cbor();
        let decoded = LocalTxSubmissionMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, LocalTxSubmissionMessage::MsgSubmitTx { tx });
    }

    #[test]
    fn accept_roundtrip() {
        let msg = LocalTxSubmissionMessage::MsgAcceptTx;
        let decoded = LocalTxSubmissionMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, LocalTxSubmissionMessage::MsgAcceptTx);
    }

    #[test]
    fn reject_roundtrip() {
        let reason = vec![0x01, 0x02];
        let msg = LocalTxSubmissionMessage::MsgRejectTx { reason: reason.clone() };
        let decoded = LocalTxSubmissionMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, LocalTxSubmissionMessage::MsgRejectTx { reason });
    }

    #[test]
    fn done_roundtrip() {
        let msg = LocalTxSubmissionMessage::MsgDone;
        let decoded = LocalTxSubmissionMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, LocalTxSubmissionMessage::MsgDone);
    }

    #[test]
    fn state_machine_idle_to_busy() {
        let state = LocalTxSubmissionState::StIdle;
        let next = state
            .transition(&LocalTxSubmissionMessage::MsgSubmitTx { tx: vec![] })
            .unwrap();
        assert_eq!(next, LocalTxSubmissionState::StBusy);
    }

    #[test]
    fn state_machine_busy_to_idle_via_accept() {
        let state = LocalTxSubmissionState::StBusy;
        let next = state
            .transition(&LocalTxSubmissionMessage::MsgAcceptTx)
            .unwrap();
        assert_eq!(next, LocalTxSubmissionState::StIdle);
    }

    #[test]
    fn state_machine_busy_to_idle_via_reject() {
        let state = LocalTxSubmissionState::StBusy;
        let next = state
            .transition(&LocalTxSubmissionMessage::MsgRejectTx { reason: vec![] })
            .unwrap();
        assert_eq!(next, LocalTxSubmissionState::StIdle);
    }

    #[test]
    fn state_machine_idle_to_done() {
        let state = LocalTxSubmissionState::StIdle;
        let next = state
            .transition(&LocalTxSubmissionMessage::MsgDone)
            .unwrap();
        assert_eq!(next, LocalTxSubmissionState::StDone);
    }

    #[test]
    fn state_machine_illegal_transition() {
        let state = LocalTxSubmissionState::StIdle;
        // Can't send MsgAcceptTx in StIdle
        let res = state.transition(&LocalTxSubmissionMessage::MsgAcceptTx);
        assert!(res.is_err());
    }
}
