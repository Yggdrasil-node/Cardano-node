//! Minimal hand-rolled CBOR encoder/decoder for protocol-level types.
//!
//! This module implements just enough of RFC 8949 (CBOR) to handle the
//! core Cardano wire-format patterns: unsigned integers, byte strings,
//! definite- and indefinite-length arrays/maps, and simple values.
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

/// Additional info value 31 signals indefinite-length for arrays, maps,
/// byte strings, and text strings (RFC 8949 §3.2.1).
const AI_INDEF: u8 = 31;

/// The "break" stop-code byte (major 7, additional info 31 = `0xff`).
const BREAK: u8 = (MAJOR_SIMPLE << 5) | AI_INDEF;

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
        self.buf
            .push((MAJOR_SIMPLE << 5) | if value { 21 } else { 20 });
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

    /// Alias for [`integer`](Self::integer) — encodes a signed `i64`.
    pub fn signed(&mut self, value: i64) -> &mut Self {
        self.integer(value)
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

    /// Writes a CBOR-in-CBOR wrapped item: `TAG(24) BYTES(data)`.
    ///
    /// This matches the Haskell `wrapCBORinCBOR` encoding used by
    /// Ouroboros for byte-string-wrapped protocol fields.
    pub fn wrapped(&mut self, data: &[u8]) -> &mut Self {
        self.tag(24).bytes(data);
        self
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Bounded-allocation helpers (defensive against attacker-controlled counts)
// ───────────────────────────────────────────────────────────────────────────

/// Default soft cap for block-body element vectors (inputs, outputs,
/// certs, withdrawals, etc.) when decoding CBOR arrays of unknown
/// upstream maximum.  An honest block is bounded by the ledger
/// `max_block_body_size` parameter (≤ 90 112 bytes on mainnet); this
/// constant only delays allocation, never rejects a valid block.
pub const BLOCK_BODY_ELEMENTS_MAX: usize = 1_048_576;

/// Allocates a `Vec` with capacity capped at `safe_max` regardless of the
/// peer-supplied `count`. The surrounding decoder loop is still free to
/// `push` beyond `safe_max` (the `Vec` grows naturally) — the cap merely
/// delays allocation so that a malformed `count` field cannot abort the
/// process via the global allocator's OOM handler.
///
/// Use this for fields where upstream Haskell does not enforce a hard
/// element-count limit (e.g. block-body inputs/outputs, which are
/// implicitly bounded by `max_block_body_size`).
///
/// Reference: RFC 8949 § 5.3 (CBOR decoders MUST NOT pre-allocate based
/// on a length field without bounds-checking).
#[inline]
pub fn vec_with_safe_capacity<T>(count: u64, safe_max: usize) -> Vec<T> {
    let cap = usize::try_from(count).unwrap_or(safe_max).min(safe_max);
    Vec::with_capacity(cap)
}

/// Strict variant of [`vec_with_safe_capacity`] — rejects the message
/// when `count` exceeds the per-protocol bound `max`.
///
/// Use this for fields where upstream Haskell enforces a hard count
/// limit (e.g. handshake version table, ChainSync intersect points,
/// TxSubmission txid batches).
#[inline]
pub fn vec_with_strict_capacity<T>(count: u64, max: usize) -> Result<Vec<T>, LedgerError> {
    let n = usize::try_from(count).map_err(|_| LedgerError::DecodedCountTooLarge { count, max })?;
    if n > max {
        return Err(LedgerError::DecodedCountTooLarge { count, max });
    }
    Ok(Vec::with_capacity(n))
}

/// `HashMap` analogue of [`vec_with_safe_capacity`].
#[inline]
pub fn hashmap_with_safe_capacity<K, V>(
    count: u64,
    safe_max: usize,
) -> std::collections::HashMap<K, V> {
    let cap = usize::try_from(count).unwrap_or(safe_max).min(safe_max);
    std::collections::HashMap::with_capacity(cap)
}

/// `BTreeMap` does not pre-allocate, but a strict-bound check is still
/// useful for its decoders. Returns the validated `usize` count or an
/// error.
#[inline]
pub fn check_decoded_count(count: u64, max: usize) -> Result<usize, LedgerError> {
    let n = usize::try_from(count).map_err(|_| LedgerError::DecodedCountTooLarge { count, max })?;
    if n > max {
        return Err(LedgerError::DecodedCountTooLarge { count, max });
    }
    Ok(n)
}

// ───────────────────────────────────────────────────────────────────────────
// Block-level raw-byte span extraction
// ───────────────────────────────────────────────────────────────────────────

/// Raw on-wire CBOR byte spans for the per-transaction fields of a block.
///
/// Cardano blocks are encoded as
/// `[header, [* tx_body], [* witness_set], { idx => metadata }, …]` where
/// the typed decoder produces high-level structs (`ShelleyTxBody`,
/// `ShelleyWitnessSet`, etc.).  Re-serializing those typed values produces
/// byte-canonical CBOR that does **not** always agree byte-for-byte with
/// the original on-wire encoding (e.g. set vs array, definite vs
/// indefinite length, integer-width canonicalization).
///
/// Several upstream Cardano invariants depend on the *exact* on-wire byte
/// layout — most importantly the linear fee formula
/// `min_fee = a · txSize + b` (`Cardano.Ledger.Shelley.Tx.minfee`) and the
/// transaction-id hash (`Cardano.Ledger.Core.txIdTxBody`).  When syncing
/// blocks from peers we **must** carry the original byte spans through to
/// fee-validation and tx-id computation; this is what `BlockTxRawSpans`
/// stores.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateFeeTooSmallUTxO`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockTxRawSpans {
    /// Raw CBOR bytes of each `transaction_body` as it appeared on the
    /// wire (parallel-indexed with `ShelleyBlock::transaction_bodies`,
    /// `ConwayBlock::transaction_bodies`, etc.).
    pub bodies: Vec<Vec<u8>>,
    /// Raw CBOR bytes of each `transaction_witness_set` as it appeared on
    /// the wire (parallel-indexed with `…::transaction_witness_sets`).
    pub witness_sets: Vec<Vec<u8>>,
}

/// Walk the outer block CBOR structure and capture the raw on-wire byte
/// spans of every `transaction_body` and `transaction_witness_set` entry.
///
/// Works for every Cardano post-Byron era because all of them encode the
/// inner block as `[header, [* tx_body], [* witness_set], …]` with the
/// bodies and witness sets at fixed array indices 1 and 2 respectively.
/// The helper also accepts hard-fork-combinator envelopes shaped
/// `[era_index, inner_block]` and unwraps them before walking the block.
/// It does NOT validate the typed contents; it just slices the raw CBOR
/// bytes between `dec.position()` markers.  Use the typed decoder
/// (`ShelleyBlock::decode_cbor`, etc.) for validation.
///
/// Returns `BlockTxRawSpans { bodies, witness_sets }` whose entries are
/// byte-for-byte identical to what the block author serialized.
///
/// Reference: `Cardano.Ledger.Shelley.Block` — block CBOR layout.
pub fn extract_block_tx_byte_spans(raw_block: &[u8]) -> Result<BlockTxRawSpans, LedgerError> {
    let mut envelope = Decoder::new(raw_block);
    if matches!(envelope.array(), Ok(2)) && envelope.unsigned().is_ok() {
        let inner = envelope.raw_value()?;
        if envelope.is_empty() {
            return extract_block_tx_byte_spans(inner);
        }
    }

    let mut dec = Decoder::new(raw_block);
    // Outer block array: at least 4 elements (header, bodies, witness_sets,
    // metadata) for Shelley/Allegra/Mary; 5 for Alonzo+ (adds invalid_txs).
    let outer_len = dec.array()?;
    if outer_len < 3 {
        return Err(LedgerError::CborInvalidLength {
            expected: 3,
            actual: outer_len as usize,
        });
    }
    // Element 0: header — skip past it.
    dec.skip()?;
    // Element 1: array of transaction bodies.
    let body_count = dec.array()?;
    let mut bodies: Vec<Vec<u8>> = vec_with_safe_capacity(body_count, BLOCK_BODY_ELEMENTS_MAX);
    for _ in 0..body_count {
        let start = dec.position();
        dec.skip()?;
        let end = dec.position();
        bodies.push(dec.slice(start, end)?.to_vec());
    }
    // Element 2: array of witness sets.
    let ws_count = dec.array()?;
    let mut witness_sets: Vec<Vec<u8>> = vec_with_safe_capacity(ws_count, BLOCK_BODY_ELEMENTS_MAX);
    for _ in 0..ws_count {
        let start = dec.position();
        dec.skip()?;
        let end = dec.position();
        witness_sets.push(dec.slice(start, end)?.to_vec());
    }
    Ok(BlockTxRawSpans {
        bodies,
        witness_sets,
    })
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

    /// Decodes a definite-length byte string (CBOR major type 2) and
    /// returns a borrowed slice into the input.
    ///
    /// For indefinite-length byte strings, use [`Self::bytes_owned`].
    pub fn bytes(&mut self) -> Result<&'a [u8], LedgerError> {
        self.bytes_definite()
    }

    /// Decodes a definite-length byte string (CBOR major type 2).
    fn bytes_definite(&mut self) -> Result<&'a [u8], LedgerError> {
        let len = self.expect_major(MAJOR_BYTES)?;
        self.read_exact(len as usize)
    }

    /// Decodes a byte string (CBOR major type 2) supporting both
    /// definite- and indefinite-length encoding.
    ///
    /// Indefinite-length byte strings (`0x5f` followed by definite-
    /// length chunks and a break stop-code `0xff`) are concatenated
    /// into an owned `Vec<u8>`.  Definite-length byte strings are
    /// returned without extra allocation.
    pub fn bytes_owned(&mut self) -> Result<Vec<u8>, LedgerError> {
        let initial = self.peek_byte()?;
        let major = initial >> 5;
        if major != MAJOR_BYTES {
            return Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_BYTES,
                actual: major,
            });
        }
        let ai = initial & 0x1f;
        if ai == AI_INDEF {
            self.pos += 1; // consume 0x5f
            let mut assembled = Vec::new();
            loop {
                if self.peek_byte()? == BREAK {
                    self.pos += 1; // consume 0xff
                    return Ok(assembled);
                }
                let chunk = self.bytes_definite()?;
                assembled.extend_from_slice(chunk);
            }
        } else {
            Ok(self.bytes_definite()?.to_vec())
        }
    }

    /// Decodes a definite-length array header (CBOR major type 4) and
    /// returns the element count.
    pub fn array(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_ARRAY)
    }

    /// Decodes an array that may be wrapped in CBOR tag 258 (set).
    ///
    /// The Cardano CDDL specification defines `set<T>` as
    /// `#6.258([* T])`.  The upstream Haskell decoder strips an
    /// optional leading tag 258 before decoding the inner array.
    /// Standard toolchains (cardano-cli) omit the tag, but third-party
    /// builders (cardano-serialization-lib, lucid) may include it.
    ///
    /// This method transparently handles both `[* T]` and
    /// `#6.258([* T])` forms.
    pub fn array_or_set(&mut self) -> Result<u64, LedgerError> {
        if self.peek_major()? == MAJOR_TAG {
            let saved = self.pos;
            let tag = self.tag()?;
            if tag != 258 {
                // Not a set tag — rewind and let array() produce the
                // correct type-mismatch error.
                self.pos = saved;
                return self.array();
            }
        }
        self.array()
    }

    /// Begins decoding an array (CBOR major type 4) that may be
    /// definite- or indefinite-length.
    ///
    /// Returns `Some(n)` for a definite-length array of `n` items, or
    /// `None` for an indefinite-length array.  When `None` is returned
    /// the caller must decode items in a loop and call
    /// [`is_break()`](Self::is_break) / [`consume_break()`](Self::consume_break)
    /// to detect the end.
    pub fn array_begin(&mut self) -> Result<Option<u64>, LedgerError> {
        let initial = self.peek_byte()?;
        let major = initial >> 5;
        if major != MAJOR_ARRAY {
            return Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_ARRAY,
                actual: major,
            });
        }
        if initial & 0x1f == AI_INDEF {
            self.pos += 1; // consume 0x9f
            Ok(None)
        } else {
            Ok(Some(self.array()?))
        }
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

    /// Returns `true` when the next byte is CBOR `null` (0xf6),
    /// without consuming it.
    pub fn peek_is_null(&self) -> bool {
        self.peek_byte().ok() == Some((MAJOR_SIMPLE << 5) | 22)
    }

    /// Returns `true` when the next byte is the CBOR break stop-code
    /// (`0xff`), without consuming it.
    ///
    /// Use this inside indefinite-length array/map iteration loops to
    /// detect the end of the container.
    pub fn is_break(&self) -> bool {
        self.peek_byte().ok() == Some(BREAK)
    }

    /// Consumes the CBOR break stop-code (`0xff`).
    ///
    /// Returns an error if the next byte is not `0xff`.
    pub fn consume_break(&mut self) -> Result<(), LedgerError> {
        let b = self.read_byte()?;
        if b == BREAK {
            Ok(())
        } else {
            Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_SIMPLE,
                actual: b >> 5,
            })
        }
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
                let val = -1i64
                    - i64::try_from(n).map_err(|_| LedgerError::CborTypeMismatch {
                        expected: MAJOR_NEGATIVE,
                        actual: MAJOR_NEGATIVE,
                    })?;
                Ok(val)
            }
            other => Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_UNSIGNED,
                actual: other,
            }),
        }
    }

    /// Alias for [`integer`](Self::integer) — decodes a signed `i64`.
    pub fn signed(&mut self) -> Result<i64, LedgerError> {
        self.integer()
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

    /// Decodes a UTF-8 text string (CBOR major type 3) supporting both
    /// definite- and indefinite-length encoding.
    pub fn text_owned(&mut self) -> Result<String, LedgerError> {
        let initial = self.peek_byte()?;
        let major = initial >> 5;
        if major != MAJOR_TEXT {
            return Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_TEXT,
                actual: major,
            });
        }
        if initial & 0x1f == AI_INDEF {
            self.pos += 1; // consume 0x7f
            let mut assembled = String::new();
            loop {
                if self.peek_byte()? == BREAK {
                    self.pos += 1; // consume 0xff
                    return Ok(assembled);
                }
                assembled.push_str(self.text()?);
            }
        } else {
            Ok(self.text()?.to_owned())
        }
    }

    /// Decodes a definite-length map header (CBOR major type 5) and
    /// returns the number of key-value pairs.
    pub fn map(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_MAP)
    }

    /// Begins decoding a map (CBOR major type 5) that may be
    /// definite- or indefinite-length.
    ///
    /// Returns `Some(n)` for a definite-length map of `n` entries, or
    /// `None` for an indefinite-length map.  When `None` is returned
    /// the caller must decode key-value pairs in a loop and call
    /// [`is_break()`](Self::is_break) / [`consume_break()`](Self::consume_break)
    /// to detect the end.
    pub fn map_begin(&mut self) -> Result<Option<u64>, LedgerError> {
        let initial = self.peek_byte()?;
        let major = initial >> 5;
        if major != MAJOR_MAP {
            return Err(LedgerError::CborTypeMismatch {
                expected: MAJOR_MAP,
                actual: major,
            });
        }
        if initial & 0x1f == AI_INDEF {
            self.pos += 1; // consume 0xbf
            Ok(None)
        } else {
            Ok(Some(self.map()?))
        }
    }

    /// Decodes a CBOR tag (major type 6) and returns the tag number.
    ///
    /// The caller must then decode exactly one data item after this.
    ///
    /// Reference: RFC 8949 §3.4.
    pub fn tag(&mut self) -> Result<u64, LedgerError> {
        self.expect_major(MAJOR_TAG)
    }

    /// Decodes a CBOR-in-CBOR wrapped item: `TAG(24) BYTES(data)`.
    ///
    /// This matches the Haskell `unwrapCBORinCBOR` decoding used by
    /// Ouroboros for byte-string-wrapped protocol fields.  Returns
    /// the inner byte string contents.
    pub fn wrapped(&mut self) -> Result<&'a [u8], LedgerError> {
        let t = self.tag()?;
        if t != 24 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 24,
                actual: t as u8,
            });
        }
        self.bytes()
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
    ///
    /// Handles both definite- and indefinite-length arrays, maps,
    /// byte strings, and text strings (RFC 8949 §3.2.1).
    pub fn skip(&mut self) -> Result<(), LedgerError> {
        let initial = self.read_byte()?;
        let major = initial >> 5;
        let ai = initial & 0x1f;
        match major {
            MAJOR_UNSIGNED | MAJOR_NEGATIVE => {
                let _ = self.read_argument(initial)?;
            }
            MAJOR_BYTES | MAJOR_TEXT => {
                if ai == AI_INDEF {
                    // Indefinite-length byte/text string: skip definite
                    // chunks until break stop-code.
                    loop {
                        if self.peek_byte()? == BREAK {
                            self.pos += 1;
                            break;
                        }
                        // Each chunk must be a definite-length item of the
                        // same major type (RFC 8949 §3.2.3).
                        self.skip()?;
                    }
                } else {
                    let len = self.read_argument(initial)?;
                    let _ = self.read_exact(len as usize)?;
                }
            }
            MAJOR_ARRAY => {
                if ai == AI_INDEF {
                    loop {
                        if self.peek_byte()? == BREAK {
                            self.pos += 1;
                            break;
                        }
                        self.skip()?;
                    }
                } else {
                    let count = self.read_argument(initial)?;
                    for _ in 0..count {
                        self.skip()?;
                    }
                }
            }
            MAJOR_MAP => {
                if ai == AI_INDEF {
                    loop {
                        if self.peek_byte()? == BREAK {
                            self.pos += 1;
                            break;
                        }
                        self.skip()?;
                        self.skip()?;
                    }
                } else {
                    let count = self.read_argument(initial)?;
                    for _ in 0..count {
                        self.skip()?;
                        self.skip()?;
                    }
                }
            }
            MAJOR_SIMPLE => {
                // Simple values (false, true, null) have no payload.
                // Float16/32/64 have fixed-size payloads.
                // ai == 31 is the break stop-code — should not appear as a
                // standalone item (only inside indefinite containers).
                match ai {
                    0..=23 => {} // simple value, no extra bytes
                    24 => {
                        let _ = self.read_byte()?;
                    }
                    25 => {
                        let _ = self.read_exact(2)?;
                    }
                    26 => {
                        let _ = self.read_exact(4)?;
                    }
                    27 => {
                        let _ = self.read_exact(8)?;
                    }
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

    /// Reads one complete CBOR data item and returns the raw bytes that
    /// comprise it.  This is useful for capturing inline CBOR values
    /// (e.g. tip or point structures) that should be stored as opaque
    /// byte vectors.
    pub fn raw_value(&mut self) -> Result<&'a [u8], LedgerError> {
        let start = self.pos;
        self.skip()?;
        self.slice(start, self.pos)
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

use crate::eras::Era;
use crate::types::{
    Anchor, BlockNo, DCert, DRep, EpochNo, HeaderHash, MirPot, MirTarget, Nonce, Point,
    PoolMetadata, PoolParams, Relay, RewardAccount, SlotNo, StakeCredential, Tip, TxId,
    UnitInterval,
};

// -- Era -------------------------------------------------------------------

impl CborEncode for Era {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let tag = match self {
            Self::Byron => 0,
            Self::Shelley => 1,
            Self::Allegra => 2,
            Self::Mary => 3,
            Self::Alonzo => 4,
            Self::Babbage => 5,
            Self::Conway => 6,
        };
        enc.unsigned(tag);
    }
}

impl CborDecode for Era {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        match dec.unsigned()? {
            0 => Ok(Self::Byron),
            1 => Ok(Self::Shelley),
            2 => Ok(Self::Allegra),
            3 => Ok(Self::Mary),
            4 => Ok(Self::Alonzo),
            5 => Ok(Self::Babbage),
            6 => Ok(Self::Conway),
            tag => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

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
        let arr: [u8; 32] = bs.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 32,
            actual: bs.len(),
        })?;
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
        let arr: [u8; 32] = bs.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 32,
            actual: bs.len(),
        })?;
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

// -- Tip -------------------------------------------------------------------
//
// Encoding matches upstream `ChainSync.Codec.encodeTip`:
//   TipGenesis  → array(0)
//   Tip(pt, bn) → array(2, point_cbor, blockNo)
// where point_cbor is the CBOR encoding of a Point ([] or [slot, hash]).

impl CborEncode for Tip {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::TipGenesis => {
                enc.array(0);
            }
            Self::Tip(point, bn) => {
                enc.array(2);
                point.encode_cbor(enc);
                bn.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for Tip {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        match len {
            0 => Ok(Self::TipGenesis),
            2 => {
                let point = Point::decode_cbor(dec)?;
                let block_no = BlockNo::decode_cbor(dec)?;
                Ok(Self::Tip(point, block_no))
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
                let arr: [u8; 32] = bs.try_into().map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: bs.len(),
                })?;
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
        let hash: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
        let data_hash: [u8; 32] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
                    Some(p) => {
                        enc.unsigned(u64::from(*p));
                    }
                    None => {
                        enc.null();
                    }
                }
                match ipv4 {
                    Some(ip) => {
                        enc.bytes(ip);
                    }
                    None => {
                        enc.null();
                    }
                }
                match ipv6 {
                    Some(ip) => {
                        enc.bytes(ip);
                    }
                    None => {
                        enc.null();
                    }
                }
            }
            Self::SingleHostName(port, name) => {
                enc.array(3).unsigned(1);
                match port {
                    Some(p) => {
                        enc.unsigned(u64::from(*p));
                    }
                    None => {
                        enc.null();
                    }
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
        let ip: [u8; 16] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
            raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
            None => {
                enc.null();
            }
        }
    }

    /// Decode pool params fields inline (no wrapping array expected).
    pub(crate) fn decode_inline(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let raw_op = dec.bytes()?;
        let operator: [u8; 28] = raw_op
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
        // pool_owners — Conway CDDL `nonempty_set<addr_keyhash>` =
        // `#6.258([+ addr_keyhash])`.  Upstream Haskell decoder
        // accepts both the legacy bare-array shape (Shelley→Babbage)
        // and the Conway tag-258 wrapped shape; `array_or_set()`
        // strips the optional tag transparently so an operator's
        // `cardano-cli stake-pool registration-certificate`
        // (Conway) parses cleanly here.
        let n_owners = dec.array_or_set()?;
        let mut pool_owners = vec_with_safe_capacity(n_owners, BLOCK_BODY_ELEMENTS_MAX);
        for _ in 0..n_owners {
            let raw = dec.bytes()?;
            let h: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                expected: 28,
                actual: raw.len(),
            })?;
            pool_owners.push(h);
        }
        // relays
        let n_relays = dec.array()?;
        let mut relays = vec_with_safe_capacity(n_relays, BLOCK_BODY_ELEMENTS_MAX);
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

impl CborEncode for PoolParams {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(9);
        self.encode_inline(enc);
    }
}

