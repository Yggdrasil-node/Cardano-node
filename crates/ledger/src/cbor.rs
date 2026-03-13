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

    /// Encodes a signed integer using major type 0 (non-negative) or
    /// major type 1 (negative).
    pub fn integer(&mut self, value: i64) -> &mut Self {
        if value >= 0 {
            self.unsigned(value as u64)
        } else {
            // -(1 + n) = value  →  n = -(value + 1)
            self.negative((-1 - value) as u64)
        }
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

    /// Returns a borrowed sub-slice of the underlying input data.
    ///
    /// Useful for capturing a range of raw CBOR bytes (e.g. after using
    /// `position()` before and after a `skip()` call).
    pub fn slice(&self, start: usize, end: usize) -> Result<&'a [u8], LedgerError> {
        if end > self.data.len() || start > end {
            return Err(LedgerError::CborUnexpectedEof);
        }
        Ok(&self.data[start..end])
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

    /// Decodes a signed integer (CBOR major type 0 or 1) and returns an `i64`.
    ///
    /// Major type 0 yields a non-negative value; major type 1 yields
    /// `-(1 + n)` where `n` is the raw argument.
    pub fn integer(&mut self) -> Result<i64, LedgerError> {
        let major = self.peek_major()?;
        match major {
            MAJOR_UNSIGNED => {
                let v = self.unsigned()?;
                i64::try_from(v).map_err(|_| LedgerError::CborTypeMismatch {
                    expected: MAJOR_UNSIGNED,
                    actual: MAJOR_UNSIGNED,
                })
            }
            MAJOR_NEGATIVE => {
                let n = self.negative()?;
                // -(1 + n); guard against overflow
                let val = -1i64 - i64::try_from(n).map_err(|_| {
                    LedgerError::CborTypeMismatch {
                        expected: MAJOR_NEGATIVE,
                        actual: MAJOR_NEGATIVE,
                    }
                })?;
                Ok(val)
            }
            other => Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_UNSIGNED,
                actual: other,
            }),
        }
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

use crate::types::{
    Anchor, BlockNo, DCert, DRep, EpochNo, HeaderHash, Nonce, Point, PoolMetadata, PoolParams,
    Relay, RewardAccount, SlotNo, StakeCredential, TxId, UnitInterval,
};

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

// -- StakeCredential -------------------------------------------------------
//
// CDDL: credential = [0, addr_keyhash] / [1, scripthash]
//
// Encoded as a 2-element CBOR array: [tag, hash28].

impl CborEncode for StakeCredential {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        match self {
            Self::AddrKeyHash(h) => {
                enc.unsigned(0);
                enc.bytes(h);
            }
            Self::ScriptHash(h) => {
                enc.unsigned(1);
                enc.bytes(h);
            }
        }
    }
}

impl CborDecode for StakeCredential {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let tag = dec.unsigned()?;
        let raw = dec.bytes()?;
        let hash: [u8; 28] = raw
            .try_into()
            .map_err(|_| LedgerError::CborInvalidLength {
                expected: 28,
                actual: raw.len(),
            })?;
        match tag {
            0 => Ok(Self::AddrKeyHash(hash)),
            1 => Ok(Self::ScriptHash(hash)),
            _ => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

// -- RewardAccount ---------------------------------------------------------
//
// CDDL: reward_account = bytes .size 29
//
// Encoded as a CBOR byte string of exactly 29 bytes. The first byte
// encodes the network and credential type.

impl CborEncode for RewardAccount {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.bytes(&self.to_bytes());
    }
}

impl CborDecode for RewardAccount {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let raw = dec.bytes()?;
        RewardAccount::from_bytes(raw).ok_or(LedgerError::CborInvalidLength {
            expected: 29,
            actual: raw.len(),
        })
    }
}

// -- Anchor ----------------------------------------------------------------
//
// CDDL: anchor = [anchor_url : url, anchor_data_hash : $hash32]

impl CborEncode for Anchor {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).text(&self.url).bytes(&self.data_hash);
    }
}

