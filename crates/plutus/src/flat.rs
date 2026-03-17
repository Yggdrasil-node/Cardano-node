//! Flat binary format decoder for UPLC programs.
//!
//! The Flat encoding is a bit-level binary format used to serialize Plutus
//! Core programs on the Cardano blockchain. This module implements the
//! decoder portion needed to parse on-chain script bytes into the UPLC
//! `Program` and `Term` representations.
//!
//! ## Wire format
//!
//! On-chain Plutus scripts are stored as CBOR bytestrings wrapping the
//! Flat-encoded program. For PlutusV1/V2 there is an additional CBOR
//! bytestring wrapper (double CBOR encoding).
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

/// Decode a UPLC program from raw Flat bytes.
pub fn decode_flat_program(bytes: &[u8]) -> Result<Program, MachineError> {
    let mut dec = FlatDecoder::new(bytes);
    let program = dec.decode_program()?;
    Ok(program)
}

/// Unwrap script bytes from the on-chain CBOR encoding, then Flat-decode.
///
/// PlutusV1/V2 scripts have a double-CBOR wrapping: the outer witness-set
/// decode produces a bytestring whose content is another CBOR bytestring
/// containing the Flat bytes. PlutusV3 uses single wrapping.
///
/// This function handles both cases by checking whether the content starts
/// with a CBOR bytestring header.
pub fn decode_script_bytes(cbor_bytes: &[u8]) -> Result<Program, MachineError> {
    let flat_bytes = unwrap_cbor_bytestring(cbor_bytes);
    decode_flat_program(flat_bytes)
}

