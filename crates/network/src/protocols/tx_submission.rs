/// States of the TxSubmission2 mini-protocol state machine.
///
/// The TxSubmission2 protocol is a pull-based protocol where the server
/// (inbound side) requests transaction identifiers and transactions from
/// the client (outbound side).  This reverses the usual agency pattern
/// (c.f. ChainSync/BlockFetch) because information flows from client to
/// server.
///
/// ```text
///  MsgInit                   MsgRequestTxIds        MsgRequestTxs
///  StInit ───► StIdle ◄───── StTxIds ────────► StIdle ◄──── StTxs
///                │              │  ▲                          │
///                │              │  │ MsgReplyTxIds            │ MsgReplyTxs
///                │              │  └──────────────            └──────────►
///                │              │
///                │              │ MsgDone (blocking only)
///                │              ▼
///                │           StDone
///                │
///                └─► MsgRequestTxIds/MsgRequestTxs ─► ...
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.TxSubmission2.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxSubmissionState {
    /// Client agency — must send `MsgInit`.
    StInit,
    /// Server agency — may send `MsgRequestTxIds` or `MsgRequestTxs`.
    StIdle,
    /// Client agency — must reply with `MsgReplyTxIds` or (if blocking)
    /// `MsgDone`.
    StTxIds {
        /// Whether this is a blocking request.
        blocking: bool,
    },
    /// Client agency — must reply with `MsgReplyTxs`.
    StTxs,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A transaction identifier paired with its serialized size in bytes.
///
/// Reference: `(txid, SizeInBytes)` tuples in TxSubmission2 protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxIdAndSize {
    /// Opaque transaction identifier.
    pub txid: Vec<u8>,
    /// Size of the serialized transaction in bytes.
    pub size: u32,
}

/// Messages of the TxSubmission2 mini-protocol.
///
/// CDDL wire tags (from upstream codec):
///
/// | Tag | Message            |
/// |-----|--------------------|
/// |  6  | `MsgInit`          |
/// |  0  | `MsgRequestTxIds`  |
/// |  1  | `MsgReplyTxIds`    |
/// |  2  | `MsgRequestTxs`    |
/// |  3  | `MsgReplyTxs`      |
/// |  4  | `MsgDone`          |
///
/// `txid` and `tx` are opaque byte vectors at this layer.
///
/// Reference: `Ouroboros.Network.Protocol.TxSubmission2.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxSubmissionMessage {
    /// `[6]` — client sends the initial message.
    ///
    /// Transition: `StInit → StIdle`.
    MsgInit,

    /// `[0, blocking, ack, req]` — server requests transaction identifiers.
    ///
    /// Transition: `StIdle → StTxIds(blocking)`.
    MsgRequestTxIds {
        /// `true` for blocking (must return non-empty reply), `false` for
        /// non-blocking (may return empty reply).
        blocking: bool,
        /// Number of outstanding transaction identifiers to acknowledge.
        ack: u16,
        /// Maximum number of new transaction identifiers to request.
        req: u16,
    },

    /// `[1, [*[txid, size]]]` — client replies with transaction identifiers.
    ///
    /// Transition: `StTxIds → StIdle`.
    MsgReplyTxIds {
        /// List of transaction identifiers and their sizes.
        txids: Vec<TxIdAndSize>,
    },

    /// `[2, [*txid]]` — server requests specific transactions by id.
    ///
    /// Transition: `StIdle → StTxs`.
    MsgRequestTxs {
        /// Transaction identifiers to fetch.
        txids: Vec<Vec<u8>>,
    },

    /// `[3, [*tx]]` — client replies with requested transactions.
    ///
    /// Transition: `StTxs → StIdle`.
    MsgReplyTxs {
        /// Serialized transactions (opaque). May omit invalid ones.
        txs: Vec<Vec<u8>>,
    },

    /// `[4]` — client terminates the protocol (only from blocking StTxIds).
    ///
    /// Transition: `StTxIds(blocking) → StDone`.
    MsgDone,
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal TxSubmission state transitions.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal TxSubmission transition: {message} not allowed in {state:?}")]
pub struct TxSubmissionTransitionError {
    pub state: TxSubmissionState,
    pub message: &'static str,
}

