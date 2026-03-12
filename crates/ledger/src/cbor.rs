//! Minimal hand-rolled CBOR encoder/decoder for protocol-level types.
//!
//! This module implements just enough of RFC 8949 (CBOR) to handle the
//! core Cardano wire-format patterns: unsigned integers, byte strings,
//! definite-length arrays, and simple values.
//!
//! Reference: RFC 8949 — Concise Binary Object Representation (CBOR).

use crate::error::LedgerError;

// ───────────────────────────────────────────────────────────────────────────
// CBOR major types (3-bit, upper bits of initial byte)
// ───────────────────────────────────────────────────────────────────────────

/// Major type 0: unsigned integer.
const MAJOR_UNSIGNED: u8 = 0;
/// Major type 1: negative integer.
const MAJOR_NEGATIVE: u8 = 1;
/// Major type 2: byte string.
const MAJOR_BYTES: u8 = 2;
/// Major type 3: text string (UTF-8).
const MAJOR_TEXT: u8 = 3;
/// Major type 4: array.
const MAJOR_ARRAY: u8 = 4;
/// Major type 5: map.
const MAJOR_MAP: u8 = 5;
/// Major type 6: tagged data item.
const MAJOR_TAG: u8 = 6;
/// Major type 7: simple values and floats.
const MAJOR_SIMPLE: u8 = 7;

// ───────────────────────────────────────────────────────────────────────────
// Encoder
// ───────────────────────────────────────────────────────────────────────────

