//! Flat binary format decoder for UPLC programs.
//!
//! The Flat encoding is a bit-level binary format used to serialize Plutus
//! Core programs on the Cardano blockchain. This module implements the
//! decoder portion needed to parse on-chain script bytes into the UPLC
//! `Program` and `Term` representations.
//!
//! ## Wire format
//!
//! Ledger CBOR decoding stores Plutus scripts as `PlutusBinary`: the raw
//! ledger bytes for `PlutusLedgerApi.Common.SerialisedScript`. Upstream
//! `decodePlutusRunnable` feeds those bytes to `scriptCBORDecoder`, which
//! first decodes one CBOR bytestring and then Flat-decodes that payload.
//!
//! ## Bit-level format
//!
//! - Term tags: 4 bits (MSB first within each byte)
//! - De Bruijn indices: variable-length natural
//! - Builtin tags: 7 bits fixed-width
//! - Integers: zigzag + variable-length natural
//! - ByteStrings: pad to byte boundary, then length-prefixed chunks
//! - Lists: 1-bit continuation flag per element
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/UntypedPlutusCore/Core/Instance/Flat.hs>

use yggdrasil_ledger::{cbor::Decoder, plutus::PlutusData};

use crate::error::MachineError;
use crate::types::{Constant, DefaultFun, Program, Term, Type};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Maximum nesting depth permitted when decoding a UPLC `Term` or its
/// constituent `Type` / `Constant` values from Flat-encoded bytes.
///
/// Untrusted Plutus scripts arrive in witness sets and are decoded directly
/// from on-chain bytes; without a depth bound a malicious script with
/// deeply nested `Apply` / `LamAbs` / `Constr` could overflow the runtime
/// stack via the recursive `FlatDecoder::decode_term` path. Real on-chain
/// scripts can exceed a few hundred levels after extensive macro expansion;
/// 512 admits known Preview Babbage reference scripts while keeping a hard
/// recursion ceiling. Exceeding the bound returns
/// [`MachineError::FlatDecodeError`] cleanly instead of a process crash.
///
/// Reference: defensive bound. Upstream Haskell relies on its lazy `Flat`
/// decoder being stack-safe by construction; the Rust port makes the limit
/// explicit.
pub const MAX_TERM_DECODE_DEPTH: usize = 512;

/// Decode a UPLC program from raw Flat bytes.
pub fn decode_flat_program(bytes: &[u8]) -> Result<Program, MachineError> {
    let mut dec = FlatDecoder::new(bytes);
    let program = dec.decode_program()?;
    validate_program_closed(&program)?;
    Ok(program)
}

/// Decode an on-chain Plutus script from its raw `PlutusBinary` bytes.
///
/// `PlutusBinary` contains the upstream `SerialisedScript`, which is a CBOR
/// bytestring whose payload is the Flat-encoded UPLC program. This function
/// mirrors `scriptCBORDecoder` strictly: decode one CBOR bytestring, reject
/// trailing bytes, then Flat-decode the payload.
pub fn decode_script_bytes(script_bytes: &[u8]) -> Result<Program, MachineError> {
    decode_script_bytes_with_remainder_policy(script_bytes, false)
}

/// Decode a Plutus V1/V2 `PlutusBinary`.
///
/// Upstream `deserialiseScript` historically allows a trailing CBOR
/// remainder for PlutusV1 and PlutusV2 after decoding the first script
/// bytestring. PlutusV3 rejects such remainders, so callers with a known
/// language version should use this helper only for V1/V2.
pub fn decode_script_bytes_allowing_remainder(
    script_bytes: &[u8],
) -> Result<Program, MachineError> {
    decode_script_bytes_with_remainder_policy(script_bytes, true)
}

fn decode_script_bytes_with_remainder_policy(
    script_bytes: &[u8],
    allow_remainder: bool,
) -> Result<Program, MachineError> {
    let mut dec = Decoder::new(script_bytes);
    let flat_bytes = dec
        .bytes_owned()
        .map_err(|e| MachineError::FlatDecodeError(format!("PlutusBinary CBOR bytestring: {e}")))?;
    if !allow_remainder && !dec.is_empty() {
        return Err(MachineError::FlatDecodeError(format!(
            "trailing bytes after PlutusBinary script: {}",
            dec.remaining()
        )));
    }
    decode_flat_program(&flat_bytes)
}

fn validate_program_closed(program: &Program) -> Result<(), MachineError> {
    validate_term_closed(&program.term, 0)
}

