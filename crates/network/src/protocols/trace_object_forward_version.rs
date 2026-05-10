//! Trace-forwarder handshake version codec — version-number
//! enum + version-data record used by the trace-forwarder
//! mini-protocol's handshake exchange.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Utils/Version.hs.
//!
//! Mirror of upstream's `ForwardingVersion` + `ForwardingVersionData`
//! types + their `CodecCBORTerm` encoders / decoders. Ports the
//! 2-version namespace upstream uses for trace-forwarder pipe
//! negotiation:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `data ForwardingVersion = ForwardingV_1 | ForwardingV_2` | [`ForwardingVersion`]            |
//! | `forwardingVersionCodec :: CodecCBORTerm ...`     | [`encode_forwarding_version`] + [`decode_forwarding_version`] |
//! | `newtype ForwardingVersionData { networkMagic }`  | [`ForwardingVersionData`]              |
//! | `instance Acceptable ForwardingVersionData`       | [`ForwardingVersionData::accept`]      |
//! | `instance Queryable ForwardingVersionData`        | [`ForwardingVersionData::is_queryable`] |
//! | `forwardingCodecCBORTerm :: ForwardingVersion -> CodecCBORTerm ...` | [`encode_forwarding_version_data`] + [`decode_forwarding_version_data`] |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Codec.CBOR.Term.Term` value-CBOR type**: upstream uses
//!   `Codec.CBOR.Term.Term` (the structured CBOR-value AST) as the
//!   intermediate type for handshake-version-data encoding.
//!   Yggdrasil's port emits the canonical bytes directly via
//!   [`yggdrasil_ledger::cbor::Encoder`] (matching the precedent in
//!   `crates/network/src/handshake/version.rs`); decoders accept
//!   raw bytes via [`yggdrasil_ledger::cbor::Decoder`]. The
//!   resulting on-the-wire encoding is byte-identical (CBOR's
//!   canonical form for `TInt` collapses to the same byte sequence).
//! - **`CodecCBORTerm` typeclass**: upstream's
//!   `CodecCBORTerm fail a` is a typeclass-style record carrying
//!   `encodeTerm` + `decodeTerm` functions. Yggdrasil's port
//!   exposes the encode/decode functions directly without the
//!   typeclass wrapper.
//! - **`NFData` / `Generic` deriving**: collapses to standard Rust
//!   `Clone + Debug + Eq + Hash` derives; no `NFData` analog
//!   needed in a strict-by-default language.

use yggdrasil_ledger::cbor::{Decoder, Encoder};

// ---------------------------------------------------------------------------
// ForwardingVersion
// ---------------------------------------------------------------------------

/// Trace-forwarder protocol-version tag exchanged at handshake.
/// Mirror of upstream's `data ForwardingVersion = ForwardingV_1 |
/// ForwardingV_2`.
///
/// CBOR wire format: `TInt 1` for `V1`, `TInt 2` for `V2` —
/// matching upstream's `encodeTerm` byte-for-byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ForwardingVersion {
    /// `ForwardingV_1` — initial protocol version.
    V1,
    /// `ForwardingV_2` — protocol version 2.
    V2,
}

impl ForwardingVersion {
    /// All known versions in upstream-canonical declaration order.
    pub const ALL: &'static [Self] = &[Self::V1, Self::V2];

    /// The wire tag: 1 for `V1`, 2 for `V2`.
    pub const fn tag(self) -> u8 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }
}

/// Errors from forwarding-version decoding.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ForwardingVersionDecodeError {
    /// Wire tag was not in the known-version set.
    #[error("decode ForwardingVersion: unknown tag: {0}")]
    UnknownTag(i64),
    /// Wire term was not an integer (upstream's
    /// `decode ForwardingVersion: unexpected term`).
    #[error("decode ForwardingVersion: unexpected term")]
    UnexpectedTerm,
}

/// Encode a [`ForwardingVersion`] as a CBOR `TInt`. Mirror of
/// upstream's `encodeTerm ForwardingV_1 = CBOR.TInt 1` /
/// `encodeTerm ForwardingV_2 = CBOR.TInt 2`.
pub fn encode_forwarding_version(version: ForwardingVersion) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.unsigned(u64::from(version.tag()));
    enc.into_bytes()
}

/// Decode a [`ForwardingVersion`] from a CBOR-encoded `TInt`.
/// Mirror of upstream's `decodeTerm` for `forwardingVersionCodec`.
///
/// Surfaces upstream's specific error messages:
/// - `(CBOR.TInt n)` with unknown `n` → [`ForwardingVersionDecodeError::UnknownTag`]
/// - any non-int term → [`ForwardingVersionDecodeError::UnexpectedTerm`]
pub fn decode_forwarding_version(
    bytes: &[u8],
) -> Result<ForwardingVersion, ForwardingVersionDecodeError> {
    let mut dec = Decoder::new(bytes);
    let n = dec
        .unsigned()
        .map_err(|_| ForwardingVersionDecodeError::UnexpectedTerm)?;
    let n_i64 = n as i64;
    match n_i64 {
        1 => Ok(ForwardingVersion::V1),
        2 => Ok(ForwardingVersion::V2),
        other => Err(ForwardingVersionDecodeError::UnknownTag(other)),
    }
}

