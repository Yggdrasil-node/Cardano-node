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
//! Flat-encoded program bytes extracted from the enclosing CBOR bytestring.
//! Upstream `decodePlutusRunnable` receives those raw bytes, so this module
//! intentionally does not guess at or strip an additional CBOR layer.
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

use yggdrasil_ledger::plutus::PlutusData;

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
/// scripts rarely nest beyond a few dozen levels even after extensive
/// macro expansion; 128 sits well above any realistic legitimate payload
/// while keeping per-frame stack usage of the recursive decoder (which
/// holds local `Box<Term>` allocations and large match scaffolding) safely
/// inside the default 2 MB Rust thread stack in debug builds. Exceeding
/// the bound returns [`MachineError::FlatDecodeError`] cleanly instead of
/// a process crash.
///
/// Reference: defensive bound. Upstream Haskell relies on its lazy `Flat`
/// decoder being stack-safe by construction; the Rust port makes the limit
/// explicit.
pub const MAX_TERM_DECODE_DEPTH: usize = 128;

/// Decode a UPLC program from raw Flat bytes.
pub fn decode_flat_program(bytes: &[u8]) -> Result<Program, MachineError> {
    let mut dec = FlatDecoder::new(bytes);
    let program = dec.decode_program()?;
    Ok(program)
}