impl CborDecode for Anchor {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let url = dec.text()?.to_owned();
        let raw = dec.bytes()?;
        let data_hash: [u8; 32] =
            raw.try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: raw.len(),
                })?;
        Ok(Self { url, data_hash })
    }
}

// -- UnitInterval ----------------------------------------------------------
//
// CDDL: unit_interval = #6.30([uint, positive_int])

impl CborEncode for UnitInterval {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.tag(30).array(2);
        enc.unsigned(self.numerator).unsigned(self.denominator);
    }
}

impl CborDecode for UnitInterval {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let t = dec.tag()?;
        if t != 30 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 30,
                actual: t as u8,
            });
        }
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let numerator = dec.unsigned()?;
        let denominator = dec.unsigned()?;
        Ok(Self {
            numerator,
            denominator,
        })
    }
}

// -- Relay -----------------------------------------------------------------
//
// CDDL:
//   relay =
//     [ 0, port / null, ipv4 / null, ipv6 / null ]
//   / [ 1, port / null, dns_name ]
//   / [ 2, dns_name ]

impl CborEncode for Relay {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::SingleHostAddr(port, ipv4, ipv6) => {
                enc.array(4).unsigned(0);
                match port {
                    Some(p) => { enc.unsigned(u64::from(*p)); }
                    None => { enc.null(); }
                }
                match ipv4 {
                    Some(ip) => { enc.bytes(ip); }
                    None => { enc.null(); }
                }
                match ipv6 {
                    Some(ip) => { enc.bytes(ip); }
                    None => { enc.null(); }
                }
            }
            Self::SingleHostName(port, name) => {
                enc.array(3).unsigned(1);
                match port {
                    Some(p) => { enc.unsigned(u64::from(*p)); }
                    None => { enc.null(); }
                }
                enc.text(name);
            }
            Self::MultiHostName(name) => {
                enc.array(2).unsigned(2).text(name);
            }
        }
    }
}

/// Decode an optional port: uint or null.
fn decode_optional_port(dec: &mut Decoder<'_>) -> Result<Option<u16>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(dec.unsigned()? as u16))
    }
}

/// Decode an optional IPv4 address: bstr .size 4 or null.
fn decode_optional_ipv4(dec: &mut Decoder<'_>) -> Result<Option<[u8; 4]>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        let raw = dec.bytes()?;
        let ip: [u8; 4] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 4,
            actual: raw.len(),
        })?;
        Ok(Some(ip))
    }
}

/// Decode an optional IPv6 address: bstr .size 16 or null.
fn decode_optional_ipv6(dec: &mut Decoder<'_>) -> Result<Option<[u8; 16]>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        let raw = dec.bytes()?;
        let ip: [u8; 16] =
            raw.try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 16,
                    actual: raw.len(),
                })?;
        Ok(Some(ip))
    }
}

impl CborDecode for Relay {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 => {
                if len != 4 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 4,
                        actual: len as usize,
                    });
                }
                let port = decode_optional_port(dec)?;
                let ipv4 = decode_optional_ipv4(dec)?;
                let ipv6 = decode_optional_ipv6(dec)?;
                Ok(Self::SingleHostAddr(port, ipv4, ipv6))
            }
            1 => {
                if len != 3 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 3,
                        actual: len as usize,
                    });
                }
                let port = decode_optional_port(dec)?;
                let name = dec.text()?.to_owned();
                Ok(Self::SingleHostName(port, name))
            }
            2 => {
                if len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: len as usize,
                    });
                }
                let name = dec.text()?.to_owned();
                Ok(Self::MultiHostName(name))
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 2,
                actual: tag as u8,
            }),
        }
    }
}

// -- PoolMetadata ----------------------------------------------------------
//
// CDDL: pool_metadata = [url, pool_metadata_hash]

impl CborEncode for PoolMetadata {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).text(&self.url).bytes(&self.metadata_hash);
    }
}