fn validate_term_closed(term: &Term, scope_depth: u64) -> Result<(), MachineError> {
    match term {
        Term::Var(index) => {
            if *index == 0 || *index > scope_depth {
                return Err(MachineError::FlatDecodeError(format!(
                    "open term: de Bruijn index {index} outside scope depth {scope_depth}"
                )));
            }
            Ok(())
        }
        Term::LamAbs(body) => validate_term_closed(body, scope_depth.saturating_add(1)),
        Term::Apply(fun, arg) => {
            validate_term_closed(fun, scope_depth)?;
            validate_term_closed(arg, scope_depth)
        }
        Term::Delay(body) | Term::Force(body) => validate_term_closed(body, scope_depth),
        Term::Constr(_, fields) => fields
            .iter()
            .try_for_each(|field| validate_term_closed(field, scope_depth)),
        Term::Case(scrutinee, branches) => {
            validate_term_closed(scrutinee, scope_depth)?;
            branches
                .iter()
                .try_for_each(|branch| validate_term_closed(branch, scope_depth))
        }
        Term::Constant(_) | Term::Builtin(_) | Term::Error => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// FlatDecoder — bit-level reader
// ---------------------------------------------------------------------------

struct FlatDecoder<'a> {
    bytes: &'a [u8],
    /// Current byte index.
    pos: usize,
    /// Current bit within the byte (0 = MSB, 7 = LSB).
    bit: u8,
}

impl<'a> FlatDecoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            bit: 0,
        }
    }

    /// Read a single bit. Returns `true` for 1, `false` for 0.
    fn read_bit(&mut self) -> Result<bool, MachineError> {
        if self.pos >= self.bytes.len() {
            return Err(MachineError::FlatDecodeError(
                "unexpected end of input".into(),
            ));
        }
        let byte = self.bytes[self.pos];
        let result = (byte >> (7 - self.bit)) & 1 == 1;
        self.bit += 1;
        if self.bit >= 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(result)
    }

    /// Read `n` bits (n ≤ 8) into the low bits of a u8.
    fn read_bits8(&mut self, n: u8) -> Result<u8, MachineError> {
        debug_assert!(n <= 8);
        let mut result: u8 = 0;
        for _ in 0..n {
            result = (result << 1) | u8::from(self.read_bit()?);
        }
        Ok(result)
    }

    /// Read a Flat natural number (variable-length, 8-bit groups, MSB continuation).
    fn read_natural(&mut self) -> Result<u64, MachineError> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_bits8(8)?;
            let val = u64::from(byte & 0x7F);
            result |= val
                .checked_shl(shift)
                .ok_or_else(|| MachineError::FlatDecodeError("natural number too large".into()))?;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err(MachineError::FlatDecodeError(
                    "natural number overflow".into(),
                ));
            }
        }
        Ok(result)
    }

    /// Read a Flat integer (zigzag-encoded natural).
    fn read_integer(&mut self) -> Result<i128, MachineError> {
        // Read as u128 to handle the full zigzag range.
        let mut result: u128 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_bits8(8)?;
            let val = u128::from(byte & 0x7F);
            result |= val
                .checked_shl(shift)
                .ok_or_else(|| MachineError::FlatDecodeError("integer too large".into()))?;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 127 {
                return Err(MachineError::FlatDecodeError("integer overflow".into()));
            }
        }
        // Zigzag decode: even → positive, odd → negative.
        let decoded = if result & 1 == 0 {
            (result >> 1) as i128
        } else {
            -((result >> 1) as i128) - 1
        };
        Ok(decoded)
    }

    /// Read the Flat filler sequence used before byte-aligned payloads.
    ///
    /// Upstream `dFiller` reads zero bits until the terminating one bit. The
    /// terminator is placed so the next read starts on a byte boundary.
    fn read_filler(&mut self) -> Result<(), MachineError> {
        while !self.read_bit()? {}
        Ok(())
    }

    /// Read a Flat bytestring: filler, then length-prefixed chunks.
    fn read_bytestring(&mut self) -> Result<Vec<u8>, MachineError> {
        self.read_filler()?;
        let mut result = Vec::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(MachineError::FlatDecodeError(
                    "unexpected end in bytestring".into(),
                ));
            }
            let chunk_len = self.bytes[self.pos] as usize;
            self.pos += 1;
            if chunk_len == 0 {
                break;
            }
            if self.pos + chunk_len > self.bytes.len() {
                return Err(MachineError::FlatDecodeError(
                    "bytestring chunk exceeds input".into(),
                ));
            }
            result.extend_from_slice(&self.bytes[self.pos..self.pos + chunk_len]);
            self.pos += chunk_len;
        }
        Ok(result)
    }

    /// Read a Flat-encoded string (UTF-8 bytestring).
    fn read_string(&mut self) -> Result<String, MachineError> {
        let bytes = self.read_bytestring()?;
        String::from_utf8(bytes)
            .map_err(|_| MachineError::FlatDecodeError("invalid UTF-8 in string constant".into()))
    }

    /// Read a Flat list using the 1-bit continuation scheme.
    fn read_list<T>(
        &mut self,
        read_elem: impl Fn(&mut Self) -> Result<T, MachineError>,
    ) -> Result<Vec<T>, MachineError> {
        let mut items = Vec::new();
        while self.read_bit()? {
            items.push(read_elem(self)?);
        }
        Ok(items)
    }

    // -------------------------------------------------------------------
    // Program / Term / Type / Constant decoding
    // -------------------------------------------------------------------

    fn decode_program(&mut self) -> Result<Program, MachineError> {
        let major = self.read_natural()? as u32;
        let minor = self.read_natural()? as u32;
        let patch = self.read_natural()? as u32;
        let term = self.decode_term()?;
        Ok(Program {
            major,
            minor,
            patch,
            term,
        })
    }

    fn decode_term(&mut self) -> Result<Term, MachineError> {
        self.decode_term_with_depth(MAX_TERM_DECODE_DEPTH)
    }

    /// Recursive flat decoder for [`Term`] with an explicit depth budget.
    ///
    /// Untrusted Plutus scripts arrive in witness sets and are decoded
    /// directly from on-chain bytes; without a depth bound a malicious
    /// script with deeply nested `Apply` / `LamAbs` / `Constr` could
    /// overflow the runtime stack. The bound is enforced on every recursive
    /// entry; exceeding it returns
    /// [`MachineError::FlatDecodeError`]. Per-frame size of `decode_term`
    /// is small (no `Vec` accumulator on the stack), so the chosen
    /// [`MAX_TERM_DECODE_DEPTH`] avoids rejecting known on-chain scripts
    /// while still bounding malicious recursive inputs.
    ///
    /// Reference: defensive bound. Upstream Haskell relies on its lazy
    /// `Flat` decoder being stack-safe by construction; the Rust port
    /// makes the limit explicit.
    fn decode_term_with_depth(&mut self, depth_remaining: usize) -> Result<Term, MachineError> {
        if depth_remaining == 0 {
            return Err(MachineError::FlatDecodeError(format!(
                "term nesting exceeded depth budget {MAX_TERM_DECODE_DEPTH}"
            )));
        }
        let next = depth_remaining - 1;
        let tag = self.read_bits8(4)?;
        match tag {
            0 => {
                // Var — de Bruijn index (natural).
                let index = self.read_natural()?;
                Ok(Term::Var(index))
            }
            1 => {
                // Delay
                let body = self.decode_term_with_depth(next)?;
                Ok(Term::Delay(Box::new(body)))
            }
            2 => {
                // LamAbs
                let body = self.decode_term_with_depth(next)?;
                Ok(Term::LamAbs(Box::new(body)))
            }
            3 => {
                // Apply
                let fun = self.decode_term_with_depth(next)?;
                let arg = self.decode_term_with_depth(next)?;
                Ok(Term::Apply(Box::new(fun), Box::new(arg)))
            }
            4 => {
                // Constant — type list then value.
                let ty = self.decode_type_list_with_depth(next)?;
                let constant = self.decode_constant_with_depth(&ty, next)?;
                Ok(Term::Constant(constant))
            }
            5 => {
                // Force
                let body = self.decode_term_with_depth(next)?;
                Ok(Term::Force(Box::new(body)))
            }
            6 => {
                // Error
                Ok(Term::Error)
            }
            7 => {
                // Builtin — 7 bits.
                let b = self.read_bits8(7)?;
                let fun = DefaultFun::from_tag(b)?;
                Ok(Term::Builtin(fun))
            }
            8 => {
                // Constr (UPLC 1.1.0+)
                let tag_val = self.read_natural()?;
                let fields = self.read_list(|d| d.decode_term_with_depth(next))?;
                Ok(Term::Constr(tag_val, fields))
            }
            9 => {
                // Case (UPLC 1.1.0+)
                let scrutinee = self.decode_term_with_depth(next)?;
                let branches = self.read_list(|d| d.decode_term_with_depth(next))?;
                Ok(Term::Case(Box::new(scrutinee), branches))
            }
            _ => Err(MachineError::FlatDecodeError(format!(
                "unknown term tag {tag}"
            ))),
        }
    }

    fn decode_type_list_with_depth(
        &mut self,
        depth_remaining: usize,
    ) -> Result<Type, MachineError> {
        let tags = self.read_list(|d| d.read_bits8(4))?;
        if tags.is_empty() {
            return Err(MachineError::FlatDecodeError(
                "empty constant type tag list".into(),
            ));
        }
        let mut parser = TypeTagParser::new(&tags);
        let ty = match parser.parse_uni(depth_remaining)? {
            DecodedUni::Star(ty) => ty,
            DecodedUni::ProtoList | DecodedUni::ProtoPair | DecodedUni::PartialPair(_) => {
                return Err(MachineError::FlatDecodeError(
                    "non-star type cannot have a Flat constant value".into(),
                ));
            }
        };
        if !parser.is_empty() {
            return Err(MachineError::FlatDecodeError(format!(
                "trailing constant type tags: {}",
                parser.remaining()
            )));
        }
        Ok(ty)
    }
}

