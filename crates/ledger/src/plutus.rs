//! PlutusData AST and Script types.
//!
//! This module provides a typed representation of Plutus data and scripts
//! as defined in the Cardano ledger CDDL specifications.
//!
//! ## PlutusData
//!
//! ```text
//! plutus_data = constr<plutus_data>
//!             / { * plutus_data => plutus_data }
//!             / [* plutus_data]
//!             / big_int
//!             / bounded_bytes
//!
//! constr<a0> = #6.121([* a0]) / #6.122([* a0]) / ... / #6.127([* a0])
//!            / #6.102([uint, [* a0]])
//!
//! big_int = int / big_uint / big_nint
//! big_uint = #6.2(bounded_bytes)
//! big_nint = #6.3(bounded_bytes)
//! ```
//!
//! ## Script
//!
//! ```text
//! script = [ 0, native_script
//!         // 1, plutus_v1_script
//!         // 2, plutus_v2_script
//!         // 3, plutus_v3_script ]
//!
//! script_ref = #6.24(bytes .cbor script)
//! ```
//!
//! References:
//! - <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/cddl/data/alonzo.cddl>
//! - <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/cddl/data/conway.cddl>

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::allegra::NativeScript;
use crate::error::LedgerError;

// ---------------------------------------------------------------------------
// PlutusData
// ---------------------------------------------------------------------------

/// Recursive Plutus data AST matching the upstream `plutus_data` CDDL.
///
/// Reference: `Cardano.Ledger.Plutus.Data` — `Data`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlutusData {
    /// Constructor application: `constr<plutus_data>`.
    ///
    /// Tags 121–127 encode alternatives 0–6 directly.
    /// Tag 102 encodes the general form `[alternative, [* fields]]`.
    Constr(u64, Vec<PlutusData>),
    /// Key-value map: `{ * plutus_data => plutus_data }`.
    Map(Vec<(PlutusData, PlutusData)>),
    /// Ordered list: `[* plutus_data]`.
    List(Vec<PlutusData>),
    /// Integer value: `big_int = int / #6.2(bounded_bytes) / #6.3(bounded_bytes)`.
    Integer(i128),
    /// Byte string: `bounded_bytes`.
    Bytes(Vec<u8>),
}

/// CBOR tag range for compact constructor alternatives 0–6.
const CONSTR_TAG_BASE: u64 = 121;
/// CBOR tag for the general constructor form.
const CONSTR_TAG_GENERAL: u64 = 102;
/// CBOR tag for big unsigned integer: `big_uint = #6.2(bounded_bytes)`.
const BIG_UINT_TAG: u64 = 2;
/// CBOR tag for big negative integer: `big_nint = #6.3(bounded_bytes)`.
const BIG_NINT_TAG: u64 = 3;

impl CborEncode for PlutusData {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Constr(alt, fields) => {
                if *alt <= 6 {
                    // Compact form: tags 121–127.
                    enc.tag(CONSTR_TAG_BASE + alt);
                    enc.array(fields.len() as u64);
                } else {
                    // General form: tag 102, [alternative, [* fields]].
                    enc.tag(CONSTR_TAG_GENERAL);
                    enc.array(2).unsigned(*alt);
                    enc.array(fields.len() as u64);
                }
                for field in fields {
                    field.encode_cbor(enc);
                }
            }
            Self::Map(entries) => {
                enc.map(entries.len() as u64);
                for (k, v) in entries {
                    k.encode_cbor(enc);
                    v.encode_cbor(enc);
                }
            }
            Self::List(items) => {
                enc.array(items.len() as u64);
                for item in items {
                    item.encode_cbor(enc);
                }
            }
            Self::Integer(n) => {
                encode_big_int(enc, *n);
            }
            Self::Bytes(b) => {
                enc.bytes(b);
            }
        }
    }
}

