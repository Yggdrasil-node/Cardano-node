/// States of the BlockFetch mini-protocol state machine.
///
/// The BlockFetch protocol lets a client request ranges of blocks from a
/// server. The server either streams the blocks or reports that no blocks are
/// available for the requested range.
///
/// ```text
///  MsgRequestRange           MsgStartBatch
///  StIdle ──────────► StBusy ──────────► StStreaming
///    ▲                  │                  │  │
///    │   MsgNoBlocks    │    MsgBlock      │  │
///    │◄─────────────────┘    (self-loop)   │  │
///    │                                     │  │
///    │              MsgBatchDone           │  │
///    │◄────────────────────────────────────┘  │
///    │                                        │
///    │  MsgClientDone                         │
///    ▼                                        │
///  StDone                                     │
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.BlockFetch.Type` —
/// `BFIdle`, `BFBusy`, `BFStreaming`, `BFDone`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockFetchState {
    /// Client agency — may send `MsgRequestRange` or `MsgClientDone`.
    StIdle,
    /// Server agency — must send `MsgStartBatch` or `MsgNoBlocks`.
    StBusy,
    /// Server agency — may send `MsgBlock` or `MsgBatchDone`.
    StStreaming,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// An inclusive range of chain points identifying a contiguous block range.
///
/// Reference: `Ouroboros.Network.Protocol.BlockFetch.Type` — `ChainRange`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainRange {
    /// Lower bound (opaque point).
    pub lower: Vec<u8>,
    /// Upper bound (opaque point).
    pub upper: Vec<u8>,
}

/// Messages of the BlockFetch mini-protocol.
///
/// CDDL wire tags (from `block-fetch.cddl`):
///
/// | Tag | Message            |
/// |-----|--------------------|
/// |  0  | `MsgRequestRange`  |
/// |  1  | `MsgClientDone`    |
/// |  2  | `MsgStartBatch`    |
/// |  3  | `MsgNoBlocks`      |
/// |  4  | `MsgBlock`         |
/// |  5  | `MsgBatchDone`     |
///
/// `block` and `point` are opaque byte vectors at this layer.
///
/// Reference: `Ouroboros.Network.Protocol.BlockFetch.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockFetchMessage {
    /// `[0, point, point]` — client requests a range of blocks.
    ///
    /// Transition: `StIdle → StBusy`.
    MsgRequestRange(ChainRange),

    /// `[1]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgClientDone,

    /// `[2]` — server begins streaming blocks.
    ///
    /// Transition: `StBusy → StStreaming`.
    MsgStartBatch,

    /// `[3]` — server has no blocks for the requested range.
    ///
    /// Transition: `StBusy → StIdle`.
    MsgNoBlocks,

    /// `[4, block]` — server streams a single block.
    ///
    /// Transition: `StStreaming → StStreaming`.
    MsgBlock {
        /// Serialized block (opaque).
        block: Vec<u8>,
    },

    /// `[5]` — server finished streaming the batch.
    ///
    /// Transition: `StStreaming → StIdle`.
    MsgBatchDone,
}

/// Error returned when a [`BlockFetchMessage`] is sent from an illegal state.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal BlockFetch transition: {message} not allowed in {state:?}")]
pub struct BlockFetchTransitionError {
    pub state: BlockFetchState,
    pub message: &'static str,
}

impl BlockFetchState {
    /// Validate that `msg` is legal from `self` and return the resulting state.
    pub fn transition(self, msg: &BlockFetchMessage) -> Result<Self, BlockFetchTransitionError> {
        match (&self, msg) {
            // Client agency — StIdle
            (Self::StIdle, BlockFetchMessage::MsgRequestRange(_)) => Ok(Self::StBusy),
            (Self::StIdle, BlockFetchMessage::MsgClientDone) => Ok(Self::StDone),

            // Server agency — StBusy
            (Self::StBusy, BlockFetchMessage::MsgStartBatch) => Ok(Self::StStreaming),
            (Self::StBusy, BlockFetchMessage::MsgNoBlocks) => Ok(Self::StIdle),

            // Server agency — StStreaming
            (Self::StStreaming, BlockFetchMessage::MsgBlock { .. }) => Ok(Self::StStreaming),
            (Self::StStreaming, BlockFetchMessage::MsgBatchDone) => Ok(Self::StIdle),

            _ => Err(BlockFetchTransitionError {
                state: self,
                message: msg.tag_name(),
            }),
        }
    }
}

impl BlockFetchMessage {
    /// Human-readable tag name used in error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::MsgRequestRange(_) => "MsgRequestRange",
            Self::MsgClientDone => "MsgClientDone",
            Self::MsgStartBatch => "MsgStartBatch",
            Self::MsgNoBlocks => "MsgNoBlocks",
            Self::MsgBlock { .. } => "MsgBlock",
            Self::MsgBatchDone => "MsgBatchDone",
        }
    }

    /// The CDDL wire tag for this message variant.
    pub fn wire_tag(&self) -> u8 {
        match self {
            Self::MsgRequestRange(_) => 0,
            Self::MsgClientDone => 1,
            Self::MsgStartBatch => 2,
            Self::MsgNoBlocks => 3,
            Self::MsgBlock { .. } => 4,
            Self::MsgBatchDone => 5,
        }
    }

    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream `block-fetch.cddl`):
    /// - `[0, point, point]` — MsgRequestRange (points inline CBOR)
    /// - `[1]` — MsgClientDone
    /// - `[2]` — MsgStartBatch
    /// - `[3]` — MsgNoBlocks
    /// - `[4, block]` — MsgBlock (block byte-wrapped)
    /// - `[5]` — MsgBatchDone
    pub fn to_cbor(&self) -> Vec<u8> {
        use yggdrasil_ledger::cbor::Encoder;

        let mut enc = Encoder::new();
        match self {
            Self::MsgRequestRange(range) => {
                enc.array(3)
                    .unsigned(0)
                    .raw(&range.lower)
                    .raw(&range.upper);
            }
            Self::MsgClientDone => {
                enc.array(1).unsigned(1);
            }
            Self::MsgStartBatch => {
                enc.array(1).unsigned(2);
            }
            Self::MsgNoBlocks => {
                enc.array(1).unsigned(3);
            }
            Self::MsgBlock { block } => {
                enc.array(2).unsigned(4).wrapped(block);
            }
            Self::MsgBatchDone => {
                enc.array(1).unsigned(5);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn from_cbor(data: &[u8]) -> Result<Self, yggdrasil_ledger::LedgerError> {
        use yggdrasil_ledger::cbor::Decoder;

        let mut dec = Decoder::new(data);
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr_len) {
            (0, 3) => Self::MsgRequestRange(ChainRange {
                lower: dec.raw_value()?.to_vec(),
                upper: dec.raw_value()?.to_vec(),
            }),
            (1, 1) => Self::MsgClientDone,
            (2, 1) => Self::MsgStartBatch,
            (3, 1) => Self::MsgNoBlocks,
            (4, 2) => Self::MsgBlock {
                block: dec.wrapped()?.to_vec(),
            },
            (5, 1) => Self::MsgBatchDone,
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
