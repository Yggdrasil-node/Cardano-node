//! Network.Mux Handshake mini-protocol codec.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side codec for the upstream
//! `Ouroboros.Network.Protocol.Handshake.Codec` wire format at
//! `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/framework/lib/Ouroboros/Network/Protocol/Handshake/Codec.hs`.
//! The Handshake runs on mini-protocol number 0 over every Mux
//! bearer; the Initiator sends `MsgProposeVersions` listing the
//! versions it speaks, the Responder replies with either
//! `MsgAcceptVersion` (selecting one) or `MsgRefuse` (with a
//! reason).
//!
//! This module covers the CBOR codec for the 4 message types
//! (ProposeVersions / AcceptVersion / Refuse / ReplyVersions) and
//! the 3 RefuseReason variants. It does NOT implement the typed-
//! protocol state machine (Idle → Confirm → Done transitions);
//! that lands when the Mux scheduler wires the handshake at
//! bearer-open time.
//!
//! ## Wire shape
//!
//! Version numbers are upstream-typed as `vNumber`; for cardano-
//! tracer specifically they're CBOR-encoded `Word32` values, but
//! the upstream codec encodes them through a generic
//! `CodecCBORTerm vNumber` so the wire allows any CBOR Term.
//! Yggdrasil-side `HandshakeMessage` carries the version number as
//! a `u32` (the cardano-tracer instantiation) and the version data
//! as opaque pre-encoded CBOR bytes (`Vec<u8>`) — callers that
//! need a different version-number type can extend the enum in a
//! follow-on round.
//!
//! - `MsgProposeVersions`: `[0, {version: data_cbor, ...}]`
//! - `MsgReplyVersions`:   `[0, {version: data_cbor, ...}]` (same
//!   shape; different state)
//! - `MsgAcceptVersion`:   `[1, version, data_cbor]`
//! - `MsgRefuse`:          `[2, RefuseReason]`
//!
//! `RefuseReason` is itself a tagged CBOR sum:
//! `[0, [v1, v2, …]]` is `VersionMismatch`,
//! `[1, version, "<error>"]` is `HandshakeDecodeError`,
//! `[2, version, "<reason>"]` is `Refused`.

use std::collections::BTreeMap;

use yggdrasil_ledger::cbor::{Decoder, Encoder};

/// One message of the Mux Handshake mini-protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeMessage {
    /// Initiator → Responder: "here are the versions I speak, with
    /// per-version data". The version map preserves ascending key
    /// order per upstream's `encodeVersions` requirement.
    ProposeVersions(BTreeMap<u32, Vec<u8>>),
    /// Responder → Initiator (query mode only): same shape as
    /// `ProposeVersions` but a different state. Sent in response
    /// to a query.
    ReplyVersions(BTreeMap<u32, Vec<u8>>),
    /// Responder → Initiator: "I picked this version; here's the
    /// agreed version data". `data_cbor` is the CBOR-encoded
    /// per-version handshake payload.
    AcceptVersion {
        /// Version number selected by the responder.
        version: u32,
        /// Pre-encoded CBOR bytes for the agreed version data.
        data_cbor: Vec<u8>,
    },
    /// Responder → Initiator: handshake failed.
    Refuse(RefuseReason),
}

/// Why the handshake was refused, per upstream's `RefuseReason`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefuseReason {
    /// The responder doesn't recognise any of the versions the
    /// initiator proposed; the included list is the set of
    /// versions the RESPONDER speaks.
    VersionMismatch(Vec<u32>),
    /// The responder decoded the version-data block as
    /// malformed CBOR.
    HandshakeDecodeError {
        /// Version under whose codec the decode failed.
        version: u32,
        /// Human-readable error message.
        message: String,
    },
    /// The responder accepted the version but rejected the
    /// agreed-data semantics (e.g., wrong network-magic).
    Refused {
        /// Version under which the refusal was raised.
        version: u32,
        /// Human-readable reason.
        reason: String,
    },
}

/// Errors surfaced from [`decode_message`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeDecodeError {
    /// CBOR decode failure with a field-name + reason hint.
    Cbor(String),
    /// First-element tag wasn't 0/1/2.
    UnknownTag {
        /// The decoded tag.
        tag: u64,
        /// The decoded outer-array length.
        len: u64,
    },
    /// Outer array length didn't match the expected shape for the
    /// decoded tag.
    ArityMismatch {
        /// The decoded tag.
        tag: u64,
        /// The decoded length.
        got_len: u64,
        /// The expected length for that tag.
        expected_len: u64,
    },
    /// `RefuseReason` payload had an unrecognised tag (must be 0/1/2).
    UnknownRefuseTag(u64),
    /// `MsgProposeVersions`/`MsgReplyVersions` map had a
    /// non-ascending key sequence.
    UnorderedVersionMap,
    /// A version number didn't fit in a `u32`.
    VersionOutOfRange(u64),
}

