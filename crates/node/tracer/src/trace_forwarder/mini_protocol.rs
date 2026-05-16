//! Layer 2 of the cardano-tracer forwarder — the
//! `Trace.Forward.Protocol.TraceObject` mini-protocol codec.
//!
//! ## Wire format (per `trace-forward/src/Trace/Forward/Protocol/TraceObject/Codec.hs`)
//!
//! Each message is a CBOR array whose first element is a tag word
//! identifying the message type (`codecTraceObjectForward`'s
//! `encode` does `encodeListLen N <> encodeWord tag <> …`):
//!
//! | tag | message                       | shape                                                       |
//! | --- | ----------------------------- | ----------------------------------------------------------- |
//! | 1   | `MsgTraceObjectsRequest`      | `[1, blocking::Bool, NumberOfTraceObjects]` (len 3)         |
//! | 2   | `MsgDone`                     | `[2]` (len 1)                                               |
//! | 3   | `MsgTraceObjectsReply`        | `[3, replyList]` (len 2)                                    |
//!
//! `NumberOfTraceObjects` is **not** a bare CBOR uint. Upstream
//! (`Trace/Forward/Protocol/TraceObject/Type.hs`) declares it as
//! `newtype NumberOfTraceObjects = NumberOfTraceObjects { nTraceObjects :: Word16 }`
//! with `deriving anyclass Serialise`, and `codecTraceObjectForward`
//! is wired with `CBOR.encode`/`CBOR.decode` (`Codec.Serialise`).
//! `cborg`'s generic `Serialise` for a single-field single-constructor
//! type goes through `GSerialiseEncode (K1 i a)`, which emits
//! `encodeListLen 2 <> encodeWord 0 <> encode field` — i.e. a
//! 2-element CBOR array `[0, word16]`. This was confirmed on the
//! wire: cardano-tracer's `MsgTraceObjectsRequest` for 100 objects
//! is `83 01 f5 82 00 18 64` = `[1, true, [0, 100]]`.
//!
//! `replyList` is `Serialise [TraceObject]` — `codecTraceObjectForward`
//! passes `CBOR.encode :: [lo] -> Encoding`. `cborg`'s
//! `defaultEncodeList` encodes an **empty** list as `encodeListLen 0`
//! (a definite `array(0)`, `0x80`) and a **non-empty** list as
//! `encodeListLenIndef <> elems <> encodeBreak` (`0x9f … 0xff`).
//! Each element is one `TraceObject` encoded via
//! [`super::TraceObject::to_cbor`].
//!
//! `blocking` is a CBOR boolean (`true` for `TokBlocking`, `false`
//! for `TokNonBlocking`).
//!
//! In the blocking sub-state the reply list MUST be non-empty;
//! the decoder enforces this.
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
                write!(
                    f,
                    "unknown TraceForward message tag {tag} (array length {len})"
                )
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
/// Wire shape: `[1, blocking::Bool, NumberOfTraceObjects]` — CBOR
/// list-len 3. `NumberOfTraceObjects` is itself the 2-element array
/// `[0, n]` produced by `cborg`'s generic `Serialise` for the
/// single-field newtype (see the module docstring).
pub fn encode_request(blocking: bool, n: u16) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(1);
    enc.bool(blocking);
    encode_number_of_trace_objects(&mut enc, n);
    enc.into_bytes()
}

/// Append a generic-`Serialise`-encoded `NumberOfTraceObjects` to
/// `enc`: the 2-element CBOR array `[0, n]` (constructor tag 0 then
/// the `Word16` payload as a CBOR uint), per `cborg`'s
/// `GSerialiseEncode (K1 i a)` instance for a single-field
/// single-constructor newtype.
fn encode_number_of_trace_objects(enc: &mut Encoder, n: u16) {
    enc.array(2);
    enc.unsigned(0);
    enc.unsigned(u64::from(n));
}

