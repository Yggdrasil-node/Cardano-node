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
/// |  8  | `MsgAcquireVolatileTip`  |
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
    /// - `MsgQuery(query)`          → `[3, <inline-cbor query>]`
    /// - `MsgResult(result)`        → `[4, <inline-cbor result>]`
    /// - `MsgRelease`               → `[5]`
    /// - `MsgReAcquire(point)`      → `[6, <inline-cbor point>]`
    /// - `MsgReAcquireVolatileTip`  → `[10]`
    /// - `MsgDone`                  → `[7]`
    ///
    /// **Wire-format parity (Round 146)**: `point`, `query`, and `result`
    /// are encoded as INLINE CBOR data items — NOT wrapped in CBOR
    /// byte-string major type 2.  The pre-fix codec called
    /// `enc.bytes(point_cbor)` which prepended a `0x58 <len>` byte-string
    /// header, so wire frames carried `[0, h'<bytes>']` instead of
    /// upstream's `[0, <point>]`.  Upstream `cardano-cli 10.16.0.0`
    /// sends the inline shape; yggdrasil's `dec.bytes()` decode then
    /// returned a type-mismatch error and tore down the bearer
    /// (operator-observable as
    /// `BearerClosed "<socket: 11> closed when reading data"`).
    /// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Codec`
    /// — `encodeMsg` / `decodeMsg`.  The `Acquire`/`ReAcquire` `point`
    /// argument is `encodePoint`; the `Query`/`Result` payload is
    /// `encodeQuery`/`encodeResult` from the application's query
    /// codec.  None of these emit a byte-string wrapper.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgAcquire {
                target: AcquireTarget::Point(point_cbor),
            } => {
                enc.array(2);
                enc.unsigned(0);
                enc.raw(point_cbor);
            }
            Self::MsgAcquire {
                target: AcquireTarget::VolatileTip,
            } => {
                // Round 146 — `MsgAcquireVolatileTip` is wire tag 8 per
                // upstream `Ouroboros.Network.Protocol.LocalStateQuery.Codec`
                // (`encodeMsg ... MsgAcquireVolatileTip = encodeListLen 1
                // <> encodeWord 8`).  Pre-fix yggdrasil emitted tag 9,
                // which upstream `cardano-cli query tip` would never
                // send and yggdrasil's own server happened to round-trip
                // with itself but rejected real cardano-cli traffic with
                // `unknown LocalStateQuery message tag: 8`, tearing
                // down the bearer.  2026-04-27 operational rehearsal
                // captured the upstream `81 08` payload via
                // `YGG_NTC_DEBUG=1`.
                enc.array(1);
                enc.unsigned(8);
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
                enc.raw(query);
            }
            Self::MsgResult { result } => {
                enc.array(2);
                enc.unsigned(4);
                enc.raw(result);
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
                enc.raw(point_cbor);
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
                // `point` is INLINE CBOR (no byte-string wrapper) — see
                // wire-format note on `to_cbor`.
                let point_cbor = dec.raw_value()?.to_vec();
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
                let query = dec.raw_value()?.to_vec();
                Ok(Self::MsgQuery { query })
            }
            4 => {
                let result = dec.raw_value()?.to_vec();
                Ok(Self::MsgResult { result })
            }
            5 => Ok(Self::MsgRelease),
            6 => {
                let point_cbor = dec.raw_value()?.to_vec();
                Ok(Self::MsgReAcquire {
                    target: AcquireTarget::Point(point_cbor),
                })
            }
            7 => Ok(Self::MsgDone),
            // Upstream tag 8 = `MsgAcquireVolatileTip`.  See encode-side
            // comment for the wire-format reference.
            8 => Ok(Self::MsgAcquire {
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
        // Round 146 — query/result payloads are now inline CBOR (no
        // byte-string wrapper), so the test bytes must themselves be
        // valid CBOR.  Use minimal-but-real CBOR shapes that exercise
        // the roundtrip without depending on the application query
        // codec: a 1-element array `[5]` for the query, a 2-element
        // array `[1, 0x42]` for the result.
        let query_bytes = vec![0x81, 0x05]; // CBOR: [5]
        let result_bytes = vec![0x82, 0x01, 0x18, 0x42]; // CBOR: [1, 66]
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

    /// Round 146 — operator-captured wire bytes from upstream
    /// `cardano-cli 10.16.0.0 query tip --testnet-magic 1`: a single
    /// 2-byte `[8]` payload (`81 08`) representing
    /// `MsgAcquireVolatileTip`.  Pre-fix yggdrasil mapped this variant
    /// to tag 9 (encode AND decode), so the upstream `81 08` payload
    /// failed yggdrasil's decoder as
    /// `unknown LocalStateQuery message tag: 8`, the server tore down
    /// the bearer, and operators saw
    /// `BearerClosed "<socket: 11> closed when reading data, waiting on next header True"`.
    /// This test pins the wire bytes of the encode side AND that the
    /// captured upstream payload decodes into the expected variant.
    /// A future drift in either direction fails clean.
    #[test]
    fn acquire_volatile_tip_wire_tag_matches_upstream_canonical_tag_8() {
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::VolatileTip,
        };
        let wire = msg.to_cbor();
        assert_eq!(
            wire,
            vec![0x81, 0x08],
            "MsgAcquireVolatileTip must encode as `[8]` per upstream \
             `Ouroboros.Network.Protocol.LocalStateQuery.Codec`",
        );
        // The upstream-captured payload must decode into the same variant.
        let decoded = LocalStateQueryMessage::from_cbor(&[0x81, 0x08]).unwrap();
        assert_eq!(decoded, msg);
    }

    /// Operator-captured `MsgQuery` payloads from upstream
    /// `cardano-cli 10.16.0.0 query tip --testnet-magic 1` against the
    /// 2026-04-27 rehearsal yggdrasil node (`YGG_NTC_DEBUG=1` trace).
    /// Both queries decode cleanly through the wire-level codec — the
    /// 8-byte payloads `82 03 82 00 82 02 81 01` and `82 03 82 00 82 02
    /// 81 00` round-trip into `MsgQuery` carrying the inline-CBOR
    /// `[0, [2, [1]]]` / `[0, [2, [0]]]` shapes.  Pinned here so any
    /// future regression of the inline-CBOR encoding (Round 146 fix)
    /// would resurface as a clearly-named decode failure on the
    /// captured upstream traffic, AND so the future
    /// HardForkBlock-query-codec slice (Finding E in
    /// `docs/operational-runs/2026-04-27-runbook-pass.md`) has a
    /// concrete starting fixture.  The query *content* layer
    /// (decoding `[0, [2, [1]]]` into a typed era-aware
    /// `BlockQuery`) is the open follow-up and explicitly out of
    /// scope for the wire-level codec tested here.
    #[test]
    fn msg_query_wire_payload_round_trips_real_cardano_cli_capture() {
        let captured_payloads: &[&[u8]] = &[
            // [3, [0, [2, [1]]]] — first MsgQuery from cardano-cli
            &[0x82, 0x03, 0x82, 0x00, 0x82, 0x02, 0x81, 0x01],
            // [3, [0, [2, [0]]]] — second MsgQuery from cardano-cli
            &[0x82, 0x03, 0x82, 0x00, 0x82, 0x02, 0x81, 0x00],
        ];
        for (idx, captured) in captured_payloads.iter().enumerate() {
            let decoded = LocalStateQueryMessage::from_cbor(captured).unwrap_or_else(|err| {
                panic!(
                    "captured cardano-cli MsgQuery payload {idx} (\
                     len={}) must decode through the wire-level \
                     codec; got {err}",
                    captured.len(),
                )
            });
            match &decoded {
                LocalStateQueryMessage::MsgQuery { query } => {
                    assert!(
                        !query.is_empty(),
                        "captured query payload {idx} must carry a non-empty \
                         inline-CBOR query body",
                    );
                    // The captured bodies start with `82 00` — outer
                    // wrapper of the upstream HardForkBlock query layer.
                    assert_eq!(
                        &query[..2],
                        &[0x82, 0x00],
                        "captured query payload {idx} body must start with the \
                         upstream HardForkBlock outer-wrapper bytes \
                         (0x82 0x00); decoding into a typed era-aware \
                         BlockQuery is the open Finding E follow-up",
                    );
                }
                other => panic!("captured payload {idx} must decode as MsgQuery; got {other:?}"),
            }
            // Inverse: re-encoding the decoded value must reproduce the
            // exact captured bytes (the inline-CBOR codec preserves
            // payload bytes verbatim, so this is a strong byte-level
            // wire-format pin).
            let re_encoded = decoded.to_cbor();
            assert_eq!(
                &re_encoded[..],
                *captured,
                "round-trip of cardano-cli MsgQuery payload {idx} must \
                 reproduce the exact wire bytes — drift here means the \
                 wire-level codec stopped preserving the inline-CBOR \
                 shape",
            );
        }
    }

    /// Round 146 wire-format pin: the `point` argument is encoded
    /// as INLINE CBOR, not wrapped in a byte-string.  Pre-fix bytes
    /// would have been `[0, h'<point>']` (`0x82 0x00 0x58 <len>
    /// <point>`); post-fix bytes must be `[0, <point>]` directly.
    /// A future drift back to `enc.bytes(...)` fails this test
    /// cleanly with the captured byte-by-byte diagnostic.
    #[test]
    fn acquire_point_wire_bytes_are_inline_not_byte_string_wrapped() {
        // A minimal valid Point CBOR: `[42, h'aa..aa']` (BlockPoint
        // shape used by upstream codec).  Inline-encoded inside
        // MsgAcquire we expect the array header + tag + Point bytes
        // verbatim.
        let mut point_enc = Encoder::new();
        point_enc.array(2);
        point_enc.unsigned(42);
        point_enc.bytes(&[0xaa; 8]);
        let point_cbor = point_enc.into_bytes();
        let msg = LocalStateQueryMessage::MsgAcquire {
            target: AcquireTarget::Point(point_cbor.clone()),
        };
        let wire = msg.to_cbor();
        // Expected: [0x82, 0x00] envelope + raw point bytes.
        assert_eq!(wire[0], 0x82, "outer array header (length 2)");
        assert_eq!(wire[1], 0x00, "MsgAcquire tag = 0");
        assert_eq!(
            &wire[2..],
            point_cbor.as_slice(),
            "point must be inline-encoded; pre-fix this carried a 0x58 \
             byte-string header followed by the point bytes",
        );
        // And the round-trip must succeed (decoder uses raw_value).
        let decoded = LocalStateQueryMessage::from_cbor(&wire).unwrap();
        assert_eq!(decoded, msg);
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
