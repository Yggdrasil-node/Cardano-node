//! Built-in function implementations for the UPLC evaluator.
//!
//! All PlutusV1, PlutusV2, and PlutusV3 builtins are implemented,
//! including BLS12-381 curve operations (CIP-0381), bitwise operations,
//! integer/bytestring conversions, and extra hash functions.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror (partial):** mirrors the runtime side of
//! upstream `PlutusCore.Default.Builtins.hs` —
//! `evaluate_builtin` is the per-builtin dispatch matching
//! upstream's `denoteBuiltin`. The cost / argument-shape side
//! lives in sibling `cost_model/*.rs`; the type-side definition
//! (`DefaultFun` enum) is in `types/default_builtins.rs`.
//! Yggdrasil keeps runtime semantics, costing, and type
//! definitions in three files; upstream's `Default.Builtins.hs`
//! carries the type-class machinery and inline denotations for
//! all three concerns.

use num_bigint::{BigInt, Sign};
use num_integer::Integer as NumInteger;
use num_traits::{One, Signed, ToPrimitive, Zero};
use yggdrasil_crypto::blake2b;
use yggdrasil_crypto::bls12_381;
use yggdrasil_crypto::secp256k1;
use yggdrasil_ledger::cbor::{CborEncode, Encoder};
use yggdrasil_ledger::plutus::PlutusData;

use crate::cost_model::{BuiltinSemanticsVariant, CostModel};
use crate::error::MachineError;
use crate::types::{Constant, DefaultFun, Type, Value};