enum DecodedUni {
    Star(Type),
    ProtoList,
    ProtoPair,
    PartialPair(Type),
}

struct TypeTagParser<'a> {
    tags: &'a [u8],
    pos: usize,
}

impl<'a> TypeTagParser<'a> {
    fn new(tags: &'a [u8]) -> Self {
        Self { tags, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos == self.tags.len()
    }

    fn remaining(&self) -> usize {
        self.tags.len().saturating_sub(self.pos)
    }

    fn next_tag(&mut self) -> Result<u8, MachineError> {
        let tag = self.tags.get(self.pos).copied().ok_or_else(|| {
            MachineError::FlatDecodeError("unexpected end of constant type tags".into())
        })?;
        self.pos += 1;
        Ok(tag)
    }

    fn parse_uni(&mut self, depth_remaining: usize) -> Result<DecodedUni, MachineError> {
        if depth_remaining == 0 {
            return Err(MachineError::FlatDecodeError(format!(
                "type nesting exceeded depth budget {MAX_TERM_DECODE_DEPTH}"
            )));
        }
        let next = depth_remaining - 1;
        let tag = self.next_tag()?;
        match tag {
            0 => Ok(DecodedUni::Star(Type::Integer)),
            1 => Ok(DecodedUni::Star(Type::ByteString)),
            2 => Ok(DecodedUni::Star(Type::String)),
            3 => Ok(DecodedUni::Star(Type::Unit)),
            4 => Ok(DecodedUni::Star(Type::Bool)),
            5 => Ok(DecodedUni::ProtoList),
            6 => Ok(DecodedUni::ProtoPair),
            7 => {
                let fun = self.parse_uni(next)?;
                let arg = self.parse_uni(next)?;
                self.apply_uni(fun, arg)
            }
            8 => Ok(DecodedUni::Star(Type::Data)),
            9 => Ok(DecodedUni::Star(Type::Bls12_381_G1_Element)),
            10 => Ok(DecodedUni::Star(Type::Bls12_381_G2_Element)),
            11 => Ok(DecodedUni::Star(Type::Bls12_381_MlResult)),
            12 => Err(MachineError::FlatDecodeError(
                "DefaultUniProtoArray constants are not supported".into(),
            )),
            13 => Err(MachineError::FlatDecodeError(
                "DefaultUniValue constants are not supported".into(),
            )),
            _ => Err(MachineError::FlatDecodeError(format!(
                "unknown type tag {tag}"
            ))),
        }
    }

    fn apply_uni(&self, fun: DecodedUni, arg: DecodedUni) -> Result<DecodedUni, MachineError> {
        match (fun, arg) {
            (DecodedUni::ProtoList, DecodedUni::Star(arg_ty)) => {
                Ok(DecodedUni::Star(Type::List(Box::new(arg_ty))))
            }
            (DecodedUni::ProtoPair, DecodedUni::Star(arg_ty)) => {
                Ok(DecodedUni::PartialPair(arg_ty))
            }
            (DecodedUni::PartialPair(left_ty), DecodedUni::Star(right_ty)) => Ok(DecodedUni::Star(
                Type::Pair(Box::new(left_ty), Box::new(right_ty)),
            )),
            _ => Err(MachineError::FlatDecodeError(
                "ill-kinded constant type application".into(),
            )),
        }
    }
}

impl<'a> FlatDecoder<'a> {
    fn decode_constant_with_depth(
        &mut self,
        ty: &Type,
        depth_remaining: usize,
    ) -> Result<Constant, MachineError> {
        if depth_remaining == 0 {
            return Err(MachineError::FlatDecodeError(format!(
                "constant nesting exceeded depth budget {MAX_TERM_DECODE_DEPTH}"
            )));
        }
        let next = depth_remaining - 1;
        match ty {
            Type::Integer => {
                let val = self.read_integer()?;
                Ok(Constant::Integer(val))
            }
            Type::ByteString => {
                let bs = self.read_bytestring()?;
                Ok(Constant::ByteString(bs))
            }
            Type::String => {
                let s = self.read_string()?;
                Ok(Constant::String(s))
            }
            Type::Unit => Ok(Constant::Unit),
            Type::Bool => {
                let b = self.read_bit()?;
                Ok(Constant::Bool(b))
            }
            Type::List(elem_ty) => {
                let items = self.read_list(|d| d.decode_constant_with_depth(elem_ty, next))?;
                Ok(Constant::ProtoList(elem_ty.as_ref().clone(), items))
            }
            Type::Pair(a_ty, b_ty) => {
                let a = self.decode_constant_with_depth(a_ty, next)?;
                let b = self.decode_constant_with_depth(b_ty, next)?;
                Ok(Constant::ProtoPair(
                    a_ty.as_ref().clone(),
                    b_ty.as_ref().clone(),
                    Box::new(a),
                    Box::new(b),
                ))
            }
            Type::Data => {
                let data = self.decode_plutus_data()?;
                Ok(Constant::Data(data))
            }
            Type::Bls12_381_G1_Element => {
                let bs = self.read_bytestring()?;
                let elem = yggdrasil_crypto::bls12_381::g1_uncompress(&bs).map_err(|e| {
                    MachineError::FlatDecodeError(format!("invalid G1 element: {e}"))
                })?;
                Ok(Constant::Bls12_381_G1_Element(elem))
            }
            Type::Bls12_381_G2_Element => {
                let bs = self.read_bytestring()?;
                let elem = yggdrasil_crypto::bls12_381::g2_uncompress(&bs).map_err(|e| {
                    MachineError::FlatDecodeError(format!("invalid G2 element: {e}"))
                })?;
                Ok(Constant::Bls12_381_G2_Element(elem))
            }
            Type::Bls12_381_MlResult => Err(MachineError::FlatDecodeError(
                "MlResult cannot appear in Flat-encoded programs".into(),
            )),
        }
    }

