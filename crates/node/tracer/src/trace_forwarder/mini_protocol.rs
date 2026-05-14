//! Layer 2 of the cardano-tracer forwarder — the
//! `Trace.Forward.Protocol.TraceObject` mini-protocol codec.
//!
//! ## Wire format (per `trace-forward/src/Trace/Forward/Protocol/TraceObject/Codec.hs`)
//!
//! Each message is a CBOR array whose first element is a tag word
//! identifying the message type:
//!
//! | tag | message                       | shape                                                       |
//! | --- | ----------------------------- | ----------------------------------------------------------- |
//! | 1   | `MsgTraceObjectsRequest`      | `[1, blocking::Bool, NumberOfTraceObjects::Word16]` (len 3) |
//! | 2   | `MsgDone`                     | `[2]` (len 1)                                               |
//! | 3   | `MsgTraceObjectsReply`        | `[3, [TraceObject_1, …, TraceObject_n]]` (len 2)            |
//!
//! `NumberOfTraceObjects` is upstream-encoded as a CBOR unsigned
//! integer via the `Serialise Word16` instance — the codec callers
//! pass a Word16-bounded value; the encoder writes it as a CBOR uint
//! (one of the standard CBOR major-0 forms).
//!
//! `blocking` is a CBOR boolean (`true` for `TokBlocking`, `false`
//! for `TokNonBlocking`).
//!
//! In the blocking sub-state the reply list MUST be non-empty;
//! the decoder enforces this.
//!
//! The TraceObject payload itself is encoded via the existing
//! [`super::TraceObject::to_cbor`] (the 8-element array from
//! Layer 1, also byte-pinned in this crate's tests). The reply
//! codec just emits a CBOR array prefix followed by each
//! TraceObject's pre-encoded bytes.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side Rust port; the wire
//! schema is the source of truth from upstream
//! `Trace.Forward.Protocol.TraceObject.Codec`. Yggdrasil collapses
//! the typed-protocol state machine (which in Haskell is split
//! across `Type.hs` + `Codec.hs` + `Forwarder.hs` + `Acceptor.hs`)
//! into a single `mini_protocol.rs` for now; the state-machine
//! driver lands when the transport is wired in.

use super::TraceObject;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// A decoded TraceForward mini-protocol message. The encoder/decoder
/// roundtrip on this enum mirrors `codecTraceObjectForward` from
/// upstream `Trace.Forward.Protocol.TraceObject.Codec`.
///
/// The state-machine context (which message is valid in which protocol
/// state) is enforced by the caller — the codec itself is
/// state-context-free, matching upstream's `codec` decoder that takes
/// a `StateToken` and validates the tag against it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceForwardMessage {
    /// Acceptor requesting `n` TraceObjects from the Forwarder.
    /// `blocking == true` blocks until at least one is available
    /// (the reply must be non-empty); `false` returns immediately
    /// with whatever is queued (the reply may be empty).
    ///
    /// Sent by the Acceptor in the `StIdle` state, advancing to
    /// `StBusy blocking`.
    Request {
        /// Blocking sub-state selector.
        blocking: bool,
        /// Upper bound on the number of objects to return. Word16
        /// per upstream.
        n: u16,
    },
    /// Forwarder replying with up to `n` TraceObjects.
    ///
    /// Sent by the Forwarder in the `StBusy blocking` state, advancing
    /// back to `StIdle`.
    Reply(Vec<TraceObject>),
    /// Acceptor closing the protocol. Sent in the `StIdle` state,
    /// advancing to `StDone`.
    Done,
}

/// Errors surfaced by `decode_message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// CBOR decode error (malformed bytes).
    Cbor(String),
    /// The first array element wasn't a recognised tag word.
    UnknownTag {
        /// The decoded tag value.
        tag: u64,
        /// The decoded array length.
        len: u64,
    },
    /// Tag was valid but the array length didn't match the expected
    /// shape for that tag.
    ArityMismatch {
        /// The decoded tag value.
        tag: u64,
        /// The decoded array length.
        got_len: u64,
        /// The required array length for `tag`.
        expected_len: u64,
    },
    /// A blocking `MsgTraceObjectsReply` arrived with an empty list —
    /// upstream's decoder rejects this case explicitly.
    EmptyBlockingReply,
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cbor(msg) => write!(f, "CBOR decode error: {msg}"),
            Self::UnknownTag { tag, len } => {
                write!(f, "unknown TraceForward message tag {tag} (array length {len})")
            }
            Self::ArityMismatch {
                tag,
                got_len,
                expected_len,
            } => write!(
                f,
                "TraceForward tag {tag} expects array length {expected_len}, got {got_len}"
            ),
            Self::EmptyBlockingReply => f.write_str(
                "blocking MsgTraceObjectsReply must carry a non-empty list \
                 (upstream codec rejects this case)",
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Encode a `MsgTraceObjectsRequest` to its CBOR wire form.
///
/// Wire shape: `[1, blocking::Bool, n::Word16]` — CBOR list-len 3.
pub fn encode_request(blocking: bool, n: u16) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(1);
    enc.bool(blocking);
    enc.unsigned(u64::from(n));
    enc.into_bytes()
}