impl core::fmt::Display for HandshakeDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cbor(msg) => write!(f, "Handshake CBOR decode error: {msg}"),
            Self::UnknownTag { tag, len } => {
                write!(f, "unknown Handshake message tag {tag} (array len {len})")
            }
            Self::ArityMismatch {
                tag,
                got_len,
                expected_len,
            } => write!(
                f,
                "Handshake tag {tag} expects array length {expected_len}, got {got_len}"
            ),
            Self::UnknownRefuseTag(t) => write!(f, "unknown RefuseReason tag {t}"),
            Self::UnorderedVersionMap => f.write_str(
                "Handshake propose/reply versions map has non-ascending keys (upstream pins ascending order)",
            ),
            Self::VersionOutOfRange(v) => write!(f, "version number {v} does not fit u32"),
        }
    }
}

impl std::error::Error for HandshakeDecodeError {}

/// Encode a `HandshakeMessage` to its CBOR wire form.
pub fn encode_message(msg: &HandshakeMessage) -> Vec<u8> {
    match msg {
        HandshakeMessage::ProposeVersions(versions) | HandshakeMessage::ReplyVersions(versions) => {
            let mut prefix = Encoder::new();
            prefix.array(2);
            prefix.unsigned(0);
            prefix.map(versions.len() as u64);
            let mut bytes = prefix.into_bytes();
            for (v, data) in versions {
                let mut key_enc = Encoder::new();
                key_enc.unsigned(u64::from(*v));
                bytes.extend_from_slice(&key_enc.into_bytes());
                // Splice in the pre-encoded CBOR data bytes
                // verbatim — this is the upstream `encodeTerm`
                // semantics for the version-data slot.
                bytes.extend_from_slice(data);
            }
            bytes
        }
        HandshakeMessage::AcceptVersion { version, data_cbor } => {
            let mut prefix = Encoder::new();
            prefix.array(3);
            prefix.unsigned(1);
            prefix.unsigned(u64::from(*version));
            let mut bytes = prefix.into_bytes();
            bytes.extend_from_slice(data_cbor);
            bytes
        }
        HandshakeMessage::Refuse(reason) => {
            let mut enc = Encoder::new();
            enc.array(2);
            enc.unsigned(2);
            encode_refuse_reason_into(&mut enc, reason);
            enc.into_bytes()
        }
    }
}

fn encode_refuse_reason_into(enc: &mut Encoder, reason: &RefuseReason) {
    match reason {
        RefuseReason::VersionMismatch(versions) => {
            enc.array(2);
            enc.unsigned(0);
            enc.array(versions.len() as u64);
            for v in versions {
                enc.unsigned(u64::from(*v));
            }
        }
        RefuseReason::HandshakeDecodeError { version, message } => {
            enc.array(3);
            enc.unsigned(1);
            enc.unsigned(u64::from(*version));
            enc.text(message);
        }
        RefuseReason::Refused { version, reason } => {
            enc.array(3);
            enc.unsigned(2);
            enc.unsigned(u64::from(*version));
            enc.text(reason);
        }
    }
}

/// Decode a CBOR-encoded `HandshakeMessage`.
///
/// `state_is_propose` tells the decoder which of
/// `ProposeVersions` (state=Propose) vs `ReplyVersions`
/// (state=Confirm) to construct when the outer tag is `0` — the
/// wire shapes are identical so the codec needs the protocol
/// state to disambiguate, mirroring upstream's `StateToken`
/// parameter.
pub fn decode_message(
    buf: &[u8],
    state_is_propose: bool,
) -> Result<HandshakeMessage, HandshakeDecodeError> {
    let mut dec = Decoder::new(buf);
    let len = dec
        .array()
        .map_err(|e| HandshakeDecodeError::Cbor(format!("outer array: {e}")))?;
    let tag = dec
        .unsigned()
        .map_err(|e| HandshakeDecodeError::Cbor(format!("tag: {e}")))?;
    match tag {
        0 => {
            if len != 2 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 2,
                });
            }
            let versions = decode_versions(&mut dec)?;
            if state_is_propose {
                Ok(HandshakeMessage::ProposeVersions(versions))
            } else {
                Ok(HandshakeMessage::ReplyVersions(versions))
            }
        }
        1 => {
            if len != 3 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 3,
                });
            }
            let version_u64 = dec
                .unsigned()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("accept version: {e}")))?;
            let version = u32::try_from(version_u64)
                .map_err(|_| HandshakeDecodeError::VersionOutOfRange(version_u64))?;
            let data_cbor = dec
                .raw_value()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("accept data: {e}")))?
                .to_vec();
            Ok(HandshakeMessage::AcceptVersion { version, data_cbor })
        }
        2 => {
            if len != 2 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag,
                    got_len: len,
                    expected_len: 2,
                });
            }
            let reason = decode_refuse_reason(&mut dec)?;
            Ok(HandshakeMessage::Refuse(reason))
        }
        _ => Err(HandshakeDecodeError::UnknownTag { tag, len }),
    }
}

