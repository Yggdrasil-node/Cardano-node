//! Built-in function implementations for the UPLC evaluator.
//!
//! All PlutusV1, PlutusV2, and PlutusV3 builtins are implemented,
//! including BLS12-381 curve operations (CIP-0381), bitwise operations,
//! integer/bytestring conversions, and extra hash functions.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>

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
mod tests {
    use super::*;
    use crate::cost_model::CostModel;

    /// Helper: make a Value from an i128.
    fn int(n: i128) -> Value {
        Value::Constant(Constant::integer(n))
    }

    fn bs(data: &[u8]) -> Value {
        Value::Constant(Constant::ByteString(data.to_vec()))
    }

    fn str_val(s: &str) -> Value {
        Value::Constant(Constant::String(s.to_string()))
    }

    fn bool_val(b: bool) -> Value {
        Value::Constant(Constant::Bool(b))
    }

    fn unit_val() -> Value {
        Value::Constant(Constant::Unit)
    }

    fn data_val(d: PlutusData) -> Value {
        Value::Constant(Constant::Data(d))
    }

    fn list_val(ty: Type, items: Vec<Constant>) -> Value {
        Value::Constant(Constant::ProtoList(ty, items))
    }

    fn pair_val(t1: Type, t2: Type, a: Constant, b: Constant) -> Value {
        Value::Constant(Constant::ProtoPair(t1, t2, Box::new(a), Box::new(b)))
    }

    fn eval(fun: DefaultFun, args: &[Value]) -> Result<Value, MachineError> {
        let cm = CostModel::default();
        let mut logs = Vec::new();
        evaluate_builtin(fun, args, &cm, &mut logs)
    }

    fn eval_with_model(
        fun: DefaultFun,
        args: &[Value],
        cm: &CostModel,
    ) -> Result<Value, MachineError> {
        let mut logs = Vec::new();
        evaluate_builtin(fun, args, cm, &mut logs)
    }

    fn eval_logged(fun: DefaultFun, args: &[Value]) -> Result<(Value, Vec<String>), MachineError> {
        let cm = CostModel::default();
        let mut logs = Vec::new();
        let result = evaluate_builtin(fun, args, &cm, &mut logs)?;
        Ok((result, logs))
    }

    fn expect_int(v: Value) -> i128 {
        match v {
            Value::Constant(Constant::Integer(n)) => n
                .to_i128()
                .unwrap_or_else(|| panic!("expected i128-sized integer, got {n}")),
            _ => panic!("expected integer, got {:?}", v.type_name()),
        }
    }

    fn expect_big_int(v: Value) -> BigInt {
        match v {
            Value::Constant(Constant::Integer(n)) => n,
            _ => panic!("expected integer, got {:?}", v.type_name()),
        }
    }

    fn expect_bool(v: Value) -> bool {
        match v {
            Value::Constant(Constant::Bool(b)) => b,
            _ => panic!("expected bool, got {:?}", v.type_name()),
        }
    }

    fn expect_bs(v: Value) -> Vec<u8> {
        match v {
            Value::Constant(Constant::ByteString(bs)) => bs,
            _ => panic!("expected bytestring"),
        }
    }

    fn expect_string(v: Value) -> String {
        match v {
            Value::Constant(Constant::String(s)) => s,
            _ => panic!("expected string"),
        }
    }

    // ===================================================================
    // Integer arithmetic
    // ===================================================================

    #[test]
    fn add_integer_basic() {
        assert_eq!(
            expect_int(eval(DefaultFun::AddInteger, &[int(3), int(4)]).unwrap()),
            7
        );
    }

    #[test]
    fn add_integer_negative() {
        assert_eq!(
            expect_int(eval(DefaultFun::AddInteger, &[int(-10), int(3)]).unwrap()),
            -7
        );
    }

    #[test]
    fn add_integer_zero() {
        assert_eq!(
            expect_int(eval(DefaultFun::AddInteger, &[int(0), int(0)]).unwrap()),
            0
        );
    }

    #[test]
    fn subtract_integer() {
        assert_eq!(
            expect_int(eval(DefaultFun::SubtractInteger, &[int(10), int(3)]).unwrap()),
            7
        );
    }

    #[test]
    fn subtract_integer_negative_result() {
        assert_eq!(
            expect_int(eval(DefaultFun::SubtractInteger, &[int(3), int(10)]).unwrap()),
            -7
        );
    }

    #[test]
    fn multiply_integer() {
        assert_eq!(
            expect_int(eval(DefaultFun::MultiplyInteger, &[int(6), int(7)]).unwrap()),
            42
        );
    }

    #[test]
    fn multiply_integer_zero() {
        assert_eq!(
            expect_int(eval(DefaultFun::MultiplyInteger, &[int(999), int(0)]).unwrap()),
            0
        );
    }

    #[test]
    fn add_integer_does_not_overflow_i128() {
        let a = BigInt::from(i128::MAX);
        let b = BigInt::from(1u8);
        let result = expect_big_int(
            eval(
                DefaultFun::AddInteger,
                &[
                    Value::Constant(Constant::integer(a.clone())),
                    Value::Constant(Constant::integer(b.clone())),
                ],
            )
            .unwrap(),
        );
        assert_eq!(result, a + b);
    }

    #[test]
    fn multiply_integer_does_not_overflow_i128() {
        let a = BigInt::from(i128::MAX);
        let b = BigInt::from(2u8);
        let result = expect_big_int(
            eval(
                DefaultFun::MultiplyInteger,
                &[
                    Value::Constant(Constant::integer(a.clone())),
                    Value::Constant(Constant::integer(b.clone())),
                ],
            )
            .unwrap(),
        );
        assert_eq!(result, a * b);
    }

    #[test]
    fn divide_integer_positive() {
        // Haskell `div`: rounds toward -inf.
        assert_eq!(
            expect_int(eval(DefaultFun::DivideInteger, &[int(7), int(2)]).unwrap()),
            3
        );
    }

    #[test]
    fn divide_integer_negative_rounds_down() {
        // -7 `div` 2 = -4 in Haskell (rounds toward -inf).
        assert_eq!(
            expect_int(eval(DefaultFun::DivideInteger, &[int(-7), int(2)]).unwrap()),
            -4
        );
    }