/// A lightweight CBOR encoder that writes into a growable byte buffer.
#[derive(Clone, Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    /// Creates a new encoder backed by an empty buffer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Creates a new encoder with a pre-allocated capacity hint.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Consumes the encoder and returns the encoded CBOR bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Returns a reference to the encoded bytes so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    // ── primitive writers ──────────────────────────────────────────────

    /// Encodes the CBOR initial byte and argument for a given major type.
    ///
    /// RFC 8949 §3.1: the initial byte contains the major type in the
    /// upper 3 bits and additional information in the lower 5 bits.
    fn write_type_and_arg(&mut self, major: u8, value: u64) {
        let mt = major << 5;
        if value < 24 {
            self.buf.push(mt | value as u8);
        } else if value <= u64::from(u8::MAX) {
            self.buf.push(mt | 24);
            self.buf.push(value as u8);
        } else if value <= u64::from(u16::MAX) {
            self.buf.push(mt | 25);
            self.buf.extend_from_slice(&(value as u16).to_be_bytes());
        } else if value <= u64::from(u32::MAX) {
            self.buf.push(mt | 26);
            self.buf.extend_from_slice(&(value as u32).to_be_bytes());
        } else {
            self.buf.push(mt | 27);
            self.buf.extend_from_slice(&value.to_be_bytes());
        }
    }

    /// Encodes an unsigned integer (CBOR major type 0).
    pub fn unsigned(&mut self, value: u64) -> &mut Self {
        self.write_type_and_arg(MAJOR_UNSIGNED, value);
        self
    }

    /// Encodes a byte string (CBOR major type 2).
    pub fn bytes(&mut self, data: &[u8]) -> &mut Self {
        self.write_type_and_arg(MAJOR_BYTES, data.len() as u64);
        self.buf.extend_from_slice(data);
        self
    }

    /// Encodes a definite-length array header (CBOR major type 4).
    ///
    /// The caller must encode exactly `len` items after this call.
    pub fn array(&mut self, len: u64) -> &mut Self {
        self.write_type_and_arg(MAJOR_ARRAY, len);
        self
    }

    /// Encodes the CBOR `null` value (major type 7, additional info 22).
    pub fn null(&mut self) -> &mut Self {
        self.buf.push((MAJOR_SIMPLE << 5) | 22);
        self
    }

    /// Encodes a CBOR boolean.
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.buf.push((MAJOR_SIMPLE << 5) | if value { 21 } else { 20 });
        self
    }

    /// Encodes a negative integer (CBOR major type 1).
    ///
    /// The CBOR encoding stores `-(1 + n)`, so `negative(0)` encodes `-1`.
    pub fn negative(&mut self, n: u64) -> &mut Self {
        self.write_type_and_arg(MAJOR_NEGATIVE, n);
        self
    }

    /// Encodes a UTF-8 text string (CBOR major type 3).
    pub fn text(&mut self, s: &str) -> &mut Self {
        self.write_type_and_arg(MAJOR_TEXT, s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
        self
    }

    /// Encodes a definite-length map header (CBOR major type 5).
    ///
    /// The caller must encode exactly `len` key-value pairs after this.
    pub fn map(&mut self, len: u64) -> &mut Self {
        self.write_type_and_arg(MAJOR_MAP, len);
        self
    }

    /// Encodes a CBOR tag (major type 6).
    ///
    /// The caller must encode exactly one data item after this call.
    /// Common tags: 258 (set), 24 (encoded CBOR), 2/3 (bignum).
    ///
    /// Reference: RFC 8949 §3.4.
    pub fn tag(&mut self, tag_number: u64) -> &mut Self {
        self.write_type_and_arg(MAJOR_TAG, tag_number);
        self
    }

    /// Writes raw bytes directly into the encoder buffer.
    ///
    /// Use with care — the caller is responsible for ensuring the bytes
    /// form valid CBOR.
    pub fn raw(&mut self, data: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(data);
        self
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Decoder
// ───────────────────────────────────────────────────────────────────────────

/// A lightweight CBOR decoder that reads from a byte slice.
#[derive(Clone, Debug)]
pub struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    /// Creates a new decoder over the given CBOR bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Returns the current read position.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Returns the number of remaining bytes.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Returns `true` when all bytes have been consumed.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    // ── primitive readers ──────────────────────────────────────────────

    fn peek_byte(&self) -> Result<u8, LedgerError> {
        self.data
            .get(self.pos)
            .copied()
            .ok_or(LedgerError::CborUnexpectedEof)
    }

    fn read_byte(&mut self) -> Result<u8, LedgerError> {
        let b = self.peek_byte()?;
        self.pos += 1;
        Ok(b)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], LedgerError> {
        if self.pos + len > self.data.len() {
            return Err(LedgerError::CborUnexpectedEof);
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    /// Reads the CBOR additional information argument for a given initial byte.
    fn read_argument(&mut self, initial: u8) -> Result<u64, LedgerError> {
        let ai = initial & 0x1f;
        match ai {
            0..=23 => Ok(u64::from(ai)),
            24 => Ok(u64::from(self.read_byte()?)),
            25 => {
                let bytes = self.read_exact(2)?;
                Ok(u64::from(u16::from_be_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| LedgerError::CborUnexpectedEof)?,
                )))
            }
            26 => {
                let bytes = self.read_exact(4)?;
                Ok(u64::from(u32::from_be_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| LedgerError::CborUnexpectedEof)?,
                )))
            }
            27 => {
                let bytes = self.read_exact(8)?;
                Ok(u64::from_be_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| LedgerError::CborUnexpectedEof)?,
                ))
            }
            _ => Err(LedgerError::CborInvalidAdditionalInfo(ai)),
        }
    }

    /// Reads the initial byte and validates the major type, returning the
    /// argument.
    fn expect_major(&mut self, expected: u8) -> Result<u64, LedgerError> {
        let initial = self.read_byte()?;
        let major = initial >> 5;
        if major != expected {
            return Err(LedgerError::CborTypeMismatch {
                expected,
                actual: major,
            });
        }
        self.read_argument(initial)
    }

    /// Decodes an unsigned integer (CBOR major type 0).
    pub fn unsigned(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_UNSIGNED)
    }

    /// Decodes a byte string (CBOR major type 2) and returns a borrowed
    /// slice into the input.
    pub fn bytes(&mut self) -> Result<&'a [u8], LedgerError> {
        let len = self.expect_major(MAJOR_BYTES)?;
        self.read_exact(len as usize)
    }

    /// Decodes a definite-length array header (CBOR major type 4) and
    /// returns the element count.
    pub fn array(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_ARRAY)
    }

    /// Decodes the CBOR `null` value.
    pub fn null(&mut self) -> Result<(), LedgerError> {
        let b = self.read_byte()?;
        if b == (MAJOR_SIMPLE << 5) | 22 {
            Ok(())
        } else {
            Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_SIMPLE,
                actual: b >> 5,
            })
        }
    }

    /// Peeks at the next major type without advancing the position.
    pub fn peek_major(&self) -> Result<u8, LedgerError> {
        Ok(self.peek_byte()? >> 5)
    }

    /// Decodes a negative integer (CBOR major type 1).
    ///
    /// Returns the raw argument `n`; the represented value is `-(1 + n)`.
    pub fn negative(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_NEGATIVE)
    }

    /// Decodes a UTF-8 text string (CBOR major type 3) and returns a
    /// borrowed `&str`.
    pub fn text(&mut self) -> Result<&'a str, LedgerError> {
        let len = self.expect_major(MAJOR_TEXT)?;
        let raw = self.read_exact(len as usize)?;
        core::str::from_utf8(raw).map_err(|_| LedgerError::CborTypeMismatch {
            expected: MAJOR_TEXT,
            actual: MAJOR_BYTES,
        })
    }

    /// Decodes a definite-length map header (CBOR major type 5) and
    /// returns the number of key-value pairs.
    pub fn map(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_MAP)
    }

    /// Decodes a CBOR tag (major type 6) and returns the tag number.
    ///
    /// The caller must then decode exactly one data item after this.
    ///
    /// Reference: RFC 8949 §3.4.
    pub fn tag(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_TAG)
    }

    /// Decodes a CBOR boolean (simple value 20 = false, 21 = true).
    pub fn bool(&mut self) -> Result<bool, LedgerError> {
        let b = self.read_byte()?;
        match b {
            v if v == (MAJOR_SIMPLE << 5) | 20 => Ok(false),
            v if v == (MAJOR_SIMPLE << 5) | 21 => Ok(true),
            _ => Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_SIMPLE,
                actual: b >> 5,
            }),
        }
    }

    /// Skips one complete CBOR data item (including nested structures).
    pub fn skip(&mut self) -> Result<(), LedgerError> {
        let initial = self.read_byte()?;
        let major = initial >> 5;
        match major {
            MAJOR_UNSIGNED | MAJOR_NEGATIVE => {
                let _ = self.read_argument(initial)?;
            }
            MAJOR_BYTES | MAJOR_TEXT => {
                let len = self.read_argument(initial)?;
                let _ = self.read_exact(len as usize)?;
            }
            MAJOR_ARRAY => {
                let count = self.read_argument(initial)?;
                for _ in 0..count {
                    self.skip()?;
                }
            }
            MAJOR_MAP => {
                let count = self.read_argument(initial)?;
                for _ in 0..count {
                    self.skip()?;
                    self.skip()?;
                }
            }
            MAJOR_SIMPLE => {
                // Simple values (false, true, null) have no payload.
                // Float16/32/64 have fixed-size payloads.
                let ai = initial & 0x1f;
                match ai {
                    0..=23 => {} // simple value, no extra bytes
                    24 => { let _ = self.read_byte()?; }
                    25 => { let _ = self.read_exact(2)?; }
                    26 => { let _ = self.read_exact(4)?; }
                    27 => { let _ = self.read_exact(8)?; }
                    _ => return Err(LedgerError::CborInvalidAdditionalInfo(ai)),
                }
            }
            // Major type 6 (tags)
            MAJOR_TAG => {
                let _ = self.read_argument(initial)?;
                self.skip()?;
            }
            _ => return Err(LedgerError::CborInvalidAdditionalInfo(major)),
        }
        Ok(())
    }
}