fn decode_versions(dec: &mut Decoder) -> Result<BTreeMap<u32, Vec<u8>>, HandshakeDecodeError> {
    let map_len = dec
        .map()
        .map_err(|e| HandshakeDecodeError::Cbor(format!("versions map: {e}")))?;
    let mut versions = BTreeMap::new();
    let mut prev: Option<u32> = None;
    for _ in 0..map_len {
        let version_u64 = dec
            .unsigned()
            .map_err(|e| HandshakeDecodeError::Cbor(format!("version key: {e}")))?;
        let version = u32::try_from(version_u64)
            .map_err(|_| HandshakeDecodeError::VersionOutOfRange(version_u64))?;
        if let Some(p) = prev {
            if version <= p {
                return Err(HandshakeDecodeError::UnorderedVersionMap);
            }
        }
        let data = dec
            .raw_value()
            .map_err(|e| HandshakeDecodeError::Cbor(format!("version data: {e}")))?
            .to_vec();
        versions.insert(version, data);
        prev = Some(version);
    }
    Ok(versions)
}

fn decode_refuse_reason(dec: &mut Decoder) -> Result<RefuseReason, HandshakeDecodeError> {
    let inner_len = dec
        .array()
        .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse outer array: {e}")))?;
    let inner_tag = dec
        .unsigned()
        .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse tag: {e}")))?;
    match inner_tag {
        0 => {
            if inner_len != 2 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag: inner_tag,
                    got_len: inner_len,
                    expected_len: 2,
                });
            }
            let list_len = dec
                .array()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse vmismatch list: {e}")))?;
            let mut versions = Vec::with_capacity(list_len as usize);
            for _ in 0..list_len {
                let v_u64 = dec.unsigned().map_err(|e| {
                    HandshakeDecodeError::Cbor(format!("refuse vmismatch element: {e}"))
                })?;
                let v = u32::try_from(v_u64)
                    .map_err(|_| HandshakeDecodeError::VersionOutOfRange(v_u64))?;
                versions.push(v);
            }
            Ok(RefuseReason::VersionMismatch(versions))
        }
        1 => {
            if inner_len != 3 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag: inner_tag,
                    got_len: inner_len,
                    expected_len: 3,
                });
            }
            let v_u64 = dec.unsigned().map_err(|e| {
                HandshakeDecodeError::Cbor(format!("refuse decode-err version: {e}"))
            })?;
            let version =
                u32::try_from(v_u64).map_err(|_| HandshakeDecodeError::VersionOutOfRange(v_u64))?;
            let message = dec
                .text()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse decode-err message: {e}")))?
                .to_string();
            Ok(RefuseReason::HandshakeDecodeError { version, message })
        }
        2 => {
            if inner_len != 3 {
                return Err(HandshakeDecodeError::ArityMismatch {
                    tag: inner_tag,
                    got_len: inner_len,
                    expected_len: 3,
                });
            }
            let v_u64 = dec
                .unsigned()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse refused version: {e}")))?;
            let version =
                u32::try_from(v_u64).map_err(|_| HandshakeDecodeError::VersionOutOfRange(v_u64))?;
            let reason = dec
                .text()
                .map_err(|e| HandshakeDecodeError::Cbor(format!("refuse refused reason: {e}")))?
                .to_string();
            Ok(RefuseReason::Refused { version, reason })
        }
        _ => Err(HandshakeDecodeError::UnknownRefuseTag(inner_tag)),
    }
}

#[cfg(test)]
mod handshake_tests {
    use super::*;

