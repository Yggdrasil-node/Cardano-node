//! UPLC term language: Program, Term, Type, Constant.
//!
//! Mirrors upstream `UntypedPlutusCore.Core.Type` (Program, Term) and
//! `PlutusCore.Core.Type` (Type) and `PlutusCore.Default.Universe`
//! (Constant atoms used by the untyped core).
//!
//! Five public types:
//!
//! - `Program` — UPLC program: version triple + body term.
//! - `Term` — UPLC term language (Var, Lambda, Apply, Force, Delay,
//!   Constant, Builtin, Error, Constr, Case).
//! - `Type` — typed-PLC type expressions (carried in error messages).
//! - `Constant` — built-in constant atoms (Integer, ByteString, String,
//!   Unit, Bool, ProtoList, ProtoPair, Data, Bls12_381*).
//!
//! Extracted from `types.rs` in R273g (Phase γ §R273 seventh slice).

use num_bigint::BigInt;
use yggdrasil_ledger::plutus::PlutusData;

use super::default_fun::DefaultFun;

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

/// A UPLC program: version triple plus a body term.
///
/// Reference: Plutus Core `Program` — `(Version, Term)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub term: Term,
}

// ---------------------------------------------------------------------------
// Term
// ---------------------------------------------------------------------------

/// UPLC term — the core untyped lambda calculus with extensions.
///
/// Terms use de Bruijn indices (1-based, most recent = 1).
///
/// Reference: `UntypedPlutusCore.Core.Type.Term`.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// Variable reference (de Bruijn indexed, 1-based).
    Var(u64),
    /// Lambda abstraction (binds one variable; body uses index 1 for it).
    LamAbs(Box<Term>),
    /// Function application.
    Apply(Box<Term>, Box<Term>),
    /// Delayed computation (introduces a type-level thunk).
    Delay(Box<Term>),
    /// Force a delayed computation.
    Force(Box<Term>),
    /// Constant value.
    Constant(Constant),
    /// Built-in function reference.
    Builtin(DefaultFun),
    /// Error — immediately halts evaluation.
    Error,
    /// Constructor application (UPLC 1.1.0+, PlutusV3).
    Constr(u64, Vec<Term>),
    /// Case analysis (UPLC 1.1.0+, PlutusV3).
    Case(Box<Term>, Vec<Term>),
}

// ---------------------------------------------------------------------------
// Type (for constant encoding in Flat)
// ---------------------------------------------------------------------------

/// Type representation used in the Flat constant encoding scheme.
///
/// Constants in Flat are prefixed by a type-tag list that describes their
/// shape, allowing the decoder to know how to interpret the value bits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Integer,
    ByteString,
    String,
    Unit,
    Bool,
    List(Box<Type>),
    Pair(Box<Type>, Box<Type>),
    Data,
    /// BLS12-381 G1 group element.
    #[allow(non_camel_case_types)]
    Bls12_381_G1_Element,
    /// BLS12-381 G2 group element.
    #[allow(non_camel_case_types)]
    Bls12_381_G2_Element,
    /// BLS12-381 Miller loop result.
    #[allow(non_camel_case_types)]
    Bls12_381_MlResult,
}

// ---------------------------------------------------------------------------
// Constant
// ---------------------------------------------------------------------------

/// UPLC constant values.
///
/// These are the literal values that can appear in a UPLC program.
/// The `Data` variant embeds a full Plutus data AST.
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    /// Arbitrary-precision Plutus `Integer`.
    ///
    /// Upstream Plutus uses Haskell `Integer`; using `BigInt` here avoids
    /// turning valid on-chain arithmetic into local overflow failures.
    Integer(BigInt),
    ByteString(Vec<u8>),
    String(String),
    Unit,
    Bool(bool),
    /// Homogeneous list.
    ProtoList(Type, Vec<Constant>),
    /// Pair of constants.
    ProtoPair(Type, Type, Box<Constant>, Box<Constant>),
    /// Embedded Plutus data.
    Data(PlutusData),
    /// BLS12-381 G1 group element.
    #[allow(non_camel_case_types)]
    Bls12_381_G1_Element(yggdrasil_crypto::G1Element),
    /// BLS12-381 G2 group element.
    #[allow(non_camel_case_types)]
    Bls12_381_G2_Element(yggdrasil_crypto::G2Element),
    /// BLS12-381 Miller loop intermediate result.
    #[allow(non_camel_case_types)]
    Bls12_381_MlResult(Box<yggdrasil_crypto::MlResult>),
}

impl Constant {
    /// Construct an arbitrary-precision Plutus integer constant.
    pub fn integer<N: Into<BigInt>>(n: N) -> Self {
        Self::Integer(n.into())
    }
}