// ───────────────────────────────────────────────────────────────────────────
// CborEncode / CborDecode traits
// ───────────────────────────────────────────────────────────────────────────

/// Types that can be encoded to CBOR.
pub trait CborEncode {
    /// Encode `self` into the given CBOR encoder.
    fn encode_cbor(&self, enc: &mut Encoder);

    /// Convenience: encode into a fresh byte vector.
    fn to_cbor_bytes(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        self.encode_cbor(&mut enc);
        enc.into_bytes()
    }
}

/// Types that can be decoded from CBOR.
pub trait CborDecode: Sized {
    /// Decode an instance from the given CBOR decoder.
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError>;

    /// Convenience: decode from a byte slice, rejecting trailing bytes.
    fn from_cbor_bytes(data: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let val = Self::decode_cbor(&mut dec)?;
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(val)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Implementations for core types
// ───────────────────────────────────────────────────────────────────────────

use crate::types::{BlockNo, EpochNo, HeaderHash, Nonce, Point, SlotNo, TxId};

// -- SlotNo ----------------------------------------------------------------

impl CborEncode for SlotNo {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.unsigned(self.0);
    }
}

impl CborDecode for SlotNo {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        Ok(Self(dec.unsigned()?))
    }
}

// -- BlockNo ---------------------------------------------------------------