impl CborDecode for PoolMetadata {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let url = dec.text()?.to_owned();
        let raw = dec.bytes()?;
        let metadata_hash: [u8; 32] =
            raw.try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: raw.len(),
                })?;
        Ok(Self { url, metadata_hash })
    }
}

// -- PoolParams ------------------------------------------------------------
//
// CDDL: pool_params = ( operator, vrf_keyhash, pledge, cost, margin,
//                        reward_account, pool_owners, relays, pool_metadata )
//
// Encoded as a 9-element inline group (not a top-level array; the
// containing certificate array provides the context).

impl PoolParams {
    /// Encode pool params fields inline (no wrapping array).
    pub(crate) fn encode_inline(&self, enc: &mut Encoder) {
        enc.bytes(&self.operator);
        enc.bytes(&self.vrf_keyhash);
        enc.unsigned(self.pledge);
        enc.unsigned(self.cost);
        self.margin.encode_cbor(enc);
        self.reward_account.encode_cbor(enc);
        // pool_owners as a CBOR array of key hashes
        enc.array(self.pool_owners.len() as u64);
        for owner in &self.pool_owners {
            enc.bytes(owner);
        }
        // relays as a CBOR array
        enc.array(self.relays.len() as u64);
        for relay in &self.relays {
            relay.encode_cbor(enc);
        }
        // pool_metadata: value or null
        match &self.pool_metadata {
            Some(pm) => pm.encode_cbor(enc),
            None => { enc.null(); }
        }
    }

    /// Decode pool params fields inline (no wrapping array expected).
    pub(crate) fn decode_inline(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let raw_op = dec.bytes()?;
        let operator: [u8; 28] =
            raw_op
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 28,
                    actual: raw_op.len(),
                })?;
        let raw_vrf = dec.bytes()?;
        let vrf_keyhash: [u8; 32] =
            raw_vrf
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: raw_vrf.len(),
                })?;
        let pledge = dec.unsigned()?;
        let cost = dec.unsigned()?;
        let margin = UnitInterval::decode_cbor(dec)?;
        let reward_account = RewardAccount::decode_cbor(dec)?;
        // pool_owners
        let n_owners = dec.array()?;
        let mut pool_owners = Vec::with_capacity(n_owners as usize);
        for _ in 0..n_owners {
            let raw = dec.bytes()?;
            let h: [u8; 28] =
                raw.try_into()
                    .map_err(|_| LedgerError::CborInvalidLength {
                        expected: 28,
                        actual: raw.len(),
                    })?;
            pool_owners.push(h);
        }
        // relays
        let n_relays = dec.array()?;
        let mut relays = Vec::with_capacity(n_relays as usize);
        for _ in 0..n_relays {
            relays.push(Relay::decode_cbor(dec)?);
        }
        // pool_metadata
        let pool_metadata = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            Some(PoolMetadata::decode_cbor(dec)?)
        };
        Ok(Self {
            operator,
            vrf_keyhash,
            pledge,
            cost,
            margin,
            reward_account,
            pool_owners,
            relays,
            pool_metadata,
        })
    }
}

// -- DRep ------------------------------------------------------------------
//
// CDDL:
//   drep =
//     [0, addr_keyhash]
//   / [1, scripthash]
//   / [2]                ; always_abstain
//   / [3]                ; always_no_confidence

impl CborEncode for DRep {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::KeyHash(h) => {
                enc.array(2).unsigned(0).bytes(h);
            }
            Self::ScriptHash(h) => {
                enc.array(2).unsigned(1).bytes(h);
            }
            Self::AlwaysAbstain => {
                enc.array(1).unsigned(2);
            }
            Self::AlwaysNoConfidence => {
                enc.array(1).unsigned(3);
            }
        }
    }
}