    /// Decode an embedded PlutusData value from Flat.
    ///
    /// PlutusData is encoded as a CBOR bytestring within the Flat stream
    /// (pad to byte boundary, read CBOR bytes, decode PlutusData from CBOR).
    fn decode_plutus_data(&mut self) -> Result<PlutusData, MachineError> {
        // PlutusData in Flat: encoded as a bytestring containing CBOR.
        let cbor_bytes = self.read_bytestring()?;
        let mut dec = yggdrasil_ledger::cbor::Decoder::new(&cbor_bytes);
        PlutusData::decode_cbor(&mut dec)
            .map_err(|e| MachineError::FlatDecodeError(format!("PlutusData CBOR: {e}")))
    }
}

use yggdrasil_ledger::cbor::CborDecode;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build Flat bytes from bits (MSB first per byte).
    fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &b) in chunk.iter().enumerate() {
                if b != 0 {
                    byte |= 1 << (7 - i);
                }
            }
            bytes.push(byte);
        }
        bytes
    }

    fn type_tag_list_bits(tags: &[u8]) -> Vec<u8> {
        let mut bits = Vec::new();
        for &tag in tags {
            bits.push(1);
            for shift in (0..4).rev() {
                bits.push((tag >> shift) & 1);
            }
        }
        bits.push(0);
        bits
    }

    fn push_filler_bits(bits: &mut Vec<u8>) {
        loop {
            let bit = u8::from((bits.len() + 1).is_multiple_of(8));
            bits.push(bit);
            if bit == 1 {
                break;
            }
        }
    }

    fn push_byte_bits(bits: &mut Vec<u8>, byte: u8) {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }

    fn flat_program_from_term_bits(term_bits: &[u8]) -> Vec<u8> {
        let mut data = vec![0x01, 0x00, 0x00]; // version 1.0.0
        data.extend(bits_to_bytes(term_bits));
        data
    }

    #[test]
    fn test_read_bit() {
        let data = [0b10110000];
        let mut dec = FlatDecoder::new(&data);
        assert!(dec.read_bit().is_ok_and(|b| b)); // 1
        assert!(dec.read_bit().is_ok_and(|b| !b)); // 0
        assert!(dec.read_bit().is_ok_and(|b| b)); // 1
        assert!(dec.read_bit().is_ok_and(|b| b)); // 1
    }

    #[test]
    fn test_read_natural_zero() {
        // Natural 0: 8 bits = 0b00000000
        let data = [0x00];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_natural().ok(), Some(0));
    }

    #[test]
    fn test_read_natural_small() {
        // Natural 42: one group, no continuation: 0b0_0101010 = 0x2A
        let data = [0x2A];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_natural().ok(), Some(42));
    }

    #[test]
    fn test_read_natural_two_groups() {
        // Natural 200: 200 = 0b11001000
        // Group 1: 200 & 0x7F = 72, continuation = 1 → 0b1_1001000 = 0xC8
        // Group 2: 200 >> 7 = 1, no continuation → 0b0_0000001 = 0x01
        let data = [0xC8, 0x01];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_natural().ok(), Some(200));
    }

    #[test]
    fn test_read_integer_positive() {
        // Integer 5: zigzag(5) = 10 = 0b00001010 = 0x0A
        let data = [0x0A];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_integer().ok(), Some(5));
    }

    #[test]
    fn test_read_integer_negative() {
        // Integer -3: zigzag(-3) = 5 = 0b00000101 = 0x05
        let data = [0x05];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_integer().ok(), Some(-3));
    }

    #[test]
    fn test_read_integer_zero() {
        // Integer 0: zigzag(0) = 0
        let data = [0x00];
        let mut dec = FlatDecoder::new(&data);
        assert_eq!(dec.read_integer().ok(), Some(0));
    }

    #[test]
    fn test_read_bytestring_empty() {
        // Empty bytestring: aligned filler byte, then zero-length chunk.
        let data = [0x01, 0x00];
        let mut dec = FlatDecoder::new(&data);
        let bs = dec.read_bytestring().expect("decode");
        assert!(bs.is_empty());
    }

    #[test]
    fn test_read_bytestring_short() {
        // Bytestring [0xAB, 0xCD]: filler, one chunk of length 2, terminator.
        let data = [0x01, 0x02, 0xAB, 0xCD, 0x00];
        let mut dec = FlatDecoder::new(&data);
        let bs = dec.read_bytestring().expect("decode");
        assert_eq!(bs, vec![0xAB, 0xCD]);
    }

    #[test]
    fn test_decode_simple_term_error() {
        // Term tag 6 = Error, followed by filler to byte boundary.
        // Tag 6 in 4 bits: 0110, then 4 bits of filler padding.
        let data = bits_to_bytes(&[0, 1, 1, 0, 0, 0, 0, 0]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Error);
    }

    #[test]
    fn test_decode_builtin_add_integer() {
        // Term tag 7 = Builtin (4 bits: 0111), then 7 bits for builtin 0.
        // 0111 0000000 + padding
        let data = bits_to_bytes(&[0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Builtin(DefaultFun::AddInteger));
    }

    // -- Additional term decoding tests ---------------------------------

    #[test]
    fn test_decode_var_term() {
        // Var = tag 0 (0000), then natural 1 (0b00000001).
        let data = bits_to_bytes(&[
            0, 0, 0, 0, // tag=0 (Var)
            0, 0, 0, 0, 0, 0, 0, 1, // natural=1
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Var(1));
    }

    #[test]
    fn test_decode_delay_term() {
        // Delay = tag 1 (0001), body = Error (tag 6 = 0110).
        let data = bits_to_bytes(&[
            0, 0, 0, 1, // tag=1 (Delay)
            0, 1, 1, 0, // tag=6 (Error)
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Delay(Box::new(Term::Error)));
    }

    #[test]
    fn test_decode_lam_abs_term() {
        // LamAbs = tag 2 (0010), body = Var(1).
        let data = bits_to_bytes(&[
            0, 0, 1, 0, // tag=2 (LamAbs)
            0, 0, 0, 0, // tag=0 (Var)
            0, 0, 0, 0, 0, 0, 0, 1, // natural=1
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::LamAbs(Box::new(Term::Var(1))));
    }

    #[test]
    fn test_decode_apply_term() {
        // Apply = tag 3 (0011), fun = Error, arg = Error.
        let data = bits_to_bytes(&[
            0, 0, 1, 1, // tag=3 (Apply)
            0, 1, 1, 0, // tag=6 (Error) -- fun
            0, 1, 1, 0, // tag=6 (Error) -- arg
            0, 0, 0, 0, // padding
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(
            term,
            Term::Apply(Box::new(Term::Error), Box::new(Term::Error))
        );
    }

    #[test]
    fn test_decode_force_term() {
        // Force = tag 5 (0101), inner = Error.
        let data = bits_to_bytes(&[
            0, 1, 0, 1, // tag=5 (Force)
            0, 1, 1, 0, // tag=6 (Error)
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Force(Box::new(Term::Error)));
    }

    #[test]
    fn test_decode_constant_unit_term() {
        // Constant = tag 4 (0100), type list [Unit=3]. No payload.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[3]));
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Unit));
    }

    #[test]
    fn test_decode_constant_bool_true() {
        // Constant = tag 4 (0100), type list [Bool=4], payload: 1 bit.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[4]));
        bits.push(1); // bool value = true
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Bool(true)));
    }

    #[test]
    fn test_decode_constant_bool_false() {
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[4]));
        bits.push(0); // bool value = false
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Bool(false)));
    }

    #[test]
    fn test_decode_constant_integer_zero() {
        // Constant = tag 4, Type = Integer (tag 0).
        // Integer 0: zigzag(0) = 0, encoded as 8-bit group: 0x00.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[0]));
        bits.extend([0, 0, 0, 0, 0, 0, 0, 0]); // integer byte 0x00
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Integer(0)));
    }

    #[test]
    fn test_decode_constant_list_type_tags() {
        // Type list [Apply=7, ProtoList=5, Bool=4], empty list payload.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[7, 5, 4]));
        bits.push(0); // empty value list
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(
            term,
            Term::Constant(Constant::ProtoList(Type::Bool, vec![]))
        );
    }

    #[test]
    fn test_decode_constant_pair_type_tags() {
        // Type list [Apply, Apply, ProtoPair, Integer, Bool], then pair payload.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[7, 7, 6, 0, 4]));
        bits.extend([0, 0, 0, 0, 0, 0, 0, 0]); // integer 0
        bits.push(1); // bool true
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(
            term,
            Term::Constant(Constant::ProtoPair(
                Type::Integer,
                Type::Bool,
                Box::new(Constant::Integer(0)),
                Box::new(Constant::Bool(true)),
            ))
        );
    }

    #[test]
    fn test_decode_constant_string_consumes_flat_filler() {
        // String payloads use upstream Flat `dFiller` before byte chunks.
        // After the term tag and type-list [String=2], we are not byte
        // aligned, so the filler must be consumed bit-by-bit before chunk
        // decoding starts.
        let mut bits = vec![
            0, 1, 0, 0, // tag=4 (Constant)
        ];
        bits.extend(type_tag_list_bits(&[2]));
        push_filler_bits(&mut bits);
        for byte in [2, b'O', b'K', 0] {
            push_byte_bits(&mut bits, byte);
        }
        let data = bits_to_bytes(&bits);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::String("OK".to_owned())));
    }

    #[test]
    fn test_decode_unknown_term_tag() {
        // Tag 10 (1010) = unknown.
        let data = bits_to_bytes(&[1, 0, 1, 0, 0, 0, 0, 0]);
        let mut dec = FlatDecoder::new(&data);
        assert!(dec.decode_term().is_err());
    }

    #[test]
    fn test_decode_unknown_builtin_tag() {
        // Builtin term (tag 7), then builtin tag 100 (> 86).
        // 0111 1100100 + padding
        let data = bits_to_bytes(&[
            0, 1, 1, 1, // tag=7 (Builtin)
            1, 1, 0, 0, 1, 0, 0, // builtin 100
            0, // padding
        ]);
        let mut dec = FlatDecoder::new(&data);
        assert!(dec.decode_term().is_err());
    }

    // -- Program decoding -----------------------------------------------

    #[test]
    fn test_decode_program_version_and_error() {
        // Program: version 1.0.0, body = Error.
        // Natural 1 = 0x01, Natural 0 = 0x00, Natural 0 = 0x00.
        // Error term = tag 6 (0110).
        let mut data = vec![0x01, 0x00, 0x00]; // version 1.0.0
        // Now append the Error term bits: 0110 + 4 bits padding = 0x60
        data.push(0x60);
        let program = decode_flat_program(&data).expect("decode");
        assert_eq!(program.major, 1);
        assert_eq!(program.minor, 0);
        assert_eq!(program.patch, 0);
        assert_eq!(program.term, Term::Error);
    }

    #[test]
    fn test_decode_flat_program_rejects_open_term() {
        // Program body = Var(1). Top-level programs must be closed.
        let data = flat_program_from_term_bits(&[
            0, 0, 0, 0, // tag=0 (Var)
            0, 0, 0, 0, 0, 0, 0, 1, // natural=1
        ]);

        let err = decode_flat_program(&data).expect_err("open term rejected");
        assert!(
            matches!(&err, MachineError::FlatDecodeError(msg) if msg.contains("open term")),
            "expected open-term FlatDecodeError, got {err:?}"
        );
    }

    #[test]
    fn test_decode_flat_program_accepts_closed_lambda() {
        // Program body = LamAbs(Var(1)).
        let data = flat_program_from_term_bits(&[
            0, 0, 1, 0, // tag=2 (LamAbs)
            0, 0, 0, 0, // tag=0 (Var)
            0, 0, 0, 0, 0, 0, 0, 1, // natural=1
        ]);

        let program = decode_flat_program(&data).expect("closed lambda decodes");
        assert_eq!(program.term, Term::LamAbs(Box::new(Term::Var(1))));
    }

    #[test]
    fn test_decode_flat_program_rejects_lambda_with_out_of_scope_var() {
        // Program body = LamAbs(Var(2)); only Var(1) is bound.
        let data = flat_program_from_term_bits(&[
            0, 0, 1, 0, // tag=2 (LamAbs)
            0, 0, 0, 0, // tag=0 (Var)
            0, 0, 0, 0, 0, 0, 1, 0, // natural=2
        ]);

        let err = decode_flat_program(&data).expect_err("out-of-scope variable rejected");
        assert!(
            matches!(&err, MachineError::FlatDecodeError(msg) if msg.contains("open term")),
            "expected open-term FlatDecodeError, got {err:?}"
        );
    }

    // -- Constr / Case decoding -----------------------------------------

    #[test]
    fn test_decode_constr_empty() {
        // Constr = tag 8 (1000), natural tag 0 (0x00), empty list (0-bit).
        let data = bits_to_bytes(&[
            1, 0, 0, 0, // tag=8 (Constr)
            0, 0, 0, 0, 0, 0, 0, 0, // natural=0
            0, // empty list (continuation=0)
            0, 0, 0, // padding
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constr(0, vec![]));
    }

    #[test]
    fn test_decode_case_empty_branches() {
        // Case = tag 9 (1001), scrutinee = Error, empty branch list.
        let data = bits_to_bytes(&[
            1, 0, 0, 1, // tag=9 (Case)
            0, 1, 1, 0, // Error (scrutinee)
            0, // empty branch list
            0, 0, 0, // padding
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Case(Box::new(Term::Error), vec![]));
    }

    // -- FlatDecoder read_bits8 ----------------------------------------

    #[test]
    fn test_read_bits8_cross_boundary() {
        // Read 4 bits, then 8 bits across byte boundary.
        let data = [0xAB, 0xCD]; // 10101011 11001101
        let mut dec = FlatDecoder::new(&data);
        let first4 = dec.read_bits8(4).unwrap();
        assert_eq!(first4, 0x0A); // 1010
        let next8 = dec.read_bits8(8).unwrap();
        assert_eq!(next8, 0xBC); // 10111100
    }

    #[test]
    fn test_read_bit_past_end() {
        let data = [];
        let mut dec = FlatDecoder::new(&data);
        assert!(dec.read_bit().is_err());
    }

    // -- read_list continuation scheme -----------------------------------

    #[test]
    fn test_read_list_empty() {
        // 0 bit = end of list.
        let data = [0x00]; // starts with 0 bit
        let mut dec = FlatDecoder::new(&data);
        let list: Vec<bool> = dec.read_list(|d| d.read_bit()).unwrap();
        assert!(list.is_empty());
    }

    // -- depth bound ----------------------------------------------------

    #[test]
    fn test_decode_term_rejects_pathologically_deep_lambda_chain() {
        // A chain of `LamAbs` (term tag 2 = `0010`) terms, then a final
        // `Error` (term tag 6 = `0110`) at the bottom. Each LamAbs adds
        // one level of recursion; building `MAX_TERM_DECODE_DEPTH + 16`
        // of them must trigger the depth-bound check rather than
        // overflow the runtime stack.
        //
        // Run on a generously-sized thread stack (64 MB) because debug
        // builds on Rust 1.95+ use larger per-frame storage than the
        // default 2 MB cargo-test stack can safely accommodate at the
        // depth budget.  Release builds (which is what production runs)
        // have frames small enough to stay under 2 MB at the same
        // budget; this is purely a test-harness allowance.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let depth = MAX_TERM_DECODE_DEPTH + 16;
                // Each `LamAbs` is a 4-bit tag `0010`. We pack two tags
                // per byte: `0010_0010 = 0x22` → two LamAbs prefixes.
                // Final `Error` is `0110`. If `depth` is even, the
                // trailing nibble for Error follows alone in the next
                // byte's high nibble; we encode explicitly using the
                // bit reader's MSB-first scheme.
                //
                // Use the FlatDecoder bit reader's expected byte order:
                // first bit read is MSB of byte 0. So the first 4-bit
                // nibble decoded is the high nibble of byte 0.
                let mut bytes = Vec::with_capacity(depth / 2 + 4);
                let mut nibbles: Vec<u8> = vec![0b0010; depth]; // LamAbs chain
                nibbles.push(0b0110); // Error tag
                for chunk in nibbles.chunks(2) {
                    let hi = chunk[0] << 4;
                    let lo = chunk.get(1).copied().unwrap_or(0);
                    bytes.push(hi | lo);
                }
                let mut dec = FlatDecoder::new(&bytes);
                let res = dec.decode_term();
                assert!(
                    matches!(&res, Err(MachineError::FlatDecodeError(msg)) if msg.contains("depth budget")),
                    "expected depth-budget FlatDecodeError, got {res:?}"
                );
            })
            .expect("spawn deep-decode test thread")
            .join()
            .expect("deep-decode test thread completed");
    }

    // -- decode_script_bytes -------------------------------------------

    #[test]
    fn test_decode_script_bytes_decodes_plutus_binary_cbor() {
        // Build a flat program: version 1.0.0, body = Error.
        let flat_bytes = vec![0x01, 0x00, 0x00, 0x60];
        let mut script_bytes = vec![0x44u8];
        script_bytes.extend_from_slice(&flat_bytes);

        let program = decode_script_bytes(&script_bytes).expect("decode");
        assert_eq!(program.major, 1);
        assert_eq!(program.term, Term::Error);
    }

    #[test]
    fn test_decode_script_bytes_rejects_raw_flat_without_cbor() {
        let raw_flat = [0x01, 0x00, 0x00, 0x60];

        assert!(decode_script_bytes(&raw_flat).is_err());
    }

    #[test]
    fn test_decode_script_bytes_rejects_trailing_remainder_by_default() {
        let with_remainder = [0x44u8, 0x01, 0x00, 0x00, 0x60, 0x00];

        assert!(decode_script_bytes(&with_remainder).is_err());
    }

    #[test]
    fn test_decode_script_bytes_allowing_remainder_matches_v1_v2() {
        let with_remainder = [0x44u8, 0x01, 0x00, 0x00, 0x60, 0x00];

        let program = decode_script_bytes_allowing_remainder(&with_remainder).expect("decode");
        assert_eq!(program.major, 1);
        assert_eq!(program.term, Term::Error);
    }
}