/// Encode a `MsgDone` to its CBOR wire form.
///
/// Wire shape: `[2]` — CBOR list-len 1.
pub fn encode_done() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1);
    enc.unsigned(2);
    enc.into_bytes()
}

/// Encode a `MsgTraceObjectsReply` to its CBOR wire form.
///
/// Wire shape: `[3, [TraceObject_1, …, TraceObject_n]]` —
/// CBOR list-len 2, with the reply list nested as an inner CBOR
/// array. Each `TraceObject` is encoded via
/// [`super::TraceObject::to_cbor`].
pub fn encode_reply(traces: &[TraceObject]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2);
    enc.unsigned(3);
    // Inner array of TraceObjects: encode each one as already-CBOR'd
    // bytes and concatenate. The CBOR-encoded TraceObject is itself
    // a complete CBOR value, so appending it after the array-len
    // header keeps the result a valid CBOR sequence.
    enc.array(traces.len() as u64);
    let mut bytes = enc.into_bytes();
    for to in traces {
        bytes.extend_from_slice(&to.to_cbor());
    }
    bytes
}

/// Decode a wire-encoded TraceForward message.
///
/// Mirrors upstream `codecTraceObjectForward`'s decoder. Unlike the
/// upstream codec which carries a `StateToken` and rejects messages
/// that don't fit the current protocol state, this Rust port returns
/// the decoded variant unconditionally — the caller (state-machine
/// driver) enforces transition validity.
///
/// `MsgTraceObjectsReply` decodes the inner array by walking each
/// element through [`super::TraceObject::from_cbor_bytes`]; the
/// `Decoder::raw_value()` helper slices each TraceObject's bytes
/// so the per-element decoder operates on a fresh sub-slice.
pub fn decode_message(buf: &[u8]) -> Result<TraceForwardMessage, ProtocolError> {
    let mut dec = Decoder::new(buf);
    let len = dec
        .array()
        .map_err(|e| ProtocolError::Cbor(format!("array length: {e}")))?;
    let tag = dec
        .unsigned()
        .map_err(|e| ProtocolError::Cbor(format!("tag word: {e}")))?;
    match tag {
        1 => {
            if len != 3 {
                return Err(ProtocolError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 3,
                });
            }
            let blocking = dec
                .bool()
                .map_err(|e| ProtocolError::Cbor(format!("blocking bool: {e}")))?;
            let n_u64 = dec
                .unsigned()
                .map_err(|e| ProtocolError::Cbor(format!("n: {e}")))?;
            let n = u16::try_from(n_u64).map_err(|_| {
                ProtocolError::Cbor(format!(
                    "NumberOfTraceObjects {n_u64} exceeds Word16 max 65535"
                ))
            })?;
            Ok(TraceForwardMessage::Request { blocking, n })
        }
        2 => {
            if len != 1 {
                return Err(ProtocolError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 1,
                });
            }
            Ok(TraceForwardMessage::Done)
        }
        3 => {
            if len != 2 {
                return Err(ProtocolError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 2,
                });
            }
            let inner_len = dec
                .array()
                .map_err(|e| ProtocolError::Cbor(format!("reply array: {e}")))?;
            let mut traces = Vec::with_capacity(inner_len as usize);
            for _ in 0..inner_len {
                // Slice out the next TraceObject's bytes via raw_value()
                // then hand the slice to the Layer-1 decoder.
                let to_bytes = dec
                    .raw_value()
                    .map_err(|e| ProtocolError::Cbor(format!("reply TraceObject: {e}")))?;
                let to = TraceObject::from_cbor_bytes(to_bytes).map_err(|e| {
                    ProtocolError::Cbor(format!("TraceObject decode: {e}"))
                })?;
                traces.push(to);
            }
            Ok(TraceForwardMessage::Reply(traces))
        }
        _ => Err(ProtocolError::UnknownTag { tag, len }),
    }
}

#[cfg(test)]
mod mini_protocol_tests {
    use super::*;
    use crate::trace_forwarder::{TraceDetail, TraceSeverity};

    /// `MsgTraceObjectsRequest` wire shape: CBOR `[1, blocking, n]` —
    /// 3-element array, tag=1, then a CBOR bool, then a CBOR uint
    /// equal to `n`.
    #[test]
    fn encode_request_byte_shape_blocking_n_zero() {
        // Choose blocking=true, n=0 so the byte shape is the
        // most-stable possible:
        //   array-len-3   = 0x83
        //   uint(1)       = 0x01
        //   bool(true)    = 0xf5
        //   uint(0)       = 0x00
        let bytes = encode_request(true, 0);
        assert_eq!(bytes, vec![0x83, 0x01, 0xf5, 0x00]);
    }