/// Evaluate a saturated built-in function.
///
/// Called by the CEK machine when a `BuiltinApp` has received all its
/// type and value arguments.
#[allow(clippy::too_many_lines)]
pub fn evaluate_builtin(
    fun: DefaultFun,
    args: &[Value],
    _cost_model: &CostModel,
    logs: &mut Vec<String>,
) -> Result<Value, MachineError> {
    use DefaultFun::*;
    match fun {
        // ---------------------------------------------------------------
        // Integer arithmetic
        // ---------------------------------------------------------------
        AddInteger => int_binop(args, |a, b| Ok(a + b)),
        SubtractInteger => int_binop(args, |a, b| Ok(a - b)),
        MultiplyInteger => int_binop(args, |a, b| Ok(a * b)),
        DivideInteger => int_binop(args, |a, b| {
            if b.is_zero() {
                return Err(MachineError::DivisionByZero);
            }
            // Haskell `div`: rounds towards negative infinity (floor division).
            Ok(a.div_floor(&b))
        }),
        QuotientInteger => int_binop(args, |a, b| {
            if b.is_zero() {
                return Err(MachineError::DivisionByZero);
            }
            // Haskell `quot`: rounds towards zero.
            Ok(a / b)
        }),
        RemainderInteger => int_binop(args, |a, b| {
            if b.is_zero() {
                return Err(MachineError::DivisionByZero);
            }
            // Haskell `rem`: sign follows dividend.
            Ok(a % b)
        }),
        ModInteger => int_binop(args, |a, b| {
            if b.is_zero() {
                return Err(MachineError::DivisionByZero);
            }
            // Haskell `mod`: sign follows divisor (floor-division remainder).
            Ok(a.mod_floor(&b))
        }),

        // ---------------------------------------------------------------
        // Integer comparison
        // ---------------------------------------------------------------
        EqualsInteger => {
            let (a, b) = get_two_ints(args)?;
            Ok(Value::Constant(Constant::Bool(a == b)))
        }
        LessThanInteger => {
            let (a, b) = get_two_ints(args)?;
            Ok(Value::Constant(Constant::Bool(a < b)))
        }
        LessThanEqualsInteger => {
            let (a, b) = get_two_ints(args)?;
            Ok(Value::Constant(Constant::Bool(a <= b)))
        }

        // ---------------------------------------------------------------
        // ByteString operations
        // ---------------------------------------------------------------
        AppendByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            let mut result = a.clone();
            result.extend_from_slice(b);
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        ConsByteString => {
            let byte_val = get_int(&args[0])?;
            let bs = get_bytestring(&args[1])?;
            let byte = match _cost_model.builtin_semantics_variant {
                BuiltinSemanticsVariant::A | BuiltinSemanticsVariant::B => byte_val
                    .mod_floor(&BigInt::from(256u16))
                    .to_u8()
                    .ok_or(MachineError::BuiltinError {
                        builtin: "consByteString".into(),
                        message: "wrapped byte did not fit in u8".into(),
                    })?,
                BuiltinSemanticsVariant::C => {
                    if byte_val < BigInt::zero() || byte_val > BigInt::from(255u8) {
                        return Err(MachineError::IndexOutOfBounds {
                            index: bigint_to_i128_for_error(&byte_val),
                            length: 256,
                        });
                    }
                    byte_val.to_u8().expect("checked byte range")
                }
            };
            let mut result = vec![byte];
            result.extend_from_slice(bs);
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        SliceByteString => {
            let start = get_int(&args[0])?;
            let count = get_int(&args[1])?;
            let bs = get_bytestring(&args[2])?;
            // Plutus semantics: i = max(start + 1, 1), j = min(start + count, len)
            // with a 1-based inclusive range; if j < i the result is empty.
            let len = BigInt::from(bs.len());
            let start_idx = start.clone().max(BigInt::zero());
            let end_idx = (start + count).min(len);
            if end_idx <= start_idx {
                Ok(Value::Constant(Constant::ByteString(Vec::new())))
            } else {
                let start_idx = start_idx.to_usize().unwrap_or(bs.len());
                let end_idx = end_idx.to_usize().unwrap_or(bs.len());
                Ok(Value::Constant(Constant::ByteString(
                    bs[start_idx..end_idx].to_vec(),
                )))
            }
        }
        LengthOfByteString => {
            let bs = get_bytestring(&args[0])?;
            Ok(Value::Constant(Constant::integer(BigInt::from(bs.len()))))
        }
        IndexByteString => {
            let bs = get_bytestring(&args[0])?;
            let idx = get_int(&args[1])?;
            if idx < BigInt::zero() || idx >= BigInt::from(bs.len()) {
                return Err(MachineError::IndexOutOfBounds {
                    index: bigint_to_i128_for_error(&idx),
                    length: bs.len(),
                });
            }
            Ok(Value::Constant(Constant::integer(BigInt::from(
                bs[idx.to_usize().expect("checked index range")],
            ))))
        }
        EqualsByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            trace_equals_bytestring(logs, a, b);
            Ok(Value::Constant(Constant::Bool(a == b)))
        }
        LessThanByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            Ok(Value::Constant(Constant::Bool(a < b)))
        }
        LessThanEqualsByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            Ok(Value::Constant(Constant::Bool(a <= b)))
        }

        // ---------------------------------------------------------------
        // Cryptographic hashing
        // ---------------------------------------------------------------
        Sha2_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = sha2_256_hash(bs);
            trace_hash_builtin(logs, "sha2_256", bs, &hash);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }
        Sha3_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = sha3_256_hash(bs);
            trace_hash_builtin(logs, "sha3_256", bs, &hash);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }
        Blake2b_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = blake2b::hash_bytes_256(bs);
            trace_hash_builtin(logs, "blake2b_256", bs, &hash.0);
            Ok(Value::Constant(Constant::ByteString(hash.0.to_vec())))
        }
        VerifyEd25519Signature => {
            let vkey = get_bytestring(&args[0])?;
            let msg = get_bytestring(&args[1])?;
            let sig = get_bytestring(&args[2])?;
            let valid = verify_ed25519(vkey, msg, sig);
            if std::env::var_os("YGGDRASIL_PLUTUS_TRACE_FAILURES").is_some() {
                logs.push(format!(
                    "verifyEd25519Signature key={} msg={} sig={} valid={valid}",
                    hex_bytes(vkey),
                    hex_bytes(msg),
                    hex_bytes(sig),
                ));
                logs.push(format!(
                    "verifyEd25519Signature sizes key_len={} msg_len={} sig_len={}",
                    vkey.len(),
                    msg.len(),
                    sig.len()
                ));
            }
            Ok(Value::Constant(Constant::Bool(valid)))
        }

        // ---------------------------------------------------------------
        // String operations
        // ---------------------------------------------------------------
        AppendString => {
            let (a, b) = get_two_strings(args)?;
            let mut result = a.clone();
            result.push_str(b);
            Ok(Value::Constant(Constant::String(result)))
        }
        EqualsString => {
            let (a, b) = get_two_strings(args)?;
            Ok(Value::Constant(Constant::Bool(a == b)))
        }
        EncodeUtf8 => {
            let s = get_string(&args[0])?;
            Ok(Value::Constant(Constant::ByteString(s.as_bytes().to_vec())))
        }
        DecodeUtf8 => {
            let bs = get_bytestring(&args[0])?;
            let s = std::str::from_utf8(bs).map_err(|_| MachineError::InvalidUtf8)?;
            Ok(Value::Constant(Constant::String(s.to_string())))
        }

        // ---------------------------------------------------------------
        // Bool / Unit
        // ---------------------------------------------------------------
        IfThenElse => {
            // args: [condition, then_val, else_val]
            let cond = get_bool(&args[0])?;
            if cond {
                Ok(args[1].clone())
            } else {
                Ok(args[2].clone())
            }
        }
        ChooseUnit => {
            // args: [unit_val, result]
            // Just returns the second argument (forces the unit check).
            get_unit(&args[0])?;
            Ok(args[1].clone())
        }

        // ---------------------------------------------------------------
        // Trace
        // ---------------------------------------------------------------
        Trace => {
            // args: [message_string, value_to_return]
            let msg = get_string(&args[0])?;
            logs.push(msg.clone());
            Ok(args[1].clone())
        }

        // ---------------------------------------------------------------
        // Pair operations
        // ---------------------------------------------------------------
        FstPair => {
            let (a, _) = get_pair(&args[0])?;
            Ok(Value::Constant(a.clone()))
        }
        SndPair => {
            let (_, b) = get_pair(&args[0])?;
            Ok(Value::Constant(b.clone()))
        }

        // ---------------------------------------------------------------
        // List operations
        // ---------------------------------------------------------------
        ChooseList => {
            // args: [list, if_nil, if_cons]
            let list = get_list(&args[0])?;
            if list.is_empty() {
                Ok(args[1].clone())
            } else {
                Ok(args[2].clone())
            }
        }
        MkCons => {
            // args: [element, list]
            let elem = args[0].as_constant()?.clone();
            let list = get_list_with_type(&args[1])?;
            let mut new_list = vec![elem];
            new_list.extend(list.1.iter().cloned());
            Ok(Value::Constant(Constant::ProtoList(
                list.0.clone(),
                new_list,
            )))
        }
        HeadList => {
            let list = get_list(&args[0])?;
            if list.is_empty() {
                return Err(MachineError::EmptyList);
            }
            Ok(Value::Constant(list[0].clone()))
        }
        TailList => {
            let list_with_type = get_list_with_type(&args[0])?;
            if list_with_type.1.is_empty() {
                return Err(MachineError::EmptyList);
            }
            Ok(Value::Constant(Constant::ProtoList(
                list_with_type.0.clone(),
                list_with_type.1[1..].to_vec(),
            )))
        }
        NullList => {
            let list = get_list(&args[0])?;
            Ok(Value::Constant(Constant::Bool(list.is_empty())))
        }

        // ---------------------------------------------------------------
        // Data operations
        // ---------------------------------------------------------------
        ChooseData => {
            // args: [data, constr_k, map_k, list_k, int_k, bs_k]
            let data = get_data(&args[0])?;
            let idx = match data {
                PlutusData::Constr(..) => 1,
                PlutusData::Map(..) => 2,
                PlutusData::List(..) => 3,
                PlutusData::Integer(..) => 4,
                PlutusData::Bytes(..) => 5,
            };
            Ok(args[idx].clone())
        }
        ConstrData => {
            let tag = get_int(&args[0])?;
            let list = get_data_list(&args[1])?;
            let tag = tag.to_u64().ok_or_else(|| MachineError::BuiltinError {
                builtin: "constrData".into(),
                message: format!("constructor tag out of range: {tag}"),
            })?;
            Ok(Value::Constant(Constant::Data(PlutusData::Constr(
                tag, list,
            ))))
        }
        MapData => {
            let list = get_data_pair_list(&args[0])?;
            Ok(Value::Constant(Constant::Data(PlutusData::Map(list))))
        }
        ListData => {
            let list = get_data_list(&args[0])?;
            Ok(Value::Constant(Constant::Data(PlutusData::List(list))))
        }
        IData => {
            let i = get_int(&args[0])?;
            Ok(Value::Constant(Constant::Data(PlutusData::integer(i))))
        }
        BData => {
            let bs = get_bytestring(&args[0])?;
            Ok(Value::Constant(Constant::Data(PlutusData::Bytes(
                bs.clone(),
            ))))
        }
        UnConstrData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::Constr(tag, fields) => {
                    let tag_const = Constant::integer(BigInt::from(*tag));
                    let fields_const = Constant::ProtoList(
                        Type::Data,
                        fields.iter().map(|d| Constant::Data(d.clone())).collect(),
                    );
                    Ok(Value::Constant(Constant::ProtoPair(
                        Type::Integer,
                        Type::List(Box::new(Type::Data)),
                        Box::new(tag_const),
                        Box::new(fields_const),
                    )))
                }
                _ => Err(MachineError::TypeMismatch {
                    expected: "Constr data",
                    actual: data_variant_name(data),
                }),
            }
        }
        UnMapData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::Map(entries) => {
                    let pairs: Vec<Constant> = entries
                        .iter()
                        .map(|(k, v)| {
                            Constant::ProtoPair(
                                Type::Data,
                                Type::Data,
                                Box::new(Constant::Data(k.clone())),
                                Box::new(Constant::Data(v.clone())),
                            )
                        })
                        .collect();
                    Ok(Value::Constant(Constant::ProtoList(
                        Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
                        pairs,
                    )))
                }
                _ => Err(MachineError::TypeMismatch {
                    expected: "Map data",
                    actual: data_variant_name(data),
                }),
            }
        }
        UnListData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::List(items) => {
                    let elems: Vec<Constant> =
                        items.iter().map(|d| Constant::Data(d.clone())).collect();
                    Ok(Value::Constant(Constant::ProtoList(Type::Data, elems)))
                }
                _ => Err(MachineError::TypeMismatch {
                    expected: "List data",
                    actual: data_variant_name(data),
                }),
            }
        }
        UnIData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::Integer(i) => Ok(Value::Constant(Constant::integer(i.clone()))),
                _ => Err(MachineError::TypeMismatch {
                    expected: "Integer data",
                    actual: data_variant_name(data),
                }),
            }
        }
        UnBData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::Bytes(bs) => Ok(Value::Constant(Constant::ByteString(bs.clone()))),
                _ => Err(MachineError::TypeMismatch {
                    expected: "Bytes data",
                    actual: data_variant_name(data),
                }),
            }
        }
        EqualsData => {
            let a = get_data(&args[0])?;
            let b = get_data(&args[1])?;
            Ok(Value::Constant(Constant::Bool(a == b)))
        }
        MkPairData => {
            let a = get_data(&args[0])?;
            let b = get_data(&args[1])?;
            Ok(Value::Constant(Constant::ProtoPair(
                Type::Data,
                Type::Data,
                Box::new(Constant::Data(a.clone())),
                Box::new(Constant::Data(b.clone())),
            )))
        }
        MkNilData => {
            // Takes a Unit argument, returns empty list of Data.
            get_unit(&args[0])?;
            Ok(Value::Constant(Constant::ProtoList(Type::Data, Vec::new())))
        }
        MkNilPairData => {
            get_unit(&args[0])?;
            Ok(Value::Constant(Constant::ProtoList(
                Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
                Vec::new(),
            )))
        }
        SerialiseData => {
            let data = get_data(&args[0])?;
            let mut enc = Encoder::new();
            data.encode_cbor(&mut enc);
            let bytes = enc.into_bytes();
            trace_serialise_data(logs, data, &bytes);
            Ok(Value::Constant(Constant::ByteString(bytes)))
        }

        // ---------------------------------------------------------------
        // PlutusV2 — secp256k1
        // ---------------------------------------------------------------
        VerifyEcdsaSecp256k1Signature => {
            let vk = get_bytestring(&args[0])?;
            let msg = get_bytestring(&args[1])?;
            let sig = get_bytestring(&args[2])?;
            let valid = verify_ecdsa_secp256k1(vk, msg, sig);
            Ok(Value::Constant(Constant::Bool(valid)))
        }
        VerifySchnorrSecp256k1Signature => {
            let vk = get_bytestring(&args[0])?;
            let msg = get_bytestring(&args[1])?;
            let sig = get_bytestring(&args[2])?;
            let valid = verify_schnorr_secp256k1(vk, msg, sig);
            Ok(Value::Constant(Constant::Bool(valid)))
        }

        // ---------------------------------------------------------------
        // PlutusV3 — BLS12-381
        // ---------------------------------------------------------------
        Bls12_381_G1_Add => {
            let a = get_g1(&args[0])?;
            let b = get_g1(&args[1])?;
            Ok(Value::Constant(Constant::Bls12_381_G1_Element(
                bls12_381::g1_add(a, b),
            )))
        }
        Bls12_381_G1_Neg => {
            let a = get_g1(&args[0])?;
            Ok(Value::Constant(Constant::Bls12_381_G1_Element(
                bls12_381::g1_neg(a),
            )))
        }
        Bls12_381_G1_ScalarMul => {
            let scalar = get_int(&args[0])?;
            let point = get_g1(&args[1])?;
            let (magnitude, negative) = int_to_scalar_bytes(scalar);
            Ok(Value::Constant(Constant::Bls12_381_G1_Element(
                bls12_381::g1_scalar_mul(&magnitude, negative, point),
            )))
        }
        Bls12_381_G1_Equal => {
            let a = get_g1(&args[0])?;
            let b = get_g1(&args[1])?;
            Ok(Value::Constant(Constant::Bool(bls12_381::g1_equal(a, b))))
        }
        Bls12_381_G1_HashToGroup => {
            let msg = get_bytestring(&args[0])?;
            let dst = get_bytestring(&args[1])?;
            let point = bls12_381::g1_hash_to_group(msg, dst)
                .map_err(|e| MachineError::CryptoError(format!("{e}")))?;
            Ok(Value::Constant(Constant::Bls12_381_G1_Element(point)))
        }
        Bls12_381_G1_Compress => {
            let point = get_g1(&args[0])?;
            Ok(Value::Constant(Constant::ByteString(
                bls12_381::g1_compress(point).to_vec(),
            )))
        }
        Bls12_381_G1_Uncompress => {
            let bs = get_bytestring(&args[0])?;
            let point = bls12_381::g1_uncompress(bs)
                .map_err(|e| MachineError::CryptoError(format!("{e}")))?;
            Ok(Value::Constant(Constant::Bls12_381_G1_Element(point)))
        }
        Bls12_381_G2_Add => {
            let a = get_g2(&args[0])?;
            let b = get_g2(&args[1])?;
            Ok(Value::Constant(Constant::Bls12_381_G2_Element(
                bls12_381::g2_add(a, b),
            )))
        }
        Bls12_381_G2_Neg => {
            let a = get_g2(&args[0])?;
            Ok(Value::Constant(Constant::Bls12_381_G2_Element(
                bls12_381::g2_neg(a),
            )))
        }
        Bls12_381_G2_ScalarMul => {
            let scalar = get_int(&args[0])?;
            let point = get_g2(&args[1])?;
            let (magnitude, negative) = int_to_scalar_bytes(scalar);
            Ok(Value::Constant(Constant::Bls12_381_G2_Element(
                bls12_381::g2_scalar_mul(&magnitude, negative, point),
            )))
        }
        Bls12_381_G2_Equal => {
            let a = get_g2(&args[0])?;
            let b = get_g2(&args[1])?;
            Ok(Value::Constant(Constant::Bool(bls12_381::g2_equal(a, b))))
        }
        Bls12_381_G2_HashToGroup => {
            let msg = get_bytestring(&args[0])?;
            let dst = get_bytestring(&args[1])?;
            let point = bls12_381::g2_hash_to_group(msg, dst)
                .map_err(|e| MachineError::CryptoError(format!("{e}")))?;
            Ok(Value::Constant(Constant::Bls12_381_G2_Element(point)))
        }
        Bls12_381_G2_Compress => {
            let point = get_g2(&args[0])?;
            Ok(Value::Constant(Constant::ByteString(
                bls12_381::g2_compress(point).to_vec(),
            )))
        }
        Bls12_381_G2_Uncompress => {
            let bs = get_bytestring(&args[0])?;
            let point = bls12_381::g2_uncompress(bs)
                .map_err(|e| MachineError::CryptoError(format!("{e}")))?;
            Ok(Value::Constant(Constant::Bls12_381_G2_Element(point)))
        }
        Bls12_381_MillerLoop => {
            let g1 = get_g1(&args[0])?;
            let g2 = get_g2(&args[1])?;
            Ok(Value::Constant(Constant::Bls12_381_MlResult(Box::new(
                bls12_381::miller_loop(g1, g2),
            ))))
        }
        Bls12_381_MulMlResult => {
            let a = get_ml(&args[0])?;
            let b = get_ml(&args[1])?;
            Ok(Value::Constant(Constant::Bls12_381_MlResult(Box::new(
                bls12_381::mul_ml_result(a, b),
            ))))
        }
        Bls12_381_FinalVerify => {
            let a = get_ml(&args[0])?;
            let b = get_ml(&args[1])?;
            Ok(Value::Constant(Constant::Bool(bls12_381::final_verify(
                a, b,
            ))))
        }

        Keccak_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = keccak_256_hash(bs);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }
        Ripemd_160 => {
            let bs = get_bytestring(&args[0])?;
            let hash = ripemd_160_hash(bs);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }

        Blake2b_224 => {
            let bs = get_bytestring(&args[0])?;
            let hash = blake2b::hash_bytes_224(bs);
            Ok(Value::Constant(Constant::ByteString(hash.0.to_vec())))
        }

        IntegerToByteString => {
            // args: [endianness_flag, length, integer]
            // endianness: 0 = big-endian, 1 = little-endian
            let endianness = get_bool(&args[0])?;
            let required_len = get_int(&args[1])?;
            let value = get_int(&args[2])?;
            let bs = integer_to_bytestring(endianness, required_len, value)?;
            Ok(Value::Constant(Constant::ByteString(bs)))
        }
        ByteStringToInteger => {
            // args: [endianness_flag, bytestring]
            // endianness: 0 = big-endian, 1 = little-endian
            let endianness = get_bool(&args[0])?;
            let bs = get_bytestring(&args[1])?;
            let value = bytestring_to_integer(endianness, bs);
            Ok(Value::Constant(Constant::integer(value)))
        }

        AndByteString => {
            // args: [padding_semantics, bs1, bs2]
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(
                a,
                b,
                pad,
                |x, y| x & y,
            ))))
        }
        OrByteString => {
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(
                a,
                b,
                pad,
                |x, y| x | y,
            ))))
        }
        XorByteString => {
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(
                a,
                b,
                pad,
                |x, y| x ^ y,
            ))))
        }
        ComplementByteString => {
            let bs = get_bytestring(&args[0])?;
            let result: Vec<u8> = bs.iter().map(|b| !b).collect();
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        ReadBit => {
            // args: [bytestring, bit_index]
            let bs = get_bytestring(&args[0])?;
            let idx = get_int(&args[1])?;
            let bit = read_bit(bs, idx)?;
            Ok(Value::Constant(Constant::Bool(bit)))
        }
        WriteBits => {
            // args: [bytestring, list_of_index_value_pairs]
            // Upstream: writeBits bs indices values
            // Simplified: args = [bs, indices_list, values_list]
            let bs = get_bytestring(&args[0])?;
            let indices = get_int_list(&args[1])?;
            let values = get_bool_list(&args[2])?;
            let result = write_bits(bs, &indices, &values)?;
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        ReplicateByte => {
            // args: [length, byte_value]
            let len = get_int(&args[0])?;
            let byte_val = get_int(&args[1])?;
            if len < BigInt::zero() || len > BigInt::from(8192u16) {
                return Err(MachineError::BuiltinError {
                    builtin: "replicateByte".into(),
                    message: format!("length out of range: {len}"),
                });
            }
            if byte_val < BigInt::zero() || byte_val > BigInt::from(255u8) {
                return Err(MachineError::BuiltinError {
                    builtin: "replicateByte".into(),
                    message: format!("byte value out of range: {byte_val}"),
                });
            }
            let byte = byte_val.to_u8().expect("checked byte range");
            let len = len.to_usize().expect("checked replicate length");
            Ok(Value::Constant(Constant::ByteString(vec![byte; len])))
        }
        ShiftByteString => {
            // args: [bytestring, shift_amount]
            // Positive = shift left, negative = shift right. Vacated bits are 0.
            let bs = get_bytestring(&args[0])?;
            let shift = get_int(&args[1])?;
            let result = shift_bytestring(bs, shift);
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        RotateByteString => {
            // args: [bytestring, rotation_amount]
            let bs = get_bytestring(&args[0])?;
            let rot = get_int(&args[1])?;
            let result = rotate_bytestring(bs, rot);
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        CountSetBits => {
            let bs = get_bytestring(&args[0])?;
            let count: usize = bs.iter().map(|b| b.count_ones() as usize).sum();
            Ok(Value::Constant(Constant::integer(BigInt::from(count))))
        }
        FindFirstSetBit => {
            let bs = get_bytestring(&args[0])?;
            let idx = find_first_set_bit(bs);
            Ok(Value::Constant(Constant::integer(idx)))
        }

        ExpModInteger => {
            // args: [base, exponent, modulus]
            let base = get_int(&args[0])?;
            let exp = get_int(&args[1])?;
            let modulus = get_int(&args[2])?;
            if modulus.is_zero() {
                return Err(MachineError::DivisionByZero);
            }
            let result = exp_mod_integer(base, exp, modulus)?;
            Ok(Value::Constant(Constant::integer(result)))
        }
    }
}

// ---------------------------------------------------------------------------
// Argument extraction helpers
// ---------------------------------------------------------------------------

fn get_int(val: &Value) -> Result<BigInt, MachineError> {
    match val.as_constant()? {
        Constant::Integer(i) => Ok(i.clone()),
        other => Err(MachineError::TypeMismatch {
            expected: "integer",
            actual: constant_type_name(other),
        }),
    }
}

fn get_bool(val: &Value) -> Result<bool, MachineError> {
    match val.as_constant()? {
        Constant::Bool(b) => Ok(*b),
        other => Err(MachineError::TypeMismatch {
            expected: "bool",
            actual: constant_type_name(other),
        }),
    }
}

fn get_unit(val: &Value) -> Result<(), MachineError> {
    match val.as_constant()? {
        Constant::Unit => Ok(()),
        other => Err(MachineError::TypeMismatch {
            expected: "unit",
            actual: constant_type_name(other),
        }),
    }
}

fn get_bytestring(val: &Value) -> Result<&Vec<u8>, MachineError> {
    match val.as_constant()? {
        Constant::ByteString(bs) => Ok(bs),
        other => Err(MachineError::TypeMismatch {
            expected: "bytestring",
            actual: constant_type_name(other),
        }),
    }
}

fn get_string(val: &Value) -> Result<&String, MachineError> {
    match val.as_constant()? {
        Constant::String(s) => Ok(s),
        other => Err(MachineError::TypeMismatch {
            expected: "string",
            actual: constant_type_name(other),
        }),
    }
}

fn get_data(val: &Value) -> Result<&PlutusData, MachineError> {
    match val.as_constant()? {
        Constant::Data(d) => Ok(d),
        other => Err(MachineError::TypeMismatch {
            expected: "data",
            actual: constant_type_name(other),
        }),
    }
}

fn get_list(val: &Value) -> Result<&[Constant], MachineError> {
    match val.as_constant()? {
        Constant::ProtoList(_, items) => Ok(items),
        other => Err(MachineError::TypeMismatch {
            expected: "list",
            actual: constant_type_name(other),
        }),
    }
}

fn get_list_with_type(val: &Value) -> Result<(&Type, &[Constant]), MachineError> {
    match val.as_constant()? {
        Constant::ProtoList(ty, items) => Ok((ty, items)),
        other => Err(MachineError::TypeMismatch {
            expected: "list",
            actual: constant_type_name(other),
        }),
    }
}

fn get_pair(val: &Value) -> Result<(&Constant, &Constant), MachineError> {
    match val.as_constant()? {
        Constant::ProtoPair(_, _, a, b) => Ok((a, b)),
        other => Err(MachineError::TypeMismatch {
            expected: "pair",
            actual: constant_type_name(other),
        }),
    }
}

fn get_two_ints(args: &[Value]) -> Result<(BigInt, BigInt), MachineError> {
    Ok((get_int(&args[0])?, get_int(&args[1])?))
}

fn get_two_bytestrings(args: &[Value]) -> Result<(&Vec<u8>, &Vec<u8>), MachineError> {
    Ok((get_bytestring(&args[0])?, get_bytestring(&args[1])?))
}

fn get_two_strings(args: &[Value]) -> Result<(&String, &String), MachineError> {
    Ok((get_string(&args[0])?, get_string(&args[1])?))
}

/// Integer binary operator helper.
fn int_binop(
    args: &[Value],
    op: impl FnOnce(BigInt, BigInt) -> Result<BigInt, MachineError>,
) -> Result<Value, MachineError> {
    let (a, b) = get_two_ints(args)?;
    Ok(Value::Constant(Constant::integer(op(a, b)?)))
}

/// Extract a `Vec<PlutusData>` from a list-of-data constant.
fn get_data_list(val: &Value) -> Result<Vec<PlutusData>, MachineError> {
    let list = get_list(val)?;
    list.iter()
        .map(|c| match c {
            Constant::Data(d) => Ok(d.clone()),
            other => Err(MachineError::TypeMismatch {
                expected: "data element",
                actual: constant_type_name(other),
            }),
        })
        .collect()
}

/// Extract a `Vec<(PlutusData, PlutusData)>` from a list-of-pairs constant.
fn get_data_pair_list(val: &Value) -> Result<Vec<(PlutusData, PlutusData)>, MachineError> {
    let list = get_list(val)?;
    list.iter()
        .map(|c| match c {
            Constant::ProtoPair(_, _, a, b) => {
                let ka = match a.as_ref() {
                    Constant::Data(d) => d.clone(),
                    other => {
                        return Err(MachineError::TypeMismatch {
                            expected: "data",
                            actual: constant_type_name(other),
                        });
                    }
                };
                let vb = match b.as_ref() {
                    Constant::Data(d) => d.clone(),
                    other => {
                        return Err(MachineError::TypeMismatch {
                            expected: "data",
                            actual: constant_type_name(other),
                        });
                    }
                };
                Ok((ka, vb))
            }
            other => Err(MachineError::TypeMismatch {
                expected: "pair",
                actual: constant_type_name(other),
            }),
        })
        .collect()
}

fn constant_type_name(c: &Constant) -> String {
    match c {
        Constant::Integer(_) => "integer".into(),
        Constant::ByteString(_) => "bytestring".into(),
        Constant::String(_) => "string".into(),
        Constant::Unit => "unit".into(),
        Constant::Bool(_) => "bool".into(),
        Constant::ProtoList(..) => "list".into(),
        Constant::ProtoPair(..) => "pair".into(),
        Constant::Data(_) => "data".into(),
        Constant::Bls12_381_G1_Element(_) => "bls12_381_G1_element".into(),
        Constant::Bls12_381_G2_Element(_) => "bls12_381_G2_element".into(),
        Constant::Bls12_381_MlResult(_) => "bls12_381_MlResult".into(),
    }
}

fn data_variant_name(d: &PlutusData) -> String {
    match d {
        PlutusData::Constr(..) => "Constr".into(),
        PlutusData::Map(..) => "Map".into(),
        PlutusData::List(..) => "List".into(),
        PlutusData::Integer(..) => "Integer".into(),
        PlutusData::Bytes(..) => "Bytes".into(),
    }
}

// ---------------------------------------------------------------------------
// BLS12-381 helpers
// ---------------------------------------------------------------------------

fn get_g1(val: &Value) -> Result<&yggdrasil_crypto::G1Element, MachineError> {
    match val.as_constant()? {
        Constant::Bls12_381_G1_Element(e) => Ok(e),
        other => Err(MachineError::TypeMismatch {
            expected: "bls12_381_G1_element",
            actual: constant_type_name(other),
        }),
    }
}

fn get_g2(val: &Value) -> Result<&yggdrasil_crypto::G2Element, MachineError> {
    match val.as_constant()? {
        Constant::Bls12_381_G2_Element(e) => Ok(e),
        other => Err(MachineError::TypeMismatch {
            expected: "bls12_381_G2_element",
            actual: constant_type_name(other),
        }),
    }
}

fn get_ml(val: &Value) -> Result<&yggdrasil_crypto::MlResult, MachineError> {
    match val.as_constant()? {
        Constant::Bls12_381_MlResult(r) => Ok(r.as_ref()),
        other => Err(MachineError::TypeMismatch {
            expected: "bls12_381_MlResult",
            actual: constant_type_name(other),
        }),
    }
}

/// Converts a Plutus integer to (magnitude_bytes, negative) for BLS scalar mul.
fn int_to_scalar_bytes<N: Into<BigInt>>(val: N) -> (Vec<u8>, bool) {
    let val = val.into();
    let negative = val.sign() == Sign::Minus;
    let abs = val.abs();
    if abs.is_zero() {
        return (vec![0], false);
    }
    let (_, be_bytes) = abs.to_bytes_be();
    (be_bytes, negative)
}

// ---------------------------------------------------------------------------
// Cryptographic helpers
// ---------------------------------------------------------------------------

/// SHA2-256 hash using the sha2 crate (already a workspace dependency).
fn sha2_256_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Ed25519 signature verification using our crypto crate.
fn verify_ed25519(vkey: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    if vkey.len() != 32 || sig.len() != 64 {
        return false;
    }
    // Safety: length is checked above, try_into cannot fail.
    let vk = yggdrasil_crypto::ed25519::VerificationKey::from_bytes(
        vkey.try_into().expect("checked 32 bytes"),
    );
    let sig =
        yggdrasil_crypto::ed25519::Signature::from_bytes(sig.try_into().expect("checked 64 bytes"));
    vk.verify(msg, &sig).is_ok()
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn trace_hash_builtin(logs: &mut Vec<String>, name: &str, input: &[u8], output: &[u8; 32]) {
    if std::env::var_os("YGGDRASIL_PLUTUS_TRACE_FAILURES").is_none() {
        return;
    }
    logs.push(format!(
        "{name} input_len={} input_blake2b256={} output={}",
        input.len(),
        hex_bytes(&blake2b::hash_bytes_256(input).0),
        hex_bytes(output),
    ));
}

fn trace_serialise_data(logs: &mut Vec<String>, data: &PlutusData, bytes: &[u8]) {
    if std::env::var_os("YGGDRASIL_PLUTUS_TRACE_FAILURES").is_none() {
        return;
    }
    let data_preview = data_preview(data);
    logs.push(format!(
        "serialiseData variant={} cbor_len={} cbor_blake2b256={} cbor_sha2_256={} preview={}",
        data_variant_name(data),
        bytes.len(),
        hex_bytes(&blake2b::hash_bytes_256(bytes).0),
        hex_bytes(&sha2_256_hash(bytes)),
        data_preview,
    ));
    if bytes.len() <= 160 {
        logs.push(format!("serialiseData cbor={}", hex_bytes(bytes)));
    }
}

fn trace_equals_bytestring(logs: &mut Vec<String>, a: &[u8], b: &[u8]) {
    if std::env::var_os("YGGDRASIL_PLUTUS_TRACE_FAILURES").is_none() {
        return;
    }
    let interesting = [28, 32, 64].contains(&a.len()) || [28, 32, 64].contains(&b.len());
    if interesting {
        logs.push(format!(
            "equalsByteString a_len={} b_len={} a={} b={} result={}",
            a.len(),
            b.len(),
            hex_bytes(a),
            hex_bytes(b),
            a == b,
        ));
    }
}

fn data_preview(data: &PlutusData) -> String {
    match data {
        PlutusData::Constr(tag, fields) => format!("Constr({tag}, fields={})", fields.len()),
        PlutusData::Map(entries) => format!("Map(len={})", entries.len()),
        PlutusData::List(items) => format!("List(len={})", items.len()),
        PlutusData::Integer(value) => format!("Integer({value})"),
        PlutusData::Bytes(bytes) => format!("Bytes(len={}, hex={})", bytes.len(), hex_bytes(bytes)),
    }
}

/// secp256k1 ECDSA signature verification.
///
/// Returns `false` for malformed keys/signatures rather than erroring,
/// matching Plutus semantics where cryptographic failures yield `False`.
fn verify_ecdsa_secp256k1(vk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    secp256k1::verify_ecdsa(vk, msg, sig).unwrap_or(false)
}

/// secp256k1 Schnorr (BIP-340) signature verification.
fn verify_schnorr_secp256k1(vk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    secp256k1::verify_schnorr(vk, msg, sig).unwrap_or(false)
}

/// SHA3-256 hash.
fn sha3_256_hash(data: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Keccak-256 hash (used by Ethereum).
fn keccak_256_hash(data: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// RIPEMD-160 hash.
fn ripemd_160_hash(data: &[u8]) -> [u8; 20] {
    use ripemd::{Digest, Ripemd160};
    let mut hasher = Ripemd160::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&result);
    out
}

// ---------------------------------------------------------------------------
// Integer ↔ ByteString conversion
// ---------------------------------------------------------------------------

/// Convert an integer to a byte string.
///
/// `endianness`: `true` = little-endian, `false` = big-endian.
/// `required_len`: 0 = use minimum bytes needed, >0 = pad/truncate to this length.
///
/// Reference: CIP-0087 / Plutus `integerToByteString`.
fn integer_to_bytestring(
    little_endian: bool,
    required_len: impl Into<BigInt>,
    value: impl Into<BigInt>,
) -> Result<Vec<u8>, MachineError> {
    let required_len = required_len.into();
    let value = value.into();
    if value.sign() == Sign::Minus {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: "negative integer".into(),
        });
    }
    if required_len.sign() == Sign::Minus {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: format!("negative required length: {required_len}"),
        });
    }
    if required_len > BigInt::from(8192u16) {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: format!("required length too large: {required_len}"),
        });
    }
    let req = required_len.to_usize().expect("checked required length");

    if value.is_zero() {
        return if req == 0 {
            Ok(vec![])
        } else {
            Ok(vec![0u8; req])
        };
    }

    let (_, big_endian) = value.to_bytes_be();

    let mut result = if req > 0 {
        if big_endian.len() > req {
            return Err(MachineError::BuiltinError {
                builtin: "integerToByteString".into(),
                message: format!(
                    "integer requires {} bytes but only {} allowed",
                    big_endian.len(),
                    req
                ),
            });
        }
        let mut padded = vec![0u8; req - big_endian.len()];
        padded.extend_from_slice(&big_endian);
        padded
    } else {
        big_endian
    };

    if little_endian {
        result.reverse();
    }

    Ok(result)
}