impl CborDecode for DRep {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 | 1 => {
                if len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: len as usize,
                    });
                }
                let raw = dec.bytes()?;
                let hash: [u8; 28] =
                    raw.try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: raw.len(),
                        })?;
                if tag == 0 {
                    Ok(Self::KeyHash(hash))
                } else {
                    Ok(Self::ScriptHash(hash))
                }
            }
            2 => {
                if len != 1 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 1,
                        actual: len as usize,
                    });
                }
                Ok(Self::AlwaysAbstain)
            }
            3 => {
                if len != 1 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 1,
                        actual: len as usize,
                    });
                }
                Ok(Self::AlwaysNoConfidence)
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 3,
                actual: tag as u8,
            }),
        }
    }
}

// -- DCert -----------------------------------------------------------------
//
// CDDL: certificate = [tag, ...fields]
// Tags 0–5 (Shelley), 7–18 (Conway).

/// Decode an optional anchor: anchor / null.
fn decode_optional_anchor(dec: &mut Decoder<'_>) -> Result<Option<Anchor>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(Anchor::decode_cbor(dec)?))
    }
}

/// Encode an optional anchor: anchor / null.
fn encode_optional_anchor(anchor: &Option<Anchor>, enc: &mut Encoder) {
    match anchor {
        Some(a) => a.encode_cbor(enc),
        None => { enc.null(); }
    }
}

/// Decode a 28-byte hash from the decoder.
fn decode_hash28(dec: &mut Decoder<'_>) -> Result<[u8; 28], LedgerError> {
    let raw = dec.bytes()?;
    raw.try_into()
        .map_err(|_| LedgerError::CborInvalidLength {
            expected: 28,
            actual: raw.len(),
        })
}

/// Decode a 32-byte hash from the decoder.
fn decode_hash32(dec: &mut Decoder<'_>) -> Result<[u8; 32], LedgerError> {
    let raw = dec.bytes()?;
    raw.try_into()
        .map_err(|_| LedgerError::CborInvalidLength {
            expected: 32,
            actual: raw.len(),
        })
}

impl CborEncode for DCert {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::AccountRegistration(cred) => {
                enc.array(2).unsigned(0);
                cred.encode_cbor(enc);
            }
            Self::AccountUnregistration(cred) => {
                enc.array(2).unsigned(1);
                cred.encode_cbor(enc);
            }
            Self::DelegationToStakePool(cred, pool) => {
                enc.array(3).unsigned(2);
                cred.encode_cbor(enc);
                enc.bytes(pool);
            }
            Self::PoolRegistration(params) => {
                enc.array(10).unsigned(3);
                params.encode_inline(enc);
            }
            Self::PoolRetirement(pool, epoch) => {
                enc.array(3).unsigned(4);
                enc.bytes(pool);
                epoch.encode_cbor(enc);
            }
            Self::GenesisDelegation(genesis, deleg, vrf) => {
                enc.array(4).unsigned(5);
                enc.bytes(genesis).bytes(deleg).bytes(vrf);
            }
            Self::AccountRegistrationDeposit(cred, coin) => {
                enc.array(3).unsigned(7);
                cred.encode_cbor(enc);
                enc.unsigned(*coin);
            }
            Self::AccountUnregistrationDeposit(cred, coin) => {
                enc.array(3).unsigned(8);
                cred.encode_cbor(enc);
                enc.unsigned(*coin);
            }
            Self::DelegationToDrep(cred, drep) => {
                enc.array(3).unsigned(9);
                cred.encode_cbor(enc);
                drep.encode_cbor(enc);
            }
            Self::DelegationToStakePoolAndDrep(cred, pool, drep) => {
                enc.array(4).unsigned(10);
                cred.encode_cbor(enc);
                enc.bytes(pool);
                drep.encode_cbor(enc);
            }
            Self::AccountRegistrationDelegationToStakePool(cred, pool, coin) => {
                enc.array(4).unsigned(11);
                cred.encode_cbor(enc);
                enc.bytes(pool);
                enc.unsigned(*coin);
            }
            Self::AccountRegistrationDelegationToDrep(cred, drep, coin) => {
                enc.array(4).unsigned(12);
                cred.encode_cbor(enc);
                drep.encode_cbor(enc);
                enc.unsigned(*coin);
            }
            Self::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, coin) => {
                enc.array(5).unsigned(13);
                cred.encode_cbor(enc);
                enc.bytes(pool);
                drep.encode_cbor(enc);
                enc.unsigned(*coin);
            }
            Self::CommitteeAuthorization(cold, hot) => {
                enc.array(3).unsigned(14);
                cold.encode_cbor(enc);
                hot.encode_cbor(enc);
            }
            Self::CommitteeResignation(cold, anchor) => {
                enc.array(3).unsigned(15);
                cold.encode_cbor(enc);
                encode_optional_anchor(anchor, enc);
            }
            Self::DrepRegistration(cred, coin, anchor) => {
                enc.array(4).unsigned(16);
                cred.encode_cbor(enc);
                enc.unsigned(*coin);
                encode_optional_anchor(anchor, enc);
            }
            Self::DrepUnregistration(cred, coin) => {
                enc.array(3).unsigned(17);
                cred.encode_cbor(enc);
                enc.unsigned(*coin);
            }
            Self::DrepUpdate(cred, anchor) => {
                enc.array(3).unsigned(18);
                cred.encode_cbor(enc);
                encode_optional_anchor(anchor, enc);
            }
        }
    }
}