    /// `MsgTraceObjectsRequest` with blocking=false, n=255 — the n
    /// crosses the small-uint / one-byte-uint boundary at 24, then
    /// the one-byte-uint encoding takes one prefix + one byte.
    #[test]
    fn encode_request_byte_shape_nonblocking_n_255() {
        // n=255 (0xFF) → 0x18 0xFF (uint8 prefix + value).
        let bytes = encode_request(false, 255);
        assert_eq!(bytes, vec![0x83, 0x01, 0xf4, 0x18, 0xff]);
    }

    /// `MsgDone` wire shape: CBOR `[2]` — 1-element array, tag=2.
    #[test]
    fn encode_done_byte_shape() {
        let bytes = encode_done();
        assert_eq!(bytes, vec![0x81, 0x02]);
    }

    /// `MsgTraceObjectsReply` with an empty inner list.
    #[test]
    fn encode_reply_empty_byte_shape() {
        let bytes = encode_reply(&[]);
        // array-len-2 = 0x82, uint(3) = 0x03, array-len-0 = 0x80.
        assert_eq!(bytes, vec![0x82, 0x03, 0x80]);
    }

    /// `MsgTraceObjectsReply` with one TraceObject. The header bytes
    /// `0x82 0x03 0x81` are pinned; the inner TraceObject CBOR is
    /// pinned in the Layer 1 tests already.
    #[test]
    fn encode_reply_one_trace_object_prefix() {
        let to = TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_namespace: vec![],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DMinimal,
            to_timestamp: (2026, 0, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        let bytes = encode_reply(std::slice::from_ref(&to));
        // Prefix: 0x82 = array-len-2; 0x03 = uint(3); 0x81 = array-len-1.
        assert_eq!(&bytes[..3], &[0x82, 0x03, 0x81]);
        // Tail: the TraceObject's own CBOR (8-element array).
        assert_eq!(&bytes[3..], to.to_cbor().as_slice());
    }

    /// Round-trip the request encoding for several (blocking, n)
    /// combinations.
    #[test]
    fn request_round_trip() {
        for (blocking, n) in [(true, 0), (false, 0), (true, 100), (false, u16::MAX)] {
            let bytes = encode_request(blocking, n);
            let msg = decode_message(&bytes).expect("decode");
            assert_eq!(
                msg,
                TraceForwardMessage::Request { blocking, n },
                "round-trip drift on (blocking={blocking}, n={n})"
            );
        }
    }

    /// Round-trip MsgDone.
    #[test]
    fn done_round_trip() {
        let bytes = encode_done();
        let msg = decode_message(&bytes).expect("decode");
        assert_eq!(msg, TraceForwardMessage::Done);
    }

    /// Round-trip an empty reply.
    #[test]
    fn empty_reply_round_trip() {
        let bytes = encode_reply(&[]);
        let msg = decode_message(&bytes).expect("decode");
        assert_eq!(msg, TraceForwardMessage::Reply(Vec::new()));
    }

    /// Round-trip a non-empty reply with two TraceObjects. The
    /// outer reply decoder now slices each TraceObject via
    /// `Decoder::raw_value()` and runs them through
    /// `TraceObject::from_cbor_bytes` — both elements come back
    /// byte-identical.
    #[test]
    fn nonempty_reply_round_trip() {
        let traces = vec![
            TraceObject {
                to_human: Some("first".into()),
                to_machine: "{\"k\":1}".into(),
                to_namespace: vec!["Net".into(), "ChainSync".into()],
                to_severity: TraceSeverity::Info,
                to_details: TraceDetail::DNormal,
                to_timestamp: (2026, 130, 0),
                to_hostname: "h1".into(),
                to_thread_id: "t1".into(),
            },
            TraceObject {
                to_human: None,
                to_machine: "{}".into(),
                to_namespace: vec!["Consensus".into()],
                to_severity: TraceSeverity::Warning,
                to_details: TraceDetail::DDetailed,
                to_timestamp: (2026, 130, 1_000_000_000_000),
                to_hostname: "h2".into(),
                to_thread_id: "t2".into(),
            },
        ];
        let bytes = encode_reply(&traces);
        let msg = decode_message(&bytes).expect("decode reply");
        assert_eq!(msg, TraceForwardMessage::Reply(traces));
    }

    /// Unknown tag rejected.
    #[test]
    fn decode_rejects_unknown_tag() {
        let mut enc = Encoder::new();
        enc.array(1);
        enc.unsigned(42);
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes).unwrap_err();
        assert!(
            matches!(err, ProtocolError::UnknownTag { tag: 42, len: 1 }),
            "expected UnknownTag(42, 1); got {err:?}"
        );
    }

    /// Tag 1 with wrong arity → ArityMismatch.
    #[test]
    fn decode_rejects_request_with_wrong_arity() {
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(1);
        enc.bool(true);
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::ArityMismatch {
                    tag: 1,
                    got_len: 2,
                    expected_len: 3
                }
            ),
            "expected ArityMismatch{{tag:1, got:2, exp:3}}; got {err:?}"
        );
    }
}
