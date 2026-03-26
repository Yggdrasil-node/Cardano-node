//! UPLC (Untyped Plutus Lambda Calculus) types.
//!
//! Defines the term language, constant values, built-in function enumeration,
//! runtime values, and evaluation environment for the CEK machine.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core>

use yggdrasil_ledger::plutus::PlutusData;

use crate::error::MachineError;

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
    Integer(i128),
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
    Bls12_381_MlResult(yggdrasil_crypto::MlResult),
}

// ---------------------------------------------------------------------------
// DefaultFun — all built-in functions
// ---------------------------------------------------------------------------

/// Enumeration of all UPLC built-in functions across PlutusV1/V2/V3.
///
/// Discriminant values match the Flat encoding index used on-chain.
/// Reference: `PlutusCore.Default.Builtins.DefaultFun`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum DefaultFun {
    // -- Integer arithmetic
    AddInteger = 0,
    SubtractInteger = 1,
    MultiplyInteger = 2,
    DivideInteger = 3,
    QuotientInteger = 4,
    RemainderInteger = 5,
    ModInteger = 6,
    EqualsInteger = 7,
    LessThanInteger = 8,
    LessThanEqualsInteger = 9,
    // -- ByteString
    AppendByteString = 10,
    ConsByteString = 11,
    SliceByteString = 12,
    LengthOfByteString = 13,
    IndexByteString = 14,
    EqualsByteString = 15,
    LessThanByteString = 16,
    LessThanEqualsByteString = 17,
    // -- Cryptographic hashing
    Sha2_256 = 18,
    Sha3_256 = 19,
    Blake2b_256 = 20,
    VerifyEd25519Signature = 21,
    // -- String
    AppendString = 22,
    EqualsString = 23,
    EncodeUtf8 = 24,
    DecodeUtf8 = 25,
    // -- Boolean / Unit
    IfThenElse = 26,
    ChooseUnit = 27,
    // -- Tracing
    Trace = 28,
    // -- Pair
    FstPair = 29,
    SndPair = 30,
    // -- List
    ChooseList = 31,
    MkCons = 32,
    HeadList = 33,
    TailList = 34,
    NullList = 35,
    // -- Data
    ChooseData = 36,
    ConstrData = 37,
    MapData = 38,
    ListData = 39,
    IData = 40,
    BData = 41,
    UnConstrData = 42,
    UnMapData = 43,
    UnListData = 44,
    UnIData = 45,
    UnBData = 46,
    EqualsData = 47,
    MkPairData = 48,
    MkNilData = 49,
    MkNilPairData = 50,
    SerialiseData = 51,
    // -- PlutusV2 additions
    VerifyEcdsaSecp256k1Signature = 52,
    VerifySchnorrSecp256k1Signature = 53,
    // -- PlutusV3 additions: BLS12-381
    Bls12_381_G1_Add = 54,
    Bls12_381_G1_Neg = 55,
    Bls12_381_G1_ScalarMul = 56,
    Bls12_381_G1_Equal = 57,
    Bls12_381_G1_HashToGroup = 58,
    Bls12_381_G1_Compress = 59,
    Bls12_381_G1_Uncompress = 60,
    Bls12_381_G2_Add = 61,
    Bls12_381_G2_Neg = 62,
    Bls12_381_G2_ScalarMul = 63,
    Bls12_381_G2_Equal = 64,
    Bls12_381_G2_HashToGroup = 65,
    Bls12_381_G2_Compress = 66,
    Bls12_381_G2_Uncompress = 67,
    Bls12_381_MillerLoop = 68,
    Bls12_381_MulMlResult = 69,
    Bls12_381_FinalVerify = 70,
    // -- PlutusV3 additions: extra hashing
    Keccak_256 = 71,
    Blake2b_224 = 72,
    // -- PlutusV3 additions: integer/bytestring conversion
    IntegerToByteString = 73,
    ByteStringToInteger = 74,
    // -- PlutusV3 additions: bitwise operations
    AndByteString = 75,
    OrByteString = 76,
    XorByteString = 77,
    ComplementByteString = 78,
    ReadBit = 79,
    WriteBits = 80,
    ReplicateByte = 81,
    ShiftByteString = 82,
    RotateByteString = 83,
    CountSetBits = 84,
    FindFirstSetBit = 85,
    Ripemd_160 = 86,
    ExpModInteger = 87,
}

