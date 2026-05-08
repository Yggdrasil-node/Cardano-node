//! Bit-level Flat decoder for UPLC programs.
//!
//! Mirrors upstream `UntypedPlutusCore.Core.Instance.Flat` /
//! `PlutusCore.Flat` bit-level decoder.
//!
//! Public-to-parent items:
//!
//! - `FlatDecoder` — bit-level reader + Term/Program/Constant decoder.
//! - `Frame`, `Wrap1Op`, `ListBuild` — work-stack enums for the iterative
//!   `decode_term` loop.
//!
//! Extracted from `flat.rs` in R273i (Phase γ §R273 ninth slice).

use num_bigint::BigInt;
use num_traits::{One, Zero};
use yggdrasil_ledger::cbor::CborDecode;
use yggdrasil_ledger::plutus::PlutusData;

use crate::error::MachineError;
use crate::types::{Constant, DefaultFun, Program, Term, Type};

use super::universe::{DecodedUni, TypeTagParser};
use super::{MAX_TERM_DECODE_DEPTH, MAX_TYPE_DECODE_DEPTH};

/// Work-stack frame for the iterative `decode_term` loop.
pub(super) enum Frame {
    /// Read the next 4-bit term tag and dispatch.
    ReadTerm,
    /// Pop one finished term and wrap it as Delay/LamAbs/Force.
    Wrap1(Wrap1Op),
    /// Pop two finished terms and wrap them as `Apply(fun, arg)`.
    WrapApply,
    /// Read a Flat list-continuation bit; if 1, queue the next element;
    /// if 0, finalize the parent (Constr/Case) by collecting all items
    /// pushed since `marker`.
    ReadListContinuation { build: ListBuild, marker: usize },
}

/// Single-child wrappers handled by `Frame::Wrap1`.
pub(super) enum Wrap1Op {
    Delay,
    LamAbs,
    Force,
}

/// Parents that read a Flat list of child terms after they're queued.
pub(super) enum ListBuild {
    Constr(u64),
    Case,
}

pub(super) struct FlatDecoder<'a> {
    bytes: &'a [u8],
    /// Current byte index.
    pos: usize,
    /// Current bit within the byte (0 = MSB, 7 = LSB).
    bit: u8,
}

