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

use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::LedgerError;

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
/// Upstream encoding (`encodeTarget`):
/// - `VolatileTip`    → CBOR `null`
/// - `ImmutableTip`   → CBOR `0` (uint)
/// - `SpecificPoint`  → CBOR `[slot, #bytes(hash)]`
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type.Target`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireTarget {
    /// Acquire at the tip of the current volatile chain (most common for clients).
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

/// Encode an `AcquireTarget` inline into `enc`.
///
/// - `VolatileTip`   → null
/// - `ImmutableTip`  → 0
/// - `SpecificPoint` → `[slot, #bytes(hash)]`
fn encode_target(enc: &mut Encoder, target: &AcquireTarget) {
    match target {
        AcquireTarget::VolatileTip => {
            enc.null();
        }
        AcquireTarget::ImmutableTip => {
            enc.unsigned(0);
        }
        AcquireTarget::SpecificPoint { slot, hash } => {
            enc.array(2).unsigned(*slot).bytes(hash);
        }
    }
}

/// Decode an `AcquireTarget` from the current position in `dec`.
fn decode_target(dec: &mut Decoder<'_>) -> Result<AcquireTarget, LocalStateQueryError> {
    let cbor_err = |e: LedgerError| LocalStateQueryError::Cbor(e.to_string());
    if dec.peek_is_null() {
        dec.null().map_err(cbor_err)?;
        return Ok(AcquireTarget::VolatileTip);
    }
    // Distinguish ImmutableTip (uint 0) from SpecificPoint (array)
    let major = dec.peek_major().map_err(cbor_err)?;
    match major {
        0 => {
            // uint → ImmutableTip
            let _v = dec.unsigned().map_err(cbor_err)?;
            Ok(AcquireTarget::ImmutableTip)
        }
        4 => {
            // array → SpecificPoint [slot, hash]
            let _len = dec.array().map_err(cbor_err)?;
            let slot = dec.unsigned().map_err(cbor_err)?;
            let hash = dec.bytes().map_err(cbor_err)?.to_vec();
            Ok(AcquireTarget::SpecificPoint { slot, hash })
        }
        other => Err(LocalStateQueryError::Cbor(format!(
            "unexpected CBOR major type {other} for AcquireTarget"
        ))),
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
            (Self::MsgAcquire { .. },   StIdle)      => Some(StAcquiring),
            (Self::MsgAcquired,         StAcquiring) => Some(StAcquired),
            (Self::MsgFailure { .. },   StAcquiring) => Some(StIdle),
            (Self::MsgRelease,          StAcquired)  => Some(StIdle),
            (Self::MsgReAcquire { .. }, StAcquired)  => Some(StAcquiring),
            (Self::MsgQuery { .. },     StAcquired)  => Some(StQuerying),
            (Self::MsgResult { .. },    StQuerying)  => Some(StAcquired),
            (Self::MsgDone,             StIdle)      => Some(StDone),
            _ => None,
        }
    }

    /// Encode this message to CBOR bytes.
    ///
    /// Wire format:
    /// - `MsgAcquire`   → `[0, target]`  (target encoding: null / 0 / [slot, hash])
    /// - `MsgAcquired`  → `[1]`
    /// - `MsgFailure`   → `[2, failure_tag]`
    /// - `MsgRelease`   → `[3]`
    /// - `MsgReAcquire` → `[4, target]`
    /// - `MsgQuery`     → `[5, #bytes(query)]`
    /// - `MsgResult`    → `[6, #bytes(result)]`
    /// - `MsgDone`      → `[7]`
    pub fn encode_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgAcquire { target } => {
                enc.array(2).unsigned(0);
                encode_target(&mut enc, target);
            }
            Self::MsgAcquired => {
                enc.array(1).unsigned(1);
            }
            Self::MsgFailure { failure } => {
                enc.array(2).unsigned(2).unsigned(failure.tag());
            }
            Self::MsgRelease => {
                enc.array(1).unsigned(3);
            }
            Self::MsgReAcquire { target } => {
                enc.array(2).unsigned(4);
                encode_target(&mut enc, target);
            }
            Self::MsgQuery { query } => {
                enc.array(2).unsigned(5).bytes(query);
            }
            Self::MsgResult { result } => {
                enc.array(2).unsigned(6).bytes(result);
            }
            Self::MsgDone => {
                enc.array(1).unsigned(7);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn decode_cbor(data: &[u8]) -> Result<Self, LocalStateQueryError> {
        let cbor_err = |e: LedgerError| LocalStateQueryError::Cbor(e.to_string());
        let mut dec = Decoder::new(data);
        let len = dec.array().map_err(cbor_err)?;
        let tag = dec.unsigned().map_err(cbor_err)?;
        match (tag, len) {
            (0, 2) => {
                let target = decode_target(&mut dec)?;
                Ok(Self::MsgAcquire { target })
            }
            (1, 1) => Ok(Self::MsgAcquired),
            (2, 2) => {
                let ft = dec.unsigned().map_err(cbor_err)?;
                let failure = AcquireFailure::from_tag(ft).ok_or_else(|| {
                    LocalStateQueryError::Cbor(format!("unknown acquire failure tag {ft}"))
                })?;
                Ok(Self::MsgFailure { failure })
            }
            (3, 1) => Ok(Self::MsgRelease),
            (4, 2) => {
                let target = decode_target(&mut dec)?;
                Ok(Self::MsgReAcquire { target })
            }
            (5, 2) => {
                let query = dec.bytes().map_err(cbor_err)?.to_vec();
                Ok(Self::MsgQuery { query })
            }
            (6, 2) => {
                let result = dec.bytes().map_err(cbor_err)?.to_vec();
                Ok(Self::MsgResult { result })
            }
            (7, 1) => Ok(Self::MsgDone),
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

impl From<LedgerError> for LocalStateQueryError {
    fn from(e: LedgerError) -> Self {
        Self::Cbor(e.to_string())
    }
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
    fn msg_acquire_immutable_tip_round_trip() {
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::ImmutableTip,
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
        assert_eq!(LocalStateQueryMessage::MsgAcquired.apply(StIdle), None);
    }
}
