/// States of the ChainSync mini-protocol state machine.
///
/// The ChainSync protocol lets a consumer (client) follow the chain of a
/// producer (server) by requesting the next header and finding intersection
/// points.
///
/// ```text
///        MsgRequestNext          MsgFindIntersect
///  ┌────►StIdle ─────────► StNext  StIdle ──────────► StIntersect
///  │       │                 │  │                       │  │
///  │       │MsgDone          │  │ MsgRollForward/       │  │ MsgIntersectFound/
///  │       ▼                 │  │ MsgRollBackward       │  │ MsgIntersectNotFound
///  │     StDone              │  └──────────────────────►│  └──────────────────────►
///  │                         │                          │
///  └─────────────────────────┘──────────────────────────┘
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.ChainSync.Type` —
/// `StIdle`, `StNext`, `StIntersect`, `StDone`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainSyncState {
    /// Client agency — may send `MsgRequestNext`, `MsgFindIntersect`, or
    /// `MsgDone`.
    StIdle,
    /// Server agency (can-await sub-state) — may send `MsgAwaitReply`,
    /// `MsgRollForward`, or `MsgRollBackward`.
    StCanAwait,
    /// Server agency (must-reply sub-state) — must send `MsgRollForward` or
    /// `MsgRollBackward`.
    StMustReply,
    /// Server agency — must send `MsgIntersectFound` or
    /// `MsgIntersectNotFound`.
    StIntersect,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the ChainSync mini-protocol.
///
/// CDDL wire tags (from `chain-sync.cddl`):
///
/// | Tag | Message                  |
/// |-----|--------------------------|
/// |  0  | `MsgRequestNext`         |
/// |  1  | `MsgAwaitReply`          |
/// |  2  | `MsgRollForward`         |
/// |  3  | `MsgRollBackward`        |
/// |  4  | `MsgFindIntersect`       |
/// |  5  | `MsgIntersectFound`      |
/// |  6  | `MsgIntersectNotFound`   |
/// |  7  | `MsgDone`               |
///
/// `header`, `point`, and `tip` are opaque byte vectors at this layer; they
/// will be refined when CBOR codec work lands.
///
/// Reference: `Ouroboros.Network.Protocol.ChainSync.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainSyncMessage {
    /// `[0]` — client requests the next update.
    ///
    /// Transition: `StIdle → StCanAwait`.
    MsgRequestNext,

    /// `[1]` — server tells the client to wait for the next update.
    ///
    /// Transition: `StCanAwait → StMustReply`.
    MsgAwaitReply,

    /// `[2, header, tip]` — server sends a new header to the client.
    ///
    /// Transition: `StCanAwait | StMustReply → StIdle`.
    MsgRollForward {
        /// Serialized block header (opaque).
        header: Vec<u8>,
        /// Current tip of the producer (opaque).
        tip: Vec<u8>,
    },

    /// `[3, point, tip]` — server instructs the client to roll back.
    ///
    /// Transition: `StCanAwait | StMustReply → StIdle`.
    MsgRollBackward {
        /// The point to roll back to (opaque).
        point: Vec<u8>,
        /// Current tip of the producer (opaque).
        tip: Vec<u8>,
    },

    /// `[4, [*point]]` — client asks for the best intersection.
    ///
    /// Transition: `StIdle → StIntersect`.
    MsgFindIntersect {
        /// Candidate points ordered by preference (highest slot first).
        points: Vec<Vec<u8>>,
    },

    /// `[5, point, tip]` — server found an intersection.
    ///
    /// Transition: `StIntersect → StIdle`.
    MsgIntersectFound {
        /// The intersection point (opaque).
        point: Vec<u8>,
        /// Current tip of the producer (opaque).
        tip: Vec<u8>,
    },

    /// `[6, tip]` — server did not find any intersection.
    ///
    /// Transition: `StIntersect → StIdle`.
    MsgIntersectNotFound {
        /// Current tip of the producer (opaque).
        tip: Vec<u8>,
    },

    /// `[7]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

/// Error returned when a [`ChainSyncMessage`] is sent from an illegal state.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal ChainSync transition: {message} not allowed in {state:?}")]
pub struct ChainSyncTransitionError {
    pub state: ChainSyncState,
    pub message: &'static str,
}

impl ChainSyncState {
    /// Validate that `msg` is legal from `self` and return the resulting state.
    pub fn transition(self, msg: &ChainSyncMessage) -> Result<Self, ChainSyncTransitionError> {
        match (&self, msg) {
            // Client agency — StIdle
            (Self::StIdle, ChainSyncMessage::MsgRequestNext) => Ok(Self::StCanAwait),
            (Self::StIdle, ChainSyncMessage::MsgFindIntersect { .. }) => Ok(Self::StIntersect),
            (Self::StIdle, ChainSyncMessage::MsgDone) => Ok(Self::StDone),

            // Server agency — StCanAwait
            (Self::StCanAwait, ChainSyncMessage::MsgAwaitReply) => Ok(Self::StMustReply),
            (Self::StCanAwait, ChainSyncMessage::MsgRollForward { .. }) => Ok(Self::StIdle),
            (Self::StCanAwait, ChainSyncMessage::MsgRollBackward { .. }) => Ok(Self::StIdle),

            // Server agency — StMustReply
            (Self::StMustReply, ChainSyncMessage::MsgRollForward { .. }) => Ok(Self::StIdle),
            (Self::StMustReply, ChainSyncMessage::MsgRollBackward { .. }) => Ok(Self::StIdle),

            // Server agency — StIntersect
            (Self::StIntersect, ChainSyncMessage::MsgIntersectFound { .. }) => Ok(Self::StIdle),
            (Self::StIntersect, ChainSyncMessage::MsgIntersectNotFound { .. }) => Ok(Self::StIdle),

            _ => Err(ChainSyncTransitionError {
                state: self,
                message: msg.tag_name(),
            }),
        }
    }
}

impl ChainSyncMessage {
    /// Human-readable tag name used in error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::MsgRequestNext => "MsgRequestNext",
            Self::MsgAwaitReply => "MsgAwaitReply",
            Self::MsgRollForward { .. } => "MsgRollForward",
            Self::MsgRollBackward { .. } => "MsgRollBackward",
            Self::MsgFindIntersect { .. } => "MsgFindIntersect",
            Self::MsgIntersectFound { .. } => "MsgIntersectFound",
            Self::MsgIntersectNotFound { .. } => "MsgIntersectNotFound",
            Self::MsgDone => "MsgDone",
        }
    }

    /// The CDDL wire tag for this message variant.
    pub fn wire_tag(&self) -> u8 {
        match self {
            Self::MsgRequestNext => 0,
            Self::MsgAwaitReply => 1,
            Self::MsgRollForward { .. } => 2,
            Self::MsgRollBackward { .. } => 3,
            Self::MsgFindIntersect { .. } => 4,
            Self::MsgIntersectFound { .. } => 5,
            Self::MsgIntersectNotFound { .. } => 6,
            Self::MsgDone => 7,
        }
    }

    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream `chain-sync.cddl`):
    /// - `[0]` — MsgRequestNext
    /// - `[1]` — MsgAwaitReply
    /// - `[2, header, tip]` — MsgRollForward (header byte-wrapped, tip inline)
    /// - `[3, point, tip]` — MsgRollBackward (both inline CBOR)
    /// - `[4, [*point]]` — MsgFindIntersect (points inline CBOR)
    /// - `[5, point, tip]` — MsgIntersectFound (both inline CBOR)
    /// - `[6, tip]` — MsgIntersectNotFound (tip inline CBOR)
    /// - `[7]` — MsgDone
    pub fn to_cbor(&self) -> Vec<u8> {
        use yggdrasil_ledger::cbor::Encoder;

        let mut enc = Encoder::new();
        match self {
            Self::MsgRequestNext => {
                enc.array(1).unsigned(0);
            }
            Self::MsgAwaitReply => {
                enc.array(1).unsigned(1);
            }
            Self::MsgRollForward { header, tip } => {
                enc.array(3).unsigned(2).raw(header).raw(tip);
            }
            Self::MsgRollBackward { point, tip } => {
                enc.array(3).unsigned(3).raw(point).raw(tip);
            }
            Self::MsgFindIntersect { points } => {
                enc.array(2).unsigned(4);
                enc.array(points.len() as u64);
                for p in points {
                    enc.raw(p);
                }
            }
            Self::MsgIntersectFound { point, tip } => {
                enc.array(3).unsigned(5).raw(point).raw(tip);
            }
            Self::MsgIntersectNotFound { tip } => {
                enc.array(2).unsigned(6).raw(tip);
            }
            Self::MsgDone => {
                enc.array(1).unsigned(7);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn from_cbor(data: &[u8]) -> Result<Self, yggdrasil_ledger::LedgerError> {
        use crate::protocol_size_limits::chainsync as chainsync_limits;
        use yggdrasil_ledger::cbor::{Decoder, vec_with_strict_capacity};

        let mut dec = Decoder::new(data);
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr_len) {
            (0, 1) => Self::MsgRequestNext,
            (1, 1) => Self::MsgAwaitReply,
            (2, 3) => Self::MsgRollForward {
                header: dec.raw_value()?.to_vec(),
                tip: dec.raw_value()?.to_vec(),
            },
            (3, 3) => Self::MsgRollBackward {
                point: dec.raw_value()?.to_vec(),
                tip: dec.raw_value()?.to_vec(),
            },
            (4, 2) => {
                let count = dec.array()?;
                let mut points =
                    vec_with_strict_capacity(count, chainsync_limits::INTERSECT_POINTS_MAX)?;
                for _ in 0..count {
                    points.push(dec.raw_value()?.to_vec());
                }
                Self::MsgFindIntersect { points }
            }
            (5, 3) => Self::MsgIntersectFound {
                point: dec.raw_value()?.to_vec(),
                tip: dec.raw_value()?.to_vec(),
            },
            (6, 2) => Self::MsgIntersectNotFound {
                tip: dec.raw_value()?.to_vec(),
            },
            (7, 1) => Self::MsgDone,
            _ => {
                return Err(yggdrasil_ledger::LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: tag as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(yggdrasil_ledger::LedgerError::CborTrailingBytes(
                dec.remaining(),
            ));
        }
        Ok(msg)
    }
}
