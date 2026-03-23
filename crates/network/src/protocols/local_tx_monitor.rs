/// States of the LocalTxMonitor mini-protocol state machine.
///
/// The LocalTxMonitor protocol allows local clients (wallets, tooling) to
/// inspect the node's mempool over the Node-to-Client socket.  A client
/// first *acquires* a consistent snapshot of the mempool, then can iterate
/// over transactions (`NextTx`), check membership (`HasTx`), or query
/// aggregate sizes (`GetSizes`) before releasing.
///
/// ```text
///  StIdle ──MsgAcquire──► StAcquiring ──MsgAcquired──► StAcquired
///                                                       │  ▲
///       ┌───────────── MsgRelease ──────────────────────┘  │
///       ▼                                                  │
///  StIdle                                                  │
///       │   MsgNextTx / MsgReplyNextTx ────────────────────┤
///       │   MsgHasTx  / MsgReplyHasTx  ────────────────────┤
///       │   MsgGetSizes / MsgReplyGetSizes ────────────────┘
///       │
///       └──MsgDone──► StDone
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorState {
    /// Client agency — may send `MsgAcquire` or `MsgDone`.
    StIdle,
    /// Server agency — must reply with `MsgAcquired`.
    StAcquiring,
    /// Client agency — may send `MsgNextTx`, `MsgHasTx`, `MsgGetSizes`,
    /// `MsgAwaitAcquire`, or `MsgRelease`.
    StAcquired,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalTxMonitor mini-protocol.
///
/// CDDL wire tags (from upstream `local-tx-monitor.cddl`):
///
/// | Tag | Message              |
/// |-----|----------------------|
/// |  0  | `MsgAcquire`         |
/// |  1  | `MsgAcquired`        |
/// |  2  | `MsgNextTx`          |
/// |  3  | `MsgReplyNextTx`     |
/// |  4  | `MsgHasTx`           |
/// |  5  | `MsgReplyHasTx`      |
/// |  6  | `MsgGetSizes`        |
/// |  7  | `MsgReplyGetSizes`   |
/// |  8  | `MsgRelease`         |
/// |  9  | `MsgDone`            |
///
/// Transaction identifiers and bodies remain opaque at this layer.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorMessage {
    /// `[0]` — client requests a mempool snapshot.
    ///
    /// Also used for re-acquiring (`MsgAwaitAcquire`) from `StAcquired`.
    ///
    /// Transition: `StIdle → StAcquiring` or `StAcquired → StAcquiring`.
    MsgAcquire,

    /// `[1, slot_no]` — server confirms snapshot acquired at a given slot.
    ///
    /// Transition: `StAcquiring → StAcquired`.
    MsgAcquired {
        /// Slot at which the mempool snapshot was taken.
        slot_no: u64,
    },

    /// `[2]` — client requests the next transaction in the snapshot.
    ///
    /// Transition: `StAcquired → StAcquired` (server replies with
    /// `MsgReplyNextTx`).
    MsgNextTx,

    /// `[3, maybe_tx]` — server replies with the next tx or `None`.
    ///
    /// When the iterator is exhausted, `tx` is `None`.
    ///
    /// Transition: `StAcquired → StAcquired`.
    MsgReplyNextTx {
        /// `Some(cbor_bytes)` for the next transaction, or `None` when done.
        tx: Option<Vec<u8>>,
    },

    /// `[4, tx_id]` — client asks whether a tx id is in the snapshot.
    ///
    /// Transition: `StAcquired → StAcquired` (server replies with
    /// `MsgReplyHasTx`).
    MsgHasTx {
        /// Transaction identifier (32-byte Blake2b-256 hash).
        tx_id: Vec<u8>,
    },

    /// `[5, bool]` — server replies whether the tx was found.
    ///
    /// Transition: `StAcquired → StAcquired`.
    MsgReplyHasTx {
        /// `true` if the transaction is in the mempool snapshot.
        has_tx: bool,
    },

    /// `[6]` — client requests aggregate mempool sizes.
    ///
    /// Transition: `StAcquired → StAcquired` (server replies with
    /// `MsgReplyGetSizes`).
    MsgGetSizes,

    /// `[7, [capacity, size, num_txs]]` — server replies with mempool metrics.
    ///
    /// Transition: `StAcquired → StAcquired`.
    MsgReplyGetSizes {
        /// Maximum mempool capacity in bytes.
        capacity_in_bytes: u32,
        /// Current aggregate size of all mempool transactions in bytes.
        size_in_bytes: u32,
        /// Number of transactions currently in the mempool.
        num_txs: u32,
    },

    /// `[8]` — client releases the acquired snapshot and returns to idle.
    ///
    /// Transition: `StAcquired → StIdle`.
    MsgRelease,

    /// `[9]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal LocalTxMonitor state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalTxMonitorTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal local-tx-monitor transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: LocalTxMonitorState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl LocalTxMonitorState {
    /// Compute the next state given a message, or return an error if the
    /// transition is illegal in the current state.
    pub fn transition(
        self,
        msg: &LocalTxMonitorMessage,
    ) -> Result<Self, LocalTxMonitorTransitionError> {
        match (self, msg) {
            // StIdle → StAcquiring (MsgAcquire)
            (Self::StIdle, LocalTxMonitorMessage::MsgAcquire) => Ok(Self::StAcquiring),
            // StIdle → StDone (MsgDone)
            (Self::StIdle, LocalTxMonitorMessage::MsgDone) => Ok(Self::StDone),
            // StAcquiring → StAcquired (MsgAcquired)
            (Self::StAcquiring, LocalTxMonitorMessage::MsgAcquired { .. }) => Ok(Self::StAcquired),
            // StAcquired → StAcquired (query / reply)
            (Self::StAcquired, LocalTxMonitorMessage::MsgNextTx) => Ok(Self::StAcquired),
            (Self::StAcquired, LocalTxMonitorMessage::MsgReplyNextTx { .. }) => {
                Ok(Self::StAcquired)
            }
            (Self::StAcquired, LocalTxMonitorMessage::MsgHasTx { .. }) => Ok(Self::StAcquired),
            (Self::StAcquired, LocalTxMonitorMessage::MsgReplyHasTx { .. }) => {
                Ok(Self::StAcquired)
            }
            (Self::StAcquired, LocalTxMonitorMessage::MsgGetSizes) => Ok(Self::StAcquired),
            (Self::StAcquired, LocalTxMonitorMessage::MsgReplyGetSizes { .. }) => {
                Ok(Self::StAcquired)
            }
            // StAcquired → StIdle (MsgRelease)
            (Self::StAcquired, LocalTxMonitorMessage::MsgRelease) => Ok(Self::StIdle),
            // StAcquired → StAcquiring (MsgAcquire = re-acquire / await)
            (Self::StAcquired, LocalTxMonitorMessage::MsgAcquire) => Ok(Self::StAcquiring),
            // Everything else is illegal.
            (from, msg) => Err(LocalTxMonitorTransitionError::IllegalTransition {
                from,
                msg_tag: msg.tag_name(),
            }),
        }
    }
}

