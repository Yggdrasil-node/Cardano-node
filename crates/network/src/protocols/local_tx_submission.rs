//! LocalTxSubmission mini-protocol — node-to-client transaction submission.
//!
//! Allows a client (wallet, dApp) to submit a transaction to the node's mempool
//! and receive an acceptance or rejection response.
//!
//! ## State Machine
//!
//! ```text
//!  StIdle ──MsgSubmitTx──► StBusy ──MsgAcceptTx──► StIdle
//!    │                              └──MsgRejectTx──► StIdle
//!    └──MsgDone──► StDone
//! ```
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Type`
//! <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxSubmission>

use minicbor::{Decode, Encode};

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// States of the LocalTxSubmission mini-protocol.
///
/// Reference: `LocalTxSubmission.Type.StIdle` / `StBusy` / `StDone`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTxSubmissionState {
    /// Client agency — may send `MsgSubmitTx` or `MsgDone`.
    StIdle,
    /// Server agency — must send `MsgAcceptTx` or `MsgRejectTx`.
    StBusy,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalTxSubmission mini-protocol.
///
/// CBOR wire tags (from upstream CDDL):
///
/// | Tag | Message          |
/// |-----|------------------|
/// |  0  | `MsgSubmitTx`    |
/// |  1  | `MsgAcceptTx`    |
/// |  2  | `MsgRejectTx`    |
/// |  3  | `MsgDone`        |
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Type.Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxSubmissionMessage {
    /// `[0, tx]` — client submits a serialised transaction.
    ///
    /// Transition: `StIdle → StBusy`.
    MsgSubmitTx {
        /// Serialised transaction bytes (era-tagged CBOR).
        tx: Vec<u8>,
    },

    /// `[1]` — node accepted the transaction into the mempool.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgAcceptTx,

    /// `[2, reject_reason]` — node rejected the transaction.
    ///
    /// The `reject_reason` is an opaque CBOR blob encoding the ledger
    /// validation error; the exact structure depends on the era.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgRejectTx {
        /// Serialised rejection reason (era-specific CBOR).
        reject_reason: Vec<u8>,
    },

    /// `[3]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

impl LocalTxSubmissionMessage {
    /// CBOR array tag for this message.
    pub fn tag(&self) -> u64 {
        match self {
            Self::MsgSubmitTx { .. } => 0,
            Self::MsgAcceptTx => 1,
            Self::MsgRejectTx { .. } => 2,
            Self::MsgDone => 3,
        }
    }

    /// State transition: returns the new state after this message is sent
    /// from `current`.
    pub fn apply(&self, current: LocalTxSubmissionState) -> Option<LocalTxSubmissionState> {
        use LocalTxSubmissionState::*;
        match (self, current) {
            (Self::MsgSubmitTx { .. }, StIdle) => Some(StBusy),
            (Self::MsgAcceptTx, StBusy) => Some(StIdle),
            (Self::MsgRejectTx { .. }, StBusy) => Some(StIdle),
            (Self::MsgDone, StIdle) => Some(StDone),
            _ => None,
        }
    }

    /// Encode this message to CBOR bytes.
    pub fn encode_cbor(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::MsgSubmitTx { tx } => {
                // [0, tx_bytes]
                minicbor::encode(&(0u64, minicbor::bytes::ByteVec::from(tx.clone())), &mut buf)
                    .expect("infallible CBOR encode");
            }
            Self::MsgAcceptTx => {
                // [1]
                minicbor::encode(&[1u64], &mut buf).expect("infallible CBOR encode");
            }
            Self::MsgRejectTx { reject_reason } => {
                // [2, reject_reason_bytes]
                minicbor::encode(
                    &(2u64, minicbor::bytes::ByteVec::from(reject_reason.clone())),
                    &mut buf,
                )
                .expect("infallible CBOR encode");
            }
            Self::MsgDone => {
                // [3]
                minicbor::encode(&[3u64], &mut buf).expect("infallible CBOR encode");
            }
        }
        buf
    }

    /// Decode a message from CBOR bytes.
    pub fn decode_cbor(data: &[u8]) -> Result<Self, LocalTxSubmissionError> {
        let mut decoder = minicbor::Decoder::new(data);
        let len = decoder
            .array()
            .map_err(|e| LocalTxSubmissionError::Cbor(e.to_string()))?;
        let tag: u64 = decoder
            .decode()
            .map_err(|e| LocalTxSubmissionError::Cbor(e.to_string()))?;
        match tag {
            0 => {
                let tx: minicbor::bytes::ByteVec = decoder
                    .decode()
                    .map_err(|e| LocalTxSubmissionError::Cbor(e.to_string()))?;
                Ok(Self::MsgSubmitTx { tx: tx.into() })
            }
            1 => Ok(Self::MsgAcceptTx),
            2 => {
                let rr: minicbor::bytes::ByteVec = decoder
                    .decode()
                    .map_err(|e| LocalTxSubmissionError::Cbor(e.to_string()))?;
                Ok(Self::MsgRejectTx { reject_reason: rr.into() })
            }
            3 => Ok(Self::MsgDone),
            _ => Err(LocalTxSubmissionError::UnknownTag(tag)),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the LocalTxSubmission protocol driver.
#[derive(Clone, Debug, thiserror::Error)]
pub enum LocalTxSubmissionError {
    #[error("CBOR codec error: {0}")]
    Cbor(String),
    #[error("unknown message tag: {0}")]
    UnknownTag(u64),
    #[error("invalid state transition for message tag {tag} in state {state:?}")]
    InvalidTransition {
        tag: u64,
        state: LocalTxSubmissionState,
    },
    #[error("channel send error")]
    ChannelSend,
    #[error("channel closed (peer disconnected)")]
    ChannelClosed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_submit_tx_round_trip() {
        let tx = vec![0xde, 0xad, 0xbe, 0xef];
        let msg = LocalTxSubmissionMessage::MsgSubmitTx { tx: tx.clone() };
        let encoded = msg.encode_cbor();
        let decoded = LocalTxSubmissionMessage::decode_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_accept_tx_round_trip() {
        let msg = LocalTxSubmissionMessage::MsgAcceptTx;
        let decoded = LocalTxSubmissionMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_reject_tx_round_trip() {
        let msg = LocalTxSubmissionMessage::MsgRejectTx {
            reject_reason: vec![0x82, 0x00, 0x01],
        };
        let decoded = LocalTxSubmissionMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_done_round_trip() {
        let msg = LocalTxSubmissionMessage::MsgDone;
        let decoded = LocalTxSubmissionMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn state_transitions() {
        use LocalTxSubmissionState::*;
        assert_eq!(
            LocalTxSubmissionMessage::MsgSubmitTx { tx: vec![] }.apply(StIdle),
            Some(StBusy)
        );
        assert_eq!(
            LocalTxSubmissionMessage::MsgAcceptTx.apply(StBusy),
            Some(StIdle)
        );
        assert_eq!(
            LocalTxSubmissionMessage::MsgRejectTx { reject_reason: vec![] }.apply(StBusy),
            Some(StIdle)
        );
        assert_eq!(
            LocalTxSubmissionMessage::MsgDone.apply(StIdle),
            Some(StDone)
        );
        // Invalid transitions
        assert_eq!(
            LocalTxSubmissionMessage::MsgAcceptTx.apply(StIdle),
            None
        );
    }
}