impl DefaultFun {
    /// Decode a builtin function from its Flat encoding index.
    pub fn from_tag(tag: u8) -> Result<Self, MachineError> {
        match tag {
            0 => Ok(Self::AddInteger),
            1 => Ok(Self::SubtractInteger),
            2 => Ok(Self::MultiplyInteger),
            3 => Ok(Self::DivideInteger),
            4 => Ok(Self::QuotientInteger),
            5 => Ok(Self::RemainderInteger),
            6 => Ok(Self::ModInteger),
            7 => Ok(Self::EqualsInteger),
            8 => Ok(Self::LessThanInteger),
            9 => Ok(Self::LessThanEqualsInteger),
            10 => Ok(Self::AppendByteString),
            11 => Ok(Self::ConsByteString),
            12 => Ok(Self::SliceByteString),
            13 => Ok(Self::LengthOfByteString),
            14 => Ok(Self::IndexByteString),
            15 => Ok(Self::EqualsByteString),
            16 => Ok(Self::LessThanByteString),
            17 => Ok(Self::LessThanEqualsByteString),
            18 => Ok(Self::Sha2_256),
            19 => Ok(Self::Sha3_256),
            20 => Ok(Self::Blake2b_256),
            21 => Ok(Self::VerifyEd25519Signature),
            22 => Ok(Self::AppendString),
            23 => Ok(Self::EqualsString),
            24 => Ok(Self::EncodeUtf8),
            25 => Ok(Self::DecodeUtf8),
            26 => Ok(Self::IfThenElse),
            27 => Ok(Self::ChooseUnit),
            28 => Ok(Self::Trace),
            29 => Ok(Self::FstPair),
            30 => Ok(Self::SndPair),
            31 => Ok(Self::ChooseList),
            32 => Ok(Self::MkCons),
            33 => Ok(Self::HeadList),
            34 => Ok(Self::TailList),
            35 => Ok(Self::NullList),
            36 => Ok(Self::ChooseData),
            37 => Ok(Self::ConstrData),
            38 => Ok(Self::MapData),
            39 => Ok(Self::ListData),
            40 => Ok(Self::IData),
            41 => Ok(Self::BData),
            42 => Ok(Self::UnConstrData),
            43 => Ok(Self::UnMapData),
            44 => Ok(Self::UnListData),
            45 => Ok(Self::UnIData),
            46 => Ok(Self::UnBData),
            47 => Ok(Self::EqualsData),
            48 => Ok(Self::MkPairData),
            49 => Ok(Self::MkNilData),
            50 => Ok(Self::MkNilPairData),
            51 => Ok(Self::SerialiseData),
            52 => Ok(Self::VerifyEcdsaSecp256k1Signature),
            53 => Ok(Self::VerifySchnorrSecp256k1Signature),
            54 => Ok(Self::Bls12_381_G1_Add),
            55 => Ok(Self::Bls12_381_G1_Neg),
            56 => Ok(Self::Bls12_381_G1_ScalarMul),
            57 => Ok(Self::Bls12_381_G1_Equal),
            58 => Ok(Self::Bls12_381_G1_HashToGroup),
            59 => Ok(Self::Bls12_381_G1_Compress),
            60 => Ok(Self::Bls12_381_G1_Uncompress),
            61 => Ok(Self::Bls12_381_G2_Add),
            62 => Ok(Self::Bls12_381_G2_Neg),
            63 => Ok(Self::Bls12_381_G2_ScalarMul),
            64 => Ok(Self::Bls12_381_G2_Equal),
            65 => Ok(Self::Bls12_381_G2_HashToGroup),
            66 => Ok(Self::Bls12_381_G2_Compress),
            67 => Ok(Self::Bls12_381_G2_Uncompress),
            68 => Ok(Self::Bls12_381_MillerLoop),
            69 => Ok(Self::Bls12_381_MulMlResult),
            70 => Ok(Self::Bls12_381_FinalVerify),
            71 => Ok(Self::Keccak_256),
            72 => Ok(Self::Blake2b_224),
            73 => Ok(Self::IntegerToByteString),
            74 => Ok(Self::ByteStringToInteger),
            75 => Ok(Self::AndByteString),
            76 => Ok(Self::OrByteString),
            77 => Ok(Self::XorByteString),
            78 => Ok(Self::ComplementByteString),
            79 => Ok(Self::ReadBit),
            80 => Ok(Self::WriteBits),
            81 => Ok(Self::ReplicateByte),
            82 => Ok(Self::ShiftByteString),
            83 => Ok(Self::RotateByteString),
            84 => Ok(Self::CountSetBits),
            85 => Ok(Self::FindFirstSetBit),
            86 => Ok(Self::Ripemd_160),
            87 => Ok(Self::ExpModInteger),
            _ => Err(MachineError::FlatDecodeError(format!(
                "unknown builtin tag {tag}"
            ))),
        }
    }

    /// Returns `(type_forces, value_args)` — how many `Force` applications
    /// and how many `Apply` value arguments this builtin expects before it
    /// can be evaluated.
    pub fn arity(self) -> (usize, usize) {
        use DefaultFun::*;
        match self {
            // Integer arithmetic — monomorphic, 2 args
            AddInteger | SubtractInteger | MultiplyInteger
            | DivideInteger | QuotientInteger | RemainderInteger
            | ModInteger => (0, 2),
            EqualsInteger | LessThanInteger | LessThanEqualsInteger => (0, 2),

            // ByteString
            AppendByteString => (0, 2),
            ConsByteString => (0, 2),
            SliceByteString => (0, 3),
            LengthOfByteString => (0, 1),
            IndexByteString => (0, 2),
            EqualsByteString | LessThanByteString | LessThanEqualsByteString => (0, 2),

            // Crypto
            Sha2_256 | Sha3_256 | Blake2b_256 => (0, 1),
            VerifyEd25519Signature => (0, 3),

            // String
            AppendString => (0, 2),
            EqualsString => (0, 2),
            EncodeUtf8 | DecodeUtf8 => (0, 1),

            // Bool / Unit — polymorphic (1 force)
            IfThenElse => (1, 3),
            ChooseUnit => (1, 2),

            // Tracing — polymorphic (1 force)
            Trace => (1, 2),

            // Pair — polymorphic in 2 type vars
            FstPair | SndPair => (2, 1),

            // List — polymorphic (various)
            ChooseList => (2, 3),
            MkCons => (1, 2),
            HeadList | TailList | NullList => (1, 1),

            // Data
            ChooseData => (1, 6),
            ConstrData | MkPairData => (0, 2),
            MapData | ListData | IData | BData => (0, 1),
            UnConstrData | UnMapData | UnListData | UnIData | UnBData => (0, 1),
            EqualsData => (0, 2),
            MkNilData | MkNilPairData => (0, 1),
            SerialiseData => (0, 1),

            // PlutusV2
            VerifyEcdsaSecp256k1Signature | VerifySchnorrSecp256k1Signature => (0, 3),

            // PlutusV3: BLS
            Bls12_381_G1_Add | Bls12_381_G1_ScalarMul | Bls12_381_G1_Equal => (0, 2),
            Bls12_381_G1_Neg | Bls12_381_G1_Compress | Bls12_381_G1_Uncompress => (0, 1),
            Bls12_381_G1_HashToGroup => (0, 2),
            Bls12_381_G2_Add | Bls12_381_G2_ScalarMul | Bls12_381_G2_Equal => (0, 2),
            Bls12_381_G2_Neg | Bls12_381_G2_Compress | Bls12_381_G2_Uncompress => (0, 1),
            Bls12_381_G2_HashToGroup => (0, 2),
            Bls12_381_MillerLoop | Bls12_381_MulMlResult | Bls12_381_FinalVerify => (0, 2),

            // PlutusV3: hashing
            Keccak_256 | Blake2b_224 | Ripemd_160 => (0, 1),

            // PlutusV3: integer/bytestring
            IntegerToByteString => (0, 3),
            ByteStringToInteger => (0, 2),

            // PlutusV3: bitwise
            AndByteString | OrByteString | XorByteString => (0, 3),
            ComplementByteString | CountSetBits | FindFirstSetBit => (0, 1),
            ReadBit => (0, 2),
            WriteBits => (0, 3),
            ReplicateByte | ShiftByteString | RotateByteString => (0, 2),
            ExpModInteger => (0, 3),
        }
    }