    #[test]
    fn divide_integer_negative_divisor() {
        // 7 `div` (-2) = -4 in Haskell (floor division, NOT Euclidean).
        assert_eq!(
            expect_int(eval(DefaultFun::DivideInteger, &[int(7), int(-2)]).unwrap()),
            -4
        );
    }

    #[test]
    fn divide_integer_both_negative() {
        // (-7) `div` (-2) = 3 in Haskell.
        assert_eq!(
            expect_int(eval(DefaultFun::DivideInteger, &[int(-7), int(-2)]).unwrap()),
            3
        );
    }

    #[test]
    fn divide_integer_by_zero() {
        let err = eval(DefaultFun::DivideInteger, &[int(10), int(0)]).unwrap_err();
        assert!(matches!(err, MachineError::DivisionByZero));
    }

    #[test]
    fn quotient_integer_positive() {
        // Haskell `quot`: rounds toward zero.
        assert_eq!(
            expect_int(eval(DefaultFun::QuotientInteger, &[int(7), int(2)]).unwrap()),
            3
        );
    }

    #[test]
    fn quotient_integer_negative_truncates() {
        // -7 `quot` 2 = -3 (truncate toward zero).
        assert_eq!(
            expect_int(eval(DefaultFun::QuotientInteger, &[int(-7), int(2)]).unwrap()),
            -3
        );
    }

    #[test]
    fn quotient_integer_by_zero() {
        assert!(eval(DefaultFun::QuotientInteger, &[int(1), int(0)]).is_err());
    }

    #[test]
    fn remainder_integer() {
        // 7 `rem` 3 = 1 (sign follows dividend).
        assert_eq!(
            expect_int(eval(DefaultFun::RemainderInteger, &[int(7), int(3)]).unwrap()),
            1
        );
    }

    #[test]
    fn remainder_integer_negative() {
        // -7 `rem` 3 = -1 (sign follows dividend).
        assert_eq!(
            expect_int(eval(DefaultFun::RemainderInteger, &[int(-7), int(3)]).unwrap()),
            -1
        );
    }

    #[test]
    fn remainder_by_zero() {
        assert!(eval(DefaultFun::RemainderInteger, &[int(1), int(0)]).is_err());
    }

    #[test]
    fn mod_integer() {
        // 7 `mod` 3 = 1 (sign follows divisor).
        assert_eq!(
            expect_int(eval(DefaultFun::ModInteger, &[int(7), int(3)]).unwrap()),
            1
        );
    }

    #[test]
    fn mod_integer_negative() {
        // -7 `mod` 3 = 2 (Haskell mod: sign follows divisor).
        assert_eq!(
            expect_int(eval(DefaultFun::ModInteger, &[int(-7), int(3)]).unwrap()),
            2
        );
    }

    #[test]
    fn mod_integer_negative_divisor() {
        // 7 `mod` (-2) = -1 in Haskell (sign follows divisor).
        assert_eq!(
            expect_int(eval(DefaultFun::ModInteger, &[int(7), int(-2)]).unwrap()),
            -1
        );
    }

    #[test]
    fn mod_integer_both_negative() {
        // (-7) `mod` (-2) = -1 in Haskell.
        assert_eq!(
            expect_int(eval(DefaultFun::ModInteger, &[int(-7), int(-2)]).unwrap()),
            -1
        );
    }

    #[test]
    fn mod_by_zero() {
        assert!(eval(DefaultFun::ModInteger, &[int(1), int(0)]).is_err());
    }

    // ===================================================================
    // Integer comparison
    // ===================================================================

    #[test]
    fn equals_integer_true() {
        assert!(expect_bool(
            eval(DefaultFun::EqualsInteger, &[int(42), int(42)]).unwrap()
        ));
    }

    #[test]
    fn equals_integer_false() {
        assert!(!expect_bool(
            eval(DefaultFun::EqualsInteger, &[int(1), int(2)]).unwrap()
        ));
    }

    #[test]
    fn less_than_integer_true() {
        assert!(expect_bool(
            eval(DefaultFun::LessThanInteger, &[int(1), int(2)]).unwrap()
        ));
    }

    #[test]
    fn less_than_integer_false_equal() {
        assert!(!expect_bool(
            eval(DefaultFun::LessThanInteger, &[int(2), int(2)]).unwrap()
        ));
    }

    #[test]
    fn less_than_equals_integer_true() {
        assert!(expect_bool(
            eval(DefaultFun::LessThanEqualsInteger, &[int(2), int(2)]).unwrap()
        ));
    }

    #[test]
    fn less_than_equals_integer_false() {
        assert!(!expect_bool(
            eval(DefaultFun::LessThanEqualsInteger, &[int(3), int(2)]).unwrap()
        ));
    }

    // ===================================================================
    // ByteString operations
    // ===================================================================

    #[test]
    fn append_bytestring() {
        let r = expect_bs(eval(DefaultFun::AppendByteString, &[bs(&[1, 2]), bs(&[3, 4])]).unwrap());
        assert_eq!(r, vec![1, 2, 3, 4]);
    }

    #[test]
    fn append_bytestring_empty() {
        let r = expect_bs(eval(DefaultFun::AppendByteString, &[bs(&[]), bs(&[1])]).unwrap());
        assert_eq!(r, vec![1]);
    }

    #[test]
    fn cons_bytestring() {
        let r = expect_bs(eval(DefaultFun::ConsByteString, &[int(0xFF), bs(&[1, 2])]).unwrap());
        assert_eq!(r, vec![0xFF, 1, 2]);
    }

    #[test]
    fn cons_bytestring_variant_c_rejects_out_of_range_byte() {
        let cm = CostModel {
            builtin_semantics_variant: BuiltinSemanticsVariant::C,
            ..CostModel::default()
        };

        assert!(eval_with_model(DefaultFun::ConsByteString, &[int(256), bs(&[])], &cm).is_err());
        assert!(eval_with_model(DefaultFun::ConsByteString, &[int(-1), bs(&[])], &cm).is_err());
    }