impl CborDecode for DCert {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let _len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            // Shelley tags 0–5
            0 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                Ok(Self::AccountRegistration(cred))
            }
            1 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                Ok(Self::AccountUnregistration(cred))
            }
            2 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let pool = decode_hash28(dec)?;
                Ok(Self::DelegationToStakePool(cred, pool))
            }
            3 => {
                let params = PoolParams::decode_inline(dec)?;
                Ok(Self::PoolRegistration(params))
            }
            4 => {
                let pool = decode_hash28(dec)?;
                let epoch = EpochNo::decode_cbor(dec)?;
                Ok(Self::PoolRetirement(pool, epoch))
            }
            5 => {
                let genesis = decode_hash28(dec)?;
                let deleg = decode_hash28(dec)?;
                let vrf = decode_hash32(dec)?;
                Ok(Self::GenesisDelegation(genesis, deleg, vrf))
            }
            // Conway tags 7–18
            7 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::AccountRegistrationDeposit(cred, coin))
            }
            8 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::AccountUnregistrationDeposit(cred, coin))
            }
            9 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let drep = DRep::decode_cbor(dec)?;
                Ok(Self::DelegationToDrep(cred, drep))
            }
            10 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let pool = decode_hash28(dec)?;
                let drep = DRep::decode_cbor(dec)?;
                Ok(Self::DelegationToStakePoolAndDrep(cred, pool, drep))
            }
            11 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let pool = decode_hash28(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::AccountRegistrationDelegationToStakePool(cred, pool, coin))
            }
            12 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let drep = DRep::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::AccountRegistrationDelegationToDrep(cred, drep, coin))
            }
            13 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let pool = decode_hash28(dec)?;
                let drep = DRep::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, coin))
            }
            14 => {
                let cold = StakeCredential::decode_cbor(dec)?;
                let hot = StakeCredential::decode_cbor(dec)?;
                Ok(Self::CommitteeAuthorization(cold, hot))
            }
            15 => {
                let cold = StakeCredential::decode_cbor(dec)?;
                let anchor = decode_optional_anchor(dec)?;
                Ok(Self::CommitteeResignation(cold, anchor))
            }
            16 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                let anchor = decode_optional_anchor(dec)?;
                Ok(Self::DrepRegistration(cred, coin, anchor))
            }
            17 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let coin = dec.unsigned()?;
                Ok(Self::DrepUnregistration(cred, coin))
            }
            18 => {
                let cred = StakeCredential::decode_cbor(dec)?;
                let anchor = decode_optional_anchor(dec)?;
                Ok(Self::DrepUpdate(cred, anchor))
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 18,
                actual: tag as u8,
            }),
        }
    }
}