/// Convert a byte string to a non-negative integer.
///
/// `endianness`: `true` = little-endian, `false` = big-endian.
///
/// Reference: CIP-0087 / Plutus `byteStringToInteger`.
fn bytestring_to_integer(little_endian: bool, bs: &[u8]) -> BigInt {
    if bs.is_empty() {
        return BigInt::zero();
    }
    if little_endian {
        let mut bytes = bs.to_vec();
        bytes.reverse();
        BigInt::from_bytes_be(Sign::Plus, &bytes)
    } else {
        BigInt::from_bytes_be(Sign::Plus, bs)
    }
}

// ---------------------------------------------------------------------------
// Bitwise operations
// ---------------------------------------------------------------------------

/// Binary bitwise operation with padding semantics.
///
/// `pad == false` (truncate / AND-like): result length = min of inputs.
/// `pad == true` (extend / OR-like): result length = max of inputs.
///
/// Reference: CIP-0122 / Plutus bitwise operations, `semanticsAndByteString` etc.
fn bitwise_binop(a: &[u8], b: &[u8], pad: bool, op: fn(u8, u8) -> u8) -> Vec<u8> {
    let (shorter, longer) = if a.len() <= b.len() { (a, b) } else { (b, a) };

    if !pad {
        // Truncation semantics: result has length of the shorter input.
        // Align from the right (least-significant end).
        let offset = longer.len() - shorter.len();
        longer[offset..]
            .iter()
            .zip(shorter.iter())
            .map(|(&x, &y)| op(x, y))
            .collect()
    } else {
        // Padding semantics: result has length of the longer input.
        // Pad the shorter input with 0x00 on the left.
        let offset = longer.len() - shorter.len();
        let mut result = Vec::with_capacity(longer.len());
        for &byte in &longer[..offset] {
            result.push(op(byte, 0x00));
        }
        for (&x, &y) in longer[offset..].iter().zip(shorter.iter()) {
            result.push(op(x, y));
        }
        result
    }
}