impl CborEncode for BlockNo {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.unsigned(self.0);
    }
}

impl CborDecode for BlockNo {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        Ok(Self(dec.unsigned()?))
    }
}

// -- EpochNo ---------------------------------------------------------------

impl CborEncode for EpochNo {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.unsigned(self.0);
    }
}

impl CborDecode for EpochNo {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        Ok(Self(dec.unsigned()?))
    }
}

// -- HeaderHash / TxId (32-byte hashes as CBOR byte strings) ---------------

impl CborEncode for HeaderHash {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.bytes(&self.0);
    }
}

impl CborDecode for HeaderHash {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let bs = dec.bytes()?;
        let arr: [u8; 32] = bs
            .try_into()
            .map_err(|_| LedgerError::CborInvalidLength { expected: 32, actual: bs.len() })?;
        Ok(Self(arr))
    }
}

impl CborEncode for TxId {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.bytes(&self.0);
    }
}

impl CborDecode for TxId {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let bs = dec.bytes()?;
        let arr: [u8; 32] = bs
            .try_into()
            .map_err(|_| LedgerError::CborInvalidLength { expected: 32, actual: bs.len() })?;
        Ok(Self(arr))
    }
}

// -- Point -----------------------------------------------------------------
//
// Encoding matches upstream `encodePoint`:
//   Origin      → array(0)
//   BlockPoint  → array(2, slot, hash)

impl CborEncode for Point {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Origin => {
                enc.array(0);
            }
            Self::BlockPoint(slot, hash) => {
                enc.array(2);
                slot.encode_cbor(enc);
                hash.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for Point {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        match len {
            0 => Ok(Self::Origin),
            2 => {
                let slot = SlotNo::decode_cbor(dec)?;
                let hash = HeaderHash::decode_cbor(dec)?;
                Ok(Self::BlockPoint(slot, hash))
            }
            _ => Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            }),
        }
    }
}

// -- Nonce -----------------------------------------------------------------
//
// Encoding matches upstream `Nonce`:
//   Neutral → array(0)
//   Hash(h) → array(1, bytes(h))

impl CborEncode for Nonce {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Neutral => {
                enc.array(0);
            }
            Self::Hash(h) => {
                enc.array(1);
                enc.bytes(h);
            }
        }
    }
}

impl CborDecode for Nonce {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        match len {
            0 => Ok(Self::Neutral),
            1 => {
                let bs = dec.bytes()?;
                let arr: [u8; 32] = bs
                    .try_into()
                    .map_err(|_| LedgerError::CborInvalidLength { expected: 32, actual: bs.len() })?;
                Ok(Self::Hash(arr))
            }
            _ => Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: len as usize,
            }),
        }
    }
}
