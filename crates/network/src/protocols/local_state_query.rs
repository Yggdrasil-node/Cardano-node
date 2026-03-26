//! LocalStateQuery mini-protocol — node-to-client ledger state queries.
//!
//! Allows a client to acquire a snapshot of the ledger at a specific point
//! on the chain and then issue typed queries against that snapshot.
//!
//! ## State Machine
//!
//! ```text
//!  StIdle ──MsgAcquire──► StAcquiring ──MsgAcquired──► StAcquired
//!    │                          │                           │
//!    │                          └──MsgFailure──► StIdle     │──MsgQuery──► StQuerying
//!    └──MsgDone──► StDone                                   │                   │
//!                                                           │◄──MsgResult────────
//!                                                           │──MsgReAcquire──► StAcquiring
//!                                                           └──MsgRelease──► StIdle
//! ```
//!
//! Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type`
//! <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalStateQuery>

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// States of the LocalStateQuery mini-protocol.
///
/// Reference: `LocalStateQuery.Type.St*`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalStateQueryState {
    /// Client agency — may send `MsgAcquire` or `MsgDone`.
    StIdle,
    /// Server agency — must send `MsgAcquired` or `MsgFailure`.
    StAcquiring,
    /// Client agency — may send `MsgQuery`, `MsgReAcquire`, or `MsgRelease`.
    StAcquired,
    /// Server agency — must send `MsgResult`.
    StQuerying,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Acquire target
// ---------------------------------------------------------------------------

/// The point at which to acquire a ledger snapshot.
///
/// Mirrors the upstream `Target` type from `LocalStateQuery.Type`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireTarget {
    /// Acquire at the tip of the current chain (most common for clients).
    VolatileTip,
    /// Acquire at the most recently immutable point.
    ImmutableTip,
    /// Acquire at a specific point (slot + block header hash).
    SpecificPoint {
        /// Slot number.
        slot: u64,
        /// Block header hash (32 bytes).
        hash: Vec<u8>,
    },
}

impl AcquireTarget {
    /// CBOR-encode the acquire target for the wire.
    ///
    /// Upstream encoding:
    /// - `VolatileTip`   → `[0]` (acquire target = volatile tip)
    /// - `ImmutableTip`  → `[1]` (acquire target = immutable tip)
    /// - Specific point  → `[slot_no, hash_bytes]` (as in ChainSync points)
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::VolatileTip => {
                minicbor::encode(&[0u64], &mut buf).expect("infallible");
            }
            Self::ImmutableTip => {
                minicbor::encode(&[1u64], &mut buf).expect("infallible");
            }
            Self::SpecificPoint { slot, hash } => {
                // [slot_no, #bytes(hash)]
                minicbor::encode(
                    &(*slot, minicbor::bytes::ByteVec::from(hash.clone())),
                    &mut buf,
                )
                .expect("infallible");
            }
        }
        buf
    }

    /// Decode an acquire target from wire CBOR.
    pub fn decode(data: &[u8]) -> Result<Self, LocalStateQueryError> {
        let mut dec = minicbor::Decoder::new(data);
        // Try array form first
        if let Ok(Some(len)) = dec.array() {
            if len == Some(1) {
                let tag: u64 = dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                return match tag {
                    0 => Ok(Self::VolatileTip),
                    1 => Ok(Self::ImmutableTip),
                    _ => Err(LocalStateQueryError::Cbor(format!("unknown acquire target tag {tag}"))),
                };
            }
        }
        // Specific point: tuple (slot, hash)
        let mut dec2 = minicbor::Decoder::new(data);
        let slot: u64 = dec2.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
        let hash: minicbor::bytes::ByteVec = dec2.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
        Ok(Self::SpecificPoint { slot, hash: hash.into() })
    }
}

// ---------------------------------------------------------------------------
// Acquire failure reason
// ---------------------------------------------------------------------------

/// Reason a ledger snapshot could not be acquired at the requested point.
///
/// Reference: `LocalStateQuery.Type.AcquireFailure`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcquireFailure {
    /// The requested point was not found on the current chain (stale / rolled back).
    AcquireFailurePointNotOnChain,
    /// The requested point is too old — it has already been garbage-collected.
    AcquireFailurePointTooOld,
}

impl AcquireFailure {
    fn tag(self) -> u64 {
        match self {
            Self::AcquireFailurePointNotOnChain => 0,
            Self::AcquireFailurePointTooOld => 1,
        }
    }

    fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::AcquireFailurePointNotOnChain),
            1 => Some(Self::AcquireFailurePointTooOld),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalStateQuery mini-protocol.