impl LocalTxMonitorMessage {
    /// Human-readable name for the message variant (for error reporting).
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::MsgAcquire => "MsgAcquire",
            Self::MsgAcquired { .. } => "MsgAcquired",
            Self::MsgNextTx => "MsgNextTx",
            Self::MsgReplyNextTx { .. } => "MsgReplyNextTx",
            Self::MsgHasTx { .. } => "MsgHasTx",
            Self::MsgReplyHasTx { .. } => "MsgReplyHasTx",
            Self::MsgGetSizes => "MsgGetSizes",
            Self::MsgReplyGetSizes { .. } => "MsgReplyGetSizes",
            Self::MsgRelease => "MsgRelease",
            Self::MsgDone => "MsgDone",
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::LedgerError;

impl LocalTxMonitorMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream CDDL):
    /// - `MsgAcquire`          → `[0]`
    /// - `MsgAcquired`         → `[1, slot_no]`
    /// - `MsgNextTx`           → `[2]`
    /// - `MsgReplyNextTx`      → `[3, [era, tx]]` or `[3]`
    /// - `MsgHasTx`            → `[4, tx_id]`
    /// - `MsgReplyHasTx`       → `[5, bool]`
    /// - `MsgGetSizes`         → `[6]`
    /// - `MsgReplyGetSizes`    → `[7, [cap, size, n]]`
    /// - `MsgRelease`          → `[8]`
    /// - `MsgDone`             → `[9]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgAcquire => {
                enc.array(1);
                enc.unsigned(0);
            }
            Self::MsgAcquired { slot_no } => {
                enc.array(2);
                enc.unsigned(1);
                enc.unsigned(*slot_no);
            }
            Self::MsgNextTx => {
                enc.array(1);
                enc.unsigned(2);
            }
            Self::MsgReplyNextTx { tx: Some(tx_bytes) } => {
                enc.array(2);
                enc.unsigned(3);
                enc.bytes(tx_bytes);
            }
            Self::MsgReplyNextTx { tx: None } => {
                enc.array(1);
                enc.unsigned(3);
            }
            Self::MsgHasTx { tx_id } => {
                enc.array(2);
                enc.unsigned(4);
                enc.bytes(tx_id);
            }
            Self::MsgReplyHasTx { has_tx } => {
                enc.array(2);
                enc.unsigned(5);
                enc.bool(*has_tx);
            }
            Self::MsgGetSizes => {
                enc.array(1);
                enc.unsigned(6);
            }
            Self::MsgReplyGetSizes {
                capacity_in_bytes,
                size_in_bytes,
                num_txs,
            } => {
                enc.array(2);
                enc.unsigned(7);
                enc.array(3);
                enc.unsigned(*capacity_in_bytes as u64);
                enc.unsigned(*size_in_bytes as u64);
                enc.unsigned(*num_txs as u64);
            }
            Self::MsgRelease => {
                enc.array(1);
                enc.unsigned(8);
            }
            Self::MsgDone => {
                enc.array(1);
                enc.unsigned(9);
            }
        }
        enc.into_bytes()
    }

    /// Decode a CBOR-encoded message from wire bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 => Ok(Self::MsgAcquire),
            1 => {
                let slot_no = dec.unsigned()?;
                Ok(Self::MsgAcquired { slot_no })
            }
            2 => Ok(Self::MsgNextTx),
            3 => {
                if len > 1 {
                    let tx_bytes = dec.bytes()?.to_vec();
                    Ok(Self::MsgReplyNextTx {
                        tx: Some(tx_bytes),
                    })
                } else {
                    Ok(Self::MsgReplyNextTx { tx: None })
                }
            }
            4 => {
                let tx_id = dec.bytes()?.to_vec();
                Ok(Self::MsgHasTx { tx_id })
            }
            5 => {
                let has_tx = dec.bool()?;
                Ok(Self::MsgReplyHasTx { has_tx })
            }
            6 => Ok(Self::MsgGetSizes),
            7 => {
                let _inner_len = dec.array()?;
                let capacity_in_bytes = dec.unsigned()? as u32;
                let size_in_bytes = dec.unsigned()? as u32;
                let num_txs = dec.unsigned()? as u32;
                Ok(Self::MsgReplyGetSizes {
                    capacity_in_bytes,
                    size_in_bytes,
                    num_txs,
                })
            }
            8 => Ok(Self::MsgRelease),
            9 => Ok(Self::MsgDone),
            tag => Err(LedgerError::CborDecodeError(format!(
                "unknown LocalTxMonitor message tag: {tag}"
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
    fn acquire_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgAcquire;
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn acquired_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgAcquired { slot_no: 42_000_000 };
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn next_tx_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgNextTx;
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn reply_next_tx_some_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgReplyNextTx {
            tx: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        };
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn reply_next_tx_none_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgReplyNextTx { tx: None };
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn has_tx_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgHasTx {
            tx_id: vec![0xAB; 32],
        };
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn reply_has_tx_roundtrip() {
        for val in [true, false] {
            let msg = LocalTxMonitorMessage::MsgReplyHasTx { has_tx: val };
            let encoded = msg.to_cbor();
            let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn get_sizes_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgGetSizes;
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn reply_get_sizes_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgReplyGetSizes {
            capacity_in_bytes: 1_048_576,
            size_in_bytes: 524_288,
            num_txs: 42,
        };
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn release_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgRelease;
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn done_roundtrip() {
        let msg = LocalTxMonitorMessage::MsgDone;
        let encoded = msg.to_cbor();
        let decoded = LocalTxMonitorMessage::from_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    // -- State machine transition tests --

    #[test]
    fn idle_acquire() {
        let state = LocalTxMonitorState::StIdle;
        let next = state.transition(&LocalTxMonitorMessage::MsgAcquire).unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquiring);
    }

    #[test]
    fn idle_done() {
        let state = LocalTxMonitorState::StIdle;
        let next = state.transition(&LocalTxMonitorMessage::MsgDone).unwrap();
        assert_eq!(next, LocalTxMonitorState::StDone);
    }

    #[test]
    fn acquiring_acquired() {
        let state = LocalTxMonitorState::StAcquiring;
        let next = state
            .transition(&LocalTxMonitorMessage::MsgAcquired { slot_no: 1 })
            .unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquired);
    }

    #[test]
    fn acquired_next_tx() {
        let state = LocalTxMonitorState::StAcquired;
        let next = state.transition(&LocalTxMonitorMessage::MsgNextTx).unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquired);
    }

    #[test]
    fn acquired_has_tx() {
        let state = LocalTxMonitorState::StAcquired;
        let next = state
            .transition(&LocalTxMonitorMessage::MsgHasTx {
                tx_id: vec![0; 32],
            })
            .unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquired);
    }

    #[test]
    fn acquired_get_sizes() {
        let state = LocalTxMonitorState::StAcquired;
        let next = state
            .transition(&LocalTxMonitorMessage::MsgGetSizes)
            .unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquired);
    }

    #[test]
    fn acquired_release() {
        let state = LocalTxMonitorState::StAcquired;
        let next = state
            .transition(&LocalTxMonitorMessage::MsgRelease)
            .unwrap();
        assert_eq!(next, LocalTxMonitorState::StIdle);
    }

    #[test]
    fn acquired_reacquire() {
        let state = LocalTxMonitorState::StAcquired;
        let next = state
            .transition(&LocalTxMonitorMessage::MsgAcquire)
            .unwrap();
        assert_eq!(next, LocalTxMonitorState::StAcquiring);
    }

    #[test]
    fn illegal_done_from_acquired() {
        let state = LocalTxMonitorState::StAcquired;
        let result = state.transition(&LocalTxMonitorMessage::MsgDone);
        assert!(result.is_err());
    }

    #[test]
    fn illegal_next_tx_from_idle() {
        let state = LocalTxMonitorState::StIdle;
        let result = state.transition(&LocalTxMonitorMessage::MsgNextTx);
        assert!(result.is_err());
    }
}