impl CborDecode for PoolParams {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 9 {
            return Err(LedgerError::CborInvalidLength {
                expected: 9,
                actual: len as usize,
            });
        }
        Self::decode_inline(dec)
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
                    raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
        None => {
            enc.null();
        }
    }
}

/// Decode a 28-byte hash from the decoder.
fn decode_hash28(dec: &mut Decoder<'_>) -> Result<[u8; 28], LedgerError> {
    let raw = dec.bytes()?;
    raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
        expected: 28,
        actual: raw.len(),
    })
}

/// Decode a 32-byte hash from the decoder.
fn decode_hash32(dec: &mut Decoder<'_>) -> Result<[u8; 32], LedgerError> {
    let raw = dec.bytes()?;
    raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
            Self::MoveInstantaneousReward(pot, target) => {
                enc.array(2).unsigned(6);
                // Inner: move_instantaneous_reward = [pot, target]
                enc.array(2).unsigned(*pot as u64);
                match target {
                    MirTarget::StakeCredentials(map) => {
                        enc.map(map.len() as u64);
                        for (cred, delta) in map {
                            cred.encode_cbor(enc);
                            enc.integer(*delta);
                        }
                    }
                    MirTarget::SendToOppositePot(coin) => {
                        enc.unsigned(*coin);
                    }
                }
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
            6 => {
                // move_instantaneous_rewards_cert = [6, move_instantaneous_reward]
                // move_instantaneous_reward = [pot, { * stake_credential => delta_coin } / coin]
                let _mir_len = dec.array()?;
                let pot_raw = dec.unsigned()?;
                let pot = match pot_raw {
                    0 => MirPot::Reserves,
                    1 => MirPot::Treasury,
                    _ => {
                        return Err(LedgerError::CborTypeMismatch {
                            expected: 1,
                            actual: pot_raw as u8,
                        });
                    }
                };
                let target = if dec.peek_major()? == MAJOR_MAP {
                    let n = dec.map()?;
                    let mut map = std::collections::BTreeMap::new();
                    for _ in 0..n {
                        let cred = StakeCredential::decode_cbor(dec)?;
                        let delta = dec.integer()?;
                        map.insert(cred, delta);
                    }
                    MirTarget::StakeCredentials(map)
                } else {
                    MirTarget::SendToOppositePot(dec.unsigned()?)
                };
                Ok(Self::MoveInstantaneousReward(pot, target))
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
                Ok(Self::AccountRegistrationDelegationToStakePool(
                    cred, pool, coin,
                ))
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
                Ok(Self::AccountRegistrationDelegationToStakePoolAndDrep(
                    cred, pool, drep, coin,
                ))
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

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── Encoder / Decoder round-trip: unsigned ──────────────────────────

    #[test]
    fn unsigned_zero() {
        let mut enc = Encoder::new();
        enc.unsigned(0);
        let bytes = enc.into_bytes();
        assert_eq!(bytes, [0x00]);
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.unsigned().unwrap(), 0);
        assert!(dec.is_empty());
    }

    #[test]
    fn unsigned_23_one_byte_boundary() {
        let mut enc = Encoder::new();
        enc.unsigned(23);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 1);
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 23);
    }

    #[test]
    fn unsigned_24_two_byte_boundary() {
        let mut enc = Encoder::new();
        enc.unsigned(24);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 2); // initial byte + 1 arg byte
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 24);
    }

    #[test]
    fn unsigned_255_u8_max() {
        let mut enc = Encoder::new();
        enc.unsigned(255);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 2);
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 255);
    }

    #[test]
    fn unsigned_256_u16_boundary() {
        let mut enc = Encoder::new();
        enc.unsigned(256);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 3); // initial byte + 2 arg bytes
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 256);
    }

    #[test]
    fn unsigned_65535_u16_max() {
        let mut enc = Encoder::new();
        enc.unsigned(65535);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 3);
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 65535);
    }

    #[test]
    fn unsigned_65536_u32_boundary() {
        let mut enc = Encoder::new();
        enc.unsigned(65536);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 5); // initial byte + 4 arg bytes
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 65536);
    }

    #[test]
    fn unsigned_u32_max() {
        let mut enc = Encoder::new();
        enc.unsigned(u32::MAX as u64);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 5);
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), u32::MAX as u64);
    }

    #[test]
    fn unsigned_u32_max_plus_one_u64_boundary() {
        let mut enc = Encoder::new();
        enc.unsigned(u32::MAX as u64 + 1);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 9); // initial byte + 8 arg bytes
        assert_eq!(
            Decoder::new(&bytes).unsigned().unwrap(),
            u32::MAX as u64 + 1
        );
    }

    #[test]
    fn unsigned_u64_max() {
        let mut enc = Encoder::new();
        enc.unsigned(u64::MAX);
        let bytes = enc.into_bytes();
        assert_eq!(bytes.len(), 9);
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), u64::MAX);
    }

    // ── Encoder / Decoder round-trip: negative ─────────────────────────

    #[test]
    fn negative_zero_means_minus_one() {
        let mut enc = Encoder::new();
        enc.negative(0);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.negative().unwrap(), 0); // raw arg
        // Through integer(): -(1+0) = -1
        let mut dec2 = Decoder::new(&bytes);
        assert_eq!(dec2.integer().unwrap(), -1);
    }

    #[test]
    fn integer_positive_round_trip() {
        let mut enc = Encoder::new();
        enc.integer(42);
        let bytes = enc.into_bytes();
        assert_eq!(Decoder::new(&bytes).integer().unwrap(), 42);
    }

    #[test]
    fn integer_negative_round_trip() {
        let mut enc = Encoder::new();
        enc.integer(-100);
        let bytes = enc.into_bytes();
        assert_eq!(Decoder::new(&bytes).integer().unwrap(), -100);
    }

    #[test]
    fn integer_i64_min() {
        let mut enc = Encoder::new();
        enc.integer(i64::MIN);
        let bytes = enc.into_bytes();
        assert_eq!(Decoder::new(&bytes).integer().unwrap(), i64::MIN);
    }

    // ── bytes ───────────────────────────────────────────────────────────

    #[test]
    fn bytes_empty() {
        let mut enc = Encoder::new();
        enc.bytes(&[]);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.bytes().unwrap(), &[] as &[u8]);
    }

    #[test]
    fn bytes_round_trip() {
        let data = b"hello world";
        let mut enc = Encoder::new();
        enc.bytes(data);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.bytes().unwrap(), data);
    }

    // ── text ────────────────────────────────────────────────────────────

    #[test]
    fn text_round_trip() {
        let mut enc = Encoder::new();
        enc.text("Cardano");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.text().unwrap(), "Cardano");
    }

    #[test]
    fn text_empty() {
        let mut enc = Encoder::new();
        enc.text("");
        let bytes = enc.into_bytes();
        assert_eq!(Decoder::new(&bytes).text().unwrap(), "");
    }

    // ── bool ────────────────────────────────────────────────────────────

    #[test]
    fn bool_true_round_trip() {
        let mut enc = Encoder::new();
        enc.bool(true);
        let bytes = enc.into_bytes();
        assert!(Decoder::new(&bytes).bool().unwrap());
    }

    #[test]
    fn bool_false_round_trip() {
        let mut enc = Encoder::new();
        enc.bool(false);
        let bytes = enc.into_bytes();
        assert!(!Decoder::new(&bytes).bool().unwrap());
    }

    // ── null ────────────────────────────────────────────────────────────

    #[test]
    fn null_round_trip() {
        let mut enc = Encoder::new();
        enc.null();
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert!(dec.peek_is_null());
        dec.null().unwrap();
        assert!(dec.is_empty());
    }

    // ── array ───────────────────────────────────────────────────────────

    #[test]
    fn array_round_trip() {
        let mut enc = Encoder::new();
        enc.array(3).unsigned(1).unsigned(2).unsigned(3);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.array().unwrap(), 3);
        assert_eq!(dec.unsigned().unwrap(), 1);
        assert_eq!(dec.unsigned().unwrap(), 2);
        assert_eq!(dec.unsigned().unwrap(), 3);
        assert!(dec.is_empty());
    }

    #[test]
    fn array_empty() {
        let mut enc = Encoder::new();
        enc.array(0);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.array().unwrap(), 0);
        assert!(dec.is_empty());
    }

    // ── map ─────────────────────────────────────────────────────────────

    #[test]
    fn map_round_trip() {
        let mut enc = Encoder::new();
        enc.map(1).unsigned(42).text("value");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.map().unwrap(), 1);
        assert_eq!(dec.unsigned().unwrap(), 42);
        assert_eq!(dec.text().unwrap(), "value");
    }

    // ── tag ─────────────────────────────────────────────────────────────

    #[test]
    fn tag_round_trip() {
        let mut enc = Encoder::new();
        enc.tag(24).bytes(b"inner");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.tag().unwrap(), 24);
        assert_eq!(dec.bytes().unwrap(), b"inner");
    }

    // ── peek_major ──────────────────────────────────────────────────────

    #[test]
    fn peek_major_does_not_consume() {
        let mut enc = Encoder::new();
        enc.unsigned(10);
        let bytes = enc.into_bytes();
        let dec = Decoder::new(&bytes);
        assert_eq!(dec.peek_major().unwrap(), 0); // MAJOR_UNSIGNED
        assert_eq!(dec.remaining(), bytes.len());
    }

    // ── skip ────────────────────────────────────────────────────────────

    #[test]
    fn skip_unsigned() {
        let mut enc = Encoder::new();
        enc.unsigned(999).text("after");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        dec.skip().unwrap();
        assert_eq!(dec.text().unwrap(), "after");
    }

    #[test]
    fn skip_nested_array() {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(1).array(1).unsigned(2);
        enc.text("sentinel");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        dec.skip().unwrap(); // skip entire array(2, 1, array(1, 2))
        assert_eq!(dec.text().unwrap(), "sentinel");
    }

    #[test]
    fn skip_map() {
        let mut enc = Encoder::new();
        enc.map(1).unsigned(0).bytes(b"x");
        enc.null();
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        dec.skip().unwrap(); // skip the map
        dec.null().unwrap();
    }

    #[test]
    fn skip_tag() {
        let mut enc = Encoder::new();
        enc.tag(30).array(2).unsigned(1).unsigned(2);
        enc.bool(true);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        dec.skip().unwrap(); // skip tagged item
        assert!(dec.bool().unwrap());
    }

    // ── slice ───────────────────────────────────────────────────────────

    #[test]
    fn slice_captures_range() {
        let mut enc = Encoder::new();
        enc.unsigned(1).unsigned(2).unsigned(3);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let start = dec.position();
        dec.unsigned().unwrap();
        let end = dec.position();
        let captured = dec.slice(start, end).unwrap();
        assert_eq!(captured, &[0x01]);
    }

    #[test]
    fn slice_out_of_range_error() {
        let bytes = [0x01];
        let dec = Decoder::new(&bytes);
        assert!(dec.slice(0, 10).is_err());
    }

    // ── position / remaining / is_empty ─────────────────────────────────

    #[test]
    fn position_remaining_is_empty() {
        let mut enc = Encoder::new();
        enc.unsigned(5);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.position(), 0);
        assert_eq!(dec.remaining(), 1);
        assert!(!dec.is_empty());
        dec.unsigned().unwrap();
        assert_eq!(dec.position(), 1);
        assert_eq!(dec.remaining(), 0);
        assert!(dec.is_empty());
    }

    // ── with_capacity ───────────────────────────────────────────────────

    #[test]
    fn encoder_with_capacity() {
        let enc = Encoder::with_capacity(128);
        assert!(enc.as_bytes().is_empty());
    }

    // ── raw ─────────────────────────────────────────────────────────────

    #[test]
    fn raw_passthrough() {
        let mut enc = Encoder::new();
        let inner = {
            let mut e2 = Encoder::new();
            e2.unsigned(42);
            e2.into_bytes()
        };
        enc.raw(&inner);
        let bytes = enc.into_bytes();
        assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 42);
    }

    // ── Error conditions ────────────────────────────────────────────────

    #[test]
    fn decode_empty_input_eof() {
        let mut dec = Decoder::new(&[]);
        assert!(dec.unsigned().is_err());
    }

    #[test]
    fn decode_type_mismatch_unsigned_vs_bytes() {
        let mut enc = Encoder::new();
        enc.bytes(b"data");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert!(dec.unsigned().is_err());
    }

    #[test]
    fn decode_type_mismatch_text_vs_unsigned() {
        let mut enc = Encoder::new();
        enc.unsigned(42);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        assert!(dec.text().is_err());
    }

    #[test]
    fn decode_truncated_bytes() {
        // byte string header says length 10 but only 2 bytes follow
        let bytes = [0x4a, 0x01, 0x02];
        let mut dec = Decoder::new(&bytes);
        assert!(dec.bytes().is_err());
    }

    #[test]
    fn from_cbor_bytes_rejects_trailing() {
        let mut enc = Encoder::new();
        enc.unsigned(1).unsigned(2);
        let bytes = enc.into_bytes();
        // SlotNo::from_cbor_bytes should reject the trailing unsigned(2)
        assert!(SlotNo::from_cbor_bytes(&bytes).is_err());
    }

    // ── CborEncode / CborDecode trait round-trips ───────────────────────

    #[test]
    fn era_round_trip_all_variants() {
        for (tag, era) in [
            (0u64, Era::Byron),
            (1, Era::Shelley),
            (2, Era::Allegra),
            (3, Era::Mary),
            (4, Era::Alonzo),
            (5, Era::Babbage),
            (6, Era::Conway),
        ] {
            let encoded = era.to_cbor_bytes();
            let decoded = Era::from_cbor_bytes(&encoded).unwrap();
            assert_eq!(decoded, era, "Era tag {tag}");
        }
    }

    #[test]
    fn slot_no_round_trip() {
        let slot = SlotNo(123_456_789);
        let decoded = SlotNo::from_cbor_bytes(&slot.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, slot);
    }

    #[test]
    fn block_no_round_trip() {
        let bn = BlockNo(42);
        assert_eq!(BlockNo::from_cbor_bytes(&bn.to_cbor_bytes()).unwrap(), bn);
    }

    #[test]
    fn epoch_no_round_trip() {
        let en = EpochNo(500);
        assert_eq!(EpochNo::from_cbor_bytes(&en.to_cbor_bytes()).unwrap(), en);
    }

    #[test]
    fn header_hash_round_trip() {
        let hh = HeaderHash([0xab; 32]);
        assert_eq!(
            HeaderHash::from_cbor_bytes(&hh.to_cbor_bytes()).unwrap(),
            hh
        );
    }

    #[test]
    fn tx_id_round_trip() {
        let txid = TxId([0xcd; 32]);
        assert_eq!(TxId::from_cbor_bytes(&txid.to_cbor_bytes()).unwrap(), txid);
    }

    #[test]
    fn point_origin_round_trip() {
        let pt = Point::Origin;
        assert_eq!(Point::from_cbor_bytes(&pt.to_cbor_bytes()).unwrap(), pt);
    }

    #[test]
    fn point_block_round_trip() {
        let pt = Point::BlockPoint(SlotNo(100), HeaderHash([0x11; 32]));
        assert_eq!(Point::from_cbor_bytes(&pt.to_cbor_bytes()).unwrap(), pt);
    }

    #[test]
    fn nonce_neutral_round_trip() {
        let n = Nonce::Neutral;
        assert_eq!(Nonce::from_cbor_bytes(&n.to_cbor_bytes()).unwrap(), n);
    }

    #[test]
    fn nonce_hash_round_trip() {
        let n = Nonce::Hash([0xff; 32]);
        assert_eq!(Nonce::from_cbor_bytes(&n.to_cbor_bytes()).unwrap(), n);
    }

    #[test]
    fn stake_credential_keyhash_round_trip() {
        let cred = StakeCredential::AddrKeyHash([0x01; 28]);
        assert_eq!(
            StakeCredential::from_cbor_bytes(&cred.to_cbor_bytes()).unwrap(),
            cred
        );
    }

    #[test]
    fn stake_credential_scripthash_round_trip() {
        let cred = StakeCredential::ScriptHash([0x02; 28]);
        assert_eq!(
            StakeCredential::from_cbor_bytes(&cred.to_cbor_bytes()).unwrap(),
            cred
        );
    }

    #[test]
    fn reward_account_round_trip() {
        let ra = RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x0a; 28]),
        };
        assert_eq!(
            RewardAccount::from_cbor_bytes(&ra.to_cbor_bytes()).unwrap(),
            ra
        );
    }

    #[test]
    fn anchor_round_trip() {
        let a = Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xee; 32],
        };
        assert_eq!(Anchor::from_cbor_bytes(&a.to_cbor_bytes()).unwrap(), a);
    }

    #[test]
    fn unit_interval_round_trip() {
        let ui = UnitInterval {
            numerator: 1,
            denominator: 3,
        };
        assert_eq!(
            UnitInterval::from_cbor_bytes(&ui.to_cbor_bytes()).unwrap(),
            ui
        );
    }

    #[test]
    fn relay_single_host_addr_round_trip() {
        let r = Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 1]), None);
        assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
    }

    #[test]
    fn relay_single_host_name_round_trip() {
        let r = Relay::SingleHostName(Some(3001), "relay.example.com".to_string());
        assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
    }

    #[test]
    fn relay_multi_host_name_round_trip() {
        let r = Relay::MultiHostName("pool.example.com".to_string());
        assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
    }

    #[test]
    fn pool_metadata_round_trip() {
        let pm = PoolMetadata {
            url: "https://meta.pool.io".to_string(),
            metadata_hash: [0xdd; 32],
        };
        assert_eq!(
            PoolMetadata::from_cbor_bytes(&pm.to_cbor_bytes()).unwrap(),
            pm
        );
    }

    #[test]
    fn pool_params_round_trip() {
        let pp = PoolParams {
            operator: [0x01; 28],
            vrf_keyhash: [0x02; 32],
            pledge: 1_000_000,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x03; 28]),
            },
            pool_owners: vec![[0x04; 28]],
            relays: vec![Relay::SingleHostName(Some(3001), "r.io".to_string())],
            pool_metadata: None,
        };
        assert_eq!(
            PoolParams::from_cbor_bytes(&pp.to_cbor_bytes()).unwrap(),
            pp
        );
    }

    #[test]
    fn drep_all_variants_round_trip() {
        for drep in [
            DRep::KeyHash([0x01; 28]),
            DRep::ScriptHash([0x02; 28]),
            DRep::AlwaysAbstain,
            DRep::AlwaysNoConfidence,
        ] {
            let decoded = DRep::from_cbor_bytes(&drep.to_cbor_bytes()).unwrap();
            assert_eq!(decoded, drep);
        }
    }

    #[test]
    fn dcert_shelley_tags_round_trip() {
        let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
        let pool = [0x0b; 28];
        let certs = vec![
            DCert::AccountRegistration(cred),
            DCert::AccountUnregistration(cred),
            DCert::DelegationToStakePool(cred, pool),
            DCert::PoolRetirement(pool, EpochNo(100)),
            DCert::GenesisDelegation([0x01; 28], [0x02; 28], [0x03; 32]),
        ];
        for cert in certs {
            let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
            assert_eq!(decoded, cert);
        }
    }

    #[test]
    fn dcert_conway_tags_round_trip() {
        let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
        let pool = [0x0b; 28];
        let drep = DRep::KeyHash([0x0c; 28]);
        let anchor = Some(Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xee; 32],
        });
        let certs = vec![
            DCert::AccountRegistrationDeposit(cred, 2_000_000),
            DCert::AccountUnregistrationDeposit(cred, 2_000_000),
            DCert::DelegationToDrep(cred, drep),
            DCert::DelegationToStakePoolAndDrep(cred, pool, drep),
            DCert::AccountRegistrationDelegationToStakePool(cred, pool, 2_000_000),
            DCert::AccountRegistrationDelegationToDrep(cred, drep, 2_000_000),
            DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, 2_000_000),
            DCert::CommitteeAuthorization(cred, StakeCredential::ScriptHash([0x0d; 28])),
            DCert::CommitteeResignation(cred, anchor.clone()),
            DCert::DrepRegistration(cred, 500_000_000, anchor.clone()),
            DCert::DrepUnregistration(cred, 500_000_000),
            DCert::DrepUpdate(cred, None),
        ];
        for cert in certs {
            let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
            assert_eq!(decoded, cert);
        }
    }

    #[test]
    fn dcert_mir_stake_credentials_round_trip() {
        let mut map = std::collections::BTreeMap::new();
        map.insert(StakeCredential::AddrKeyHash([0x01; 28]), 100i64);
        map.insert(StakeCredential::ScriptHash([0x02; 28]), -50i64);
        let cert =
            DCert::MoveInstantaneousReward(MirPot::Reserves, MirTarget::StakeCredentials(map));
        let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, cert);
    }

    #[test]
    fn dcert_mir_send_to_pot_round_trip() {
        let cert = DCert::MoveInstantaneousReward(
            MirPot::Treasury,
            MirTarget::SendToOppositePot(1_000_000),
        );
        let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, cert);
    }

    /// Encoder-side drift guard for the full `DCert` wire-tag space.
    ///
    /// Closes a subtle gap left by the round-trip tests above: a coupled
    /// typo where the encoder and decoder agree on a wrong tag value
    /// (e.g. both `enc.unsigned(0)` for `AccountRegistration` mistakenly
    /// becomes `enc.unsigned(1)` AND the decoder's `1 => Account
    /// Registration` arm is added in lockstep) would still round-trip
    /// cleanly while silently breaking on-chain wire compat with upstream.
    ///
    /// For every variant in the 0..=18 tag space, this test:
    ///   1. Constructs a representative value
    ///   2. Encodes it via `to_cbor_bytes`
    ///   3. Independently decodes the leading array header + tag, NOT
    ///      via the cascade — direct byte inspection of the array length
    ///      prefix and first integer
    ///   4. Asserts BOTH the encoded array length AND the tag against
    ///      the canonical CDDL-specified values
    ///
    /// Exhaustive variant coverage in BOTH directions: every variant is
    /// constructed; every tag 0..=18 is reached. A new upstream
    /// certificate variant (tag 19+) added without a matching
    /// `expected.push((19, ..., DCert::...))` entry here would slip past
    /// CI undetected — but the next regression that mistypes any
    /// existing tag fails immediately with a clearly-named diagnostic.
    ///
    /// Reference: `Cardano.Ledger.Conway.TxCert` — `ConwayTxCert`
    /// constructor tags; CDDL `certificate` rule in
    /// `cardano-ledger-conway/cddl-files/conway.cddl`.
    #[test]
    fn dcert_encoder_tag_and_arity_match_canonical_cddl() {
        let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
        let pool = [0x0b; 28];
        let drep = DRep::KeyHash([0x0c; 28]);
        let anchor = Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xee; 32],
        };

        // (canonical tag, canonical array length, variant). Lengths from
        // the CDDL `certificate` rule and matched in `encode_cbor` above.
        // PoolRegistration is length 10 (one tag + 9 inline fields per
        // upstream `pool_registration_cert`); MIR is length 2 (tag +
        // inner pair). Everything else follows the
        // `[tag, *fields]` shape with field count from the variant arity.
        let pool_params = pool_params_for_test();
        let mir_target = MirTarget::SendToOppositePot(42);

        let cases: Vec<(u64, u64, DCert)> = vec![
            (0, 2, DCert::AccountRegistration(cred)),
            (1, 2, DCert::AccountUnregistration(cred)),
            (2, 3, DCert::DelegationToStakePool(cred, pool)),
            (3, 10, DCert::PoolRegistration(pool_params)),
            (4, 3, DCert::PoolRetirement(pool, EpochNo(7))),
            (
                5,
                4,
                DCert::GenesisDelegation([0x01; 28], [0x02; 28], [0x03; 32]),
            ),
            (
                6,
                2,
                DCert::MoveInstantaneousReward(MirPot::Treasury, mir_target),
            ),
            (7, 3, DCert::AccountRegistrationDeposit(cred, 2_000_000)),
            (8, 3, DCert::AccountUnregistrationDeposit(cred, 2_000_000)),
            (9, 3, DCert::DelegationToDrep(cred, drep)),
            (10, 4, DCert::DelegationToStakePoolAndDrep(cred, pool, drep)),
            (
                11,
                4,
                DCert::AccountRegistrationDelegationToStakePool(cred, pool, 2_000_000),
            ),
            (
                12,
                4,
                DCert::AccountRegistrationDelegationToDrep(cred, drep, 2_000_000),
            ),
            (
                13,
                5,
                DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, 2_000_000),
            ),
            (
                14,
                3,
                DCert::CommitteeAuthorization(cred, StakeCredential::ScriptHash([0x0d; 28])),
            ),
            (
                15,
                3,
                DCert::CommitteeResignation(cred, Some(anchor.clone())),
            ),
            (16, 4, DCert::DrepRegistration(cred, 5_000_000, None)),
            (17, 3, DCert::DrepUnregistration(cred, 5_000_000)),
            (18, 3, DCert::DrepUpdate(cred, Some(anchor))),
        ];

        // Pin: exactly 19 cases (tags 0..=18) so a future upstream tag-19
        // variant added to the enum WITHOUT extending this test fails CI
        // (the existing round-trip tests would also need updating, but
        // that's a separate signal — this is the canonical "did you
        // remember to extend the drift-guard table" check).
        assert_eq!(
            cases.len(),
            19,
            "DCert tag space must be 0..=18 (19 variants)",
        );

        let mut seen_tags: Vec<u64> = Vec::with_capacity(19);
        for (canonical_tag, canonical_len, cert) in cases {
            let bytes = cert.to_cbor_bytes();
            let mut dec = Decoder::new(&bytes);
            let len = dec.array().expect("DCert encodes as a CBOR array");
            assert_eq!(
                len, canonical_len,
                "DCert::{:?} encoded with array length {len}, expected {canonical_len}",
                cert,
            );
            let tag = dec.unsigned().expect("first array element is the tag");
            assert_eq!(
                tag, canonical_tag,
                "DCert::{:?} encoded with tag {tag}, expected canonical CDDL tag {canonical_tag}",
                cert,
            );
            seen_tags.push(tag);
        }

        // Bidirectional completeness: every tag 0..=18 must appear
        // exactly once across the 19 cases. A duplicate tag (two
        // variants accidentally encoded with the same wire ID) or a
        // missing tag fails here naming the gap.
        seen_tags.sort();
        let expected_tags: Vec<u64> = (0..=18).collect();
        assert_eq!(
            seen_tags, expected_tags,
            "encoded DCert tag set must be exactly 0..=18 with no duplicates",
        );
    }

    /// Encoder-side drift guard for the Shelley `Relay` wire-tag space.
    ///
    /// 3 variants (tags 0..=2) with mixed array lengths (4/3/2):
    /// `SingleHostAddr` (port + ipv4 + ipv6, all optional, length 4),
    /// `SingleHostName` (port + DNS, length 3), `MultiHostName`
    /// (DNS only, length 2). A typo swapping tag-0 SingleHostAddr and
    /// tag-1 SingleHostName would silently misinterpret every pool's
    /// announced relay endpoints — every operator-published pool
    /// metadata would point at garbage, breaking peer discovery.
    ///
    /// Reference: `Cardano.Ledger.Shelley.TxBody.StakePoolRelay`;
    /// CDDL `relay` rule.
    #[test]
    fn relay_encoder_tag_and_arity_match_canonical_cddl() {
        let cases: Vec<(u64, u64, Relay)> = vec![
            (
                0,
                4,
                Relay::SingleHostAddr(Some(3001), Some([192, 168, 1, 1]), None),
            ),
            (
                1,
                3,
                Relay::SingleHostName(Some(3001), "relays.example.com".to_string()),
            ),
            (2, 2, Relay::MultiHostName("relays.example.com".to_string())),
        ];
        assert_eq!(cases.len(), 3, "Relay tag space must be 0..=2");

        let mut seen: Vec<u64> = Vec::with_capacity(3);
        for (canonical_tag, canonical_len, relay) in cases {
            let bytes = relay.to_cbor_bytes();
            let mut dec = Decoder::new(&bytes);
            let len = dec.array().expect("Relay encodes as a CBOR array");
            assert_eq!(
                len, canonical_len,
                "Relay::{relay:?} array length {len}, expected {canonical_len}",
            );
            let tag = dec.unsigned().expect("first array element is the tag");
            assert_eq!(tag, canonical_tag, "Relay::{relay:?} tag drift");
            seen.push(tag);
        }
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2], "Relay tag set must be exactly 0..=2");
    }

    /// Encoder-side drift guard for `MirPot` (the move-instantaneous-rewards
    /// source-pot enum embedded inside DCert tag 6).
    ///
    /// 2 values: `Reserves = 0`, `Treasury = 1`. Encoded as bare unsigned
    /// inside the inner MIR array. A typo swapping the two would silently
    /// misinterpret every MIR certificate's source pot — turning a
    /// reserves-funded reward into a treasury-funded one and vice versa,
    /// silently misallocating epoch-boundary fund movement (Shelley-Babbage;
    /// Conway no longer supports MIR).
    ///
    /// `MirPot` is encoded inline in the DCert tag-6 cascade rather than
    /// having its own `CborEncode` impl, so this test exercises the
    /// embedded encoding by constructing a full `DCert::MoveInstantaneous
    /// Reward`, encoding it, and inspecting the inner array's pot byte.
    ///
    /// Reference: `Cardano.Ledger.Shelley.TxCert.MIRPot`;
    /// `move_instantaneous_reward` CDDL rule.
    #[test]
    fn mir_pot_encoder_value_matches_canonical_cddl() {
        for (canonical, pot) in [(0u64, MirPot::Reserves), (1u64, MirPot::Treasury)] {
            let cert = DCert::MoveInstantaneousReward(pot, MirTarget::SendToOppositePot(0));
            let bytes = cert.to_cbor_bytes();
            // Outer DCert: [6, [pot, 0]] — array(2) tag(6) array(2) pot 0
            let mut dec = Decoder::new(&bytes);
            let outer_len = dec.array().expect("outer DCert array");
            assert_eq!(outer_len, 2, "DCert MIR outer array must be length 2");
            let dcert_tag = dec.unsigned().expect("DCert tag");
            assert_eq!(dcert_tag, 6, "MIR DCert tag must be 6");
            let inner_len = dec.array().expect("inner MIR array");
            assert_eq!(
                inner_len, 2,
                "inner MIR array must be length 2 (pot, target)"
            );
            let pot_value = dec.unsigned().expect("inner pot value");
            assert_eq!(
                pot_value, canonical,
                "MirPot::{pot:?} encoded as {pot_value}, expected {canonical}",
            );
        }
    }

    /// Encoder-side drift guard for the Conway `DRep` wire-tag space.
    ///
    /// 4 variants with mixed array lengths: `KeyHash=0` and `ScriptHash=1`
    /// are length 2 (tag + hash); `AlwaysAbstain=2` and `AlwaysNoConfidence=3`
    /// are length 1 (tag only). A drift here would silently misinterpret
    /// DRep delegation/vote credentials — e.g. a stake delegation pointing
    /// at a real DRep key hash that decodes as `AlwaysAbstain` would
    /// strip a real voter's voice and silently flip them to abstain.
    ///
    /// Pins per-variant array length AND tag, plus bidirectional
    /// completeness (cases.len() == 4, sorted observed tags == 0..=3).
    ///
    /// Reference: `Cardano.Ledger.Conway.Governance.DRep`; CDDL `drep`
    /// rule.
    #[test]
    fn drep_encoder_tag_and_arity_match_canonical_cddl() {
        let h = [0x66; 28];
        // (canonical tag, canonical array length, variant)
        let cases: Vec<(u64, u64, DRep)> = vec![
            (0, 2, DRep::KeyHash(h)),
            (1, 2, DRep::ScriptHash(h)),
            (2, 1, DRep::AlwaysAbstain),
            (3, 1, DRep::AlwaysNoConfidence),
        ];
        assert_eq!(cases.len(), 4, "DRep tag space must be 0..=3 (4 variants)");

        let mut seen: Vec<u64> = Vec::with_capacity(4);
        for (canonical_tag, canonical_len, drep) in cases {
            let bytes = drep.to_cbor_bytes();
            let mut dec = Decoder::new(&bytes);
            let len = dec.array().expect("DRep encodes as a CBOR array");
            assert_eq!(
                len, canonical_len,
                "DRep::{drep:?} array length {len}, expected {canonical_len}",
            );
            let tag = dec.unsigned().expect("first array element is the tag");
            assert_eq!(tag, canonical_tag, "DRep::{drep:?} tag drift");
            seen.push(tag);
        }
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2, 3], "DRep tag set must be exactly 0..=3");
    }

    /// Build a minimal valid `PoolParams` for `dcert_encoder_tag_and_arity_match
    /// _canonical_cddl`. Kept as a free helper so the test body stays
    /// focused on the tag/arity invariants.
    fn pool_params_for_test() -> PoolParams {
        PoolParams {
            operator: [0x0b; 28],
            vrf_keyhash: [0x0c; 32],
            pledge: 0,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x0d; 28]),
            },
            pool_owners: vec![],
            relays: vec![],
            pool_metadata: None,
        }
    }

    #[test]
    fn header_hash_wrong_length_rejected() {
        let mut enc = Encoder::new();
        enc.bytes(&[0u8; 16]); // 16 bytes, not 32
        let bytes = enc.into_bytes();
        assert!(HeaderHash::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn era_invalid_tag_rejected() {
        let mut enc = Encoder::new();
        enc.unsigned(99);
        let bytes = enc.into_bytes();
        assert!(Era::from_cbor_bytes(&bytes).is_err());
    }

    #[test]
    fn point_invalid_length_rejected() {
        let mut enc = Encoder::new();
        enc.array(1).unsigned(0);
        let bytes = enc.into_bytes();
        assert!(Point::from_cbor_bytes(&bytes).is_err());
    }

    // ── Indefinite-length CBOR tests ─────────────────────────────────

    #[test]
    fn skip_indefinite_array() {
        // 0x9f 01 02 03 ff = [_ 1, 2, 3]
        let data = [0x9f, 0x01, 0x02, 0x03, 0xff];
        let mut dec = Decoder::new(&data);
        dec.skip().unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn skip_indefinite_map() {
        // 0xbf 01 02 03 04 ff = {_ 1: 2, 3: 4}
        let data = [0xbf, 0x01, 0x02, 0x03, 0x04, 0xff];
        let mut dec = Decoder::new(&data);
        dec.skip().unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn skip_indefinite_bytes() {
        // 0x5f 42 0102 43 030405 ff = (_ h'0102', h'030405')
        let data = [0x5f, 0x42, 0x01, 0x02, 0x43, 0x03, 0x04, 0x05, 0xff];
        let mut dec = Decoder::new(&data);
        dec.skip().unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn skip_indefinite_text() {
        // 0x7f 63 666f6f 63 626172 ff = (_ "foo", "bar")
        let data = [0x7f, 0x63, b'f', b'o', b'o', 0x63, b'b', b'a', b'r', 0xff];
        let mut dec = Decoder::new(&data);
        dec.skip().unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn skip_nested_indefinite() {
        // [_ [_ 1, 2], {_ 3: 4}]
        let data = [
            0x9f, // indef array
            0x9f, 0x01, 0x02, 0xff, // indef array [1, 2]
            0xbf, 0x03, 0x04, 0xff, // indef map {3: 4}
            0xff, // end outer
        ];
        let mut dec = Decoder::new(&data);
        dec.skip().unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn array_begin_definite() {
        // 83 01 02 03 = [1, 2, 3]
        let data = [0x83, 0x01, 0x02, 0x03];
        let mut dec = Decoder::new(&data);
        let count = dec.array_begin().unwrap();
        assert_eq!(count, Some(3));
        for _ in 0..3 {
            dec.unsigned().unwrap();
        }
        assert!(dec.is_empty());
    }

    #[test]
    fn array_begin_indefinite() {
        // 9f 01 02 03 ff = [_ 1, 2, 3]
        let data = [0x9f, 0x01, 0x02, 0x03, 0xff];
        let mut dec = Decoder::new(&data);
        let count = dec.array_begin().unwrap();
        assert_eq!(count, None);
        let mut items = Vec::new();
        while !dec.is_break() {
            items.push(dec.unsigned().unwrap());
        }
        dec.consume_break().unwrap();
        assert_eq!(items, vec![1, 2, 3]);
        assert!(dec.is_empty());
    }

    #[test]
    fn map_begin_indefinite() {
        // bf 01 02 ff = {_ 1: 2}
        let data = [0xbf, 0x01, 0x02, 0xff];
        let mut dec = Decoder::new(&data);
        let count = dec.map_begin().unwrap();
        assert_eq!(count, None);
        let mut entries = Vec::new();
        while !dec.is_break() {
            let k = dec.unsigned().unwrap();
            let v = dec.unsigned().unwrap();
            entries.push((k, v));
        }
        dec.consume_break().unwrap();
        assert_eq!(entries, vec![(1, 2)]);
    }

    #[test]
    fn bytes_owned_definite() {
        let mut enc = Encoder::new();
        enc.bytes(&[0x01, 0x02, 0x03]);
        let data = enc.into_bytes();
        let mut dec = Decoder::new(&data);
        assert_eq!(dec.bytes_owned().unwrap(), vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn bytes_owned_indefinite() {
        // 5f 42 0102 43 030405 ff = (_ h'0102', h'030405')
        let data = [0x5f, 0x42, 0x01, 0x02, 0x43, 0x03, 0x04, 0x05, 0xff];
        let mut dec = Decoder::new(&data);
        assert_eq!(
            dec.bytes_owned().unwrap(),
            vec![0x01, 0x02, 0x03, 0x04, 0x05]
        );
        assert!(dec.is_empty());
    }

    #[test]
    fn text_owned_definite() {
        let mut enc = Encoder::new();
        enc.text("hello");
        let data = enc.into_bytes();
        let mut dec = Decoder::new(&data);
        assert_eq!(dec.text_owned().unwrap(), "hello");
    }

    #[test]
    fn text_owned_indefinite() {
        // 7f 63 666f6f 63 626172 ff = (_ "foo", "bar")
        let data = [0x7f, 0x63, b'f', b'o', b'o', 0x63, b'b', b'a', b'r', 0xff];
        let mut dec = Decoder::new(&data);
        assert_eq!(dec.text_owned().unwrap(), "foobar");
        assert!(dec.is_empty());
    }

    #[test]
    fn raw_value_captures_indefinite_array() {
        // 9f 01 02 ff followed by 05
        let data = [0x9f, 0x01, 0x02, 0xff, 0x05];
        let mut dec = Decoder::new(&data);
        let raw = dec.raw_value().unwrap();
        assert_eq!(raw, &[0x9f, 0x01, 0x02, 0xff]);
        assert_eq!(dec.unsigned().unwrap(), 5);
    }

    // ── array_or_set: CBOR tag 258 transparent set decode ──────────────

    #[test]
    fn array_or_set_plain_array() {
        // Plain array: 83 01 02 03  →  [1, 2, 3]
        let data = [0x83, 0x01, 0x02, 0x03];
        let mut dec = Decoder::new(&data);
        let len = dec.array_or_set().unwrap();
        assert_eq!(len, 3);
        assert_eq!(dec.unsigned().unwrap(), 1);
        assert_eq!(dec.unsigned().unwrap(), 2);
        assert_eq!(dec.unsigned().unwrap(), 3);
        assert!(dec.is_empty());
    }

    #[test]
    fn array_or_set_tagged_258() {
        // Tag 258 wrapping array: d9 0102 83 01 02 03  →  258([1, 2, 3])
        let data = [0xd9, 0x01, 0x02, 0x83, 0x01, 0x02, 0x03];
        let mut dec = Decoder::new(&data);
        let len = dec.array_or_set().unwrap();
        assert_eq!(len, 3);
        assert_eq!(dec.unsigned().unwrap(), 1);
        assert_eq!(dec.unsigned().unwrap(), 2);
        assert_eq!(dec.unsigned().unwrap(), 3);
        assert!(dec.is_empty());
    }

    #[test]
    fn array_or_set_empty_tagged_258() {
        // Tag 258 wrapping empty array: d9 0102 80  →  258([])
        let data = [0xd9, 0x01, 0x02, 0x80];
        let mut dec = Decoder::new(&data);
        let len = dec.array_or_set().unwrap();
        assert_eq!(len, 0);
        assert!(dec.is_empty());
    }

    #[test]
    fn array_or_set_rejects_non_array_non_tag() {
        // Unsigned integer 0x05 — neither array nor tag.
        let data = [0x05];
        let mut dec = Decoder::new(&data);
        assert!(dec.array_or_set().is_err());
    }

    // ── extract_block_tx_byte_spans ────────────────────────────────────

    /// Synthesizes a minimal but structurally-correct block envelope:
    /// `[header, [body0, body1], [ws0, ws1], { 0 => meta0 }]`.
    /// Each "body" / "ws" / "header" is a single CBOR unsigned-int marker so
    /// we can verify the helper sliced exactly the right bytes back.
    #[test]
    fn extract_block_tx_byte_spans_round_trip() {
        let mut enc = Encoder::new();
        enc.array(4);
        // Header (tag 0xAA → unsigned 0x18 0xAA).
        enc.unsigned(0xAA);
        // Bodies array of 2.
        enc.array(2);
        let body0_start = enc.as_bytes().len();
        enc.unsigned(0x10);
        let body0_end = enc.as_bytes().len();
        let body1_start = enc.as_bytes().len();
        enc.unsigned(0x11);
        let body1_end = enc.as_bytes().len();
        // Witness sets array of 2.
        enc.array(2);
        let ws0_start = enc.as_bytes().len();
        enc.unsigned(0x20);
        let ws0_end = enc.as_bytes().len();
        let ws1_start = enc.as_bytes().len();
        enc.unsigned(0x21);
        let ws1_end = enc.as_bytes().len();
        // Metadata map (single entry).
        enc.map(1).unsigned(0).unsigned(0xFF);

        let raw = enc.into_bytes();
        let spans = extract_block_tx_byte_spans(&raw).expect("extract spans");
        assert_eq!(spans.bodies.len(), 2);
        assert_eq!(spans.witness_sets.len(), 2);
        assert_eq!(spans.bodies[0], &raw[body0_start..body0_end]);
        assert_eq!(spans.bodies[1], &raw[body1_start..body1_end]);
        assert_eq!(spans.witness_sets[0], &raw[ws0_start..ws0_end]);
        assert_eq!(spans.witness_sets[1], &raw[ws1_start..ws1_end]);
    }

    #[test]
    fn extract_block_tx_byte_spans_unwraps_hfc_envelope() {
        let inner: Vec<u8> = vec![
            0x84, // array(4)
            0x00, //   header
            0x81, //   bodies array(1)
            0x9f, 0x18, 0x42, 0xff, // body uses non-canonical-on-reencode form
            0x81, //   witnesses array(1)
            0x18, 0x55, // witness set marker
            0xa0, //   metadata
        ];
        let mut wrapped = Vec::with_capacity(inner.len() + 2);
        wrapped.push(0x82); // HFC [era_index, inner_block]
        wrapped.push(0x06); // Babbage-era index in deployed networks
        wrapped.extend_from_slice(&inner);

        let spans = extract_block_tx_byte_spans(&wrapped).expect("extract wrapped spans");
        assert_eq!(spans.bodies, vec![vec![0x9f, 0x18, 0x42, 0xff]]);
        assert_eq!(spans.witness_sets, vec![vec![0x18, 0x55]]);
    }

    /// Verifies the central parity invariant: when re-serialising a typed
    /// value yields different bytes than the on-wire encoding (here, a
    /// definite-length array vs an indefinite-length array), the helper
    /// returns the **on-wire** bytes byte-for-byte.  This is what the
    /// linear fee formula and tx-id hash both depend on.
    ///
    /// Reference: 2026-04-27 preprod sync rehearsal finding (440-lovelace
    /// gap on the first transaction after the Byron→Shelley boundary,
    /// preprod slot ≈ 518 460), captured in
    /// docs/REAL_PREPROD_POOL_VERIFICATION.md.
    #[test]
    fn extract_block_tx_byte_spans_returns_on_wire_bytes_for_indefinite_encoding() {
        // Hand-craft a block with an indefinite-length body so re-encoding
        // (which always emits definite-length) would produce different
        // bytes than what the helper extracts.
        //
        // Outer:  array(4)
        //   header: unsigned(0)
        //   bodies: array(1) [ indefinite_array(0xff_terminated) [unsigned(0x42)] ]
        //   witnesses: array(1) [ unsigned(0x55) ]
        //   metadata: map(0)
        let raw: Vec<u8> = vec![
            0x84, // array(4)
            0x00, //   unsigned(0)            ← header
            0x81, //   array(1)               ← bodies
            0x9f, 0x18, 0x42, 0xff, //     indef-array [unsigned(0x42)]
            0x81, //   array(1)               ← witnesses
            0x18, 0x55, //     unsigned(0x55)
            0xa0, //   map(0)                 ← metadata
        ];
        let spans = extract_block_tx_byte_spans(&raw).expect("extract spans");
        assert_eq!(spans.bodies.len(), 1);
        assert_eq!(spans.witness_sets.len(), 1);
        // The body span MUST be the indefinite-length encoding 0x9f 0x18 0x42 0xff.
        // Re-serialising via `to_cbor_bytes()` would emit 0x81 0x18 0x42 instead
        // (definite-length, 3 bytes) — that 1-byte difference is exactly the
        // class of mismatch that caused the 440-lovelace fee gap upstream.
        assert_eq!(spans.bodies[0], &[0x9f, 0x18, 0x42, 0xff]);
        assert_eq!(spans.witness_sets[0], &[0x18, 0x55]);
    }

    #[test]
    fn extract_block_tx_byte_spans_rejects_truncated_header() {
        // Just `array(4)` with no further data.
        let raw = [0x84];
        assert!(extract_block_tx_byte_spans(&raw).is_err());
    }

    #[test]
    fn extract_block_tx_byte_spans_rejects_too_few_outer_elements() {
        // array(2) — only two elements, missing bodies + witnesses.
        let raw = [0x82, 0x00, 0x00];
        assert!(extract_block_tx_byte_spans(&raw).is_err());
    }
}
