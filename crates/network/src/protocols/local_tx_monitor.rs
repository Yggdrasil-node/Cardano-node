//! LocalTxMonitor mini-protocol — node-to-client mempool monitoring.
//!
//! Allows a client to observe the current contents of the node's mempool.
//! The client acquires a snapshot of the mempool at a specific slot, then
//! iterates through pending transactions or queries membership and capacity.
//!
//! ## State Machine
//!
//! ```text
//!  StIdle ──MsgAcquire──► StAcquiring ──MsgAcquired──► StAcquired
//!    │                          │                           │
//!    └──MsgDone──► StDone       │ (await loop)              ├──MsgNextTx──► StBusy ──MsgReplyNextTx──► StAcquired
//!                               │                           ├──MsgHasTx──► StBusy ──MsgReplyHasTx──► StAcquired
//!                               │                           ├──MsgGetSizes──► StBusy ──MsgReplyGetSizes──► StAcquired
//!  StAcquired ──MsgAwaitAcquire──► StAcquiring (re-acquire) └──MsgRelease──► StIdle
//! ```
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Type`
//! <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxMonitor>

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// States of the LocalTxMonitor mini-protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorState {
    /// Client agency — may send `MsgAcquire`, `MsgDone`.
    StIdle,
    /// Server agency — acquiring or awaiting a new mempool snapshot.
    StAcquiring,
    /// Client agency — may query the snapshot.
    StAcquired,
    /// Server agency — responding to a query.
    StBusy,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Mempool sizes
// ---------------------------------------------------------------------------

/// Mempool capacity and current usage, as reported by `MsgReplyGetSizes`.
///
/// Reference: `LocalTxMonitor.Type.MempoolSizeAndCapacity`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MempoolSizeAndCapacity {
    /// Maximum number of bytes the mempool can hold.
    pub capacity_in_bytes: u32,
    /// Current total byte size of all transactions in the mempool.
    pub size_in_bytes: u32,
    /// Number of transactions currently in the mempool.
    pub number_of_txs: u32,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalTxMonitor mini-protocol.
///
/// CBOR wire tags (from upstream CDDL):
///
/// | Tag | Message               |
/// |-----|-----------------------|
/// |  0  | `MsgAcquire`          |
/// |  1  | `MsgAcquired`         |
/// |  2  | `MsgAwaitAcquire`     |
/// |  3  | `MsgRelease`          |
/// |  4  | `MsgNextTx`           |
/// |  5  | `MsgReplyNextTx`      |
/// |  6  | `MsgHasTx`            |
/// |  7  | `MsgReplyHasTx`       |
/// |  8  | `MsgGetSizes`         |
/// |  9  | `MsgReplyGetSizes`    |
/// | 10  | `MsgDone`             |
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Type.Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorMessage {
    /// `[0]` — client requests a new mempool snapshot.
    ///
    /// Transition: `StIdle → StAcquiring`.
    MsgAcquire,

    /// `[1, slot_no, tx_count, capacity]` — server has acquired a snapshot.
    ///
    /// Carries the slot number at which the snapshot was taken, the number
    /// of transactions, and the byte capacity of the mempool.
    ///
    /// Transition: `StAcquiring → StAcquired`.
    MsgAcquired {
        /// Slot at which the mempool snapshot was taken.
        slot_no: u64,
        /// Byte capacity of the mempool.
        mempool_capacity: u32,
    },

    /// `[2]` — client asks the server to wait until the mempool changes and
    /// then re-acquire.
    ///
    /// Transition: `StAcquired → StAcquiring`.
    MsgAwaitAcquire,

    /// `[3]` — client releases the current snapshot.
    ///
    /// Transition: `StAcquired → StIdle`.
    MsgRelease,

    /// `[4]` — client asks for the next transaction in the snapshot.
    ///
    /// Transition: `StAcquired → StBusy`.
    MsgNextTx,

    /// `[5, maybe_tx]` — server replies with the next transaction.
    ///
    /// `tx` is `None` when there are no more transactions in the snapshot.
    ///
    /// Transition: `StBusy → StAcquired`.
    MsgReplyNextTx {
        /// The next pending transaction, or `None` if the snapshot is exhausted.
        tx: Option<Vec<u8>>,
    },

    /// `[6, tx_id]` — client asks whether a specific transaction is in the snapshot.
    ///
    /// Transition: `StAcquired → StBusy`.
    MsgHasTx {
        /// Transaction ID to query (raw bytes).
        tx_id: Vec<u8>,
    },

    /// `[7, has_tx]` — server replies whether the transaction is present.
    ///
    /// Transition: `StBusy → StAcquired`.
    MsgReplyHasTx {
        /// `true` if the transaction is in the current snapshot.
        has_tx: bool,
    },

    /// `[8]` — client requests mempool size and capacity information.
    ///
    /// Transition: `StAcquired → StBusy`.
    MsgGetSizes,

    /// `[9, sizes]` — server replies with mempool size/capacity.
    ///
    /// Transition: `StBusy → StAcquired`.
    MsgReplyGetSizes {
        /// Mempool size and capacity.
        sizes: MempoolSizeAndCapacity,
    },

    /// `[10]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

