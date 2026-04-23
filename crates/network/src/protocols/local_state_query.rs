/// States of the LocalStateQuery mini-protocol state machine.
///
/// The LocalStateQuery protocol lets a local client acquire a ledger-state
/// snapshot at a specific chain point (or the current volatile tip) and then
/// issue one or more queries against that snapshot before releasing it.
///
/// ```text
///  MsgAcquire(point) / MsgAcquireVolatileTip
///  StIdle ──────────────────────────────────► StAcquiring
///    ▲                                               │ MsgAcquired
///    │                                               ▼
///    │  MsgRelease                            StAcquired ──MsgQuery──► StQuerying
///    └──────────────────────────────────────────────────                    │ MsgResult
///                                                    ◄─────────────────────────
///    ▲  MsgReAcquire(point)
///    │  ──────────────────────────────── StAcquiring (again)
///
///  StIdle ──MsgDone──► StDone
///  StAcquiring ──MsgFailure──► StIdle
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalStateQueryState {
    /// Client agency — may acquire, release, or terminate.
    StIdle,
    /// Server agency — must respond with `MsgAcquired` or `MsgFailure`.
    StAcquiring,
    /// Client agency — may query, release, or re-acquire.
    StAcquired,
    /// Server agency — must reply with `MsgResult`.
    StQuerying,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Query acquire target
// ---------------------------------------------------------------------------

/// The chain point the client wants to acquire for state queries.
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type` — `Target`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireTarget {
    /// Acquire the state at a specific chain point (slot + block hash).
    ///
    /// The raw bytes are the CBOR-encoded `Point`.
    Point(Vec<u8>),
    /// Acquire the current volatile tip (the most recently applied block).
    VolatileTip,
}

// ---------------------------------------------------------------------------
// Acquire failure reason
// ---------------------------------------------------------------------------

/// Why acquiring a chain-point snapshot failed.
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type` — `AcquireFailure`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcquireFailure {
    /// The requested point is older than the immutable tip.
    ///
    /// Wire tag: `0`.
    PointTooOld,
    /// The requested point is not on the current chain.
    ///
    /// Wire tag: `1`.
    PointNotOnChain,
}

impl std::fmt::Display for AcquireFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PointTooOld => f.write_str("point too old (older than the immutable tip)"),
            Self::PointNotOnChain => f.write_str("point not on current chain"),
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the LocalStateQuery mini-protocol.
///
/// CDDL wire tags (from upstream `local-state-query.cddl`):
///
/// | Tag | Message                  |
/// |-----|--------------------------|
/// |  0  | `MsgAcquire(point)`      |
/// |  1  | `MsgAcquired`            |
/// |  2  | `MsgFailure(reason)`     |
/// |  3  | `MsgQuery(query_bytes)`  |
/// |  4  | `MsgResult(result_bytes)`|
/// |  5  | `MsgRelease`             |
/// |  6  | `MsgReAcquire(point)`    |
/// |  7  | `MsgDone`                |
/// |  9  | `MsgAcquireVolatileTip`  |
/// | 10  | `MsgReAcquireVolatileTip`|
///
/// Query and result payloads remain opaque bytes at this layer; the node
/// layer decodes them per query type.
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalStateQueryMessage {
    /// `[0, point]` — client requests state at a specific point.
    ///
    /// Transition: `StIdle → StAcquiring`.
    MsgAcquire {
        /// CBOR-encoded target chain point.
        target: AcquireTarget,
    },

    /// `[1]` — server signals the snapshot is ready.
    ///
    /// Transition: `StAcquiring → StAcquired`.
    MsgAcquired,

    /// `[2, reason]` — server could not acquire the requested snapshot.
    ///
    /// Transition: `StAcquiring → StIdle`.
    MsgFailure {
        /// Reason the acquire failed.
        reason: AcquireFailure,
    },

    /// `[3, query_bytes]` — client issues a query against the acquired snapshot.
    ///
    /// Transition: `StAcquired → StQuerying`.
    MsgQuery {
        /// Opaque CBOR-encoded query payload.
        query: Vec<u8>,
    },

    /// `[4, result_bytes]` — server delivers the query result.
    ///
    /// Transition: `StQuerying → StAcquired`.
    MsgResult {
        /// Opaque CBOR-encoded result payload.
        result: Vec<u8>,
    },

    /// `[5]` — client releases the snapshot and returns to idle.
    ///
    /// Transition: `StAcquired → StIdle`.
    MsgRelease,

    /// `[6, point]` or `[10]` — client re-acquires at a new point.
    ///
    /// Transition: `StAcquired → StAcquiring`.
    MsgReAcquire {
        /// New target for re-acquisition.
        target: AcquireTarget,
    },

    /// `[7]` — client terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal LocalStateQuery state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalStateQueryTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal local-state-query transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: LocalStateQueryState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl LocalStateQueryState {
    /// Compute the next state given a message, or return an error if the
    /// transition is illegal in the current state.
    pub fn transition(
        self,
        msg: &LocalStateQueryMessage,
    ) -> Result<Self, LocalStateQueryTransitionError> {
        match (self, msg) {
            (Self::StIdle, LocalStateQueryMessage::MsgAcquire { .. }) => Ok(Self::StAcquiring),
            (Self::StIdle, LocalStateQueryMessage::MsgDone) => Ok(Self::StDone),
            (Self::StAcquiring, LocalStateQueryMessage::MsgAcquired) => Ok(Self::StAcquired),
            (Self::StAcquiring, LocalStateQueryMessage::MsgFailure { .. }) => Ok(Self::StIdle),
            (Self::StAcquired, LocalStateQueryMessage::MsgQuery { .. }) => Ok(Self::StQuerying),
            (Self::StAcquired, LocalStateQueryMessage::MsgRelease) => Ok(Self::StIdle),
            (Self::StAcquired, LocalStateQueryMessage::MsgReAcquire { .. }) => {
                Ok(Self::StAcquiring)
            }
            (Self::StQuerying, LocalStateQueryMessage::MsgResult { .. }) => Ok(Self::StAcquired),
            (from, msg) => Err(LocalStateQueryTransitionError::IllegalTransition {
                from,
                msg_tag: match msg {
                    LocalStateQueryMessage::MsgAcquire { .. } => "MsgAcquire",
                    LocalStateQueryMessage::MsgAcquired => "MsgAcquired",
                    LocalStateQueryMessage::MsgFailure { .. } => "MsgFailure",
                    LocalStateQueryMessage::MsgQuery { .. } => "MsgQuery",
                    LocalStateQueryMessage::MsgResult { .. } => "MsgResult",
                    LocalStateQueryMessage::MsgRelease => "MsgRelease",
                    LocalStateQueryMessage::MsgReAcquire { .. } => "MsgReAcquire",
                    LocalStateQueryMessage::MsgDone => "MsgDone",
                },
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

impl LocalStateQueryMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream CDDL):
    /// - `MsgAcquire(point)`        → `[0, point_cbor]`
    /// - `MsgAcquireVolatileTip`    → `[9]`
    /// - `MsgAcquired`              → `[1]`
    /// - `MsgFailure(PointTooOld)`  → `[2, 0]`
    /// - `MsgFailure(PointNotOnChain)` → `[2, 1]`
    /// - `MsgQuery(bytes)`          → `[3, bytes]`
    /// - `MsgResult(bytes)`         → `[4, bytes]`
    /// - `MsgRelease`               → `[5]`
    /// - `MsgReAcquire(point)`      → `[6, point_cbor]`
    /// - `MsgReAcquireVolatileTip`  → `[10]`
    /// - `MsgDone`                  → `[7]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgAcquire {
                target: AcquireTarget::Point(point_cbor),
            } => {
                enc.array(2);
                enc.unsigned(0);
                enc.bytes(point_cbor);
            }
            Self::MsgAcquire {
                target: AcquireTarget::VolatileTip,
            } => {
                enc.array(1);
                enc.unsigned(9);
            }
            Self::MsgAcquired => {
                enc.array(1);
                enc.unsigned(1);
            }
            Self::MsgFailure { reason } => {
                enc.array(2);
                enc.unsigned(2);
                enc.unsigned(match reason {
                    AcquireFailure::PointTooOld => 0,
                    AcquireFailure::PointNotOnChain => 1,
                });
            }
            Self::MsgQuery { query } => {
                enc.array(2);
                enc.unsigned(3);
                enc.bytes(query);
            }
            Self::MsgResult { result } => {
                enc.array(2);
                enc.unsigned(4);
                enc.bytes(result);
            }
            Self::MsgRelease => {
                enc.array(1);
                enc.unsigned(5);
            }
            Self::MsgReAcquire {
                target: AcquireTarget::Point(point_cbor),
            } => {
                enc.array(2);
                enc.unsigned(6);
                enc.bytes(point_cbor);
            }
            Self::MsgReAcquire {
                target: AcquireTarget::VolatileTip,
            } => {
                enc.array(1);
                enc.unsigned(10);
            }
            Self::MsgDone => {
                enc.array(1);
                enc.unsigned(7);
            }
        }
        enc.into_bytes()
    }

    /// Decode a CBOR-encoded message from wire bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let _len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 => {
                let point_cbor = dec.bytes()?.to_vec();
                Ok(Self::MsgAcquire {
                    target: AcquireTarget::Point(point_cbor),
                })
            }
            1 => Ok(Self::MsgAcquired),
            2 => {
                let reason_tag = dec.unsigned()?;
                let reason = match reason_tag {
                    0 => AcquireFailure::PointTooOld,
                    1 => AcquireFailure::PointNotOnChain,
                    r => {
                        return Err(LedgerError::CborDecodeError(format!(
                            "unknown AcquireFailure tag: {r}"
                        )));
                    }
                };
                Ok(Self::MsgFailure { reason })
            }
            3 => {
                let query = dec.bytes()?.to_vec();
                Ok(Self::MsgQuery { query })
            }
            4 => {
                let result = dec.bytes()?.to_vec();
                Ok(Self::MsgResult { result })
            }
            5 => Ok(Self::MsgRelease),
            6 => {
                let point_cbor = dec.bytes()?.to_vec();
                Ok(Self::MsgReAcquire {
                    target: AcquireTarget::Point(point_cbor),
                })
            }
            7 => Ok(Self::MsgDone),
            9 => Ok(Self::MsgAcquire {
                target: AcquireTarget::VolatileTip,
            }),
            10 => Ok(Self::MsgReAcquire {
                target: AcquireTarget::VolatileTip,
            }),
            tag => Err(LedgerError::CborDecodeError(format!(
                "unknown LocalStateQuery message tag: {tag}"
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
    fn acquire_failure_display_point_too_old() {
        // Display output should be human-readable (not Debug-formatted) so
        // LSQ clients logging `AcquireFailed(AcquireFailure)` surface a
        // descriptive diagnostic rather than "PointTooOld".
        let s = format!("{}", AcquireFailure::PointTooOld);
        assert!(
            s.contains("too old") || s.contains("immutable tip"),
            "PointTooOld Display must identify the rule: {s}",
        );
        // Not the Debug-formatted identifier.
        assert!(
            !s.contains("PointTooOld"),
            "Display must not leak the Debug variant name: {s}",
        );
    }

    #[test]
    fn acquire_failure_display_point_not_on_chain() {
        let s = format!("{}", AcquireFailure::PointNotOnChain);
        assert!(
            s.contains("not on") && s.contains("chain"),
            "PointNotOnChain Display must identify the rule: {s}",
        );
        assert!(
            !s.contains("PointNotOnChain"),
            "Display must not leak the Debug variant name: {s}",
        );
    }

    #[test]
    fn acquire_point_roundtrip() {
        let point = vec![0x82, 0x01, 0x41, 0xAB];
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::Point(point.clone()),
        };
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(
            decoded,
            LocalStateQueryMessage::MsgAcquire {
                target: AcquireTarget::Point(point)
            }
        );
    }

    #[test]
    fn acquire_volatile_tip_roundtrip() {
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::VolatileTip,
        };
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn acquired_roundtrip() {
        let msg = LocalStateQueryMessage::MsgAcquired;
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn failure_point_too_old_roundtrip() {
        let msg = LocalStateQueryMessage::MsgFailure {
            reason: AcquireFailure::PointTooOld,
        };
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn failure_not_on_chain_roundtrip() {
        let msg = LocalStateQueryMessage::MsgFailure {
            reason: AcquireFailure::PointNotOnChain,
        };
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn query_result_roundtrip() {
        let query_bytes = vec![0xDE, 0xAD];
        let result_bytes = vec![0xBE, 0xEF];
        let q_msg = LocalStateQueryMessage::MsgQuery {
            query: query_bytes.clone(),
        };
        let r_msg = LocalStateQueryMessage::MsgResult {
            result: result_bytes.clone(),
        };
        assert_eq!(
            LocalStateQueryMessage::from_cbor(&q_msg.to_cbor()).unwrap(),
            LocalStateQueryMessage::MsgQuery { query: query_bytes }
        );
        assert_eq!(
            LocalStateQueryMessage::from_cbor(&r_msg.to_cbor()).unwrap(),
            LocalStateQueryMessage::MsgResult {
                result: result_bytes
            }
        );
    }

    #[test]
    fn release_done_roundtrip() {
        for msg in [
            LocalStateQueryMessage::MsgRelease,
            LocalStateQueryMessage::MsgDone,
        ] {
            let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn reacquire_roundtrip() {
        let msg = LocalStateQueryMessage::MsgReAcquire {
            target: AcquireTarget::VolatileTip,
        };
        let decoded = LocalStateQueryMessage::from_cbor(&msg.to_cbor()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn state_machine_idle_acquire_acquired() {
        let s0 = LocalStateQueryState::StIdle;
        let s1 = s0
            .transition(&LocalStateQueryMessage::MsgAcquire {
                target: AcquireTarget::VolatileTip,
            })
            .unwrap();
        assert_eq!(s1, LocalStateQueryState::StAcquiring);
        let s2 = s1.transition(&LocalStateQueryMessage::MsgAcquired).unwrap();
        assert_eq!(s2, LocalStateQueryState::StAcquired);
    }

    #[test]
    fn state_machine_query_result_cycle() {
        let s = LocalStateQueryState::StAcquired;
        let s = s
            .transition(&LocalStateQueryMessage::MsgQuery { query: vec![] })
            .unwrap();
        assert_eq!(s, LocalStateQueryState::StQuerying);
        let s = s
            .transition(&LocalStateQueryMessage::MsgResult { result: vec![] })
            .unwrap();
        assert_eq!(s, LocalStateQueryState::StAcquired);
    }

    #[test]
    fn state_machine_failure_returns_to_idle() {
        let s = LocalStateQueryState::StAcquiring;
        let s = s
            .transition(&LocalStateQueryMessage::MsgFailure {
                reason: AcquireFailure::PointTooOld,
            })
            .unwrap();
        assert_eq!(s, LocalStateQueryState::StIdle);
    }

    #[test]
    fn state_machine_illegal_transition() {
        let s = LocalStateQueryState::StAcquired;
        // Can't send MsgAcquire in StAcquired
        assert!(
            s.transition(&LocalStateQueryMessage::MsgAcquire {
                target: AcquireTarget::VolatileTip
            })
            .is_err()
        );
    }
}