impl<'a> FlatDecoder<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            bit: 0,
        }
    }

    /// Read a single bit. Returns `true` for 1, `false` for 0.
    pub(super) fn read_bit(&mut self) -> Result<bool, MachineError> {
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
    pub(super) fn read_bits8(&mut self, n: u8) -> Result<u8, MachineError> {
        debug_assert!(n <= 8);
        let mut result: u8 = 0;
        for _ in 0..n {
            result = (result << 1) | u8::from(self.read_bit()?);
        }
        Ok(result)
    }

    /// Read a Flat natural number (variable-length, 8-bit groups, MSB continuation).
    pub(super) fn read_natural(&mut self) -> Result<u64, MachineError> {
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
    pub(super) fn read_integer(&mut self) -> Result<BigInt, MachineError> {
        let mut result = BigInt::zero();
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_bits8(8)?;
            let val = BigInt::from(byte & 0x7F);
            result |= val << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        // Zigzag decode: even → positive, odd → negative.
        let decoded = if (&result & BigInt::one()).is_zero() {
            result >> 1
        } else {
            BigInt::zero() - ((result >> 1) + BigInt::from(1u8))
        };
        Ok(decoded)
    }

    /// Read the Flat filler sequence used before byte-aligned payloads.
    ///
    /// Upstream `dFiller` reads zero bits until the terminating one bit. The
    /// terminator is placed so the next read starts on a byte boundary.
    pub(super) fn read_filler(&mut self) -> Result<(), MachineError> {
        while !self.read_bit()? {}
        Ok(())
    }

    /// Read a Flat bytestring: filler, then length-prefixed chunks.
    pub(super) fn read_bytestring(&mut self) -> Result<Vec<u8>, MachineError> {
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
    pub(super) fn read_string(&mut self) -> Result<String, MachineError> {
        let bytes = self.read_bytestring()?;
        String::from_utf8(bytes)
            .map_err(|_| MachineError::FlatDecodeError("invalid UTF-8 in string constant".into()))
    }

    /// Read a Flat list using the 1-bit continuation scheme.
    pub(super) fn read_list<T>(
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

    pub(super) fn decode_program(&mut self) -> Result<Program, MachineError> {
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

    /// Iterative Flat decoder for [`Term`].
    ///
    /// Untrusted Plutus scripts arrive in witness sets and are decoded
    /// directly from on-chain bytes; a recursive descent decoder hits
    /// Rust's native stack limit on legitimately deep on-chain scripts
    /// (preview reference script `b89b0443…bc5a` exceeds 6,000 levels of
    /// `Apply`/`LamAbs` nesting). This implementation drives the decode
    /// from a heap-allocated work stack so depth is bounded only by
    /// available memory, matching upstream Haskell's stack-safe `Flat`
    /// decoder.
    ///
    /// Reference: `PlutusCore.Flat.dTerm` — recursive on the heap via
    /// GHC's lazy stack; we emulate that behaviour with an explicit
    /// `Vec<Frame>`.
    pub(super) fn decode_term(&mut self) -> Result<Term, MachineError> {
        let mut work: Vec<Frame> = vec![Frame::ReadTerm];
        let mut results: Vec<Term> = Vec::new();
        while let Some(frame) = work.pop() {
            match frame {
                Frame::ReadTerm => self.dispatch_term_tag(&mut work, &mut results)?,
                Frame::Wrap1(op) => {
                    let body = results.pop().ok_or_else(|| {
                        MachineError::FlatDecodeError("term build underflow".into())
                    })?;
                    let term = match op {
                        Wrap1Op::Delay => Term::Delay(Box::new(body)),
                        Wrap1Op::LamAbs => Term::LamAbs(Box::new(body)),
                        Wrap1Op::Force => Term::Force(Box::new(body)),
                    };
                    results.push(term);
                }
                Frame::WrapApply => {
                    let arg = results.pop().ok_or_else(|| {
                        MachineError::FlatDecodeError("apply arg underflow".into())
                    })?;
                    let fun = results.pop().ok_or_else(|| {
                        MachineError::FlatDecodeError("apply fun underflow".into())
                    })?;
                    results.push(Term::Apply(Box::new(fun), Box::new(arg)));
                }
                Frame::ReadListContinuation { build, marker } => {
                    if self.read_bit()? {
                        // Another item: re-arm the continuation, then read the next term.
                        work.push(Frame::ReadListContinuation { build, marker });
                        work.push(Frame::ReadTerm);
                    } else {
                        // List terminator: collect items pushed since `marker`.
                        if results.len() < marker {
                            return Err(MachineError::FlatDecodeError(
                                "list build underflow".into(),
                            ));
                        }
                        let items = results.split_off(marker);
                        let term = match build {
                            ListBuild::Constr(tag_val) => Term::Constr(tag_val, items),
                            ListBuild::Case => {
                                let mut iter = items.into_iter();
                                let scrutinee = iter.next().ok_or_else(|| {
                                    MachineError::FlatDecodeError("case missing scrutinee".into())
                                })?;
                                let branches: Vec<Term> = iter.collect();
                                Term::Case(Box::new(scrutinee), branches)
                            }
                        };
                        results.push(term);
                    }
                }
            }
        }
        if results.len() != 1 {
            return Err(MachineError::FlatDecodeError(format!(
                "term decoder finished with {} results on stack (expected 1)",
                results.len(),
            )));
        }
        Ok(results.pop().expect("len==1 guaranteed"))
    }

    /// Read a single term tag and update the work/result stacks. Inlined
    /// from `decode_term`'s loop so the dispatch branch can append to the
    /// work stack instead of recursing.
    pub(super) fn dispatch_term_tag(
        &mut self,
        work: &mut Vec<Frame>,
        results: &mut Vec<Term>,
    ) -> Result<(), MachineError> {
        let tag = self.read_bits8(4)?;
        match tag {
            0 => {
                let index = self.read_natural()?;
                results.push(Term::Var(index));
            }
            1 => {
                work.push(Frame::Wrap1(Wrap1Op::Delay));
                work.push(Frame::ReadTerm);
            }
            2 => {
                work.push(Frame::Wrap1(Wrap1Op::LamAbs));
                work.push(Frame::ReadTerm);
            }
            3 => {
                // Apply: read fun first, then arg, then wrap.
                work.push(Frame::WrapApply);
                work.push(Frame::ReadTerm); // arg (decoded second, popped first)
                work.push(Frame::ReadTerm); // fun (decoded first, popped second)
            }
            4 => {
                // Constant — type list then value. Both inner decoders are
                // bounded by the type tag list size, so they can stay
                // recursive without risking stack overflow.
                let ty = self.decode_type_list_with_depth(MAX_TYPE_DECODE_DEPTH)?;
                let constant = self.decode_constant_with_depth(&ty, MAX_TYPE_DECODE_DEPTH)?;
                results.push(Term::Constant(constant));
            }
            5 => {
                work.push(Frame::Wrap1(Wrap1Op::Force));
                work.push(Frame::ReadTerm);
            }
            6 => {
                results.push(Term::Error);
            }
            7 => {
                let b = self.read_bits8(7)?;
                let fun = DefaultFun::from_tag(b)?;
                results.push(Term::Builtin(fun));
            }
            8 => {
                // Constr (UPLC 1.1.0+): natural tag, then list of fields.
                let tag_val = self.read_natural()?;
                let marker = results.len();
                work.push(Frame::ReadListContinuation {
                    build: ListBuild::Constr(tag_val),
                    marker,
                });
            }
            9 => {
                // Case (UPLC 1.1.0+): scrutinee, then list of branches. The
                // continuation frame collects scrutinee + branches starting
                // at `marker`, so the scrutinee read is queued AFTER the
                // continuation (popped first).
                let marker = results.len();
                work.push(Frame::ReadListContinuation {
                    build: ListBuild::Case,
                    marker,
                });
                work.push(Frame::ReadTerm); // scrutinee
            }
            _ => {
                return Err(MachineError::FlatDecodeError(format!(
                    "unknown term tag {tag}"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn decode_type_list_with_depth(
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

impl<'a> FlatDecoder<'a> {
    pub(super) fn decode_constant_with_depth(
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
                Ok(Constant::integer(val))
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
    pub(super) fn decode_plutus_data(&mut self) -> Result<PlutusData, MachineError> {
        // PlutusData in Flat: encoded as a bytestring containing CBOR.
        let cbor_bytes = self.read_bytestring()?;
        let mut dec = yggdrasil_ledger::cbor::Decoder::new(&cbor_bytes);
        PlutusData::decode_cbor(&mut dec)
            .map_err(|e| MachineError::FlatDecodeError(format!("PlutusData CBOR: {e}")))
    }
}