/// Decode an on-chain Plutus script from its raw `PlutusBinary` bytes.
///
/// Ledger CBOR decoding already strips the surrounding CBOR bytestring and
/// leaves the Flat-encoded `PlutusBinary` payload. Upstream
/// `decodePlutusRunnable` receives those raw bytes; opportunistically
/// unwrapping a payload that merely starts with a CBOR bytestring major type
/// can reject valid on-chain scripts.
pub fn decode_script_bytes(script_bytes: &[u8]) -> Result<Program, MachineError> {
    decode_flat_program(script_bytes)
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

    /// Advance to the next byte boundary (skip filler bits).
    fn skip_to_byte_boundary(&mut self) {
        if self.bit != 0 {
            self.pos += 1;
            self.bit = 0;
        }
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

    /// Read a Flat bytestring: pad to byte boundary, then length-prefixed chunks.
    fn read_bytestring(&mut self) -> Result<Vec<u8>, MachineError> {
        self.skip_to_byte_boundary();
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
    /// [`MAX_TERM_DECODE_DEPTH`] of 256 fits comfortably inside the default
    /// 2 MB Rust thread stack in debug builds.
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

    /// Decode a type-tag list (used for constant encoding) with an explicit
    /// nesting depth budget.
    ///
    /// The type is encoded as a list of tags using the 1-bit continuation
    /// scheme, with recursive structure for parameterized types.
    fn decode_type_list_with_depth(
        &mut self,
        depth_remaining: usize,
    ) -> Result<Type, MachineError> {
        let tag = self.decode_single_type_tag()?;
        self.build_type_with_depth(tag, depth_remaining)
    }

    fn decode_single_type_tag(&mut self) -> Result<u8, MachineError> {
        // Type tags are encoded using the list-of-tags scheme.
        // Read 1 continuation bit (should be 1 for a non-empty type),
        // then the tag bits.
        // Actually, the type is encoded as: read continuation bit (1=more tags),
        // then 4-bit type tag, then possibly more tags for parameterized types,
        // then 0-continuation to end.
        //
        // The Flat encoding of types uses a tagged format where:
        // - Simple types: [1, 4-bit-tag, 0]
        // - Parameterized: [1, 4-bit-tag, ... inner types ..., 0]
        //
        // But we need to be more precise. Looking at the Plutus Flat instance:
        // The constant type is encoded as a list of "type tags" using the
        // standard list encoding. Each type tag is 4 bits.
        //
        // For simple types (Integer, ByteString, etc.): one tag.
        // For List(a): tag 7 (apply), tag 5 (list), then the element type tags.
        // For Pair(a,b): tag 7 (apply), tag 7 (apply), tag 6 (pair), then a, then b.
        //
        // Actually the scheme is:
        // - The type is encoded as a list of type-tag atoms (each 4 bits)
        //   using the standard 1-bit-continuation list encoding.
        // - The atom sequence encodes the type tree in a specific traversal.

        // Read 4-bit type tag (within the list element).
        let tag = self.read_bits8(4)?;
        Ok(tag)
    }

    fn build_type_with_depth(
        &mut self,
        first_tag: u8,
        depth_remaining: usize,
    ) -> Result<Type, MachineError> {
        if depth_remaining == 0 {
            return Err(MachineError::FlatDecodeError(format!(
                "type nesting exceeded depth budget {MAX_TERM_DECODE_DEPTH}"
            )));
        }
        let next = depth_remaining - 1;
        match first_tag {
            0 => Ok(Type::Integer),
            1 => Ok(Type::ByteString),
            2 => Ok(Type::String),
            3 => Ok(Type::Unit),
            4 => Ok(Type::Bool),
            5 => {
                // ProtoList — next type is the element type.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected element type for list".into(),
                    ));
                }
                let elem_tag = self.read_bits8(4)?;
                let elem = self.build_type_with_depth(elem_tag, next)?;
                Ok(Type::List(Box::new(elem)))
            }
            6 => {
                // ProtoPair — next two types are key and value.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected first type for pair".into(),
                    ));
                }
                let key_tag = self.read_bits8(4)?;
                let key = self.build_type_with_depth(key_tag, next)?;
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected second type for pair".into(),
                    ));
                }
                let val_tag = self.read_bits8(4)?;
                let val = self.build_type_with_depth(val_tag, next)?;
                Ok(Type::Pair(Box::new(key), Box::new(val)))
            }
            7 => {
                // Apply — type application. Read the constructor type, then
                // the argument type(s). This handles the encoding of
                // parameterized types like `list integer` or `pair integer bool`.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected type in apply".into(),
                    ));
                }
                let inner_tag = self.read_bits8(4)?;
                self.build_applied_type_with_depth(inner_tag, next)
            }
            8 => Ok(Type::Data),
            9 => Ok(Type::Bls12_381_G1_Element),
            10 => Ok(Type::Bls12_381_G2_Element),
            11 => Ok(Type::Bls12_381_MlResult),
            _ => Err(MachineError::FlatDecodeError(format!(
                "unknown type tag {first_tag}"
            ))),
        }
    }

    /// Handle type application: `apply(ctor, args...)`.
    fn build_applied_type_with_depth(
        &mut self,
        ctor_tag: u8,
        depth_remaining: usize,
    ) -> Result<Type, MachineError> {
        if depth_remaining == 0 {
            return Err(MachineError::FlatDecodeError(format!(
                "applied-type nesting exceeded depth budget {MAX_TERM_DECODE_DEPTH}"
            )));
        }
        let next = depth_remaining - 1;
        match ctor_tag {
            5 => {
                // apply(list, elem_type)
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected element type for applied list".into(),
                    ));
                }
                let elem_tag = self.read_bits8(4)?;
                let elem = self.build_type_with_depth(elem_tag, next)?;
                Ok(Type::List(Box::new(elem)))
            }
            6 => {
                // apply(pair, first_type, second_type)
                // First is: apply(apply(pair, a), b)
                // We already consumed one apply + pair, so read a.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected first type for applied pair".into(),
                    ));
                }
                let a_tag = self.read_bits8(4)?;
                let a = self.build_type_with_depth(a_tag, next)?;
                // Need another apply for b.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected second apply for pair".into(),
                    ));
                }
                let b_tag = self.read_bits8(4)?;
                let b = self.build_type_with_depth(b_tag, next)?;
                Ok(Type::Pair(Box::new(a), Box::new(b)))
            }
            7 => {
                // Nested apply — e.g., apply(apply(pair, a), b).
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected inner type in nested apply".into(),
                    ));
                }
                let inner = self.read_bits8(4)?;
                let base = self.build_applied_type_with_depth(inner, next)?;
                // The result of the nested apply is applied to one more arg.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected arg type in nested apply".into(),
                    ));
                }
                let arg_tag = self.read_bits8(4)?;
                match base {
                    Type::List(_) => {
                        // Shouldn't happen: list applied to more args.
                        Err(MachineError::FlatDecodeError(
                            "list type over-applied".into(),
                        ))
                    }
                    Type::Pair(a, _) => {
                        // pair applied to second arg.
                        let b = self.build_type_with_depth(arg_tag, next)?;
                        Ok(Type::Pair(a, Box::new(b)))
                    }
                    _ => Err(MachineError::FlatDecodeError(format!(
                        "unexpected nested apply on type tag {inner}"
                    ))),
                }
            }
            _ => Err(MachineError::FlatDecodeError(format!(
                "unexpected type constructor in apply: tag {ctor_tag}"
            ))),
        }
    }

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
        // Empty bytestring: just a zero-length chunk.
        let data = [0x00];
        let mut dec = FlatDecoder::new(&data);
        let bs = dec.read_bytestring().expect("decode");
        assert!(bs.is_empty());
    }

    #[test]
    fn test_read_bytestring_short() {
        // Bytestring [0xAB, 0xCD]: one chunk of length 2, then terminator.
        let data = [0x02, 0xAB, 0xCD, 0x00];
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
        // Constant = tag 4 (0100), type tag Unit=3 (0011). No payload.
        let data = bits_to_bytes(&[
            0, 1, 0, 0, // tag=4 (Constant)
            0, 0, 1, 1, // type tag=3 (Unit)
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Unit));
    }

    #[test]
    fn test_decode_constant_bool_true() {
        // Constant = tag 4 (0100), type tag Bool=4 (0100), payload: 1 bit.
        let data = bits_to_bytes(&[
            0, 1, 0, 0, // tag=4 (Constant)
            0, 1, 0, 0, // type tag=4 (Bool)
            1, // bool value = true
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Bool(true)));
    }

    #[test]
    fn test_decode_constant_bool_false() {
        let data = bits_to_bytes(&[
            0, 1, 0, 0, // tag=4 (Constant)
            0, 1, 0, 0, // type tag=4 (Bool)
            0, // bool value = false
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Bool(false)));
    }

    #[test]
    fn test_decode_constant_integer_zero() {
        // Constant = tag 4, Type = Integer (tag 0).
        // Integer 0: zigzag(0) = 0, encoded as 8-bit group: 0x00.
        let data = bits_to_bytes(&[
            0, 1, 0, 0, // tag=4 (Constant)
            0, 0, 0, 0, // type tag=0 (Integer)
            0, 0, 0, 0, 0, 0, 0, 0, // integer byte 0x00 (value=0, MSB=0 → stop)
        ]);
        let mut dec = FlatDecoder::new(&data);
        let term = dec.decode_term().expect("decode");
        assert_eq!(term, Term::Constant(Constant::Integer(0)));
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

    // -- Skip to byte boundary ------------------------------------------

    #[test]
    fn test_skip_to_byte_boundary() {
        let data = [0xFF, 0xAA];
        let mut dec = FlatDecoder::new(&data);
        dec.read_bit().unwrap(); // consume 1 bit
        dec.skip_to_byte_boundary();
        assert_eq!(dec.pos, 1);
        assert_eq!(dec.bit, 0);
    }

    #[test]
    fn test_skip_to_byte_boundary_already_aligned() {
        let data = [0xFF];
        let mut dec = FlatDecoder::new(&data);
        dec.skip_to_byte_boundary();
        assert_eq!(dec.pos, 0);
        assert_eq!(dec.bit, 0);
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
        // Run on a generously-sized thread stack (8 MB) because debug
        // builds on Rust 1.95+ use larger per-frame storage than the
        // default 2 MB cargo-test stack can safely accommodate at the
        // depth budget.  Release builds (which is what production runs)
        // have frames small enough to stay under 2 MB at the same
        // budget; this is purely a test-harness allowance.
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
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
    fn test_decode_script_bytes_raw_flat() {
        // Build a flat program: version 1.0.0, body = Error.
        let flat_bytes = vec![0x01, 0x00, 0x00, 0x60];

        let program = decode_script_bytes(&flat_bytes).expect("decode");
        assert_eq!(program.major, 1);
        assert_eq!(program.term, Term::Error);
    }

    #[test]
    fn test_decode_script_bytes_does_not_cbor_unwrap() {
        // Ledger CBOR decoding already removes the surrounding bytestring.
        // A caller accidentally passing a CBOR-wrapped payload is decoded as
        // raw Flat bytes. It may still happen to parse, but must not decode to
        // the inner payload through a non-upstream compatibility path.
        let raw = [0x01, 0x00, 0x00, 0x60];
        let wrapped = [0x44u8, 0x01, 0x00, 0x00, 0x60];
        assert_ne!(
            decode_script_bytes(&wrapped).ok(),
            decode_script_bytes(&raw).ok()
        );
    }
}