/// Read a single bit from a byte string.
///
/// Bit indexing: bit 0 is the LSB of the last byte.
fn read_bit(bs: &[u8], bit_index: impl Into<BigInt>) -> Result<bool, MachineError> {
    let bit_index = bit_index.into();
    let total_bits = BigInt::from(bs.len() * 8);
    if bit_index < BigInt::zero() || bit_index >= total_bits {
        return Err(MachineError::IndexOutOfBounds {
            index: bigint_to_i128_for_error(&bit_index),
            length: bs.len() * 8,
        });
    }
    let bit_index = bit_index.to_usize().expect("checked bit index range");
    let byte_idx = bs.len() - 1 - (bit_index / 8);
    let bit_offset = (bit_index % 8) as u32;
    Ok((bs[byte_idx] >> bit_offset) & 1 == 1)
}

/// Write bits at specified indices.
///
/// Bit indexing: bit 0 is the LSB of the last byte.
fn write_bits(bs: &[u8], indices: &[BigInt], values: &[bool]) -> Result<Vec<u8>, MachineError> {
    if indices.len() != values.len() {
        return Err(MachineError::BuiltinError {
            builtin: "writeBits".into(),
            message: format!(
                "indices length ({}) != values length ({})",
                indices.len(),
                values.len()
            ),
        });
    }
    let total_bits = BigInt::from(bs.len() * 8);
    let mut result = bs.to_vec();
    for (idx, &val) in indices.iter().zip(values.iter()) {
        if idx < &BigInt::zero() || idx >= &total_bits {
            return Err(MachineError::IndexOutOfBounds {
                index: bigint_to_i128_for_error(idx),
                length: bs.len() * 8,
            });
        }
        let idx = idx.to_usize().expect("checked bit index range");
        let byte_idx = result.len() - 1 - (idx / 8);
        let bit_offset = (idx % 8) as u32;
        if val {
            result[byte_idx] |= 1 << bit_offset;
        } else {
            result[byte_idx] &= !(1 << bit_offset);
        }
    }
    Ok(result)
}