    /// Human-readable name matching the upstream Haskell constructor.
    pub fn name(self) -> &'static str {
        use DefaultFun::*;
        match self {
            AddInteger => "addInteger",
            SubtractInteger => "subtractInteger",
            MultiplyInteger => "multiplyInteger",
            DivideInteger => "divideInteger",
            QuotientInteger => "quotientInteger",
            RemainderInteger => "remainderInteger",
            ModInteger => "modInteger",
            EqualsInteger => "equalsInteger",
            LessThanInteger => "lessThanInteger",
            LessThanEqualsInteger => "lessThanEqualsInteger",
            AppendByteString => "appendByteString",
            ConsByteString => "consByteString",
            SliceByteString => "sliceByteString",
            LengthOfByteString => "lengthOfByteString",
            IndexByteString => "indexByteString",
            EqualsByteString => "equalsByteString",
            LessThanByteString => "lessThanByteString",
            LessThanEqualsByteString => "lessThanEqualsByteString",
            Sha2_256 => "sha2_256",
            Sha3_256 => "sha3_256",
            Blake2b_256 => "blake2b_256",
            VerifyEd25519Signature => "verifyEd25519Signature",
            AppendString => "appendString",
            EqualsString => "equalsString",
            EncodeUtf8 => "encodeUtf8",
            DecodeUtf8 => "decodeUtf8",
            IfThenElse => "ifThenElse",
            ChooseUnit => "chooseUnit",
            Trace => "trace",
            FstPair => "fstPair",
            SndPair => "sndPair",
            ChooseList => "chooseList",
            MkCons => "mkCons",
            HeadList => "headList",
            TailList => "tailList",
            NullList => "nullList",
            ChooseData => "chooseData",
            ConstrData => "constrData",
            MapData => "mapData",
            ListData => "listData",
            IData => "iData",
            BData => "bData",
            UnConstrData => "unConstrData",
            UnMapData => "unMapData",
            UnListData => "unListData",
            UnIData => "unIData",
            UnBData => "unBData",
            EqualsData => "equalsData",
            MkPairData => "mkPairData",
            MkNilData => "mkNilData",
            MkNilPairData => "mkNilPairData",
            SerialiseData => "serialiseData",
            VerifyEcdsaSecp256k1Signature => "verifyEcdsaSecp256k1Signature",
            VerifySchnorrSecp256k1Signature => "verifySchnorrSecp256k1Signature",
            Bls12_381_G1_Add => "bls12_381_G1_add",
            Bls12_381_G1_Neg => "bls12_381_G1_neg",
            Bls12_381_G1_ScalarMul => "bls12_381_G1_scalarMul",
            Bls12_381_G1_Equal => "bls12_381_G1_equal",
            Bls12_381_G1_HashToGroup => "bls12_381_G1_hashToGroup",
            Bls12_381_G1_Compress => "bls12_381_G1_compress",
            Bls12_381_G1_Uncompress => "bls12_381_G1_uncompress",
            Bls12_381_G2_Add => "bls12_381_G2_add",
            Bls12_381_G2_Neg => "bls12_381_G2_neg",
            Bls12_381_G2_ScalarMul => "bls12_381_G2_scalarMul",
            Bls12_381_G2_Equal => "bls12_381_G2_equal",
            Bls12_381_G2_HashToGroup => "bls12_381_G2_hashToGroup",
            Bls12_381_G2_Compress => "bls12_381_G2_compress",
            Bls12_381_G2_Uncompress => "bls12_381_G2_uncompress",
            Bls12_381_MillerLoop => "bls12_381_millerLoop",
            Bls12_381_MulMlResult => "bls12_381_mulMlResult",
            Bls12_381_FinalVerify => "bls12_381_finalVerify",
            Keccak_256 => "keccak_256",
            Blake2b_224 => "blake2b_224",
            IntegerToByteString => "integerToByteString",
            ByteStringToInteger => "byteStringToInteger",
            AndByteString => "andByteString",
            OrByteString => "orByteString",
            XorByteString => "xorByteString",
            ComplementByteString => "complementByteString",
            ReadBit => "readBit",
            WriteBits => "writeBits",
            ReplicateByte => "replicateByte",
            ShiftByteString => "shiftByteString",
            RotateByteString => "rotateByteString",
            CountSetBits => "countSetBits",
            FindFirstSetBit => "findFirstSetBit",
            Ripemd_160 => "ripemd_160",
            ExpModInteger => "expModInteger",
        }
    }
}

// ---------------------------------------------------------------------------
// ExBudget
// ---------------------------------------------------------------------------

/// Execution budget tracking CPU steps and memory units.
///
/// Mirrors `ExUnits` from the ledger but used within the evaluator to
/// track consumption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}

impl ExBudget {
    pub fn new(cpu: i64, mem: i64) -> Self {
        Self { cpu, mem }
    }

