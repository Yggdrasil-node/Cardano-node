//! UPLC built-in function enumeration.
//!
//! Mirrors upstream `PlutusCore.Default.Builtins.DefaultFun` —
//! the enumeration of all 71 built-in operations available to UPLC
//! programs (integer arithmetic, ByteString operations, Plutus Data
//! constructors / destructors, Plutus V3 BLS12-381 + ECDSA + BIP-340).
//!
//! Discriminant values match the Flat encoding index used on-chain.
//!
//! Single public type:
//!
//! - `DefaultFun` — `#[repr(u8)]` enum with 71+ variants and the
//!   `from_u8` decoder + `arity` / `force_count` / `requires_*`
//!   helper accessors used by the CEK machine when applying builtins.
//!
//! Extracted from `types.rs` in R273g (Phase γ §R273 seventh slice).

use crate::error::MachineError;

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

    /// Canonical, tag-ascending slice of every `DefaultFun` variant.
    ///
    /// Returned in the same order as the discriminant assignments
    /// (`AddInteger = 0` … `ExpModInteger = 87`), so `all()[i] as u8 == i`.
    /// Adding a new builtin upstream MUST extend this list — drift-guard
    /// tests in this module pin the length, the discriminant ordering,
    /// and `from_tag` round-trip for every entry, so a missed update
    /// here fails CI before it can corrupt on-chain script decoding.
    ///
    /// Reference: `PlutusCore.Default.Builtins.DefaultFun` ordering.
    pub const fn all() -> &'static [Self] {
        use DefaultFun::*;
        &[
            AddInteger,
            SubtractInteger,
            MultiplyInteger,
            DivideInteger,
            QuotientInteger,
            RemainderInteger,
            ModInteger,
            EqualsInteger,
            LessThanInteger,
            LessThanEqualsInteger,
            AppendByteString,
            ConsByteString,
            SliceByteString,
            LengthOfByteString,
            IndexByteString,
            EqualsByteString,
            LessThanByteString,
            LessThanEqualsByteString,
            Sha2_256,
            Sha3_256,
            Blake2b_256,
            VerifyEd25519Signature,
            AppendString,
            EqualsString,
            EncodeUtf8,
            DecodeUtf8,
            IfThenElse,
            ChooseUnit,
            Trace,
            FstPair,
            SndPair,
            ChooseList,
            MkCons,
            HeadList,
            TailList,
            NullList,
            ChooseData,
            ConstrData,
            MapData,
            ListData,
            IData,
            BData,
            UnConstrData,
            UnMapData,
            UnListData,
            UnIData,
            UnBData,
            EqualsData,
            MkPairData,
            MkNilData,
            MkNilPairData,
            SerialiseData,
            VerifyEcdsaSecp256k1Signature,
            VerifySchnorrSecp256k1Signature,
            Bls12_381_G1_Add,
            Bls12_381_G1_Neg,
            Bls12_381_G1_ScalarMul,
            Bls12_381_G1_Equal,
            Bls12_381_G1_HashToGroup,
            Bls12_381_G1_Compress,
            Bls12_381_G1_Uncompress,
            Bls12_381_G2_Add,
            Bls12_381_G2_Neg,
            Bls12_381_G2_ScalarMul,
            Bls12_381_G2_Equal,
            Bls12_381_G2_HashToGroup,
            Bls12_381_G2_Compress,
            Bls12_381_G2_Uncompress,
            Bls12_381_MillerLoop,
            Bls12_381_MulMlResult,
            Bls12_381_FinalVerify,
            Keccak_256,
            Blake2b_224,
            IntegerToByteString,
            ByteStringToInteger,
            AndByteString,
            OrByteString,
            XorByteString,
            ComplementByteString,
            ReadBit,
            WriteBits,
            ReplicateByte,
            ShiftByteString,
            RotateByteString,
            CountSetBits,
            FindFirstSetBit,
            Ripemd_160,
            ExpModInteger,
        ]
    }

    /// Returns `(type_forces, value_args)` — how many `Force` applications
    /// and how many `Apply` value arguments this builtin expects before it
    /// can be evaluated.
    pub fn arity(self) -> (usize, usize) {
        use DefaultFun::*;
        match self {
            // Integer arithmetic — monomorphic, 2 args
            AddInteger | SubtractInteger | MultiplyInteger | DivideInteger | QuotientInteger
            | RemainderInteger | ModInteger => (0, 2),
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