/// Shift a byte string left (positive) or right (negative).
///
/// Vacated bits are filled with zeros.
fn shift_bytestring(bs: &[u8], shift: impl Into<BigInt>) -> Vec<u8> {
    let shift = shift.into();
    if bs.is_empty() {
        return Vec::new();
    }
    let total_bits = bs.len() * 8;
    let abs_shift_big = shift.abs();

    if abs_shift_big >= BigInt::from(total_bits) {
        return vec![0u8; bs.len()];
    }
    let abs_shift = abs_shift_big.to_usize().expect("checked shift range");

    let byte_shift = abs_shift / 8;
    let bit_shift = abs_shift % 8;

    let mut result = vec![0u8; bs.len()];

    match shift.sign() {
        Sign::Plus => {
            // Shift left: MSB direction.
            for i in 0..bs.len() {
                if i + byte_shift < bs.len() {
                    result[i] = bs[i + byte_shift] << bit_shift;
                    if bit_shift > 0 && i + byte_shift + 1 < bs.len() {
                        result[i] |= bs[i + byte_shift + 1] >> (8 - bit_shift);
                    }
                }
            }
        }
        Sign::Minus => {
            // Shift right: LSB direction.
            for i in (0..bs.len()).rev() {
                if i >= byte_shift {
                    result[i] = bs[i - byte_shift] >> bit_shift;
                    if bit_shift > 0 && i > byte_shift {
                        result[i] |= bs[i - byte_shift - 1] << (8 - bit_shift);
                    }
                }
            }
        }
        Sign::NoSign => {
            result.copy_from_slice(bs);
        }
    }

    result
}