    #[test]
    fn cons_bytestring_variants_a_and_b_wrap_byte_modulo_256() {
        for variant in [BuiltinSemanticsVariant::A, BuiltinSemanticsVariant::B] {
            let cm = CostModel {
                builtin_semantics_variant: variant,
                ..CostModel::default()
            };
            let hi = expect_bs(
                eval_with_model(DefaultFun::ConsByteString, &[int(256), bs(&[1])], &cm).unwrap(),
            );
            let neg = expect_bs(
                eval_with_model(DefaultFun::ConsByteString, &[int(-1), bs(&[1])], &cm).unwrap(),
            );

            assert_eq!(hi, vec![0, 1]);
            assert_eq!(neg, vec![255, 1]);
        }
    }

    #[test]
    fn slice_bytestring_basic() {
        let r = expect_bs(
            eval(
                DefaultFun::SliceByteString,
                &[int(1), int(2), bs(&[0, 1, 2, 3])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![1, 2]);
    }

    #[test]
    fn slice_bytestring_clamp() {
        // Start beyond end → empty.
        let r = expect_bs(
            eval(
                DefaultFun::SliceByteString,
                &[int(100), int(5), bs(&[1, 2])],
            )
            .unwrap(),
        );
        assert!(r.is_empty());
    }

    #[test]
    fn slice_bytestring_negative_start_uses_plutus_range_semantics() {
        let r = expect_bs(
            eval(
                DefaultFun::SliceByteString,
                &[int(-1), int(2), bs(&[10, 11, 12])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![10]);
    }

    #[test]
    fn slice_bytestring_negative_count_is_empty() {
        let r = expect_bs(
            eval(
                DefaultFun::SliceByteString,
                &[int(1), int(-1), bs(&[10, 11, 12])],
            )
            .unwrap(),
        );
        assert!(r.is_empty());
    }

    #[test]
    fn length_of_bytestring() {
        assert_eq!(
            expect_int(eval(DefaultFun::LengthOfByteString, &[bs(&[1, 2, 3])]).unwrap()),
            3
        );
    }

    #[test]
    fn length_of_bytestring_empty() {
        assert_eq!(
            expect_int(eval(DefaultFun::LengthOfByteString, &[bs(&[])]).unwrap()),
            0
        );
    }

    #[test]
    fn index_bytestring_valid() {
        assert_eq!(
            expect_int(eval(DefaultFun::IndexByteString, &[bs(&[10, 20, 30]), int(1)]).unwrap()),
            20
        );
    }

    #[test]
    fn index_bytestring_out_of_bounds() {
        assert!(eval(DefaultFun::IndexByteString, &[bs(&[1]), int(1)]).is_err());
        assert!(eval(DefaultFun::IndexByteString, &[bs(&[1]), int(-1)]).is_err());
    }

    #[test]
    fn equals_bytestring_true() {
        assert!(expect_bool(
            eval(DefaultFun::EqualsByteString, &[bs(&[1, 2]), bs(&[1, 2])]).unwrap()
        ));
    }

    #[test]
    fn equals_bytestring_false() {
        assert!(!expect_bool(
            eval(DefaultFun::EqualsByteString, &[bs(&[1]), bs(&[2])]).unwrap()
        ));
    }

    #[test]
    fn less_than_bytestring_true() {
        assert!(expect_bool(
            eval(DefaultFun::LessThanByteString, &[bs(&[1]), bs(&[2])]).unwrap()
        ));
    }

    #[test]
    fn less_than_bytestring_false() {
        assert!(!expect_bool(
            eval(DefaultFun::LessThanByteString, &[bs(&[2]), bs(&[1])]).unwrap()
        ));
    }

    #[test]
    fn less_than_equals_bytestring_less() {
        assert!(expect_bool(
            eval(DefaultFun::LessThanEqualsByteString, &[bs(&[1]), bs(&[2])]).unwrap()
        ));
    }

    #[test]
    fn less_than_equals_bytestring_equal() {
        assert!(expect_bool(
            eval(
                DefaultFun::LessThanEqualsByteString,
                &[bs(&[1, 2]), bs(&[1, 2])]
            )
            .unwrap()
        ));
    }

    #[test]
    fn less_than_equals_bytestring_greater() {
        assert!(!expect_bool(
            eval(DefaultFun::LessThanEqualsByteString, &[bs(&[2]), bs(&[1])]).unwrap()
        ));
    }

    #[test]
    fn less_than_equals_bytestring_empty() {
        assert!(expect_bool(
            eval(DefaultFun::LessThanEqualsByteString, &[bs(&[]), bs(&[])]).unwrap()
        ));
    }

    // ===================================================================
    // Cryptographic hashing
    // ===================================================================

    #[test]
    fn sha2_256_empty() {
        let r = expect_bs(eval(DefaultFun::Sha2_256, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 32);
        // SHA-256 of empty = e3b0c44298fc1c149afbf4c8996fb924...
        assert_eq!(r[0], 0xe3);
    }

    #[test]
    fn sha3_256_empty() {
        let r = expect_bs(eval(DefaultFun::Sha3_256, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn blake2b_256_empty() {
        let r = expect_bs(eval(DefaultFun::Blake2b_256, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn blake2b_224_empty() {
        let r = expect_bs(eval(DefaultFun::Blake2b_224, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 28);
    }

    #[test]
    fn keccak_256_empty() {
        let r = expect_bs(eval(DefaultFun::Keccak_256, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn ripemd_160_empty() {
        let r = expect_bs(eval(DefaultFun::Ripemd_160, &[bs(&[])]).unwrap());
        assert_eq!(r.len(), 20);
    }

    #[test]
    fn sha2_256_deterministic() {
        let data = &[0x61, 0x62, 0x63]; // "abc"
        let r1 = expect_bs(eval(DefaultFun::Sha2_256, &[bs(data)]).unwrap());
        let r2 = expect_bs(eval(DefaultFun::Sha2_256, &[bs(data)]).unwrap());
        assert_eq!(r1, r2);
    }

    // ===================================================================
    // String operations
    // ===================================================================

    #[test]
    fn append_string() {
        let r = expect_string(
            eval(
                DefaultFun::AppendString,
                &[str_val("hello"), str_val(" world")],
            )
            .unwrap(),
        );
        assert_eq!(r, "hello world");
    }

    #[test]
    fn equals_string_true() {
        assert!(expect_bool(
            eval(DefaultFun::EqualsString, &[str_val("abc"), str_val("abc")]).unwrap()
        ));
    }

    #[test]
    fn equals_string_false() {
        assert!(!expect_bool(
            eval(DefaultFun::EqualsString, &[str_val("a"), str_val("b")]).unwrap()
        ));
    }

    #[test]
    fn encode_utf8() {
        let r = expect_bs(eval(DefaultFun::EncodeUtf8, &[str_val("abc")]).unwrap());
        assert_eq!(r, vec![0x61, 0x62, 0x63]);
    }

    #[test]
    fn decode_utf8_valid() {
        let r = expect_string(eval(DefaultFun::DecodeUtf8, &[bs(&[0x61, 0x62, 0x63])]).unwrap());
        assert_eq!(r, "abc");
    }

    #[test]
    fn decode_utf8_invalid() {
        let err = eval(DefaultFun::DecodeUtf8, &[bs(&[0xFF, 0xFE])]).unwrap_err();
        assert!(matches!(err, MachineError::InvalidUtf8));
    }

    // ===================================================================
    // Bool / Unit
    // ===================================================================

    #[test]
    fn if_then_else_true() {
        let r = eval(DefaultFun::IfThenElse, &[bool_val(true), int(1), int(2)]).unwrap();
        assert_eq!(expect_int(r), 1);
    }

    #[test]
    fn if_then_else_false() {
        let r = eval(DefaultFun::IfThenElse, &[bool_val(false), int(1), int(2)]).unwrap();
        assert_eq!(expect_int(r), 2);
    }

    #[test]
    fn choose_unit() {
        let r = eval(DefaultFun::ChooseUnit, &[unit_val(), int(42)]).unwrap();
        assert_eq!(expect_int(r), 42);
    }

    #[test]
    fn choose_unit_type_mismatch() {
        assert!(eval(DefaultFun::ChooseUnit, &[int(1), int(2)]).is_err());
    }

    // ===================================================================
    // Trace
    // ===================================================================

    #[test]
    fn trace_returns_value() {
        let (r, logs) = eval_logged(DefaultFun::Trace, &[str_val("msg"), int(42)]).unwrap();
        assert_eq!(expect_int(r), 42);
        assert_eq!(logs, vec!["msg".to_string()]);
    }

    #[test]
    fn trace_empty_message() {
        let (_, logs) = eval_logged(DefaultFun::Trace, &[str_val(""), int(0)]).unwrap();
        assert_eq!(logs, vec!["".to_string()]);
    }

    // ===================================================================
    // Pair operations
    // ===================================================================

    #[test]
    fn fst_pair() {
        let p = pair_val(
            Type::Integer,
            Type::ByteString,
            Constant::integer(1),
            Constant::ByteString(vec![2]),
        );
        let r = eval(DefaultFun::FstPair, &[p]).unwrap();
        assert_eq!(expect_int(r), 1);
    }

    #[test]
    fn snd_pair() {
        let p = pair_val(
            Type::Integer,
            Type::ByteString,
            Constant::integer(1),
            Constant::ByteString(vec![2]),
        );
        let r = eval(DefaultFun::SndPair, &[p]).unwrap();
        assert_eq!(expect_bs(r), vec![2]);
    }

    #[test]
    fn fst_pair_type_mismatch() {
        assert!(eval(DefaultFun::FstPair, &[int(1)]).is_err());
    }

    // ===================================================================
    // List operations
    // ===================================================================

    #[test]
    fn choose_list_nil() {
        let empty = list_val(Type::Integer, vec![]);
        let r = eval(DefaultFun::ChooseList, &[empty, int(1), int(2)]).unwrap();
        assert_eq!(expect_int(r), 1);
    }

    #[test]
    fn choose_list_cons() {
        let non_empty = list_val(Type::Integer, vec![Constant::integer(10)]);
        let r = eval(DefaultFun::ChooseList, &[non_empty, int(1), int(2)]).unwrap();
        assert_eq!(expect_int(r), 2);
    }

    #[test]
    fn mk_cons() {
        let empty = list_val(Type::Integer, vec![]);
        let r = eval(DefaultFun::MkCons, &[int(42), empty]).unwrap();
        match r {
            Value::Constant(Constant::ProtoList(_, items)) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0], Constant::integer(42));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn head_list() {
        let l = list_val(
            Type::Integer,
            vec![Constant::integer(1), Constant::integer(2)],
        );
        let r = eval(DefaultFun::HeadList, &[l]).unwrap();
        assert_eq!(expect_int(r), 1);
    }

    #[test]
    fn head_list_empty() {
        let l = list_val(Type::Integer, vec![]);
        assert!(matches!(
            eval(DefaultFun::HeadList, &[l]).unwrap_err(),
            MachineError::EmptyList
        ));
    }

    #[test]
    fn tail_list() {
        let l = list_val(
            Type::Integer,
            vec![
                Constant::integer(1),
                Constant::integer(2),
                Constant::integer(3),
            ],
        );
        let r = eval(DefaultFun::TailList, &[l]).unwrap();
        match r {
            Value::Constant(Constant::ProtoList(_, items)) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], Constant::integer(2));
                assert_eq!(items[1], Constant::integer(3));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn tail_list_empty() {
        let l = list_val(Type::Integer, vec![]);
        assert!(matches!(
            eval(DefaultFun::TailList, &[l]).unwrap_err(),
            MachineError::EmptyList
        ));
    }

    #[test]
    fn null_list_true() {
        let l = list_val(Type::Integer, vec![]);
        assert!(expect_bool(eval(DefaultFun::NullList, &[l]).unwrap()));
    }

    #[test]
    fn null_list_false() {
        let l = list_val(Type::Integer, vec![Constant::integer(1)]);
        assert!(!expect_bool(eval(DefaultFun::NullList, &[l]).unwrap()));
    }

    // ===================================================================
    // Data operations
    // ===================================================================

    #[test]
    fn choose_data_constr() {
        let d = data_val(PlutusData::Constr(0, vec![]));
        let r = eval(
            DefaultFun::ChooseData,
            &[d, int(1), int(2), int(3), int(4), int(5)],
        )
        .unwrap();
        assert_eq!(expect_int(r), 1);
    }

    #[test]
    fn choose_data_map() {
        let d = data_val(PlutusData::Map(vec![]));
        let r = eval(
            DefaultFun::ChooseData,
            &[d, int(1), int(2), int(3), int(4), int(5)],
        )
        .unwrap();
        assert_eq!(expect_int(r), 2);
    }

    #[test]
    fn choose_data_list() {
        let d = data_val(PlutusData::List(vec![]));
        let r = eval(
            DefaultFun::ChooseData,
            &[d, int(1), int(2), int(3), int(4), int(5)],
        )
        .unwrap();
        assert_eq!(expect_int(r), 3);
    }

    #[test]
    fn choose_data_integer() {
        let d = data_val(PlutusData::integer(42));
        let r = eval(
            DefaultFun::ChooseData,
            &[d, int(1), int(2), int(3), int(4), int(5)],
        )
        .unwrap();
        assert_eq!(expect_int(r), 4);
    }

    #[test]
    fn choose_data_bytes() {
        let d = data_val(PlutusData::Bytes(vec![1]));
        let r = eval(
            DefaultFun::ChooseData,
            &[d, int(1), int(2), int(3), int(4), int(5)],
        )
        .unwrap();
        assert_eq!(expect_int(r), 5);
    }

    #[test]
    fn constr_data() {
        let field_list = list_val(Type::Data, vec![Constant::Data(PlutusData::integer(1))]);
        let r = eval(DefaultFun::ConstrData, &[int(0), field_list]).unwrap();
        match r {
            Value::Constant(Constant::Data(PlutusData::Constr(tag, fields))) => {
                assert_eq!(tag, 0);
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("expected ConstrData"),
        }
    }

    #[test]
    fn map_data() {
        let pair = Constant::ProtoPair(
            Type::Data,
            Type::Data,
            Box::new(Constant::Data(PlutusData::integer(1))),
            Box::new(Constant::Data(PlutusData::integer(2))),
        );
        let l = list_val(
            Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
            vec![pair],
        );
        let r = eval(DefaultFun::MapData, &[l]).unwrap();
        assert!(matches!(
            r,
            Value::Constant(Constant::Data(PlutusData::Map(_)))
        ));
    }

    #[test]
    fn list_data() {
        let l = list_val(Type::Data, vec![Constant::Data(PlutusData::integer(1))]);
        let r = eval(DefaultFun::ListData, &[l]).unwrap();
        assert!(matches!(
            r,
            Value::Constant(Constant::Data(PlutusData::List(_)))
        ));
    }

    #[test]
    fn i_data() {
        let r = eval(DefaultFun::IData, &[int(42)]).unwrap();
        assert!(matches!(
            r,
            Value::Constant(Constant::Data(PlutusData::Integer(ref n))) if n == &BigInt::from(42)
        ));
    }

    #[test]
    fn b_data() {
        let r = eval(DefaultFun::BData, &[bs(&[1, 2])]).unwrap();
        assert!(matches!(
            r,
            Value::Constant(Constant::Data(PlutusData::Bytes(_)))
        ));
    }

    #[test]
    fn un_constr_data() {
        let d = data_val(PlutusData::Constr(1, vec![PlutusData::integer(10)]));
        let r = eval(DefaultFun::UnConstrData, &[d]).unwrap();
        // Should be a pair (tag, list of data).
        assert!(matches!(r, Value::Constant(Constant::ProtoPair(..))));
    }

    #[test]
    fn un_constr_data_wrong_type() {
        let d = data_val(PlutusData::integer(1));
        assert!(eval(DefaultFun::UnConstrData, &[d]).is_err());
    }

    #[test]
    fn un_map_data() {
        let d = data_val(PlutusData::Map(vec![(
            PlutusData::integer(1),
            PlutusData::integer(2),
        )]));
        let r = eval(DefaultFun::UnMapData, &[d]).unwrap();
        assert!(matches!(r, Value::Constant(Constant::ProtoList(..))));
    }

    #[test]
    fn un_map_data_wrong_type() {
        assert!(eval(DefaultFun::UnMapData, &[data_val(PlutusData::integer(1))]).is_err());
    }

    #[test]
    fn un_list_data() {
        let d = data_val(PlutusData::List(vec![PlutusData::integer(1)]));
        let r = eval(DefaultFun::UnListData, &[d]).unwrap();
        assert!(matches!(r, Value::Constant(Constant::ProtoList(..))));
    }

    #[test]
    fn un_list_data_wrong_type() {
        assert!(
            eval(
                DefaultFun::UnListData,
                &[data_val(PlutusData::Bytes(vec![]))]
            )
            .is_err()
        );
    }

    #[test]
    fn un_i_data() {
        let d = data_val(PlutusData::integer(99));
        assert_eq!(expect_int(eval(DefaultFun::UnIData, &[d]).unwrap()), 99);
    }

    #[test]
    fn un_i_data_wrong_type() {
        assert!(eval(DefaultFun::UnIData, &[data_val(PlutusData::Bytes(vec![]))]).is_err());
    }

    #[test]
    fn un_b_data() {
        let d = data_val(PlutusData::Bytes(vec![0xAB]));
        assert_eq!(
            expect_bs(eval(DefaultFun::UnBData, &[d]).unwrap()),
            vec![0xAB]
        );
    }

    #[test]
    fn un_b_data_wrong_type() {
        assert!(eval(DefaultFun::UnBData, &[data_val(PlutusData::integer(1))]).is_err());
    }

    #[test]
    fn equals_data_true() {
        let a = data_val(PlutusData::integer(42));
        let b = data_val(PlutusData::integer(42));
        assert!(expect_bool(eval(DefaultFun::EqualsData, &[a, b]).unwrap()));
    }

    #[test]
    fn equals_data_false() {
        let a = data_val(PlutusData::integer(1));
        let b = data_val(PlutusData::integer(2));
        assert!(!expect_bool(eval(DefaultFun::EqualsData, &[a, b]).unwrap()));
    }

    #[test]
    fn mk_pair_data() {
        let a = data_val(PlutusData::integer(1));
        let b = data_val(PlutusData::integer(2));
        let r = eval(DefaultFun::MkPairData, &[a, b]).unwrap();
        assert!(matches!(r, Value::Constant(Constant::ProtoPair(..))));
    }

    #[test]
    fn mk_nil_data() {
        let r = eval(DefaultFun::MkNilData, &[unit_val()]).unwrap();
        match r {
            Value::Constant(Constant::ProtoList(Type::Data, items)) => assert!(items.is_empty()),
            _ => panic!("expected empty data list"),
        }
    }

    #[test]
    fn mk_nil_pair_data() {
        let r = eval(DefaultFun::MkNilPairData, &[unit_val()]).unwrap();
        match r {
            Value::Constant(Constant::ProtoList(_, items)) => assert!(items.is_empty()),
            _ => panic!("expected empty pair list"),
        }
    }

    #[test]
    fn serialise_data() {
        let d = data_val(PlutusData::integer(42));
        let r = expect_bs(eval(DefaultFun::SerialiseData, &[d]).unwrap());
        assert!(!r.is_empty());
    }

    // ===================================================================
    // Integer ↔ ByteString conversion
    // ===================================================================

    #[test]
    fn integer_to_bytestring_big_endian() {
        let r = expect_bs(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(0), int(256)],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![1, 0]); // 256 = 0x0100
    }

    #[test]
    fn integer_to_bytestring_little_endian() {
        let r = expect_bs(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(true), int(0), int(256)],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0, 1]); // 256 LE = 0x0001
    }

    #[test]
    fn integer_to_bytestring_zero() {
        let r = expect_bs(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(0), int(0)],
            )
            .unwrap(),
        );
        assert!(r.is_empty()); // 0 with no required len = empty
    }

    #[test]
    fn integer_to_bytestring_zero_with_len() {
        let r = expect_bs(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(4), int(0)],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0, 0, 0, 0]);
    }

    #[test]
    fn integer_to_bytestring_padded() {
        let r = expect_bs(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(4), int(1)],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0, 0, 0, 1]);
    }

    #[test]
    fn integer_to_bytestring_negative_error() {
        assert!(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(0), int(-1)]
            )
            .is_err()
        );
    }

    #[test]
    fn integer_to_bytestring_too_large_len() {
        assert!(
            eval(
                DefaultFun::IntegerToByteString,
                &[bool_val(false), int(9000), int(1)]
            )
            .is_err()
        );
    }

    #[test]
    fn bytestring_to_integer_big_endian() {
        assert_eq!(
            expect_int(
                eval(
                    DefaultFun::ByteStringToInteger,
                    &[bool_val(false), bs(&[1, 0])]
                )
                .unwrap()
            ),
            256
        );
    }

    #[test]
    fn bytestring_to_integer_little_endian() {
        assert_eq!(
            expect_int(
                eval(
                    DefaultFun::ByteStringToInteger,
                    &[bool_val(true), bs(&[0, 1])]
                )
                .unwrap()
            ),
            256
        );
    }

    #[test]
    fn bytestring_to_integer_empty() {
        assert_eq!(
            expect_int(eval(DefaultFun::ByteStringToInteger, &[bool_val(false), bs(&[])]).unwrap()),
            0
        );
    }

    // ===================================================================
    // Bitwise operations
    // ===================================================================

    #[test]
    fn and_bytestring_truncate() {
        // AND with truncation (pad=false): result = min length.
        let r = expect_bs(
            eval(
                DefaultFun::AndByteString,
                &[bool_val(false), bs(&[0xFF, 0x0F]), bs(&[0x0F])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0x0F]); // only last byte used from shorter
    }

    #[test]
    fn and_bytestring_pad() {
        // AND with padding (pad=true): shorter is zero-padded on left.
        let r = expect_bs(
            eval(
                DefaultFun::AndByteString,
                &[bool_val(true), bs(&[0xFF, 0x0F]), bs(&[0x0F])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0x00, 0x0F]); // 0xFF & 0x00 = 0x00, 0x0F & 0x0F = 0x0F
    }

    #[test]
    fn or_bytestring() {
        let r = expect_bs(
            eval(
                DefaultFun::OrByteString,
                &[bool_val(false), bs(&[0xF0]), bs(&[0x0F])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0xFF]);
    }

    #[test]
    fn xor_bytestring() {
        let r = expect_bs(
            eval(
                DefaultFun::XorByteString,
                &[bool_val(false), bs(&[0xFF]), bs(&[0xFF])],
            )
            .unwrap(),
        );
        assert_eq!(r, vec![0x00]);
    }

    #[test]
    fn complement_bytestring() {
        let r = expect_bs(eval(DefaultFun::ComplementByteString, &[bs(&[0x00, 0xFF])]).unwrap());
        assert_eq!(r, vec![0xFF, 0x00]);
    }

    #[test]
    fn complement_bytestring_empty() {
        let r = expect_bs(eval(DefaultFun::ComplementByteString, &[bs(&[])]).unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn read_bit_basic() {
        // 0b10000000 = 0x80; bit 7 (MSB of byte = bit 7 from LSB) should be set.
        assert!(expect_bool(
            eval(DefaultFun::ReadBit, &[bs(&[0x80]), int(7)]).unwrap()
        ));
        assert!(!expect_bool(
            eval(DefaultFun::ReadBit, &[bs(&[0x80]), int(0)]).unwrap()
        ));
    }

    #[test]
    fn read_bit_out_of_bounds() {
        assert!(eval(DefaultFun::ReadBit, &[bs(&[0xFF]), int(8)]).is_err());
        assert!(eval(DefaultFun::ReadBit, &[bs(&[0xFF]), int(-1)]).is_err());
    }

    #[test]
    fn write_bits_basic() {
        // Start with 0x00, set bit 0 to true.
        let indices = list_val(Type::Integer, vec![Constant::integer(0)]);
        let values = list_val(Type::Bool, vec![Constant::Bool(true)]);
        let r = expect_bs(eval(DefaultFun::WriteBits, &[bs(&[0x00]), indices, values]).unwrap());
        assert_eq!(r, vec![0x01]);
    }

    #[test]
    fn write_bits_clear() {
        // Start with 0xFF, clear bit 0.
        let indices = list_val(Type::Integer, vec![Constant::integer(0)]);
        let values = list_val(Type::Bool, vec![Constant::Bool(false)]);
        let r = expect_bs(eval(DefaultFun::WriteBits, &[bs(&[0xFF]), indices, values]).unwrap());
        assert_eq!(r, vec![0xFE]);
    }

    #[test]
    fn write_bits_length_mismatch() {
        let indices = list_val(Type::Integer, vec![Constant::integer(0)]);
        let values = list_val(Type::Bool, vec![]);
        assert!(eval(DefaultFun::WriteBits, &[bs(&[0x00]), indices, values]).is_err());
    }

    #[test]
    fn replicate_byte() {
        let r = expect_bs(eval(DefaultFun::ReplicateByte, &[int(3), int(0xAB)]).unwrap());
        assert_eq!(r, vec![0xAB, 0xAB, 0xAB]);
    }

    #[test]
    fn replicate_byte_zero_length() {
        let r = expect_bs(eval(DefaultFun::ReplicateByte, &[int(0), int(0xFF)]).unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn replicate_byte_bad_length() {
        assert!(eval(DefaultFun::ReplicateByte, &[int(-1), int(0)]).is_err());
        assert!(eval(DefaultFun::ReplicateByte, &[int(9000), int(0)]).is_err());
    }

    #[test]
    fn replicate_byte_bad_value() {
        assert!(eval(DefaultFun::ReplicateByte, &[int(1), int(256)]).is_err());
        assert!(eval(DefaultFun::ReplicateByte, &[int(1), int(-1)]).is_err());
    }

    #[test]
    fn shift_bytestring_left() {
        // 0x80 << 1 = 0x00 (bit shifted out).
        let r = expect_bs(eval(DefaultFun::ShiftByteString, &[bs(&[0x80]), int(1)]).unwrap());
        assert_eq!(r, vec![0x00]);
    }

    #[test]
    fn shift_bytestring_right() {
        // 0x01 >> 1 = 0x00.
        let r = expect_bs(eval(DefaultFun::ShiftByteString, &[bs(&[0x01]), int(-1)]).unwrap());
        assert_eq!(r, vec![0x00]);
    }

    #[test]
    fn shift_bytestring_zero() {
        let r = expect_bs(eval(DefaultFun::ShiftByteString, &[bs(&[0xAB]), int(0)]).unwrap());
        assert_eq!(r, vec![0xAB]);
    }

    #[test]
    fn shift_bytestring_empty() {
        let r = expect_bs(eval(DefaultFun::ShiftByteString, &[bs(&[]), int(5)]).unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn rotate_bytestring_full() {
        // Rotating by total bits should be identity.
        let r = expect_bs(eval(DefaultFun::RotateByteString, &[bs(&[0xAB]), int(8)]).unwrap());
        assert_eq!(r, vec![0xAB]);
    }

    #[test]
    fn rotate_bytestring_zero() {
        let r = expect_bs(eval(DefaultFun::RotateByteString, &[bs(&[0xAB]), int(0)]).unwrap());
        assert_eq!(r, vec![0xAB]);
    }

    #[test]
    fn rotate_bytestring_empty() {
        let r = expect_bs(eval(DefaultFun::RotateByteString, &[bs(&[]), int(5)]).unwrap());
        assert!(r.is_empty());
    }

    #[test]
    fn count_set_bits() {
        assert_eq!(
            expect_int(eval(DefaultFun::CountSetBits, &[bs(&[0xFF])]).unwrap()),
            8
        );
        assert_eq!(
            expect_int(eval(DefaultFun::CountSetBits, &[bs(&[0x00])]).unwrap()),
            0
        );
        assert_eq!(
            expect_int(eval(DefaultFun::CountSetBits, &[bs(&[0x0F, 0xF0])]).unwrap()),
            8
        );
    }

    #[test]
    fn count_set_bits_empty() {
        assert_eq!(
            expect_int(eval(DefaultFun::CountSetBits, &[bs(&[])]).unwrap()),
            0
        );
    }

    #[test]
    fn find_first_set_bit_basic() {
        // 0x01: bit 0 is set.
        assert_eq!(
            expect_int(eval(DefaultFun::FindFirstSetBit, &[bs(&[0x01])]).unwrap()),
            0
        );
        // 0x02: bit 1 is set (bit 0 is the LSB of last byte).
        assert_eq!(
            expect_int(eval(DefaultFun::FindFirstSetBit, &[bs(&[0x02])]).unwrap()),
            1
        );
    }

    #[test]
    fn find_first_set_bit_all_zeros() {
        assert_eq!(
            expect_int(eval(DefaultFun::FindFirstSetBit, &[bs(&[0x00])]).unwrap()),
            -1
        );
    }

    #[test]
    fn find_first_set_bit_empty() {
        assert_eq!(
            expect_int(eval(DefaultFun::FindFirstSetBit, &[bs(&[])]).unwrap()),
            -1
        );
    }

    // ===================================================================
    // ExpModInteger
    // ===================================================================

    #[test]
    fn exp_mod_integer_basic() {
        // 2^10 mod 1000 = 1024 mod 1000 = 24.
        assert_eq!(
            expect_int(eval(DefaultFun::ExpModInteger, &[int(2), int(10), int(1000)]).unwrap()),
            24
        );
    }

    #[test]
    fn exp_mod_integer_zero_exp() {
        // x^0 mod m = 1 (for m > 1).
        assert_eq!(
            expect_int(eval(DefaultFun::ExpModInteger, &[int(5), int(0), int(7)]).unwrap()),
            1
        );
    }

    #[test]
    fn exp_mod_integer_mod_one() {
        // x^e mod 1 = 0.
        assert_eq!(
            expect_int(eval(DefaultFun::ExpModInteger, &[int(5), int(10), int(1)]).unwrap()),
            0
        );
    }

    #[test]
    fn exp_mod_integer_zero_mod() {
        assert!(eval(DefaultFun::ExpModInteger, &[int(2), int(3), int(0)]).is_err());
    }

    #[test]
    fn exp_mod_integer_negative_exp() {
        assert!(eval(DefaultFun::ExpModInteger, &[int(2), int(-1), int(5)]).is_err());
    }

    #[test]
    fn exp_mod_integer_negative_base() {
        // (-2)^3 mod 5 = -8 mod 5 = 2 (normalized).
        assert_eq!(
            expect_int(eval(DefaultFun::ExpModInteger, &[int(-2), int(3), int(5)]).unwrap()),
            2
        );
    }

    // ===================================================================
    // Ed25519 verify (via builtin)
    // ===================================================================

    #[test]
    fn verify_ed25519_bad_key_length() {
        // Too short key → false.
        assert!(!expect_bool(
            eval(
                DefaultFun::VerifyEd25519Signature,
                &[bs(&[0; 16]), bs(&[]), bs(&[0; 64])]
            )
            .unwrap()
        ));
    }

    #[test]
    fn verify_ed25519_bad_sig_length() {
        assert!(!expect_bool(
            eval(
                DefaultFun::VerifyEd25519Signature,
                &[bs(&[0; 32]), bs(&[]), bs(&[0; 32])]
            )
            .unwrap()
        ));
    }

    // ===================================================================
    // Type mismatch edge cases
    // ===================================================================

    #[test]
    fn add_integer_type_mismatch() {
        assert!(eval(DefaultFun::AddInteger, &[int(1), bool_val(true)]).is_err());
    }

    #[test]
    fn head_list_not_a_list() {
        assert!(eval(DefaultFun::HeadList, &[int(1)]).is_err());
    }

    #[test]
    fn fst_pair_not_a_pair() {
        assert!(eval(DefaultFun::FstPair, &[bool_val(true)]).is_err());
    }

    // ===================================================================
    // constant_type_name coverage
    // ===================================================================

    #[test]
    fn constant_type_name_all_variants() {
        assert_eq!(constant_type_name(&Constant::integer(0)), "integer");
        assert_eq!(
            constant_type_name(&Constant::ByteString(vec![])),
            "bytestring"
        );
        assert_eq!(
            constant_type_name(&Constant::String(String::new())),
            "string"
        );
        assert_eq!(constant_type_name(&Constant::Unit), "unit");
        assert_eq!(constant_type_name(&Constant::Bool(true)), "bool");
        assert_eq!(
            constant_type_name(&Constant::ProtoList(Type::Integer, vec![])),
            "list"
        );
        assert_eq!(
            constant_type_name(&Constant::ProtoPair(
                Type::Integer,
                Type::Integer,
                Box::new(Constant::integer(0)),
                Box::new(Constant::integer(0)),
            )),
            "pair"
        );
        assert_eq!(
            constant_type_name(&Constant::Data(PlutusData::integer(0))),
            "data"
        );
    }

    // ===================================================================
    // data_variant_name coverage
    // ===================================================================

    #[test]
    fn data_variant_name_all() {
        assert_eq!(data_variant_name(&PlutusData::Constr(0, vec![])), "Constr");
        assert_eq!(data_variant_name(&PlutusData::Map(vec![])), "Map");
        assert_eq!(data_variant_name(&PlutusData::List(vec![])), "List");
        assert_eq!(data_variant_name(&PlutusData::integer(0)), "Integer");
        assert_eq!(data_variant_name(&PlutusData::Bytes(vec![])), "Bytes");
    }

    // ===================================================================
    // Helpers: integer_to_bytestring / bytestring_to_integer round-trip
    // ===================================================================

    #[test]
    fn int_bs_round_trip_big_endian() {
        let val = 12345;
        let bs_bytes = integer_to_bytestring(false, 0, val).unwrap();
        let back = bytestring_to_integer(false, &bs_bytes);
        assert_eq!(back, BigInt::from(val));
    }

    #[test]
    fn int_bs_round_trip_little_endian() {
        let val = 12345;
        let bs_bytes = integer_to_bytestring(true, 0, val).unwrap();
        let back = bytestring_to_integer(true, &bs_bytes);
        assert_eq!(back, BigInt::from(val));
    }

    // ===================================================================
    // Helper functions: read_bit, write_bits, find_first_set_bit
    // ===================================================================

    #[test]
    fn read_bit_multi_byte() {
        // [0x00, 0x01]: bit 0 of last byte = bit 0 overall.
        assert!(read_bit(&[0x00, 0x01], 0).unwrap());
        assert!(!read_bit(&[0x00, 0x01], 1).unwrap());
        // bit 8 = bit 0 of first byte = 0.
        assert!(!read_bit(&[0x00, 0x01], 8).unwrap());
    }

    #[test]
    fn shift_bytestring_large_shift() {
        // Shift by more than total bits → all zeros.
        let r = shift_bytestring(&[0xFF, 0xFF], 100);
        assert_eq!(r, vec![0, 0]);
    }

    #[test]
    fn rotate_bytestring_negative() {
        // Rotate right by 8 on a 1-byte value is identity.
        let r = rotate_bytestring(&[0xAB], -8);
        assert_eq!(r, vec![0xAB]);
    }

    // ===================================================================
    // bitwise_binop internal tests
    // ===================================================================

    #[test]
    fn bitwise_binop_same_length() {
        let r = bitwise_binop(&[0xF0], &[0x0F], false, |a, b| a | b);
        assert_eq!(r, vec![0xFF]);
    }

    #[test]
    fn bitwise_binop_empty() {
        let r = bitwise_binop(&[], &[0xFF], false, |a, b| a & b);
        assert!(r.is_empty());
    }

    // ===================================================================
    // int_to_scalar_bytes
    // ===================================================================

    #[test]
    fn int_to_scalar_bytes_zero() {
        let (bytes, neg) = int_to_scalar_bytes(0);
        assert_eq!(bytes, vec![0]);
        assert!(!neg);
    }

    #[test]
    fn int_to_scalar_bytes_positive() {
        let (bytes, neg) = int_to_scalar_bytes(256);
        assert!(!neg);
        assert_eq!(bytes, vec![1, 0]); // 256 big-endian
    }

    #[test]
    fn int_to_scalar_bytes_negative() {
        let (bytes, neg) = int_to_scalar_bytes(-42);
        assert!(neg);
        assert_eq!(bytes, vec![42]);
    }

    #[test]
    fn int_to_scalar_bytes_min() {
        let (_, neg) = int_to_scalar_bytes(i128::MIN);
        assert!(neg);
    }
}
