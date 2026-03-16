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
}

// ---------------------------------------------------------------------------
// DefaultFun — all built-in functions
// ---------------------------------------------------------------------------

/// Enumeration of all UPLC built-in functions across PlutusV1/V2/V3.
///
/// Discriminant values match the Flat encoding index used on-chain.
/// Reference: `PlutusCore.Default.Builtins.DefaultFun`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    // -- Cryptographic hashing
    Sha2_256 = 17,
    Sha3_256 = 18,
    Blake2b_256 = 19,
    VerifyEd25519Signature = 20,
    // -- String
    AppendString = 21,
    EqualsString = 22,
    EncodeUtf8 = 23,
    DecodeUtf8 = 24,
    // -- Boolean / Unit
    IfThenElse = 25,
    ChooseUnit = 26,
    // -- Tracing
    Trace = 27,
    // -- Pair
    FstPair = 28,
    SndPair = 29,
    // -- List
    ChooseList = 30,
    MkCons = 31,
    HeadList = 32,
    TailList = 33,
    NullList = 34,
    // -- Data
    ChooseData = 35,
    ConstrData = 36,
    MapData = 37,
    ListData = 38,
    IData = 39,
    BData = 40,
    UnConstrData = 41,
    UnMapData = 42,
    UnListData = 43,
    UnIData = 44,
    UnBData = 45,
    EqualsData = 46,
    MkPairData = 47,
    MkNilData = 48,
    MkNilPairData = 49,
    SerialiseData = 50,
    // -- PlutusV2 additions
    VerifyEcdsaSecp256k1Signature = 51,
    VerifySchnorrSecp256k1Signature = 52,
    // -- PlutusV3 additions: BLS12-381
    Bls12_381_G1_Add = 53,
    Bls12_381_G1_Neg = 54,
    Bls12_381_G1_ScalarMul = 55,
    Bls12_381_G1_Equal = 56,
    Bls12_381_G1_HashToGroup = 57,
    Bls12_381_G1_Compress = 58,
    Bls12_381_G1_Uncompress = 59,
    Bls12_381_G2_Add = 60,
    Bls12_381_G2_Neg = 61,
    Bls12_381_G2_ScalarMul = 62,
    Bls12_381_G2_Equal = 63,
    Bls12_381_G2_HashToGroup = 64,
    Bls12_381_G2_Compress = 65,
    Bls12_381_G2_Uncompress = 66,
    Bls12_381_MillerLoop = 67,
    Bls12_381_MulMlResult = 68,
    Bls12_381_FinalVerify = 69,
    // -- PlutusV3 additions: extra hashing
    Keccak_256 = 70,
    Blake2b_224 = 71,
    // -- PlutusV3 additions: integer/bytestring conversion
    IntegerToByteString = 72,
    ByteStringToInteger = 73,
    // -- PlutusV3 additions: bitwise operations
    AndByteString = 74,
    OrByteString = 75,
    XorByteString = 76,
    ComplementByteString = 77,
    ReadBit = 78,
    WriteBits = 79,
    ReplicateByte = 80,
    ShiftByteString = 81,
    RotateByteString = 82,
    CountSetBits = 83,
    FindFirstSetBit = 84,
    Ripemd_160 = 85,
    ExpModInteger = 86,
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
            17 => Ok(Self::Sha2_256),
            18 => Ok(Self::Sha3_256),
            19 => Ok(Self::Blake2b_256),
            20 => Ok(Self::VerifyEd25519Signature),
            21 => Ok(Self::AppendString),
            22 => Ok(Self::EqualsString),
            23 => Ok(Self::EncodeUtf8),
            24 => Ok(Self::DecodeUtf8),
            25 => Ok(Self::IfThenElse),
            26 => Ok(Self::ChooseUnit),
            27 => Ok(Self::Trace),
            28 => Ok(Self::FstPair),
            29 => Ok(Self::SndPair),
            30 => Ok(Self::ChooseList),
            31 => Ok(Self::MkCons),
            32 => Ok(Self::HeadList),
            33 => Ok(Self::TailList),
            34 => Ok(Self::NullList),
            35 => Ok(Self::ChooseData),
            36 => Ok(Self::ConstrData),
            37 => Ok(Self::MapData),
            38 => Ok(Self::ListData),
            39 => Ok(Self::IData),
            40 => Ok(Self::BData),
            41 => Ok(Self::UnConstrData),
            42 => Ok(Self::UnMapData),
            43 => Ok(Self::UnListData),
            44 => Ok(Self::UnIData),
            45 => Ok(Self::UnBData),
            46 => Ok(Self::EqualsData),
            47 => Ok(Self::MkPairData),
            48 => Ok(Self::MkNilData),
            49 => Ok(Self::MkNilPairData),
            50 => Ok(Self::SerialiseData),
            51 => Ok(Self::VerifyEcdsaSecp256k1Signature),
            52 => Ok(Self::VerifySchnorrSecp256k1Signature),
            53 => Ok(Self::Bls12_381_G1_Add),
            54 => Ok(Self::Bls12_381_G1_Neg),
            55 => Ok(Self::Bls12_381_G1_ScalarMul),
            56 => Ok(Self::Bls12_381_G1_Equal),
            57 => Ok(Self::Bls12_381_G1_HashToGroup),
            58 => Ok(Self::Bls12_381_G1_Compress),
            59 => Ok(Self::Bls12_381_G1_Uncompress),
            60 => Ok(Self::Bls12_381_G2_Add),
            61 => Ok(Self::Bls12_381_G2_Neg),
            62 => Ok(Self::Bls12_381_G2_ScalarMul),
            63 => Ok(Self::Bls12_381_G2_Equal),
            64 => Ok(Self::Bls12_381_G2_HashToGroup),
            65 => Ok(Self::Bls12_381_G2_Compress),
            66 => Ok(Self::Bls12_381_G2_Uncompress),
            67 => Ok(Self::Bls12_381_MillerLoop),
            68 => Ok(Self::Bls12_381_MulMlResult),
            69 => Ok(Self::Bls12_381_FinalVerify),
            70 => Ok(Self::Keccak_256),
            71 => Ok(Self::Blake2b_224),
            72 => Ok(Self::IntegerToByteString),
            73 => Ok(Self::ByteStringToInteger),
            74 => Ok(Self::AndByteString),
            75 => Ok(Self::OrByteString),
            76 => Ok(Self::XorByteString),
            77 => Ok(Self::ComplementByteString),
            78 => Ok(Self::ReadBit),
            79 => Ok(Self::WriteBits),
            80 => Ok(Self::ReplicateByte),
            81 => Ok(Self::ShiftByteString),
            82 => Ok(Self::RotateByteString),
            83 => Ok(Self::CountSetBits),
            84 => Ok(Self::FindFirstSetBit),
            85 => Ok(Self::Ripemd_160),
            86 => Ok(Self::ExpModInteger),
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
            EqualsByteString | LessThanByteString => (0, 2),

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