impl TxSubmissionState {
    /// Computes the next state given an incoming message, or returns
    /// an error if the transition is illegal.
    pub fn transition(
        self,
        msg: &TxSubmissionMessage,
    ) -> Result<Self, TxSubmissionTransitionError> {
        match (self, msg) {
            // Client agency — StInit
            (Self::StInit, TxSubmissionMessage::MsgInit) => Ok(Self::StIdle),

            // Server agency — StIdle
            (Self::StIdle, TxSubmissionMessage::MsgRequestTxIds { blocking, .. }) => {
                Ok(Self::StTxIds { blocking: *blocking })
            }
            (Self::StIdle, TxSubmissionMessage::MsgRequestTxs { .. }) => Ok(Self::StTxs),

            // Client agency — StTxIds
            (Self::StTxIds { .. }, TxSubmissionMessage::MsgReplyTxIds { .. }) => Ok(Self::StIdle),
            // MsgDone only from blocking StTxIds
            (
                Self::StTxIds { blocking: true },
                TxSubmissionMessage::MsgDone,
            ) => Ok(Self::StDone),

            // Client agency — StTxs
            (Self::StTxs, TxSubmissionMessage::MsgReplyTxs { .. }) => Ok(Self::StIdle),

            (state, msg) => Err(TxSubmissionTransitionError {
                state,
                message: msg.tag_name(),
            }),
        }
    }
}

impl TxSubmissionMessage {
    /// Human-readable tag name used in error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::MsgInit => "MsgInit",
            Self::MsgRequestTxIds { .. } => "MsgRequestTxIds",
            Self::MsgReplyTxIds { .. } => "MsgReplyTxIds",
            Self::MsgRequestTxs { .. } => "MsgRequestTxs",
            Self::MsgReplyTxs { .. } => "MsgReplyTxs",
            Self::MsgDone => "MsgDone",
        }
    }

    /// The CDDL wire tag for this message variant.
    pub fn wire_tag(&self) -> u8 {
        match self {
            Self::MsgInit => 6,
            Self::MsgRequestTxIds { .. } => 0,
            Self::MsgReplyTxIds { .. } => 1,
            Self::MsgRequestTxs { .. } => 2,
            Self::MsgReplyTxs { .. } => 3,
            Self::MsgDone => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::LedgerError;

impl TxSubmissionMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream `encodeTxSubmission2`):
    /// - `MsgInit`          → `[6]`
    /// - `MsgRequestTxIds`  → `[0, blocking, ack, req]`
    /// - `MsgReplyTxIds`    → `[1, [[txid, size], ...]]`
    /// - `MsgRequestTxs`    → `[2, [txid, ...]]`
    /// - `MsgReplyTxs`      → `[3, [tx, ...]]`
    /// - `MsgDone`          → `[4]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgInit => {
                enc.array(1).unsigned(6);
            }
            Self::MsgRequestTxIds { blocking, ack, req } => {
                enc.array(4)
                    .unsigned(0)
                    .bool(*blocking)
                    .unsigned(u64::from(*ack))
                    .unsigned(u64::from(*req));
            }
            Self::MsgReplyTxIds { txids } => {
                enc.array(2).unsigned(1);
                enc.array(txids.len() as u64);
                for item in txids {
                    enc.array(2).bytes(&item.txid).unsigned(u64::from(item.size));
                }
            }
            Self::MsgRequestTxs { txids } => {
                enc.array(2).unsigned(2);
                enc.array(txids.len() as u64);
                for txid in txids {
                    enc.bytes(txid);
                }
            }
            Self::MsgReplyTxs { txs } => {
                enc.array(2).unsigned(3);
                enc.array(txs.len() as u64);
                for tx in txs {
                    enc.bytes(tx);
                }
            }
            Self::MsgDone => {
                enc.array(1).unsigned(4);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn from_cbor(data: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr_len) {
            (6, 1) => Self::MsgInit,
            (0, 4) => {
                let blocking = dec.bool()?;
                let ack = dec.unsigned()? as u16;
                let req = dec.unsigned()? as u16;
                Self::MsgRequestTxIds { blocking, ack, req }
            }
            (1, 2) => {
                let count = dec.array()?;
                let mut txids = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let inner_len = dec.array()?;
                    if inner_len != 2 {
                        return Err(LedgerError::CborInvalidLength {
                            expected: 2,
                            actual: inner_len as usize,
                        });
                    }
                    let txid = dec.bytes()?.to_vec();
                    let size = dec.unsigned()? as u32;
                    txids.push(TxIdAndSize { txid, size });
                }
                Self::MsgReplyTxIds { txids }
            }
            (2, 2) => {
                let count = dec.array()?;
                let mut txids = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    txids.push(dec.bytes()?.to_vec());
                }
                Self::MsgRequestTxs { txids }
            }
            (3, 2) => {
                let count = dec.array()?;
                let mut txs = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    txs.push(dec.bytes()?.to_vec());
                }
                Self::MsgReplyTxs { txs }
            }
            (4, 1) => Self::MsgDone,
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: tag as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(msg)
    }
}
