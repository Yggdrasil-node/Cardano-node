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
                let b = dec.bytes()?.to_vec();
                Ok(Self::Bytes(b))
            }
            // Array (major 4) → List.
            4 => {
                let len = dec.array()?;
                let mut items = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    items.push(Self::decode_cbor(dec)?);
                }
                Ok(Self::List(items))
            }
            // Map (major 5) → Map.
            5 => {
                let len = dec.map()?;
                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let k = Self::decode_cbor(dec)?;
                    let v = Self::decode_cbor(dec)?;
                    entries.push((k, v));
                }
                Ok(Self::Map(entries))
            }
            // Tag (major 6) → constructor or big integer.
            6 => {
                let tag = dec.tag()?;
                match tag {
                    121..=127 => {
                        let alt = tag - CONSTR_TAG_BASE;
                        let len = dec.array()?;
                        let mut fields = Vec::with_capacity(len as usize);
                        for _ in 0..len {
                            fields.push(Self::decode_cbor(dec)?);
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
                        let inner_len = dec.array()?;
                        let mut fields = Vec::with_capacity(inner_len as usize);
                        for _ in 0..inner_len {
                            fields.push(Self::decode_cbor(dec)?);
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