/// Strip one layer of CBOR bytestring wrapping if present.
fn unwrap_cbor_bytestring(bytes: &[u8]) -> &[u8] {
    if bytes.is_empty() {
        return bytes;
    }
    let major = bytes[0] >> 5;
    if major != 2 {
        // Not a CBOR bytestring — already raw Flat bytes.
        return bytes;
    }
    let additional = bytes[0] & 0x1f;
    match additional {
        0..=23 => {
            let len = additional as usize;
            if bytes.len() >= 1 + len {
                &bytes[1..1 + len]
            } else {
                bytes
            }
        }
        24 if bytes.len() >= 2 => {
            let len = bytes[1] as usize;
            if bytes.len() >= 2 + len {
                &bytes[2..2 + len]
            } else {
                bytes
            }
        }
        25 if bytes.len() >= 3 => {
            let len = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
            if bytes.len() >= 3 + len {
                &bytes[3..3 + len]
            } else {
                bytes
            }
        }
        26 if bytes.len() >= 5 => {
            let len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
            if bytes.len() >= 5 + len {
                &bytes[5..5 + len]
            } else {
                bytes
            }
        }
        _ => bytes,
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
            result |= val.checked_shl(shift).ok_or_else(|| {
                MachineError::FlatDecodeError("natural number too large".into())
            })?;
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
            result |= val.checked_shl(shift).ok_or_else(|| {
                MachineError::FlatDecodeError("integer too large".into())
            })?;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 127 {
                return Err(MachineError::FlatDecodeError(
                    "integer overflow".into(),
                ));
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
        let tag = self.read_bits8(4)?;
        match tag {
            0 => {
                // Var — de Bruijn index (natural).
                let index = self.read_natural()?;
                Ok(Term::Var(index))
            }
            1 => {
                // Delay
                let body = self.decode_term()?;
                Ok(Term::Delay(Box::new(body)))
            }
            2 => {
                // LamAbs
                let body = self.decode_term()?;
                Ok(Term::LamAbs(Box::new(body)))
            }
            3 => {
                // Apply
                let fun = self.decode_term()?;
                let arg = self.decode_term()?;
                Ok(Term::Apply(Box::new(fun), Box::new(arg)))
            }
            4 => {
                // Constant — type list then value.
                let ty = self.decode_type_list()?;
                let constant = self.decode_constant(&ty)?;
                Ok(Term::Constant(constant))
            }
            5 => {
                // Force
                let body = self.decode_term()?;
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
                let fields = self.read_list(|d| d.decode_term())?;
                Ok(Term::Constr(tag_val, fields))
            }
            9 => {
                // Case (UPLC 1.1.0+)
                let scrutinee = self.decode_term()?;
                let branches = self.read_list(|d| d.decode_term())?;
                Ok(Term::Case(Box::new(scrutinee), branches))
            }
            _ => Err(MachineError::FlatDecodeError(format!(
                "unknown term tag {tag}"
            ))),
        }
    }

    /// Decode a type-tag list (used for constant encoding).
    ///
    /// The type is encoded as a list of tags using the 1-bit continuation
    /// scheme, with recursive structure for parameterized types.
    fn decode_type_list(&mut self) -> Result<Type, MachineError> {
        let tag = self.decode_single_type_tag()?;
        self.build_type(tag)
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

    fn build_type(&mut self, first_tag: u8) -> Result<Type, MachineError> {
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
                let elem = self.build_type(elem_tag)?;
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
                let key = self.build_type(key_tag)?;
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected second type for pair".into(),
                    ));
                }
                let val_tag = self.read_bits8(4)?;
                let val = self.build_type(val_tag)?;
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
                self.build_applied_type(inner_tag)
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
    fn build_applied_type(&mut self, ctor_tag: u8) -> Result<Type, MachineError> {
        match ctor_tag {
            5 => {
                // apply(list, elem_type)
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected element type for applied list".into(),
                    ));
                }
                let elem_tag = self.read_bits8(4)?;
                let elem = self.build_type(elem_tag)?;
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
                let a = self.build_type(a_tag)?;
                // Need another apply for b.
                if !self.read_bit()? {
                    return Err(MachineError::FlatDecodeError(
                        "expected second apply for pair".into(),
                    ));
                }
                let b_tag = self.read_bits8(4)?;
                let b = self.build_type(b_tag)?;
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
                let base = self.build_applied_type(inner)?;
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
                        let b = self.build_type(arg_tag)?;
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

    fn decode_constant(&mut self, ty: &Type) -> Result<Constant, MachineError> {
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
                let items = self.read_list(|d| d.decode_constant(elem_ty))?;
                Ok(Constant::ProtoList(elem_ty.as_ref().clone(), items))
            }
            Type::Pair(a_ty, b_ty) => {
                let a = self.decode_constant(a_ty)?;
                let b = self.decode_constant(b_ty)?;
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
                let elem = yggdrasil_crypto::bls12_381::g1_uncompress(&bs)
                    .map_err(|e| MachineError::FlatDecodeError(format!("invalid G1 element: {e}")))?;
                Ok(Constant::Bls12_381_G1_Element(elem))
            }
            Type::Bls12_381_G2_Element => {
                let bs = self.read_bytestring()?;
                let elem = yggdrasil_crypto::bls12_381::g2_uncompress(&bs)
                    .map_err(|e| MachineError::FlatDecodeError(format!("invalid G2 element: {e}")))?;
                Ok(Constant::Bls12_381_G2_Element(elem))
            }
            Type::Bls12_381_MlResult => {
                Err(MachineError::FlatDecodeError("MlResult cannot appear in Flat-encoded programs".into()))
            }
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
    fn test_unwrap_cbor_bytestring_single() {
        // CBOR bytestring of length 3: 0x43 followed by 3 bytes.
        let data = [0x43, 0x01, 0x02, 0x03];
        let result = unwrap_cbor_bytestring(&data);
        assert_eq!(result, &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_unwrap_cbor_bytestring_not_cbor() {
        // Not a CBOR bytestring (major type 0 = unsigned int).
        let data = [0x01, 0x02, 0x03];
        let result = unwrap_cbor_bytestring(&data);
        assert_eq!(result, &data[..]);
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
}