/// Rotate a byte string left (positive) or right (negative).
fn rotate_bytestring(bs: &[u8], rot: impl Into<BigInt>) -> Vec<u8> {
    let rot = rot.into();
    if bs.is_empty() {
        return Vec::new();
    }
    let total_bits = BigInt::from(bs.len() * 8);
    // Normalize rotation to [0, total_bits).
    let effective = rot.mod_floor(&total_bits);
    if effective.is_zero() {
        return bs.to_vec();
    }

    // Use shift-based rotation: rotate_left(n) = (bs << n) | (bs >> (total - n))
    let left = shift_bytestring(bs, effective.clone());
    let right = shift_bytestring(bs, effective - total_bits);
    left.iter()
        .zip(right.iter())
        .map(|(&a, &b)| a | b)
        .collect()
}

/// Find the index of the lowest set bit (bit 0 = LSB of last byte).
///
/// Returns -1 if the byte string is empty or all zeros.
fn find_first_set_bit(bs: &[u8]) -> BigInt {
    for (byte_idx_from_end, &byte) in bs.iter().rev().enumerate() {
        if byte != 0 {
            let bit_within_byte = byte.trailing_zeros() as usize;
            return BigInt::from(byte_idx_from_end * 8 + bit_within_byte);
        }
    }
    BigInt::from(-1)
}