// ---------------------------------------------------------------------------
// ForwardingVersionData
// ---------------------------------------------------------------------------

/// Per-version handshake payload — carries the network-magic of
/// the wire conversation. Mirror of upstream's
/// `newtype ForwardingVersionData { networkMagic :: NetworkMagic }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ForwardingVersionData {
    /// Cardano `NetworkMagic` value (32-bit unsigned). Mirror of
    /// upstream's `unNetworkMagic networkMagic`.
    pub network_magic: u32,
}

/// Outcome of an [`ForwardingVersionData::accept`] negotiation.
/// Mirror of upstream's `data Accept v = Accept v | Refuse Text`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptForwardingVersionData {
    /// Accept the version with the supplied data.
    Accept(ForwardingVersionData),
    /// Refuse with the supplied human-readable reason.
    Refuse(String),
}

impl ForwardingVersionData {
    /// Decide whether to accept the remote-supplied
    /// [`ForwardingVersionData`] given the local-supplied one.
    /// Mirror of upstream's `instance Acceptable
    /// ForwardingVersionData`:
    ///
    /// ```haskell
    /// acceptableVersion local remote
    ///   | local == remote = Accept local
    ///   | otherwise       = Refuse $ T.pack $ "ForwardingVersionData mismatch: " ++ show local ++ " /= " ++ show remote
    /// ```
    ///
    /// Yggdrasil emits the same human-readable refuse-string for
    /// operator-facing log parity.
    pub fn accept(local: Self, remote: Self) -> AcceptForwardingVersionData {
        if local == remote {
            AcceptForwardingVersionData::Accept(local)
        } else {
            AcceptForwardingVersionData::Refuse(format!(
                "ForwardingVersionData mismatch: {local:?} /= {remote:?}"
            ))
        }
    }

    /// Whether the version data carries query-mode semantics.
    /// Mirror of upstream's `instance Queryable ForwardingVersionData;
    /// queryVersion _ = False` — always false for trace-forwarder.
    pub const fn is_queryable(_self: Self) -> bool {
        false
    }
}

/// Errors from forwarding-version-data decoding.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ForwardingVersionDataDecodeError {
    /// Network-magic value was outside the 32-bit unsigned range.
    /// Mirror of upstream's
    /// `Left $ T.pack $ "networkMagic out of bound: " <> show x`.
    #[error("networkMagic out of bound: {0}")]
    OutOfBound(i64),
    /// Wire term was not an integer. Mirror of upstream's
    /// `Left $ T.pack $ "unknown encoding: " ++ show t`.
    #[error("unknown encoding")]
    UnknownEncoding,
}

/// Encode a [`ForwardingVersionData`] as a CBOR `TInt`. Mirror of
/// upstream's `forwardingCodecCBORTerm`'s `encodeTerm`. The
/// version arg is unused per upstream (the CBOR shape is the same
/// across versions); accepted for API parity.
pub fn encode_forwarding_version_data(
    _version: ForwardingVersion,
    data: ForwardingVersionData,
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.unsigned(u64::from(data.network_magic));
    enc.into_bytes()
}