/// Encode a `big_int`: use plain CBOR int when it fits in i64,
/// otherwise use tagged bignum encoding.
fn encode_big_int(enc: &mut Encoder, n: i128) {
    if n >= 0 {
        if let Ok(u) = u64::try_from(n) {
            enc.unsigned(u);
        } else {
            // big_uint: #6.2(bounded_bytes)
            let bytes = n.to_be_bytes();
            let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
            enc.tag(BIG_UINT_TAG).bytes(&bytes[start..]);
        }
    } else {
        // CBOR negative: encode as -(1+n), where n is the unsigned magnitude
        let magnitude = (-1 - n) as u128;
        if let Ok(u) = u64::try_from(magnitude) {
            enc.negative(u);
        } else {
            // big_nint: #6.3(bounded_bytes) — encodes -(1+n) as big unsigned
            let bytes = magnitude.to_be_bytes();
            let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
            enc.tag(BIG_NINT_TAG).bytes(&bytes[start..]);
        }
    }
}

impl CborDecode for PlutusData {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        match major {
            // Unsigned integer (major 0).
            0 => {
                let v = dec.unsigned()?;
                Ok(Self::Integer(i128::from(v)))
            }
            // Negative integer (major 1).
            1 => {
                let v = dec.negative()?;
                // Decoder.negative() returns the magnitude n for CBOR -1-n.
                Ok(Self::Integer(-1 - i128::from(v)))
            }
            // Byte string (major 2).
            2 => {
                let b = dec.bytes_owned()?;
                Ok(Self::Bytes(b))
            }
            // Array (major 4) → List.
            4 => {
                let mut items = Vec::new();
                match dec.array_begin()? {
                    Some(len) => {
                        items.reserve(len as usize);
                        for _ in 0..len {
                            items.push(Self::decode_cbor(dec)?);
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            items.push(Self::decode_cbor(dec)?);
                        }
                        dec.consume_break()?;
                    }
                }
                Ok(Self::List(items))
            }
            // Map (major 5) → Map.
            5 => {
                let mut entries = Vec::new();
                match dec.map_begin()? {
                    Some(len) => {
                        entries.reserve(len as usize);
                        for _ in 0..len {
                            let k = Self::decode_cbor(dec)?;
                            let v = Self::decode_cbor(dec)?;
                            entries.push((k, v));
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            let k = Self::decode_cbor(dec)?;
                            let v = Self::decode_cbor(dec)?;
                            entries.push((k, v));
                        }
                        dec.consume_break()?;
                    }
                }
                Ok(Self::Map(entries))
            }
            // Tag (major 6) → constructor or big integer.
            6 => {
                let tag = dec.tag()?;
                match tag {
                    121..=127 => {
                        let alt = tag - CONSTR_TAG_BASE;
                        let mut fields = Vec::new();
                        match dec.array_begin()? {
                            Some(len) => {
                                fields.reserve(len as usize);
                                for _ in 0..len {
                                    fields.push(Self::decode_cbor(dec)?);
                                }
                            }
                            None => {
                                while !dec.is_break() {
                                    fields.push(Self::decode_cbor(dec)?);
                                }
                                dec.consume_break()?;
                            }
                        }
                        Ok(Self::Constr(alt, fields))
                    }
                    CONSTR_TAG_GENERAL => {
                        let outer_len = dec.array()?;
                        if outer_len != 2 {
                            return Err(LedgerError::CborInvalidLength {
                                expected: 2,
                                actual: outer_len as usize,
                            });
                        }
                        let alt = dec.unsigned()?;
                        let mut fields = Vec::new();
                        match dec.array_begin()? {
                            Some(len) => {
                                fields.reserve(len as usize);
                                for _ in 0..len {
                                    fields.push(Self::decode_cbor(dec)?);
                                }
                            }
                            None => {
                                while !dec.is_break() {
                                    fields.push(Self::decode_cbor(dec)?);
                                }
                                dec.consume_break()?;
                            }
                        }
                        Ok(Self::Constr(alt, fields))
                    }
                    BIG_UINT_TAG => {
                        // big_uint = #6.2(bounded_bytes)
                        let raw = dec.bytes()?;
                        let mut val: i128 = 0;
                        for &b in raw {
                            val = val.checked_shl(8).ok_or(LedgerError::CborTypeMismatch {
                                expected: 0,
                                actual: 0,
                            })? | i128::from(b);
                        }
                        Ok(Self::Integer(val))
                    }
                    BIG_NINT_TAG => {
                        // big_nint = #6.3(bounded_bytes) — value is -(1+n)
                        let raw = dec.bytes()?;
                        let mut magnitude: u128 = 0;
                        for &b in raw {
                            magnitude = magnitude
                                .checked_shl(8)
                                .ok_or(LedgerError::CborTypeMismatch {
                                    expected: 0,
                                    actual: 0,
                                })?
                                | u128::from(b);
                        }
                        let val = -1 - magnitude as i128;
                        Ok(Self::Integer(val))
                    }
                    _ => Err(LedgerError::CborTypeMismatch {
                        expected: 121,
                        actual: tag as u8,
                    }),
                }
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 0,
                actual: major,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Script
// ---------------------------------------------------------------------------

/// A Cardano script covering all eras from Allegra through Conway.
///
/// ```text
/// script = [ 0, native_script
///         // 1, plutus_v1_script
///         // 2, plutus_v2_script
///         // 3, plutus_v3_script ]
/// ```
///
/// Native scripts are fully typed; Plutus scripts are stored as opaque
/// byte blobs (`distinct_bytes` in CDDL).
///
/// Reference: `Cardano.Ledger.Core` — `Script`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Script {
    /// Tag 0: Timelock/native script.
    Native(NativeScript),
    /// Tag 1: Plutus V1 script bytes.
    PlutusV1(Vec<u8>),
    /// Tag 2: Plutus V2 script bytes.
    PlutusV2(Vec<u8>),
    /// Tag 3: Plutus V3 script bytes.
    PlutusV3(Vec<u8>),
}

impl Script {
    /// Returns the serialized binary size of the script content.
    ///
    /// For Plutus scripts, this is the length of the stored byte blob.
    /// For native scripts, this is the CBOR-encoded length.
    ///
    /// Reference: `Cardano.Ledger.Core` — `getScriptBinary`.
    pub fn binary_size(&self) -> usize {
        match self {
            Self::Native(ns) => ns.to_cbor_bytes().len(),
            Self::PlutusV1(bytes) | Self::PlutusV2(bytes) | Self::PlutusV3(bytes) => bytes.len(),
        }
    }
}

impl CborEncode for Script {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Native(ns) => {
                enc.array(2).unsigned(0);
                ns.encode_cbor(enc);
            }
            Self::PlutusV1(bytes) => {
                enc.array(2).unsigned(1).bytes(bytes);
            }
            Self::PlutusV2(bytes) => {
                enc.array(2).unsigned(2).bytes(bytes);
            }
            Self::PlutusV3(bytes) => {
                enc.array(2).unsigned(3).bytes(bytes);
            }
        }
    }
}