impl LocalTxMonitorMessage {
    /// CBOR array tag for this message.
    pub fn tag(&self) -> u64 {
        match self {
            Self::MsgAcquire             => 0,
            Self::MsgAcquired { .. }     => 1,
            Self::MsgAwaitAcquire        => 2,
            Self::MsgRelease             => 3,
            Self::MsgNextTx              => 4,
            Self::MsgReplyNextTx { .. }  => 5,
            Self::MsgHasTx { .. }        => 6,
            Self::MsgReplyHasTx { .. }   => 7,
            Self::MsgGetSizes            => 8,
            Self::MsgReplyGetSizes { .. } => 9,
            Self::MsgDone               => 10,
        }
    }

    /// State transition: returns the new state after sending this message.
    pub fn apply(&self, current: LocalTxMonitorState) -> Option<LocalTxMonitorState> {
        use LocalTxMonitorState::*;
        match (self, current) {
            (Self::MsgAcquire,               StIdle)     => Some(StAcquiring),
            (Self::MsgAcquired { .. },       StAcquiring) => Some(StAcquired),
            (Self::MsgAwaitAcquire,          StAcquired) => Some(StAcquiring),
            (Self::MsgRelease,               StAcquired) => Some(StIdle),
            (Self::MsgNextTx,                StAcquired) => Some(StBusy),
            (Self::MsgReplyNextTx { .. },    StBusy)     => Some(StAcquired),
            (Self::MsgHasTx { .. },          StAcquired) => Some(StBusy),
            (Self::MsgReplyHasTx { .. },     StBusy)     => Some(StAcquired),
            (Self::MsgGetSizes,              StAcquired) => Some(StBusy),
            (Self::MsgReplyGetSizes { .. },  StBusy)     => Some(StAcquired),
            (Self::MsgDone,                  StIdle)     => Some(StDone),
            _ => None,
        }
    }