    /// Returns `true` if both components are non-negative.
    pub fn is_within_limit(&self) -> bool {
        self.cpu >= 0 && self.mem >= 0
    }

    /// Spend some budget. Returns an error if the budget is exceeded.
    pub fn spend(&mut self, cost: ExBudget) -> Result<(), MachineError> {
        self.cpu -= cost.cpu;
        self.mem -= cost.mem;
        if self.cpu < 0 || self.mem < 0 {
            Err(MachineError::OutOfBudget(format!(
                "remaining cpu={}, mem={}",
                self.cpu, self.mem
            )))
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Value — CEK machine runtime values
// ---------------------------------------------------------------------------

/// Runtime value produced by the CEK machine.
#[derive(Clone, Debug)]
pub enum Value {
    /// A constant.
    Constant(Constant),
    /// A lambda closure capturing its environment.
    Lambda(Term, Environment),
    /// A delayed computation capturing its environment.
    Delay(Term, Environment),
    /// A partially applied built-in function.
    BuiltinApp {
        fun: DefaultFun,
        /// Number of `Force` (type) arguments received so far.
        forces: usize,
        /// Value arguments received so far (in application order).
        args: Vec<Value>,
    },
    /// A constructed value (UPLC 1.1.0+).
    Constr(u64, Vec<Value>),
}

impl Value {
    /// Extract as a constant, or return a type mismatch error.
    pub fn as_constant(&self) -> Result<&Constant, MachineError> {
        match self {
            Self::Constant(c) => Ok(c),
            other => Err(MachineError::TypeMismatch {
                expected: "constant",
                actual: other.type_name().to_string(),
            }),
        }
    }

    /// Human-readable type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Constant(_) => "constant",
            Self::Lambda(..) => "lambda",
            Self::Delay(..) => "delay",
            Self::BuiltinApp { .. } => "builtin",
            Self::Constr(..) => "constr",
        }
    }
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// CEK environment mapping de Bruijn indices to values.
///
/// Index 1 refers to the most recently bound variable (last element).
#[derive(Clone, Debug, Default)]
pub struct Environment {
    values: Vec<Value>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new environment with `val` as the most recent binding.
    pub fn extend(&self, val: Value) -> Self {
        let mut values = self.values.clone();
        values.push(val);
        Self { values }
    }

    /// Look up a 1-based de Bruijn index.
    pub fn lookup(&self, index: u64) -> Result<&Value, MachineError> {
        if index == 0 || index as usize > self.values.len() {
            return Err(MachineError::UnboundVariable(index));
        }
        Ok(&self.values[self.values.len() - index as usize])
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Program ----------------------------------------------------------

    #[test]
    fn program_clone_and_eq() {
        let p = Program {
            major: 1,
            minor: 1,
            patch: 0,
            term: Term::Constant(Constant::Integer(42)),
        };
        assert_eq!(p.clone(), p);
    }

    #[test]
    fn program_debug() {
        let p = Program {
            major: 1,
            minor: 0,
            patch: 0,
            term: Term::Error,
        };
        let dbg = format!("{:?}", p);
        assert!(dbg.contains("Program"));
    }

    #[test]
    fn program_ne() {
        let a = Program { major: 1, minor: 0, patch: 0, term: Term::Error };
        let b = Program { major: 2, minor: 0, patch: 0, term: Term::Error };
        assert_ne!(a, b);
    }

    // -- Term variants ----------------------------------------------------

    #[test]
    fn term_var() {
        let t = Term::Var(1);
        assert_eq!(t.clone(), Term::Var(1));
    }

    #[test]
    fn term_lam_abs() {
        let t = Term::LamAbs(Box::new(Term::Var(1)));
        assert_eq!(t, Term::LamAbs(Box::new(Term::Var(1))));
    }

    #[test]
    fn term_apply() {
        let f = Term::LamAbs(Box::new(Term::Var(1)));
        let a = Term::Constant(Constant::Integer(10));
        let t = Term::Apply(Box::new(f.clone()), Box::new(a.clone()));
        assert_eq!(t, Term::Apply(Box::new(f), Box::new(a)));
    }

    #[test]
    fn term_delay_force() {
        let inner = Term::Constant(Constant::Unit);
        let d = Term::Delay(Box::new(inner.clone()));
        let f = Term::Force(Box::new(d.clone()));
        assert_eq!(f, Term::Force(Box::new(Term::Delay(Box::new(inner)))));
    }

    #[test]
    fn term_constant_integer() {
        let t = Term::Constant(Constant::Integer(i128::MAX));
        assert_eq!(t, Term::Constant(Constant::Integer(i128::MAX)));
    }

    #[test]
    fn term_constant_bytestring() {
        let t = Term::Constant(Constant::ByteString(vec![1, 2, 3]));
        if let Term::Constant(Constant::ByteString(bs)) = &t {
            assert_eq!(bs, &[1, 2, 3]);
        } else {
            panic!("expected ByteString");
        }
    }

    #[test]
    fn term_constant_string() {
        let t = Term::Constant(Constant::String("hello".into()));
        if let Term::Constant(Constant::String(s)) = &t {
            assert_eq!(s, "hello");
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn term_constant_unit() {
        let t = Term::Constant(Constant::Unit);
        assert_eq!(t, Term::Constant(Constant::Unit));
    }

    #[test]
    fn term_constant_bool_true() {
        let t = Term::Constant(Constant::Bool(true));
        assert_eq!(t, Term::Constant(Constant::Bool(true)));
    }

    #[test]
    fn term_constant_bool_false() {
        let t = Term::Constant(Constant::Bool(false));
        assert_ne!(t, Term::Constant(Constant::Bool(true)));
    }

    #[test]
    fn term_builtin() {
        let t = Term::Builtin(DefaultFun::AddInteger);
        assert_eq!(t, Term::Builtin(DefaultFun::AddInteger));
    }

    #[test]
    fn term_error() {
        assert_eq!(Term::Error, Term::Error);
    }

    #[test]
    fn term_constr_empty() {
        let t = Term::Constr(0, vec![]);
        assert_eq!(t, Term::Constr(0, vec![]));
    }

    #[test]
    fn term_constr_with_fields() {
        let t = Term::Constr(1, vec![
            Term::Constant(Constant::Integer(1)),
            Term::Constant(Constant::Integer(2)),
        ]);
        if let Term::Constr(tag, fields) = &t {
            assert_eq!(*tag, 1);
            assert_eq!(fields.len(), 2);
        } else {
            panic!("expected Constr");
        }
    }

    #[test]
    fn term_case() {
        let scrutinee = Term::Constr(0, vec![]);
        let branch = Term::Constant(Constant::Integer(42));
        let t = Term::Case(Box::new(scrutinee), vec![branch]);
        if let Term::Case(_, branches) = &t {
            assert_eq!(branches.len(), 1);
        } else {
            panic!("expected Case");
        }
    }

    #[test]
    fn term_debug_format() {
        let t = Term::Var(42);
        assert!(format!("{:?}", t).contains("Var"));
    }

    // -- Type -------------------------------------------------------------

    #[test]
    fn type_simple_variants() {
        assert_eq!(Type::Integer, Type::Integer);
        assert_eq!(Type::ByteString, Type::ByteString);
        assert_eq!(Type::String, Type::String);
        assert_eq!(Type::Unit, Type::Unit);
        assert_eq!(Type::Bool, Type::Bool);
        assert_eq!(Type::Data, Type::Data);
    }

    #[test]
    fn type_ne() {
        assert_ne!(Type::Integer, Type::ByteString);
    }

    #[test]
    fn type_list() {
        let t = Type::List(Box::new(Type::Integer));
        assert_eq!(t.clone(), Type::List(Box::new(Type::Integer)));
    }

    #[test]
    fn type_pair() {
        let t = Type::Pair(Box::new(Type::Integer), Box::new(Type::ByteString));
        assert_eq!(t.clone(), Type::Pair(Box::new(Type::Integer), Box::new(Type::ByteString)));
    }

    #[test]
    fn type_bls_variants() {
        assert_eq!(Type::Bls12_381_G1_Element, Type::Bls12_381_G1_Element);
        assert_eq!(Type::Bls12_381_G2_Element, Type::Bls12_381_G2_Element);
        assert_eq!(Type::Bls12_381_MlResult, Type::Bls12_381_MlResult);
    }

    #[test]
    fn type_nested_list_of_pairs() {
        let inner = Type::Pair(Box::new(Type::Data), Box::new(Type::Data));
        let outer = Type::List(Box::new(inner.clone()));
        assert_eq!(
            outer,
            Type::List(Box::new(Type::Pair(
                Box::new(Type::Data),
                Box::new(Type::Data),
            )))
        );
    }

    // -- Constant ---------------------------------------------------------

    #[test]
    fn constant_integer_eq() {
        assert_eq!(Constant::Integer(0), Constant::Integer(0));
        assert_ne!(Constant::Integer(1), Constant::Integer(2));
    }

    #[test]
    fn constant_bytestring_eq() {
        assert_eq!(
            Constant::ByteString(vec![0xDE, 0xAD]),
            Constant::ByteString(vec![0xDE, 0xAD]),
        );
    }

    #[test]
    fn constant_string_eq() {
        assert_eq!(Constant::String("abc".into()), Constant::String("abc".into()));
    }

    #[test]
    fn constant_unit_eq() {
        assert_eq!(Constant::Unit, Constant::Unit);
    }

    #[test]
    fn constant_bool() {
        assert_eq!(Constant::Bool(true), Constant::Bool(true));
        assert_ne!(Constant::Bool(true), Constant::Bool(false));
    }

    #[test]
    fn constant_proto_list() {
        let c = Constant::ProtoList(
            Type::Integer,
            vec![Constant::Integer(1), Constant::Integer(2)],
        );
        if let Constant::ProtoList(ty, items) = &c {
            assert_eq!(*ty, Type::Integer);
            assert_eq!(items.len(), 2);
        } else {
            panic!("expected ProtoList");
        }
    }

    #[test]
    fn constant_proto_pair() {
        let c = Constant::ProtoPair(
            Type::Integer,
            Type::ByteString,
            Box::new(Constant::Integer(42)),
            Box::new(Constant::ByteString(vec![1])),
        );
        if let Constant::ProtoPair(t1, t2, a, b) = &c {
            assert_eq!(*t1, Type::Integer);
            assert_eq!(*t2, Type::ByteString);
            assert_eq!(**a, Constant::Integer(42));
            assert_eq!(**b, Constant::ByteString(vec![1]));
        } else {
            panic!("expected ProtoPair");
        }
    }

    #[test]
    fn constant_data() {
        let d = PlutusData::Integer(99);
        let c = Constant::Data(d.clone());
        assert_eq!(c, Constant::Data(PlutusData::Integer(99)));
    }

    #[test]
    fn constant_empty_list() {
        let c = Constant::ProtoList(Type::Data, vec![]);
        if let Constant::ProtoList(_, items) = &c {
            assert!(items.is_empty());
        } else {
            panic!("expected ProtoList");
        }
    }

    // -- DefaultFun -------------------------------------------------------

    #[test]
    fn default_fun_from_tag_all_valid() {
        // Every tag 0..=87 should return Ok.
        for tag in 0..=87u8 {
            assert!(
                DefaultFun::from_tag(tag).is_ok(),
                "tag {tag} should be valid"
            );
        }
    }

    #[test]
    fn default_fun_from_tag_invalid() {
        assert!(DefaultFun::from_tag(88).is_err());
        assert!(DefaultFun::from_tag(255).is_err());
    }

    #[test]
    fn default_fun_from_tag_round_trip() {
        // from_tag(n) should produce the variant with discriminant n.
        let f = DefaultFun::from_tag(0).unwrap();
        assert_eq!(f, DefaultFun::AddInteger);
        assert_eq!(f as u8, 0);

        let f87 = DefaultFun::from_tag(87).unwrap();
        assert_eq!(f87, DefaultFun::ExpModInteger);
        assert_eq!(f87 as u8, 87);
    }

    #[test]
    fn default_fun_from_tag_error_message() {
        let err = DefaultFun::from_tag(100).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("100"), "error should mention the tag: {msg}");
    }

    #[test]
    fn default_fun_name_all_88() {
        // Every variant should have a non-empty name.
        for tag in 0..=87u8 {
            let f = DefaultFun::from_tag(tag).unwrap();
            let name = f.name();
            assert!(!name.is_empty(), "tag {tag} should have a name");
        }
    }

    #[test]
    fn default_fun_name_spot_checks() {
        assert_eq!(DefaultFun::AddInteger.name(), "addInteger");
        assert_eq!(DefaultFun::SubtractInteger.name(), "subtractInteger");
        assert_eq!(DefaultFun::MultiplyInteger.name(), "multiplyInteger");
        assert_eq!(DefaultFun::Sha2_256.name(), "sha2_256");
        assert_eq!(DefaultFun::LessThanEqualsByteString.name(), "lessThanEqualsByteString");
        assert_eq!(DefaultFun::IfThenElse.name(), "ifThenElse");
        assert_eq!(DefaultFun::HeadList.name(), "headList");
        assert_eq!(DefaultFun::ConstrData.name(), "constrData");
        assert_eq!(DefaultFun::EqualsData.name(), "equalsData");
        assert_eq!(DefaultFun::VerifyEcdsaSecp256k1Signature.name(), "verifyEcdsaSecp256k1Signature");
        assert_eq!(DefaultFun::Bls12_381_G1_Add.name(), "bls12_381_G1_add");
        assert_eq!(DefaultFun::Keccak_256.name(), "keccak_256");
        assert_eq!(DefaultFun::IntegerToByteString.name(), "integerToByteString");
        assert_eq!(DefaultFun::AndByteString.name(), "andByteString");
        assert_eq!(DefaultFun::ExpModInteger.name(), "expModInteger");
    }

    #[test]
    fn default_fun_arity_integer_ops() {
        assert_eq!(DefaultFun::AddInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::SubtractInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::MultiplyInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::DivideInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::QuotientInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::RemainderInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::ModInteger.arity(), (0, 2));
    }

    #[test]
    fn default_fun_arity_comparison() {
        assert_eq!(DefaultFun::EqualsInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::LessThanInteger.arity(), (0, 2));
        assert_eq!(DefaultFun::LessThanEqualsInteger.arity(), (0, 2));
    }

    #[test]
    fn default_fun_arity_bytestring() {
        assert_eq!(DefaultFun::AppendByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::ConsByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::SliceByteString.arity(), (0, 3));
        assert_eq!(DefaultFun::LengthOfByteString.arity(), (0, 1));
        assert_eq!(DefaultFun::IndexByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::EqualsByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::LessThanByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::LessThanEqualsByteString.arity(), (0, 2));
    }

    #[test]
    fn default_fun_arity_crypto() {
        assert_eq!(DefaultFun::Sha2_256.arity(), (0, 1));
        assert_eq!(DefaultFun::Sha3_256.arity(), (0, 1));
        assert_eq!(DefaultFun::Blake2b_256.arity(), (0, 1));
        assert_eq!(DefaultFun::VerifyEd25519Signature.arity(), (0, 3));
    }

    #[test]
    fn default_fun_arity_polymorphic() {
        // Polymorphic builtins require force applications.
        assert_eq!(DefaultFun::IfThenElse.arity(), (1, 3));
        assert_eq!(DefaultFun::ChooseUnit.arity(), (1, 2));
        assert_eq!(DefaultFun::Trace.arity(), (1, 2));
        assert_eq!(DefaultFun::FstPair.arity(), (2, 1));
        assert_eq!(DefaultFun::SndPair.arity(), (2, 1));
        assert_eq!(DefaultFun::ChooseList.arity(), (2, 3));
        assert_eq!(DefaultFun::MkCons.arity(), (1, 2));
        assert_eq!(DefaultFun::HeadList.arity(), (1, 1));
        assert_eq!(DefaultFun::TailList.arity(), (1, 1));
        assert_eq!(DefaultFun::NullList.arity(), (1, 1));
        assert_eq!(DefaultFun::ChooseData.arity(), (1, 6));
    }

    #[test]
    fn default_fun_arity_data_ops() {
        assert_eq!(DefaultFun::ConstrData.arity(), (0, 2));
        assert_eq!(DefaultFun::MapData.arity(), (0, 1));
        assert_eq!(DefaultFun::ListData.arity(), (0, 1));
        assert_eq!(DefaultFun::IData.arity(), (0, 1));
        assert_eq!(DefaultFun::BData.arity(), (0, 1));
        assert_eq!(DefaultFun::UnConstrData.arity(), (0, 1));
        assert_eq!(DefaultFun::UnMapData.arity(), (0, 1));
        assert_eq!(DefaultFun::UnListData.arity(), (0, 1));
        assert_eq!(DefaultFun::UnIData.arity(), (0, 1));
        assert_eq!(DefaultFun::UnBData.arity(), (0, 1));
        assert_eq!(DefaultFun::EqualsData.arity(), (0, 2));
        assert_eq!(DefaultFun::MkPairData.arity(), (0, 2));
        assert_eq!(DefaultFun::MkNilData.arity(), (0, 1));
        assert_eq!(DefaultFun::MkNilPairData.arity(), (0, 1));
        assert_eq!(DefaultFun::SerialiseData.arity(), (0, 1));
    }

    #[test]
    fn default_fun_arity_v2() {
        assert_eq!(DefaultFun::VerifyEcdsaSecp256k1Signature.arity(), (0, 3));
        assert_eq!(DefaultFun::VerifySchnorrSecp256k1Signature.arity(), (0, 3));
    }

    #[test]
    fn default_fun_arity_bls() {
        assert_eq!(DefaultFun::Bls12_381_G1_Add.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_G1_Neg.arity(), (0, 1));
        assert_eq!(DefaultFun::Bls12_381_G1_ScalarMul.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_G1_Equal.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_G1_HashToGroup.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_G1_Compress.arity(), (0, 1));
        assert_eq!(DefaultFun::Bls12_381_G1_Uncompress.arity(), (0, 1));
        assert_eq!(DefaultFun::Bls12_381_MillerLoop.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_MulMlResult.arity(), (0, 2));
        assert_eq!(DefaultFun::Bls12_381_FinalVerify.arity(), (0, 2));
    }

    #[test]
    fn default_fun_arity_v3_hashing() {
        assert_eq!(DefaultFun::Keccak_256.arity(), (0, 1));
        assert_eq!(DefaultFun::Blake2b_224.arity(), (0, 1));
        assert_eq!(DefaultFun::Ripemd_160.arity(), (0, 1));
    }

    #[test]
    fn default_fun_arity_v3_conversion() {
        assert_eq!(DefaultFun::IntegerToByteString.arity(), (0, 3));
        assert_eq!(DefaultFun::ByteStringToInteger.arity(), (0, 2));
    }

    #[test]
    fn default_fun_arity_v3_bitwise() {
        assert_eq!(DefaultFun::AndByteString.arity(), (0, 3));
        assert_eq!(DefaultFun::OrByteString.arity(), (0, 3));
        assert_eq!(DefaultFun::XorByteString.arity(), (0, 3));
        assert_eq!(DefaultFun::ComplementByteString.arity(), (0, 1));
        assert_eq!(DefaultFun::ReadBit.arity(), (0, 2));
        assert_eq!(DefaultFun::WriteBits.arity(), (0, 3));
        assert_eq!(DefaultFun::ReplicateByte.arity(), (0, 2));
        assert_eq!(DefaultFun::ShiftByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::RotateByteString.arity(), (0, 2));
        assert_eq!(DefaultFun::CountSetBits.arity(), (0, 1));
        assert_eq!(DefaultFun::FindFirstSetBit.arity(), (0, 1));
        assert_eq!(DefaultFun::ExpModInteger.arity(), (0, 3));
    }

    #[test]
    fn default_fun_hash_and_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DefaultFun::AddInteger);
        set.insert(DefaultFun::AddInteger);
        assert_eq!(set.len(), 1);
        set.insert(DefaultFun::SubtractInteger);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn default_fun_copy_semantics() {
        let a = DefaultFun::AddInteger;
        let b = a; // Copy
        assert_eq!(a, b);
    }

    // -- ExBudget ---------------------------------------------------------

    #[test]
    fn ex_budget_new() {
        let b = ExBudget::new(100, 200);
        assert_eq!(b.cpu, 100);
        assert_eq!(b.mem, 200);
    }

    #[test]
    fn ex_budget_default() {
        let b = ExBudget::default();
        assert_eq!(b.cpu, 0);
        assert_eq!(b.mem, 0);
    }

    #[test]
    fn ex_budget_is_within_limit_positive() {
        let b = ExBudget::new(100, 200);
        assert!(b.is_within_limit());
    }

    #[test]
    fn ex_budget_is_within_limit_zero() {
        let b = ExBudget::new(0, 0);
        assert!(b.is_within_limit());
    }

    #[test]
    fn ex_budget_is_within_limit_negative_cpu() {
        let b = ExBudget::new(-1, 100);
        assert!(!b.is_within_limit());
    }

    #[test]
    fn ex_budget_is_within_limit_negative_mem() {
        let b = ExBudget::new(100, -1);
        assert!(!b.is_within_limit());
    }

    #[test]
    fn ex_budget_is_within_limit_both_negative() {
        let b = ExBudget::new(-5, -10);
        assert!(!b.is_within_limit());
    }

    #[test]
    fn ex_budget_spend_success() {
        let mut b = ExBudget::new(100, 200);
        let cost = ExBudget::new(50, 100);
        assert!(b.spend(cost).is_ok());
        assert_eq!(b.cpu, 50);
        assert_eq!(b.mem, 100);
    }

    #[test]
    fn ex_budget_spend_exact() {
        let mut b = ExBudget::new(100, 200);
        assert!(b.spend(ExBudget::new(100, 200)).is_ok());
        assert_eq!(b.cpu, 0);
        assert_eq!(b.mem, 0);
    }

    #[test]
    fn ex_budget_spend_exceeds_cpu() {
        let mut b = ExBudget::new(10, 200);
        let err = b.spend(ExBudget::new(20, 0)).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("budget"));
    }

    #[test]
    fn ex_budget_spend_exceeds_mem() {
        let mut b = ExBudget::new(200, 10);
        let err = b.spend(ExBudget::new(0, 20)).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("budget"));
    }

    #[test]
    fn ex_budget_spend_multiple() {
        let mut b = ExBudget::new(100, 100);
        b.spend(ExBudget::new(30, 20)).unwrap();
        b.spend(ExBudget::new(30, 20)).unwrap();
        assert_eq!(b.cpu, 40);
        assert_eq!(b.mem, 60);
        b.spend(ExBudget::new(40, 60)).unwrap();
        assert_eq!(b.cpu, 0);
        assert_eq!(b.mem, 0);
    }

    #[test]
    fn ex_budget_clone_and_eq() {
        let a = ExBudget::new(42, 99);
        assert_eq!(a, a.clone());
    }

    #[test]
    fn ex_budget_copy() {
        let a = ExBudget::new(1, 2);
        let b = a; // Copy
        assert_eq!(a, b);
    }

    // -- Value ------------------------------------------------------------

    #[test]
    fn value_constant_type_name() {
        let v = Value::Constant(Constant::Integer(1));
        assert_eq!(v.type_name(), "constant");
    }

    #[test]
    fn value_lambda_type_name() {
        let v = Value::Lambda(Term::Var(1), Environment::new());
        assert_eq!(v.type_name(), "lambda");
    }

    #[test]
    fn value_delay_type_name() {
        let v = Value::Delay(Term::Var(1), Environment::new());
        assert_eq!(v.type_name(), "delay");
    }

    #[test]
    fn value_builtin_type_name() {
        let v = Value::BuiltinApp {
            fun: DefaultFun::AddInteger,
            forces: 0,
            args: vec![],
        };
        assert_eq!(v.type_name(), "builtin");
    }

    #[test]
    fn value_constr_type_name() {
        let v = Value::Constr(0, vec![]);
        assert_eq!(v.type_name(), "constr");
    }

    #[test]
    fn value_as_constant_ok() {
        let v = Value::Constant(Constant::Integer(42));
        let c = v.as_constant().unwrap();
        assert_eq!(*c, Constant::Integer(42));
    }

    #[test]
    fn value_as_constant_err_lambda() {
        let v = Value::Lambda(Term::Var(1), Environment::new());
        let err = v.as_constant().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("lambda"));
    }