/// Decode a [`ForwardingVersionData`] from a CBOR-encoded `TInt`.
/// Mirror of upstream's `forwardingCodecCBORTerm`'s `decodeTerm`.
///
/// Surfaces upstream's specific error messages:
/// - `TInt x` with `x < 0 || x > 0xffffffff` →
///   [`ForwardingVersionDataDecodeError::OutOfBound`]
/// - any non-int term → [`ForwardingVersionDataDecodeError::UnknownEncoding`]
pub fn decode_forwarding_version_data(
    _version: ForwardingVersion,
    bytes: &[u8],
) -> Result<ForwardingVersionData, ForwardingVersionDataDecodeError> {
    let mut dec = Decoder::new(bytes);
    let n = dec
        .unsigned()
        .map_err(|_| ForwardingVersionDataDecodeError::UnknownEncoding)?;
    if n > 0xffff_ffff {
        return Err(ForwardingVersionDataDecodeError::OutOfBound(n as i64));
    }
    Ok(ForwardingVersionData {
        network_magic: n as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarding_version_tags_match_upstream() {
        // Lock down the wire tags upstream defines: V_1 = 1, V_2 = 2.
        assert_eq!(ForwardingVersion::V1.tag(), 1);
        assert_eq!(ForwardingVersion::V2.tag(), 2);
    }

    #[test]
    fn forwarding_version_all_in_canonical_order() {
        assert_eq!(
            ForwardingVersion::ALL,
            &[ForwardingVersion::V1, ForwardingVersion::V2]
        );
    }

    #[test]
    fn encode_forwarding_version_round_trips_v1() {
        let bytes = encode_forwarding_version(ForwardingVersion::V1);
        // CBOR canonical form for unsigned 1 is a single byte 0x01.
        assert_eq!(bytes, vec![0x01]);
        assert_eq!(
            decode_forwarding_version(&bytes).expect("decode"),
            ForwardingVersion::V1
        );
    }

    #[test]
    fn encode_forwarding_version_round_trips_v2() {
        let bytes = encode_forwarding_version(ForwardingVersion::V2);
        assert_eq!(bytes, vec![0x02]);
        assert_eq!(
            decode_forwarding_version(&bytes).expect("decode"),
            ForwardingVersion::V2
        );
    }

    #[test]
    fn decode_forwarding_version_unknown_tag_errors() {
        // CBOR canonical form for unsigned 5 is single byte 0x05.
        let result = decode_forwarding_version(&[0x05]);
        assert_eq!(result, Err(ForwardingVersionDecodeError::UnknownTag(5)));
    }

    #[test]
    fn decode_forwarding_version_non_int_errors() {
        // CBOR text-string "x" — major type 3, length 1, char 0x78.
        let result = decode_forwarding_version(&[0x61, 0x78]);
        assert_eq!(result, Err(ForwardingVersionDecodeError::UnexpectedTerm));
    }

    #[test]
    fn forwarding_version_data_accept_matching_local_remote() {
        let local = ForwardingVersionData {
            network_magic: 764824073,
        };
        let remote = ForwardingVersionData {
            network_magic: 764824073,
        };
        let result = ForwardingVersionData::accept(local, remote);
        assert_eq!(
            result,
            AcceptForwardingVersionData::Accept(ForwardingVersionData {
                network_magic: 764824073,
            })
        );
    }

    #[test]
    fn forwarding_version_data_refuse_mismatched_magic() {
        let local = ForwardingVersionData { network_magic: 1 };
        let remote = ForwardingVersionData { network_magic: 2 };
        let result = ForwardingVersionData::accept(local, remote);
        match result {
            AcceptForwardingVersionData::Refuse(msg) => {
                assert!(msg.contains("ForwardingVersionData mismatch"));
                assert!(msg.contains("network_magic: 1"));
                assert!(msg.contains("network_magic: 2"));
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    #[test]
    fn forwarding_version_data_is_queryable_always_false() {
        let data = ForwardingVersionData { network_magic: 0 };
        assert!(!ForwardingVersionData::is_queryable(data));
    }

    #[test]
    fn encode_forwarding_version_data_round_trips_mainnet_magic() {
        let v = ForwardingVersion::V1;
        let data = ForwardingVersionData {
            network_magic: 764824073,
        };
        let bytes = encode_forwarding_version_data(v, data);
        let decoded = decode_forwarding_version_data(v, &bytes).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn encode_forwarding_version_data_round_trips_zero() {
        let v = ForwardingVersion::V2;
        let data = ForwardingVersionData { network_magic: 0 };
        let bytes = encode_forwarding_version_data(v, data);
        let decoded = decode_forwarding_version_data(v, &bytes).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn encode_forwarding_version_data_round_trips_max_u32() {
        let v = ForwardingVersion::V1;
        let data = ForwardingVersionData {
            network_magic: 0xffff_ffff,
        };
        let bytes = encode_forwarding_version_data(v, data);
        let decoded = decode_forwarding_version_data(v, &bytes).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn decode_forwarding_version_data_out_of_bound_errors() {
        // CBOR canonical form for unsigned 2^32 (4_294_967_296):
        // major type 0, additional info 27 (8-byte u64), then 8
        // big-endian bytes for the value 0x0000_0001_0000_0000.
        let bytes = vec![
            0x1B, // major 0 + ai 27
            0x00, 0x00, 0x00, 0x01, // upper 4 bytes
            0x00, 0x00, 0x00, 0x00, // lower 4 bytes
        ];
        let result = decode_forwarding_version_data(ForwardingVersion::V1, &bytes);
        match result {
            Err(ForwardingVersionDataDecodeError::OutOfBound(n)) => {
                assert_eq!(n, 0x1_0000_0000);
            }
            other => panic!("expected OutOfBound, got {other:?}"),
        }
    }

    #[test]
    fn decode_forwarding_version_data_non_int_errors() {
        // CBOR text-string "x".
        let result = decode_forwarding_version_data(ForwardingVersion::V1, &[0x61, 0x78]);
        assert_eq!(
            result,
            Err(ForwardingVersionDataDecodeError::UnknownEncoding)
        );
    }
}