    /// Encode this message to CBOR bytes.
    pub fn encode_cbor(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::MsgAcquire => {
                minicbor::encode(&[0u64], &mut buf).expect("infallible");
            }
            Self::MsgAcquired { slot_no, mempool_capacity } => {
                minicbor::encode(&(1u64, slot_no, mempool_capacity), &mut buf)
                    .expect("infallible");
            }
            Self::MsgAwaitAcquire => {
                minicbor::encode(&[2u64], &mut buf).expect("infallible");
            }
            Self::MsgRelease => {
                minicbor::encode(&[3u64], &mut buf).expect("infallible");
            }
            Self::MsgNextTx => {
                minicbor::encode(&[4u64], &mut buf).expect("infallible");
            }
            Self::MsgReplyNextTx { tx } => {
                match tx {
                    None => minicbor::encode(&(5u64, ()), &mut buf).expect("infallible"),
                    Some(bytes) => minicbor::encode(
                        &(5u64, minicbor::bytes::ByteVec::from(bytes.clone())),
                        &mut buf,
                    )
                    .expect("infallible"),
                }
            }
            Self::MsgHasTx { tx_id } => {
                minicbor::encode(
                    &(6u64, minicbor::bytes::ByteVec::from(tx_id.clone())),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgReplyHasTx { has_tx } => {
                minicbor::encode(&(7u64, has_tx), &mut buf).expect("infallible");
            }
            Self::MsgGetSizes => {
                minicbor::encode(&[8u64], &mut buf).expect("infallible");
            }
            Self::MsgReplyGetSizes { sizes } => {
                minicbor::encode(
                    &(
                        9u64,
                        sizes.capacity_in_bytes,
                        sizes.size_in_bytes,
                        sizes.number_of_txs,
                    ),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgDone => {
                minicbor::encode(&[10u64], &mut buf).expect("infallible");
            }
        }
        buf
    }

    /// Decode a message from CBOR bytes.
    pub fn decode_cbor(data: &[u8]) -> Result<Self, LocalTxMonitorError> {
        let mut dec = minicbor::Decoder::new(data);
        dec.array().map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
        let tag: u64 = dec.decode().map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
        match tag {
            0  => Ok(Self::MsgAcquire),
            1  => {
                let slot_no: u64 = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                let capacity: u32 = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                Ok(Self::MsgAcquired { slot_no, mempool_capacity: capacity })
            }
            2  => Ok(Self::MsgAwaitAcquire),
            3  => Ok(Self::MsgRelease),
            4  => Ok(Self::MsgNextTx),
            5  => {
                // None if next is unit/null, Some if next is bytes
                let datatype = dec.datatype()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                let tx = if datatype == minicbor::data::Type::Null
                    || datatype == minicbor::data::Type::Undefined
                {
                    dec.skip().map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                    None
                } else {
                    let bytes: minicbor::bytes::ByteVec = dec.decode()
                        .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                    Some(bytes.into())
                };
                Ok(Self::MsgReplyNextTx { tx })
            }
            6  => {
                let id: minicbor::bytes::ByteVec = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                Ok(Self::MsgHasTx { tx_id: id.into() })
            }
            7  => {
                let has: bool = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                Ok(Self::MsgReplyHasTx { has_tx: has })
            }
            8  => Ok(Self::MsgGetSizes),
            9  => {
                let cap: u32 = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                let sz: u32 = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                let ntx: u32 = dec.decode()
                    .map_err(|e| LocalTxMonitorError::Cbor(e.to_string()))?;
                Ok(Self::MsgReplyGetSizes {
                    sizes: MempoolSizeAndCapacity {
                        capacity_in_bytes: cap,
                        size_in_bytes: sz,
                        number_of_txs: ntx,
                    },
                })
            }
            10 => Ok(Self::MsgDone),
            _  => Err(LocalTxMonitorError::UnknownTag(tag)),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the LocalTxMonitor protocol driver.
#[derive(Clone, Debug, thiserror::Error)]
pub enum LocalTxMonitorError {
    #[error("CBOR codec error: {0}")]
    Cbor(String),
    #[error("unknown message tag: {0}")]
    UnknownTag(u64),
    #[error("invalid state transition for message tag {tag} in state {state:?}")]
    InvalidTransition {
        tag: u64,
        state: LocalTxMonitorState,
    },
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
    fn msg_acquire_round_trip() {
        let msg = LocalTxMonitorMessage::MsgAcquire;
        let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_acquired_round_trip() {
        let msg = LocalTxMonitorMessage::MsgAcquired {
            slot_no: 10_000_000,
            mempool_capacity: 4_096_000,
        };
        let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_next_tx_some_round_trip() {
        let msg = LocalTxMonitorMessage::MsgReplyNextTx {
            tx: Some(vec![0x82, 0x01, 0x02]),
        };
        let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_next_tx_none_round_trip() {
        let msg = LocalTxMonitorMessage::MsgReplyNextTx { tx: None };
        let encoded = msg.encode_cbor();
        let decoded = LocalTxMonitorMessage::decode_cbor(&encoded).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_has_tx_round_trip() {
        let msg = LocalTxMonitorMessage::MsgHasTx { tx_id: vec![0xabu8; 32] };
        let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_reply_has_tx_round_trip() {
        for b in [true, false] {
            let msg = LocalTxMonitorMessage::MsgReplyHasTx { has_tx: b };
            let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn msg_get_sizes_round_trip() {
        let msg = LocalTxMonitorMessage::MsgReplyGetSizes {
            sizes: MempoolSizeAndCapacity {
                capacity_in_bytes: 4_000_000,
                size_in_bytes: 12_288,
                number_of_txs: 3,
            },
        };
        let decoded = LocalTxMonitorMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn state_transitions() {
        use LocalTxMonitorState::*;
        assert_eq!(LocalTxMonitorMessage::MsgAcquire.apply(StIdle), Some(StAcquiring));
        assert_eq!(
            LocalTxMonitorMessage::MsgAcquired { slot_no: 0, mempool_capacity: 0 }.apply(StAcquiring),
            Some(StAcquired)
        );
        assert_eq!(LocalTxMonitorMessage::MsgNextTx.apply(StAcquired), Some(StBusy));
        assert_eq!(
            LocalTxMonitorMessage::MsgReplyNextTx { tx: None }.apply(StBusy),
            Some(StAcquired)
        );
        assert_eq!(LocalTxMonitorMessage::MsgGetSizes.apply(StAcquired), Some(StBusy));
        assert_eq!(
            LocalTxMonitorMessage::MsgReplyGetSizes {
                sizes: MempoolSizeAndCapacity { capacity_in_bytes: 0, size_in_bytes: 0, number_of_txs: 0 }
            }.apply(StBusy),
            Some(StAcquired)
        );
        assert_eq!(LocalTxMonitorMessage::MsgRelease.apply(StAcquired), Some(StIdle));
        assert_eq!(LocalTxMonitorMessage::MsgDone.apply(StIdle), Some(StDone));
        // Invalid
        assert_eq!(LocalTxMonitorMessage::MsgAcquired { slot_no: 0, mempool_capacity: 0 }.apply(StIdle), None);
    }
}