/// Extract a list of integers from a ProtoList of Integers.
fn get_int_list(val: &Value) -> Result<Vec<BigInt>, MachineError> {
    let list = get_list(val)?;
    list.iter()
        .map(|c| match c {
            Constant::Integer(i) => Ok(i.clone()),
            other => Err(MachineError::TypeMismatch {
                expected: "integer list element",
                actual: constant_type_name(other),
            }),
        })
        .collect()
}

/// Extract a list of booleans from a ProtoList of Bools.
fn get_bool_list(val: &Value) -> Result<Vec<bool>, MachineError> {
    let list = get_list(val)?;
    list.iter()
        .map(|c| match c {
            Constant::Bool(b) => Ok(*b),
            other => Err(MachineError::TypeMismatch {
                expected: "bool list element",
                actual: constant_type_name(other),
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Modular exponentiation
// ---------------------------------------------------------------------------

/// Compute `base^exp mod modulus`.
///
/// For negative exponents, returns 0 (matching upstream Plutus behavior
/// where modular inverse is not supported and negative exponents yield 0).
///
/// Reference: CIP-0109 / Plutus `expModInteger`.
fn exp_mod_integer(base: BigInt, exp: BigInt, modulus: BigInt) -> Result<BigInt, MachineError> {
    if modulus.is_zero() {
        return Err(MachineError::DivisionByZero);
    }
    if exp.sign() == Sign::Minus {
        // Upstream Plutus: negative exponent → error (we follow the spec).
        return Err(MachineError::BuiltinError {
            builtin: "expModInteger".into(),
            message: "negative exponent".into(),
        });
    }
    let m = modulus.abs();
    if m.is_one() {
        return Ok(BigInt::zero());
    }

    Ok(base.modpow(&exp, &m))
}

fn bigint_to_i128_for_error(value: &BigInt) -> i128 {
    value.to_i128().unwrap_or_else(|| {
        if value.sign() == Sign::Minus {
            i128::MIN
        } else {
            i128::MAX
        }
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests;
