//! Universe-tag parsing for Flat-encoded UPLC constants.
//!
//! Mirrors upstream `PlutusCore.Default.Universe` / `Data.Either` Flat
//! universe-tag encoding — each `Type` in a UPLC `Constant` is prefixed
//! with a sequence of 4-bit tags identifying the universe variant
//! (Integer, ByteString, Pair, List, Data, BLS12-381, etc.).
//!
//! Two items:
//!
//! - `DecodedUni` — internal lookahead variant (`Type` plus the synthetic
//!   `ProtoList` / `ProtoPair` tags that take type arguments).
//! - `TypeTagParser` — recursive-descent parser that consumes a
//!   universe-tag list and produces a `Type`.
//!
//! Extracted from `flat.rs` in R273i (Phase γ §R273 ninth slice).

use crate::error::MachineError;
use crate::types::Type;

use super::MAX_TERM_DECODE_DEPTH;

pub(super) enum DecodedUni {
    Star(Type),
    ProtoList,
    ProtoPair,
    PartialPair(Type),
}

pub(super) struct TypeTagParser<'a> {
    tags: &'a [u8],
    pos: usize,
}

impl<'a> TypeTagParser<'a> {
    pub(super) fn new(tags: &'a [u8]) -> Self {
        Self { tags, pos: 0 }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.pos == self.tags.len()
    }

    pub(super) fn remaining(&self) -> usize {
        self.tags.len().saturating_sub(self.pos)
    }

    pub(super) fn next_tag(&mut self) -> Result<u8, MachineError> {
        let tag = self.tags.get(self.pos).copied().ok_or_else(|| {
            MachineError::FlatDecodeError("unexpected end of constant type tags".into())
        })?;
        self.pos += 1;
        Ok(tag)
    }

    pub(super) fn parse_uni(&mut self, depth_remaining: usize) -> Result<DecodedUni, MachineError> {
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

    pub(super) fn apply_uni(
        &self,
        fun: DecodedUni,
        arg: DecodedUni,
    ) -> Result<DecodedUni, MachineError> {
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