    /// Helper: encode `n` as a CBOR uint (single-byte for n ≤ 23,
    /// otherwise the additional-info-byte form). Used to pre-build
    /// version-data slots for the propose-versions tests.
    fn cbor_uint_bytes(n: u32) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.unsigned(u64::from(n));
        enc.into_bytes()
    }

    /// Round-trip MsgProposeVersions with three versions.
    #[test]
    fn propose_versions_round_trip() {
        let mut versions = BTreeMap::new();
        versions.insert(1u32, cbor_uint_bytes(764_824_073)); // mainnet magic
        versions.insert(2u32, cbor_uint_bytes(1)); // preprod
        versions.insert(3u32, cbor_uint_bytes(2)); // preview
        let msg = HandshakeMessage::ProposeVersions(versions);
        let bytes = encode_message(&msg);
        let decoded = decode_message(&bytes, true).expect("decode");
        assert_eq!(decoded, msg);
    }

    /// Round-trip MsgAcceptVersion.
    #[test]
    fn accept_version_round_trip() {
        let msg = HandshakeMessage::AcceptVersion {
            version: 2,
            data_cbor: cbor_uint_bytes(764_824_073),
        };
        let bytes = encode_message(&msg);
        let decoded = decode_message(&bytes, false).expect("decode");
        assert_eq!(decoded, msg);
    }

    /// Round-trip MsgRefuse with each of the three RefuseReason
    /// variants.
    #[test]
    fn refuse_round_trip_all_variants() {
        let cases = vec![
            HandshakeMessage::Refuse(RefuseReason::VersionMismatch(vec![1, 2, 3])),
            HandshakeMessage::Refuse(RefuseReason::HandshakeDecodeError {
                version: 2,
                message: "malformed version data".to_string(),
            }),
            HandshakeMessage::Refuse(RefuseReason::Refused {
                version: 2,
                reason: "wrong network magic".to_string(),
            }),
        ];
        for msg in cases {
            let bytes = encode_message(&msg);
            let decoded = decode_message(&bytes, false).expect("decode");
            assert_eq!(decoded, msg, "round-trip drift on {msg:?}");
        }
    }

    /// State-disambiguation: tag 0 + state=Confirm decodes as
    /// `ReplyVersions`; tag 0 + state=Propose decodes as
    /// `ProposeVersions`. Wire is identical; the caller's state
    /// is the tie-breaker.
    #[test]
    fn tag_zero_state_disambiguation() {
        let mut versions = BTreeMap::new();
        versions.insert(2u32, cbor_uint_bytes(1));
        // Build a `ProposeVersions` and encode it.
        let propose = HandshakeMessage::ProposeVersions(versions.clone());
        let bytes = encode_message(&propose);

        let as_propose = decode_message(&bytes, true).expect("propose");
        assert_eq!(as_propose, propose);

        // Same bytes decoded with state=Confirm yields ReplyVersions.
        let as_reply = decode_message(&bytes, false).expect("reply");
        assert_eq!(as_reply, HandshakeMessage::ReplyVersions(versions));
    }

    /// Unknown outer tag is rejected.
    #[test]
    fn decoder_rejects_unknown_tag() {
        let mut enc = Encoder::new();
        enc.array(1);
        enc.unsigned(99);
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes, true).expect_err("unknown tag");
        assert!(
            matches!(err, HandshakeDecodeError::UnknownTag { tag: 99, len: 1 }),
            "expected UnknownTag(99,1); got {err:?}"
        );
    }

    /// Unordered version map is rejected (keys must ascend).
    #[test]
    fn decoder_rejects_unordered_version_map() {
        // Hand-build a CBOR Propose with version keys 5 then 3
        // (descending), bypassing BTreeMap's auto-ordering.
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(0);
        enc.map(2);
        enc.unsigned(5);
        enc.unsigned(0); // version-data placeholder
        enc.unsigned(3);
        enc.unsigned(0);
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes, true).expect_err("unordered");
        assert!(
            matches!(err, HandshakeDecodeError::UnorderedVersionMap),
            "expected UnorderedVersionMap; got {err:?}"
        );
    }

    /// Versions out of u32 range rejected.
    #[test]
    fn decoder_rejects_version_out_of_range() {
        let mut enc = Encoder::new();
        enc.array(3);
        enc.unsigned(1);
        enc.unsigned(u64::MAX); // way past u32 max
        enc.unsigned(0);
        let bytes = enc.into_bytes();
        let err = decode_message(&bytes, false).expect_err("out of range");
        assert!(
            matches!(err, HandshakeDecodeError::VersionOutOfRange(_)),
            "expected VersionOutOfRange; got {err:?}"
        );
    }
}