impl CborDecode for Script {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let tag = dec.unsigned()?;
        match tag {
            0 => Ok(Self::Native(NativeScript::decode_cbor(dec)?)),
            1 => Ok(Self::PlutusV1(dec.bytes()?.to_vec())),
            2 => Ok(Self::PlutusV2(dec.bytes()?.to_vec())),
            3 => Ok(Self::PlutusV3(dec.bytes()?.to_vec())),
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 0,
                actual: tag as u8,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// ScriptRef
// ---------------------------------------------------------------------------

/// An inline script reference carried in a transaction output.
///
/// CDDL: `script_ref = #6.24(bytes .cbor script)`
///
/// The outer CBOR tag 24 wraps a bytestring that contains a CBOR-encoded
/// `Script`. This type handles the double encoding transparently.
///
/// Reference: `Cardano.Ledger.Babbage.TxBody` — `ScriptRef`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptRef(pub Script);

impl CborEncode for ScriptRef {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // Encode inner Script to bytes, then wrap in tag 24.
        let inner_bytes = self.0.to_cbor_bytes();
        enc.tag(24).bytes(&inner_bytes);
    }
}

impl CborDecode for ScriptRef {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let tag = dec.tag()?;
        if tag != 24 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 24,
                actual: tag as u8,
            });
        }
        let inner_bytes = dec.bytes()?;
        let mut inner_dec = Decoder::new(inner_bytes);
        let script = Script::decode_cbor(&mut inner_dec)?;
        Ok(Self(script))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── PlutusData: Integer ────────────────────────────────────────────

    #[test]
    fn integer_zero_round_trip() {
        let d = PlutusData::Integer(0);
        let bytes = d.to_cbor_bytes();
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_positive_small_round_trip() {
        let d = PlutusData::Integer(42);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_u64_max_round_trip() {
        let d = PlutusData::Integer(i128::from(u64::MAX));
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_negative_small_round_trip() {
        let d = PlutusData::Integer(-1);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_negative_large_round_trip() {
        // Fits in CBOR negative int (magnitude fits in u64)
        let d = PlutusData::Integer(-1_000_000_000_000);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_big_uint_round_trip() {
        // Exceeds u64::MAX → uses tag 2 big_uint
        let big = i128::from(u64::MAX) + 1;
        let d = PlutusData::Integer(big);
        let bytes = d.to_cbor_bytes();
        // Should contain tag 2
        assert!(bytes.iter().any(|&b| b == 0xc2)); // tag(2) = 0xc2
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn integer_big_nint_round_trip() {
        // Exceeds i64 negative range → uses tag 3 big_nint
        // -(1 + magnitude) where magnitude > u64::MAX
        let val = -1 - i128::from(u64::MAX) - 1;
        let d = PlutusData::Integer(val);
        let bytes = d.to_cbor_bytes();
        // Should contain tag 3
        assert!(bytes.iter().any(|&b| b == 0xc3)); // tag(3) = 0xc3
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: Bytes ──────────────────────────────────────────────

    #[test]
    fn bytes_empty_round_trip() {
        let d = PlutusData::Bytes(vec![]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn bytes_non_empty_round_trip() {
        let d = PlutusData::Bytes(vec![0xde, 0xad, 0xbe, 0xef]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: List ───────────────────────────────────────────────

    #[test]
    fn list_empty_round_trip() {
        let d = PlutusData::List(vec![]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn list_with_items_round_trip() {
        let d = PlutusData::List(vec![
            PlutusData::Integer(1),
            PlutusData::Bytes(vec![0x01]),
            PlutusData::List(vec![]),
        ]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: Map ────────────────────────────────────────────────

    #[test]
    fn map_empty_round_trip() {
        let d = PlutusData::Map(vec![]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn map_with_entries_round_trip() {
        let d = PlutusData::Map(vec![
            (PlutusData::Integer(0), PlutusData::Bytes(vec![0xaa])),
            (PlutusData::Bytes(vec![0x01]), PlutusData::Integer(99)),
        ]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: Constr ─────────────────────────────────────────────

    #[test]
    fn constr_compact_tag_121_round_trip() {
        // Alternative 0 → tag 121
        let d = PlutusData::Constr(0, vec![PlutusData::Integer(1)]);
        let bytes = d.to_cbor_bytes();
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn constr_compact_tag_127_round_trip() {
        // Alternative 6 → tag 127
        let d = PlutusData::Constr(6, vec![]);
        let bytes = d.to_cbor_bytes();
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn constr_all_compact_alternatives() {
        for alt in 0..=6 {
            let d = PlutusData::Constr(alt, vec![PlutusData::Integer(alt as i128)]);
            let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
            assert_eq!(decoded, d, "Compact constructor alt={alt} failed");
        }
    }

    #[test]
    fn constr_general_form_round_trip() {
        // Alternative 7 → tag 102 (general form)
        let d = PlutusData::Constr(7, vec![PlutusData::Integer(42)]);
        let bytes = d.to_cbor_bytes();
        let decoded = PlutusData::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn constr_general_form_large_alt() {
        let d = PlutusData::Constr(1000, vec![]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn constr_nested_fields() {
        let d = PlutusData::Constr(
            0,
            vec![
                PlutusData::Constr(1, vec![PlutusData::Integer(10)]),
                PlutusData::List(vec![PlutusData::Bytes(vec![0xff])]),
            ],
        );
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: deeply nested ──────────────────────────────────────

    #[test]
    fn deeply_nested_round_trip() {
        let d = PlutusData::Map(vec![(
            PlutusData::Constr(
                0,
                vec![PlutusData::List(vec![
                    PlutusData::Integer(-100),
                    PlutusData::Bytes(vec![0x01, 0x02, 0x03]),
                ])],
            ),
            PlutusData::Constr(10, vec![PlutusData::Map(vec![])]),
        )]);
        let decoded = PlutusData::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    // ── PlutusData: decode errors ──────────────────────────────────────

    #[test]
    fn decode_unknown_tag_rejected() {
        // Tag 200 is not a valid PlutusData tag
        let mut enc = Encoder::new();
        enc.tag(200).unsigned(0);
        let bytes = enc.into_bytes();
        assert!(PlutusData::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn decode_text_string_rejected() {
        // Major type 3 (text) is not valid PlutusData
        let mut enc = Encoder::new();
        enc.text("hello");
        let bytes = enc.into_bytes();
        assert!(PlutusData::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn decode_general_constr_bad_length_rejected() {
        // Tag 102 with 3-element array (should be 2)
        let mut enc = Encoder::new();
        enc.tag(102).array(3).unsigned(0).array(0).unsigned(0);
        let bytes = enc.into_bytes();
        assert!(PlutusData::from_cbor_bytes(&bytes).is_err());
    }

    // ── encode_big_int internals ───────────────────────────────────────

    #[test]
    fn encode_big_int_small_positive_is_plain_unsigned() {
        let d = PlutusData::Integer(23);
        let bytes = d.to_cbor_bytes();
        // CBOR unsigned 23 = single byte 0x17
        assert_eq!(bytes, [0x17]);
    }

    #[test]
    fn encode_big_int_negative_one_is_plain_negative() {
        let d = PlutusData::Integer(-1);
        let bytes = d.to_cbor_bytes();
        // CBOR negative -1 (magnitude 0) = 0x20
        assert_eq!(bytes, [0x20]);
    }

    // ── Script ─────────────────────────────────────────────────────────

    #[test]
    fn script_plutus_v1_round_trip() {
        let s = Script::PlutusV1(vec![0x01, 0x02, 0x03]);
        let decoded = Script::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_plutus_v2_round_trip() {
        let s = Script::PlutusV2(vec![0xca, 0xfe]);
        let decoded = Script::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_plutus_v3_round_trip() {
        let s = Script::PlutusV3(vec![0xff]);
        let decoded = Script::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_native_round_trip() {
        // NativeScript::ScriptPubkey is tag 0
        let ns = NativeScript::ScriptPubkey([0xab; 28]);
        let s = Script::Native(ns);
        let decoded = Script::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_unknown_tag_rejected() {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(4).bytes(&[0x01]); // tag 4 invalid
        let bytes = enc.into_bytes();
        assert!(Script::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn script_bad_array_length_rejected() {
        let mut enc = Encoder::new();
        enc.array(3).unsigned(1).bytes(&[0x01]).unsigned(0);
        let bytes = enc.into_bytes();
        assert!(Script::from_cbor_bytes(&bytes).is_err());
    }

    // ── ScriptRef ──────────────────────────────────────────────────────

    #[test]
    fn script_ref_plutus_v1_round_trip() {
        let sr = ScriptRef(Script::PlutusV1(vec![0x01, 0x02]));
        let bytes = sr.to_cbor_bytes();
        let decoded = ScriptRef::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, sr);
    }

    #[test]
    fn script_ref_plutus_v3_round_trip() {
        let sr = ScriptRef(Script::PlutusV3(vec![0xaa, 0xbb, 0xcc]));
        let decoded = ScriptRef::from_cbor_bytes(&sr.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, sr);
    }

    #[test]
    fn script_ref_native_round_trip() {
        let sr = ScriptRef(Script::Native(NativeScript::ScriptPubkey([0x01; 28])));
        let decoded = ScriptRef::from_cbor_bytes(&sr.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, sr);
    }

    #[test]
    fn script_ref_wrong_tag_rejected() {
        // Tag 25 instead of 24
        let mut enc = Encoder::new();
        enc.tag(25).bytes(&[0x01]);
        let bytes = enc.into_bytes();
        assert!(ScriptRef::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn script_ref_double_encoding() {
        // Verify the double encoding: outer = tag24(bytes), inner = script CBOR
        let inner = Script::PlutusV2(vec![0xde, 0xad]);
        let inner_cbor = inner.to_cbor_bytes();
        let sr = ScriptRef(inner.clone());
        let outer = sr.to_cbor_bytes();
        // The outer CBOR should contain the inner bytes embedded
        // Decode manually: tag(24) + bstr(inner_cbor)
        let mut dec = Decoder::new(&outer);
        assert_eq!(dec.tag().unwrap(), 24);
        let payload = dec.bytes().unwrap();
        assert_eq!(payload, &inner_cbor);
    }

    // ── Indefinite-length PlutusData tests ───────────────────────────

    #[test]
    fn plutus_data_indefinite_list() {
        // 9f 01 02 03 ff = [_ 1, 2, 3]
        let data = [0x9f, 0x01, 0x02, 0x03, 0xff];
        let pd = PlutusData::from_cbor_bytes(&data).unwrap();
        assert_eq!(pd, PlutusData::List(vec![
            PlutusData::Integer(1),
            PlutusData::Integer(2),
            PlutusData::Integer(3),
        ]));
    }

    #[test]
    fn plutus_data_indefinite_map() {
        // bf 01 02 03 04 ff = {_ 1: 2, 3: 4}
        let data = [0xbf, 0x01, 0x02, 0x03, 0x04, 0xff];
        let pd = PlutusData::from_cbor_bytes(&data).unwrap();
        assert_eq!(pd, PlutusData::Map(vec![
            (PlutusData::Integer(1), PlutusData::Integer(2)),
            (PlutusData::Integer(3), PlutusData::Integer(4)),
        ]));
    }

    #[test]
    fn plutus_data_indefinite_bytes() {
        // 5f 42 0102 42 0304 ff = (_ h'0102', h'0304')
        let data = [0x5f, 0x42, 0x01, 0x02, 0x42, 0x03, 0x04, 0xff];
        let pd = PlutusData::from_cbor_bytes(&data).unwrap();
        assert_eq!(pd, PlutusData::Bytes(vec![0x01, 0x02, 0x03, 0x04]));
    }

    #[test]
    fn plutus_data_constr_indefinite_fields() {
        // d8 79 (tag 121) 9f 01 02 ff = Constr(0, [_ 1, 2])
        let data = [0xd8, 0x79, 0x9f, 0x01, 0x02, 0xff];
        let pd = PlutusData::from_cbor_bytes(&data).unwrap();
        assert_eq!(pd, PlutusData::Constr(0, vec![
            PlutusData::Integer(1),
            PlutusData::Integer(2),
        ]));
    }

    #[test]
    fn plutus_data_nested_indefinite() {
        // [_ {_ 1: [_ 2, 3]}, (_ h'ff')]
        #[rustfmt::skip]
        let data = [
            0x9f,                         // indef array
            0xbf,                         //   indef map
            0x01,                         //     key: 1
            0x9f, 0x02, 0x03, 0xff,       //     value: [_ 2, 3]
            0xff,                         //   end map
            0x5f, 0x41, 0xff, 0xff,       //   (_ h'ff')
            0xff,                         // end array
        ];
        let pd = PlutusData::from_cbor_bytes(&data).unwrap();
        assert_eq!(pd, PlutusData::List(vec![
            PlutusData::Map(vec![
                (PlutusData::Integer(1), PlutusData::List(vec![
                    PlutusData::Integer(2),
                    PlutusData::Integer(3),
                ])),
            ]),
            PlutusData::Bytes(vec![0xff]),
        ]));
    }
}