///
/// CBOR wire tags (from upstream CDDL):
///
/// | Tag | Message            |
/// |-----|--------------------|
/// |  0  | `MsgAcquire`       |
/// |  1  | `MsgAcquired`      |
/// |  2  | `MsgFailure`       |
/// |  3  | `MsgRelease`       |
/// |  4  | `MsgReAcquire`     |
/// |  5  | `MsgQuery`         |
/// |  6  | `MsgResult`        |
/// |  7  | `MsgDone`          |
///
/// Query and result payloads are opaque CBOR — the actual encoding depends
/// on the era and the specific query type.
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type.Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalStateQueryMessage {
    /// `[0, target]` — client requests acquisition of a ledger snapshot.
    ///
    /// Transition: `StIdle → StAcquiring`.
    MsgAcquire {
        /// The point at which to acquire the snapshot.
        target: AcquireTarget,
    },

    /// `[1]` — server has acquired the snapshot.
    ///
    /// Transition: `StAcquiring → StAcquired`.
    MsgAcquired,

    /// `[2, failure]` — server failed to acquire the snapshot.
    ///
    /// Transition: `StAcquiring → StIdle`.
    MsgFailure {
        /// Why the acquire failed.
        failure: AcquireFailure,
    },

    /// `[3]` — client releases the acquired snapshot.
    ///
    /// Transition: `StAcquired → StIdle`.
    MsgRelease,

    /// `[4, target]` — client re-acquires at a (possibly different) point
    /// without returning to `StIdle` first.
    ///
    /// Transition: `StAcquired → StAcquiring`.
    MsgReAcquire {
        /// The new point to acquire.
        target: AcquireTarget,
    },

    /// `[5, query]` — client issues a query against the acquired snapshot.
    ///
    /// The `query` bytes carry an era-tagged CBOR-encoded query.
    ///
    /// Transition: `StAcquired → StQuerying`.
    MsgQuery {
        /// Era-tagged, CBOR-encoded query payload.
        query: Vec<u8>,
    },

    /// `[6, result]` — server replies with the query result.
    ///
    /// Transition: `StQuerying → StAcquired`.
    MsgResult {
        /// Era-tagged, CBOR-encoded result payload.
        result: Vec<u8>,
    },

    /// `[7]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

impl LocalStateQueryMessage {
    /// CBOR array tag for this message.
    pub fn tag(&self) -> u64 {
        match self {
            Self::MsgAcquire { .. }   => 0,
            Self::MsgAcquired         => 1,
            Self::MsgFailure { .. }   => 2,
            Self::MsgRelease          => 3,
            Self::MsgReAcquire { .. } => 4,
            Self::MsgQuery { .. }     => 5,
            Self::MsgResult { .. }    => 6,
            Self::MsgDone             => 7,
        }
    }

    /// State transition: returns the new state after sending this message.
    pub fn apply(&self, current: LocalStateQueryState) -> Option<LocalStateQueryState> {
        use LocalStateQueryState::*;
        match (self, current) {
            (Self::MsgAcquire { .. },   StIdle)     => Some(StAcquiring),
            (Self::MsgAcquired,         StAcquiring) => Some(StAcquired),
            (Self::MsgFailure { .. },   StAcquiring) => Some(StIdle),
            (Self::MsgRelease,          StAcquired) => Some(StIdle),
            (Self::MsgReAcquire { .. }, StAcquired) => Some(StAcquiring),
            (Self::MsgQuery { .. },     StAcquired) => Some(StQuerying),
            (Self::MsgResult { .. },    StQuerying) => Some(StAcquired),
            (Self::MsgDone,             StIdle)     => Some(StDone),
            _ => None,
        }
    }

    /// Encode this message to CBOR bytes.
    pub fn encode_cbor(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::MsgAcquire { target } => {
                // [0, target_cbor]
                let target_bytes = target.encode();
                minicbor::encode(
                    &(0u64, minicbor::bytes::ByteVec::from(target_bytes)),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgAcquired => {
                minicbor::encode(&[1u64], &mut buf).expect("infallible");
            }
            Self::MsgFailure { failure } => {
                minicbor::encode(&(2u64, failure.tag()), &mut buf).expect("infallible");
            }
            Self::MsgRelease => {
                minicbor::encode(&[3u64], &mut buf).expect("infallible");
            }
            Self::MsgReAcquire { target } => {
                let target_bytes = target.encode();
                minicbor::encode(
                    &(4u64, minicbor::bytes::ByteVec::from(target_bytes)),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgQuery { query } => {
                minicbor::encode(
                    &(5u64, minicbor::bytes::ByteVec::from(query.clone())),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgResult { result } => {
                minicbor::encode(
                    &(6u64, minicbor::bytes::ByteVec::from(result.clone())),
                    &mut buf,
                )
                .expect("infallible");
            }
            Self::MsgDone => {
                minicbor::encode(&[7u64], &mut buf).expect("infallible");
            }
        }
        buf
    }

    /// Decode a message from CBOR bytes.
    pub fn decode_cbor(data: &[u8]) -> Result<Self, LocalStateQueryError> {
        let mut dec = minicbor::Decoder::new(data);
        dec.array().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
        let tag: u64 = dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
        match tag {
            0 => {
                let target_bytes: minicbor::bytes::ByteVec =
                    dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                let target = AcquireTarget::decode(&target_bytes)?;
                Ok(Self::MsgAcquire { target })
            }
            1 => Ok(Self::MsgAcquired),
            2 => {
                let ft: u64 = dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                let failure = AcquireFailure::from_tag(ft)
                    .ok_or(LocalStateQueryError::Cbor(format!("unknown failure tag {ft}")))?;
                Ok(Self::MsgFailure { failure })
            }
            3 => Ok(Self::MsgRelease),
            4 => {
                let target_bytes: minicbor::bytes::ByteVec =
                    dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                let target = AcquireTarget::decode(&target_bytes)?;
                Ok(Self::MsgReAcquire { target })
            }
            5 => {
                let q: minicbor::bytes::ByteVec =
                    dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                Ok(Self::MsgQuery { query: q.into() })
            }
            6 => {
                let r: minicbor::bytes::ByteVec =
                    dec.decode().map_err(|e| LocalStateQueryError::Cbor(e.to_string()))?;
                Ok(Self::MsgResult { result: r.into() })
            }
            7 => Ok(Self::MsgDone),
            _ => Err(LocalStateQueryError::UnknownTag(tag)),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the LocalStateQuery protocol driver.
#[derive(Clone, Debug, thiserror::Error)]
pub enum LocalStateQueryError {
    #[error("CBOR codec error: {0}")]
    Cbor(String),
    #[error("unknown message tag: {0}")]
    UnknownTag(u64),
    #[error("invalid state transition for message tag {tag} in state {state:?}")]
    InvalidTransition {
        tag: u64,
        state: LocalStateQueryState,
    },
    #[error("acquire failed: {0:?}")]
    AcquireFailed(AcquireFailure),
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
    fn msg_acquire_volatile_tip_round_trip() {
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::VolatileTip,
        };
        let decoded = LocalStateQueryMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_acquire_specific_point_round_trip() {
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::SpecificPoint {
                slot: 42_000_000,
                hash: vec![0xabu8; 32],
            },
        };
        let decoded = LocalStateQueryMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_acquired_round_trip() {
        let msg = LocalStateQueryMessage::MsgAcquired;
        let decoded = LocalStateQueryMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_failure_round_trip() {
        let msg = LocalStateQueryMessage::MsgFailure {
            failure: AcquireFailure::AcquireFailurePointTooOld,
        };
        let decoded = LocalStateQueryMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn msg_query_result_round_trip() {
        let msg = LocalStateQueryMessage::MsgQuery {
            query: vec![0x82, 0x00, 0x82, 0x01, 0x00],
        };
        let decoded = LocalStateQueryMessage::decode_cbor(&msg.encode_cbor()).unwrap();
        assert_eq!(decoded, msg);
        let result_msg = LocalStateQueryMessage::MsgResult {
            result: vec![0x83, 0x01, 0x02, 0x03],
        };
        let decoded = LocalStateQueryMessage::decode_cbor(&result_msg.encode_cbor()).unwrap();
        assert_eq!(decoded, result_msg);
    }

    #[test]
    fn state_transitions() {
        use LocalStateQueryState::*;
        assert_eq!(
            LocalStateQueryMessage::MsgAcquire { target: AcquireTarget::VolatileTip }
                .apply(StIdle),
            Some(StAcquiring)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgAcquired.apply(StAcquiring),
            Some(StAcquired)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgFailure {
                failure: AcquireFailure::AcquireFailurePointNotOnChain
            }
            .apply(StAcquiring),
            Some(StIdle)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgQuery { query: vec![] }.apply(StAcquired),
            Some(StQuerying)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgResult { result: vec![] }.apply(StQuerying),
            Some(StAcquired)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgRelease.apply(StAcquired),
            Some(StIdle)
        );
        assert_eq!(
            LocalStateQueryMessage::MsgDone.apply(StIdle),
            Some(StDone)
        );
        // Invalid
        assert_eq!(
            LocalStateQueryMessage::MsgAcquired.apply(StIdle),
            None
        );
    }
}