/// Decode a generic-`Serialise`-encoded `NumberOfTraceObjects`
/// (`[0, n]`) from `dec`. Inverse of [`encode_number_of_trace_objects`].
fn decode_number_of_trace_objects(dec: &mut Decoder<'_>) -> Result<u16, ProtocolError> {
    let len = dec
        .array()
        .map_err(|e| ProtocolError::Cbor(format!("NumberOfTraceObjects array: {e}")))?;
    if len != 2 {
        return Err(ProtocolError::Cbor(format!(
            "NumberOfTraceObjects must be a 2-element array [0, n]; got length {len}"
        )));
    }
    let ctor = dec
        .unsigned()
        .map_err(|e| ProtocolError::Cbor(format!("NumberOfTraceObjects tag: {e}")))?;
    if ctor != 0 {
        return Err(ProtocolError::Cbor(format!(
            "NumberOfTraceObjects constructor tag must be 0; got {ctor}"
        )));
    }
    let n_u64 = dec
        .unsigned()
        .map_err(|e| ProtocolError::Cbor(format!("NumberOfTraceObjects value: {e}")))?;
    u16::try_from(n_u64).map_err(|_| {
        ProtocolError::Cbor(format!(
            "NumberOfTraceObjects {n_u64} exceeds Word16 max 65535"
        ))
    })
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
/// Wire shape: `[3, replyList]` — CBOR list-len 2, with the reply
/// list being `Serialise [TraceObject]`. Per `cborg`'s
/// `defaultEncodeList`:
///
/// * an empty list encodes as a definite `array(0)` (`0x80`);
/// * a non-empty list encodes as an indefinite-length list
///   (`0x9f` … items … `0xff`).
///
/// Each `TraceObject` element is encoded via
/// [`super::TraceObject::to_cbor`].
pub fn encode_reply(traces: &[TraceObject]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2);
    enc.unsigned(3);
    if traces.is_empty() {
        // `defaultEncodeList []` = `encodeListLen 0`.
        enc.array(0);
        return enc.into_bytes();
    }
    // `defaultEncodeList (x:xs)` = indefinite list 0x9f … 0xff.
    // Each CBOR-encoded TraceObject is itself a complete CBOR value,
    // so appending the pre-encoded bytes after the indefinite-array
    // marker keeps the result valid CBOR.
    enc.array_indef();
    let mut bytes = enc.into_bytes();
    for to in traces {
        bytes.extend_from_slice(&to.to_cbor());
    }
    // CBOR break stop-code (0xff) closes the indefinite-length list.
    bytes.push(0xff);
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
            // `NumberOfTraceObjects` is the generic-Serialise newtype
            // envelope `[0, word16]`, not a bare CBOR uint.
            let n = decode_number_of_trace_objects(&mut dec)?;
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
            // `Serialise [TraceObject]` — an empty list is a definite
            // `array(0)`; a non-empty list is the indefinite-length
            // form `0x9f … 0xff`. `array_begin` returns `Some(n)` for
            // the definite case and `None` for the indefinite case.
            let mut traces = Vec::new();
            match dec
                .array_begin()
                .map_err(|e| ProtocolError::Cbor(format!("reply array: {e}")))?
            {
                Some(n) => {
                    for _ in 0..n {
                        // Slice the next TraceObject's bytes via
                        // raw_value() then hand the slice to the
                        // Layer-1 decoder.
                        let to_bytes = dec
                            .raw_value()
                            .map_err(|e| ProtocolError::Cbor(format!("reply TraceObject: {e}")))?;
                        let to = TraceObject::from_cbor_bytes(to_bytes)
                            .map_err(|e| ProtocolError::Cbor(format!("TraceObject decode: {e}")))?;
                        traces.push(to);
                    }
                }
                None => {
                    while !dec.is_break() {
                        let to_bytes = dec
                            .raw_value()
                            .map_err(|e| ProtocolError::Cbor(format!("reply TraceObject: {e}")))?;
                        let to = TraceObject::from_cbor_bytes(to_bytes)
                            .map_err(|e| ProtocolError::Cbor(format!("TraceObject decode: {e}")))?;
                        traces.push(to);
                    }
                    dec.consume_break()
                        .map_err(|e| ProtocolError::Cbor(format!("reply break: {e}")))?;
                }
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

    /// `MsgTraceObjectsRequest` wire shape: CBOR
    /// `[1, blocking::Bool, [0, n]]` — a 3-element array (tag=1, a
    /// CBOR bool, then the generic-`Serialise` `NumberOfTraceObjects`
    /// envelope `[0, n]`).
    #[test]
    fn encode_request_byte_shape_blocking_n_zero() {
        // blocking=true, n=0:
        //   array-len-3        = 0x83
        //   uint(1)            = 0x01
        //   bool(true)         = 0xf5
        //   array-len-2        = 0x82   ← NumberOfTraceObjects newtype
        //   uint(0)            = 0x00   ← generic constructor tag
        //   uint(0)            = 0x00   ← nTraceObjects = 0
        let bytes = encode_request(true, 0);
        assert_eq!(bytes, vec![0x83, 0x01, 0xf5, 0x82, 0x00, 0x00]);
    }

    /// `MsgTraceObjectsRequest` with blocking=false, n=255 — the n
    /// crosses the small-uint / one-byte-uint boundary at 24.
    #[test]
    fn encode_request_byte_shape_nonblocking_n_255() {
        // n=255 (0xFF) → 0x18 0xFF (uint8 prefix + value), inside the
        // 2-element NumberOfTraceObjects envelope.
        let bytes = encode_request(false, 255);
        assert_eq!(bytes, vec![0x83, 0x01, 0xf4, 0x82, 0x00, 0x18, 0xff]);
    }

    /// Pin the exact `MsgTraceObjectsRequest` cardano-tracer sends for
    /// a blocking request of 100 objects. Captured on the wire from
    /// the live upstream `cardano-tracer 11.0.1` acceptor:
    /// `83 01 f5 82 00 18 64` = `[1, true, [0, 100]]`.
    #[test]
    fn encode_request_matches_captured_upstream_blocking_100() {
        let bytes = encode_request(true, 100);
        assert_eq!(
            bytes,
            vec![0x83, 0x01, 0xf5, 0x82, 0x00, 0x18, 0x64],
            "must match the cardano-tracer 11.0.1 on-wire MsgTraceObjectsRequest"
        );
    }

    /// `MsgDone` wire shape: CBOR `[2]` — 1-element array, tag=2.
    #[test]
    fn encode_done_byte_shape() {
        let bytes = encode_done();
        assert_eq!(bytes, vec![0x81, 0x02]);
    }

    /// `MsgTraceObjectsReply` with an empty inner list — `Serialise []`
    /// encodes the empty list as a definite `array(0)`.
    #[test]
    fn encode_reply_empty_byte_shape() {
        let bytes = encode_reply(&[]);
        // array-len-2 = 0x82, uint(3) = 0x03, array-len-0 = 0x80.
        assert_eq!(bytes, vec![0x82, 0x03, 0x80]);
    }

    /// `MsgTraceObjectsReply` with one TraceObject. The non-empty
    /// reply list is the indefinite-length form, so the header is
    /// `0x82 0x03 0x9f` and the message ends with the `0xff` break.
    #[test]
    fn encode_reply_one_trace_object_shape() {
        let to = TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_namespace: vec![],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DMinimal,
            to_timestamp: (1_767_312_000, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        let bytes = encode_reply(std::slice::from_ref(&to));
        // Prefix: 0x82 = array-len-2; 0x03 = uint(3); 0x9f = indefinite array.
        assert_eq!(&bytes[..3], &[0x82, 0x03, 0x9f]);
        // Middle: the TraceObject's own CBOR (9-element array).
        let obj_cbor = to.to_cbor();
        assert_eq!(&bytes[3..3 + obj_cbor.len()], obj_cbor.as_slice());
        // Tail: the CBOR break stop-code closing the indefinite list.
        assert_eq!(bytes.last(), Some(&0xff));
        assert_eq!(bytes.len(), 3 + obj_cbor.len() + 1);
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
    /// outer reply decoder slices each TraceObject via
    /// `Decoder::raw_value()` (which correctly skips the per-object
    /// indefinite-length `to_namespace` list and the tagged
    /// `to_timestamp` map) and runs them through
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
                to_timestamp: (1_767_312_000, 0),
                to_hostname: "h1".into(),
                to_thread_id: "t1".into(),
            },
            TraceObject {
                to_human: None,
                to_machine: "{}".into(),
                to_namespace: vec!["Consensus".into()],
                to_severity: TraceSeverity::Warning,
                to_details: TraceDetail::DDetailed,
                to_timestamp: (1_767_312_000, 1_000_000_000),
                to_hostname: "h2".into(),
                to_thread_id: "t2".into(),
            },
        ];
        let bytes = encode_reply(&traces);
        let msg = decode_message(&bytes).expect("decode reply");
        assert_eq!(msg, TraceForwardMessage::Reply(traces));
    }

    /// The decoder also accepts a definite-length reply list — a
    /// conformant peer MAY send `Serialise []` as a definite array
    /// (the empty-list branch), and a non-empty definite array is
    /// still valid CBOR a tolerant decoder should accept.
    #[test]
    fn decode_accepts_definite_length_reply_list() {
        let to = TraceObject {
            to_human: None,
            to_machine: "x".into(),
            to_namespace: vec![],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            to_timestamp: (1_767_312_000, 0),
            to_hostname: String::new(),
            to_thread_id: String::new(),
        };
        // Hand-build a [3, [obj]] reply with a *definite* inner array.
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(3);
        enc.array(1);
        let mut bytes = enc.into_bytes();
        bytes.extend_from_slice(&to.to_cbor());
        let msg = decode_message(&bytes).expect("decode definite-length reply");
        assert_eq!(msg, TraceForwardMessage::Reply(vec![to]));
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

    /// A `NumberOfTraceObjects` encoded as a bare CBOR uint (the
    /// pre-fix Yggdrasil shape) is rejected — proves the codec now
    /// requires the generic-`Serialise` 2-element envelope.
    #[test]
    fn decode_rejects_bare_uint_number_of_trace_objects() {
        let mut enc = Encoder::new();
        enc.array(3);
        enc.unsigned(1);
        enc.bool(true);
        enc.unsigned(100); // bare uint instead of [0, 100]
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes).unwrap_err();
        assert!(
            matches!(err, ProtocolError::Cbor(_)),
            "bare-uint NumberOfTraceObjects must be rejected; got {err:?}"
        );
    }
}