    #[test]
    fn value_as_constant_err_delay() {
        let v = Value::Delay(Term::Error, Environment::new());
        let err = v.as_constant().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("delay"));
    }

    #[test]
    fn value_as_constant_err_builtin() {
        let v = Value::BuiltinApp {
            fun: DefaultFun::AddInteger,
            forces: 0,
            args: vec![],
        };
        let err = v.as_constant().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("builtin"));
    }

    #[test]
    fn value_as_constant_err_constr() {
        let v = Value::Constr(0, vec![]);
        let err = v.as_constant().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("constr"));
    }

    // -- Environment ------------------------------------------------------

    #[test]
    fn env_new_empty() {
        let env = Environment::new();
        assert!(env.lookup(1).is_err());
    }

    #[test]
    fn env_extend_and_lookup() {
        let env = Environment::new();
        let env = env.extend(Value::Constant(Constant::Integer(10)));
        let val = env.lookup(1).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Integer(10))));
    }

    #[test]
    fn env_debruijn_ordering() {
        // Index 1 = most recent, 2 = next, etc.
        let env = Environment::new();
        let env = env.extend(Value::Constant(Constant::Integer(1)));
        let env = env.extend(Value::Constant(Constant::Integer(2)));
        let env = env.extend(Value::Constant(Constant::Integer(3)));

        // Index 1 = most recent = 3
        if let Value::Constant(Constant::Integer(n)) = env.lookup(1).unwrap() {
            assert_eq!(*n, 3);
        } else {
            panic!("expected integer");
        }

        // Index 2 = 2
        if let Value::Constant(Constant::Integer(n)) = env.lookup(2).unwrap() {
            assert_eq!(*n, 2);
        } else {
            panic!("expected integer");
        }

        // Index 3 = oldest = 1
        if let Value::Constant(Constant::Integer(n)) = env.lookup(3).unwrap() {
            assert_eq!(*n, 1);
        } else {
            panic!("expected integer");
        }
    }

    #[test]
    fn env_lookup_zero_is_error() {
        let env = Environment::new()
            .extend(Value::Constant(Constant::Unit));
        assert!(env.lookup(0).is_err());
    }

    #[test]
    fn env_lookup_out_of_range() {
        let env = Environment::new()
            .extend(Value::Constant(Constant::Unit));
        assert!(env.lookup(2).is_err());
    }

    #[test]
    fn env_unbound_variable_error_message() {
        let env = Environment::new();
        let err = env.lookup(5).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("5"));
    }

    #[test]
    fn env_extend_does_not_mutate_original() {
        let env1 = Environment::new()
            .extend(Value::Constant(Constant::Integer(1)));
        let env2 = env1.extend(Value::Constant(Constant::Integer(2)));

        // env1 should still have only 1 binding.
        assert!(env1.lookup(2).is_err());
        // env2 has 2 bindings.
        assert!(env2.lookup(2).is_ok());
    }

    #[test]
    fn env_default() {
        let env = Environment::default();
        assert!(env.lookup(1).is_err());
    }

    #[test]
    fn env_deep_nesting() {
        let mut env = Environment::new();
        for i in 0..100 {
            env = env.extend(Value::Constant(Constant::Integer(i)));
        }
        // Index 1 = 99, Index 100 = 0.
        if let Value::Constant(Constant::Integer(n)) = env.lookup(1).unwrap() {
            assert_eq!(*n, 99);
        } else {
            panic!("expected integer");
        }
        if let Value::Constant(Constant::Integer(n)) = env.lookup(100).unwrap() {
            assert_eq!(*n, 0);
        } else {
            panic!("expected integer");
        }
    }

    // -- Constant::ProtoList nested -----------------------------------------

    #[test]
    fn constant_proto_list_of_pairs() {
        let c = Constant::ProtoList(
            Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
            vec![
                Constant::ProtoPair(
                    Type::Data,
                    Type::Data,
                    Box::new(Constant::Data(PlutusData::Integer(1))),
                    Box::new(Constant::Data(PlutusData::Integer(2))),
                ),
            ],
        );
        if let Constant::ProtoList(ty, items) = &c {
            assert_eq!(*ty, Type::Pair(Box::new(Type::Data), Box::new(Type::Data)));
            assert_eq!(items.len(), 1);
        } else {
            panic!("expected ProtoList");
        }
    }
}
