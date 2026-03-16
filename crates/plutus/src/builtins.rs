//! Built-in function implementations for the UPLC evaluator.
//!
//! All PlutusV1 and PlutusV2 builtins are implemented. PlutusV3
//! builtins (BLS12-381, bitwise, integer/bytestring conversion,
//! modular exponentiation) are implemented except for BLS12-381
//! which returns `MachineError::UnimplementedBuiltin`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>

use yggdrasil_crypto::blake2b;
use yggdrasil_crypto::secp256k1;
use yggdrasil_ledger::cbor::{Encoder, CborEncode};
use yggdrasil_ledger::plutus::PlutusData;

use crate::cost_model::CostModel;
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
        AddInteger => int_binop(args, |a, b| a.checked_add(b).ok_or(MachineError::IntegerOverflow)),
        SubtractInteger => int_binop(args, |a, b| a.checked_sub(b).ok_or(MachineError::IntegerOverflow)),
        MultiplyInteger => int_binop(args, |a, b| a.checked_mul(b).ok_or(MachineError::IntegerOverflow)),
        DivideInteger => int_binop(args, |a, b| {
            if b == 0 { return Err(MachineError::DivisionByZero); }
            // Haskell `div`: rounds towards negative infinity.
            Ok(a.div_euclid(b))
        }),
        QuotientInteger => int_binop(args, |a, b| {
            if b == 0 { return Err(MachineError::DivisionByZero); }
            // Haskell `quot`: rounds towards zero.
            Ok(a / b)
        }),
        RemainderInteger => int_binop(args, |a, b| {
            if b == 0 { return Err(MachineError::DivisionByZero); }
            // Haskell `rem`: sign follows dividend.
            Ok(a % b)
        }),
        ModInteger => int_binop(args, |a, b| {
            if b == 0 { return Err(MachineError::DivisionByZero); }
            // Haskell `mod`: sign follows divisor.
            Ok(a.rem_euclid(b))
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
            if !(0..=255).contains(&byte_val) {
                return Err(MachineError::IndexOutOfBounds {
                    index: byte_val,
                    length: 256,
                });
            }
            let mut result = vec![byte_val as u8];
            result.extend_from_slice(bs);
            Ok(Value::Constant(Constant::ByteString(result)))
        }
        SliceByteString => {
            let start = get_int(&args[0])?;
            let count = get_int(&args[1])?;
            let bs = get_bytestring(&args[2])?;
            // Plutus semantics: clamp to valid range silently.
            let len = bs.len() as i128;
            let s = start.max(0).min(len) as usize;
            let c = count.max(0).min(len - s as i128) as usize;
            Ok(Value::Constant(Constant::ByteString(bs[s..s + c].to_vec())))
        }
        LengthOfByteString => {
            let bs = get_bytestring(&args[0])?;
            Ok(Value::Constant(Constant::Integer(bs.len() as i128)))
        }
        IndexByteString => {
            let bs = get_bytestring(&args[0])?;
            let idx = get_int(&args[1])?;
            if idx < 0 || idx as usize >= bs.len() {
                return Err(MachineError::IndexOutOfBounds {
                    index: idx,
                    length: bs.len(),
                });
            }
            Ok(Value::Constant(Constant::Integer(i128::from(bs[idx as usize]))))
        }
        EqualsByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            Ok(Value::Constant(Constant::Bool(a == b)))
        }
        LessThanByteString => {
            let (a, b) = get_two_bytestrings(args)?;
            Ok(Value::Constant(Constant::Bool(a < b)))
        }

        // ---------------------------------------------------------------
        // Cryptographic hashing
        // ---------------------------------------------------------------
        Sha2_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = sha2_256_hash(bs);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }
        Sha3_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = sha3_256_hash(bs);
            Ok(Value::Constant(Constant::ByteString(hash.to_vec())))
        }
        Blake2b_256 => {
            let bs = get_bytestring(&args[0])?;
            let hash = blake2b::hash_bytes_256(bs);
            Ok(Value::Constant(Constant::ByteString(hash.0.to_vec())))
        }
        VerifyEd25519Signature => {
            let vkey = get_bytestring(&args[0])?;
            let msg = get_bytestring(&args[1])?;
            let sig = get_bytestring(&args[2])?;
            let valid = verify_ed25519(vkey, msg, sig);
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
            let _ = get_unit(&args[0])?;
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
            Ok(Value::Constant(Constant::ProtoList(list.0.clone(), new_list)))
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
            Ok(Value::Constant(Constant::Data(PlutusData::Constr(
                tag as u64,
                list,
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
            Ok(Value::Constant(Constant::Data(PlutusData::Integer(i))))
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
                    let tag_const = Constant::Integer(*tag as i128);
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
                PlutusData::Integer(i) => Ok(Value::Constant(Constant::Integer(*i))),
                _ => Err(MachineError::TypeMismatch {
                    expected: "Integer data",
                    actual: data_variant_name(data),
                }),
            }
        }
        UnBData => {
            let data = get_data(&args[0])?;
            match data {
                PlutusData::Bytes(bs) => {
                    Ok(Value::Constant(Constant::ByteString(bs.clone())))
                }
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
            let _ = get_unit(&args[0])?;
            Ok(Value::Constant(Constant::ProtoList(Type::Data, Vec::new())))
        }
        MkNilPairData => {
            let _ = get_unit(&args[0])?;
            Ok(Value::Constant(Constant::ProtoList(
                Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
                Vec::new(),
            )))
        }
        SerialiseData => {
            let data = get_data(&args[0])?;
            let mut enc = Encoder::new();
            data.encode_cbor(&mut enc);
            Ok(Value::Constant(Constant::ByteString(enc.into_bytes())))
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
        // PlutusV3 — BLS12-381, bitwise, extra hashing (not yet)
        // ---------------------------------------------------------------
        Bls12_381_G1_Add | Bls12_381_G1_Neg | Bls12_381_G1_ScalarMul
        | Bls12_381_G1_Equal | Bls12_381_G1_HashToGroup
        | Bls12_381_G1_Compress | Bls12_381_G1_Uncompress
        | Bls12_381_G2_Add | Bls12_381_G2_Neg | Bls12_381_G2_ScalarMul
        | Bls12_381_G2_Equal | Bls12_381_G2_HashToGroup
        | Bls12_381_G2_Compress | Bls12_381_G2_Uncompress
        | Bls12_381_MillerLoop | Bls12_381_MulMlResult
        | Bls12_381_FinalVerify => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
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
            Ok(Value::Constant(Constant::Integer(value)))
        }

        AndByteString => {
            // args: [padding_semantics, bs1, bs2]
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(a, b, pad, |x, y| x & y))))
        }
        OrByteString => {
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(a, b, pad, |x, y| x | y))))
        }
        XorByteString => {
            let pad = get_bool(&args[0])?;
            let a = get_bytestring(&args[1])?;
            let b = get_bytestring(&args[2])?;
            Ok(Value::Constant(Constant::ByteString(bitwise_binop(a, b, pad, |x, y| x ^ y))))
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
            if !(0..=8192).contains(&len) {
                return Err(MachineError::BuiltinError {
                    builtin: "replicateByte".into(),
                    message: format!("length out of range: {len}"),
                });
            }
            if !(0..=255).contains(&byte_val) {
                return Err(MachineError::BuiltinError {
                    builtin: "replicateByte".into(),
                    message: format!("byte value out of range: {byte_val}"),
                });
            }
            Ok(Value::Constant(Constant::ByteString(vec![byte_val as u8; len as usize])))
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
            let count: i128 = bs.iter().map(|b| i128::from(b.count_ones())).sum();
            Ok(Value::Constant(Constant::Integer(count)))
        }
        FindFirstSetBit => {
            let bs = get_bytestring(&args[0])?;
            let idx = find_first_set_bit(bs);
            Ok(Value::Constant(Constant::Integer(idx)))
        }

        ExpModInteger => {
            // args: [base, exponent, modulus]
            let base = get_int(&args[0])?;
            let exp = get_int(&args[1])?;
            let modulus = get_int(&args[2])?;
            if modulus == 0 {
                return Err(MachineError::DivisionByZero);
            }
            let result = exp_mod_integer(base, exp, modulus)?;
            Ok(Value::Constant(Constant::Integer(result)))
        }
    }
}

// ---------------------------------------------------------------------------
// Argument extraction helpers
// ---------------------------------------------------------------------------

fn get_int(val: &Value) -> Result<i128, MachineError> {
    match val.as_constant()? {
        Constant::Integer(i) => Ok(*i),
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

fn get_two_ints(args: &[Value]) -> Result<(i128, i128), MachineError> {
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
    op: impl FnOnce(i128, i128) -> Result<i128, MachineError>,
) -> Result<Value, MachineError> {
    let (a, b) = get_two_ints(args)?;
    Ok(Value::Constant(Constant::Integer(op(a, b)?)))
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
                        })
                    }
                };
                let vb = match b.as_ref() {
                    Constant::Data(d) => d.clone(),
                    other => {
                        return Err(MachineError::TypeMismatch {
                            expected: "data",
                            actual: constant_type_name(other),
                        })
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
    let sig = yggdrasil_crypto::ed25519::Signature::from_bytes(
        sig.try_into().expect("checked 64 bytes"),
    );
    vk.verify(msg, &sig).is_ok()
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
    required_len: i128,
    value: i128,
) -> Result<Vec<u8>, MachineError> {
    if value < 0 {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: "negative integer".into(),
        });
    }
    if required_len < 0 {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: format!("negative required length: {required_len}"),
        });
    }
    if required_len > 8192 {
        return Err(MachineError::BuiltinError {
            builtin: "integerToByteString".into(),
            message: format!("required length too large: {required_len}"),
        });
    }
    let req = required_len as usize;

    if value == 0 {
        return if req == 0 {
            Ok(vec![])
        } else {
            Ok(vec![0u8; req])
        };
    }

    // Convert to big-endian bytes.
    let big_endian = {
        let mut v = value;
        let mut bytes = Vec::new();
        while v > 0 {
            bytes.push((v & 0xFF) as u8);
            v >>= 8;
        }
        bytes.reverse();
        bytes
    };

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
fn bytestring_to_integer(little_endian: bool, bs: &[u8]) -> i128 {
    if bs.is_empty() {
        return 0;
    }
    let iter: Box<dyn Iterator<Item = &u8>> = if little_endian {
        Box::new(bs.iter().rev())
    } else {
        Box::new(bs.iter())
    };
    let mut result: i128 = 0;
    for &byte in iter {
        result = (result << 8) | i128::from(byte);
    }
    result
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
fn bitwise_binop(
    a: &[u8],
    b: &[u8],
    pad: bool,
    op: fn(u8, u8) -> u8,
) -> Vec<u8> {
    let (shorter, longer) = if a.len() <= b.len() {
        (a, b)
    } else {
        (b, a)
    };

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
fn read_bit(bs: &[u8], bit_index: i128) -> Result<bool, MachineError> {
    let total_bits = (bs.len() as i128) * 8;
    if bit_index < 0 || bit_index >= total_bits {
        return Err(MachineError::IndexOutOfBounds {
            index: bit_index,
            length: total_bits as usize,
        });
    }
    let byte_idx = bs.len() - 1 - (bit_index / 8) as usize;
    let bit_offset = (bit_index % 8) as u32;
    Ok((bs[byte_idx] >> bit_offset) & 1 == 1)
}

/// Write bits at specified indices.
///
/// Bit indexing: bit 0 is the LSB of the last byte.
fn write_bits(
    bs: &[u8],
    indices: &[i128],
    values: &[bool],
) -> Result<Vec<u8>, MachineError> {
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
    let total_bits = (bs.len() as i128) * 8;
    let mut result = bs.to_vec();
    for (&idx, &val) in indices.iter().zip(values.iter()) {
        if idx < 0 || idx >= total_bits {
            return Err(MachineError::IndexOutOfBounds {
                index: idx,
                length: total_bits as usize,
            });
        }
        let byte_idx = result.len() - 1 - (idx / 8) as usize;
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
fn shift_bytestring(bs: &[u8], shift: i128) -> Vec<u8> {
    if bs.is_empty() {
        return Vec::new();
    }
    let total_bits = bs.len() * 8;
    let abs_shift = shift.unsigned_abs() as usize;

    if abs_shift >= total_bits {
        return vec![0u8; bs.len()];
    }

    let byte_shift = abs_shift / 8;
    let bit_shift = abs_shift % 8;

    let mut result = vec![0u8; bs.len()];

    if shift > 0 {
        // Shift left: MSB direction.
        for i in 0..bs.len() {
            if i + byte_shift < bs.len() {
                result[i] = bs[i + byte_shift] << bit_shift;
                if bit_shift > 0 && i + byte_shift + 1 < bs.len() {
                    result[i] |= bs[i + byte_shift + 1] >> (8 - bit_shift);
                }
            }
        }
    } else if shift < 0 {
        // Shift right: LSB direction.
        for i in (0..bs.len()).rev() {
            if i >= byte_shift {
                result[i] = bs[i - byte_shift] >> bit_shift;
                if bit_shift > 0 && i > byte_shift {
                    result[i] |= bs[i - byte_shift - 1] << (8 - bit_shift);
                }
            }
        }
    } else {
        result.copy_from_slice(bs);
    }

    result
}

/// Rotate a byte string left (positive) or right (negative).
fn rotate_bytestring(bs: &[u8], rot: i128) -> Vec<u8> {
    if bs.is_empty() {
        return Vec::new();
    }
    let total_bits = (bs.len() * 8) as i128;
    // Normalize rotation to [0, total_bits).
    let effective = ((rot % total_bits) + total_bits) % total_bits;
    if effective == 0 {
        return bs.to_vec();
    }

    // Use shift-based rotation: rotate_left(n) = (bs << n) | (bs >> (total - n))
    let left = shift_bytestring(bs, effective);
    let right = shift_bytestring(bs, effective - total_bits);
    left.iter().zip(right.iter()).map(|(&a, &b)| a | b).collect()
}

/// Find the index of the lowest set bit (bit 0 = LSB of last byte).
///
/// Returns -1 if the byte string is empty or all zeros.
fn find_first_set_bit(bs: &[u8]) -> i128 {
    for (byte_idx_from_end, &byte) in bs.iter().rev().enumerate() {
        if byte != 0 {
            let bit_within_byte = byte.trailing_zeros() as i128;
            return (byte_idx_from_end as i128) * 8 + bit_within_byte;
        }
    }
    -1
}

/// Extract a list of integers from a ProtoList of Integers.
fn get_int_list(val: &Value) -> Result<Vec<i128>, MachineError> {
    let list = get_list(val)?;
    list.iter()
        .map(|c| match c {
            Constant::Integer(i) => Ok(*i),
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

/// Compute `base^exp mod modulus` for arbitrary-precision i128 values.
///
/// For negative exponents, returns 0 (matching upstream Plutus behavior
/// where modular inverse is not supported and negative exponents yield 0).
///
/// Reference: CIP-0109 / Plutus `expModInteger`.
fn exp_mod_integer(base: i128, exp: i128, modulus: i128) -> Result<i128, MachineError> {
    if modulus == 0 {
        return Err(MachineError::DivisionByZero);
    }
    if exp < 0 {
        // Upstream Plutus: negative exponent → error (we follow the spec).
        return Err(MachineError::BuiltinError {
            builtin: "expModInteger".into(),
            message: "negative exponent".into(),
        });
    }
    let m = modulus.unsigned_abs();
    if m == 1 {
        return Ok(0);
    }

    // Normalize base to [0, m).
    let mut result: u128 = 1;
    let mut b = ((base % modulus) + modulus) as u128 % m;
    let mut e = exp as u128;

    while e > 0 {
        if e & 1 == 1 {
            result = mul_mod(result, b, m);
        }
        e >>= 1;
        b = mul_mod(b, b, m);
    }

    Ok(result as i128)
}

/// Multiply two u128 values modulo m, avoiding overflow.
fn mul_mod(a: u128, b: u128, m: u128) -> u128 {
    // For values that fit in u64, use direct multiplication.
    if a <= u64::MAX as u128 && b <= u64::MAX as u128 {
        return (a * b) % m;
    }
    // Fallback: Russian peasant multiplication to avoid u128 overflow.
    let mut result: u128 = 0;
    let mut a = a % m;
    let mut b = b % m;
    while b > 0 {
        if b & 1 == 1 {
            result = (result + a) % m;
        }
        a = (a << 1) % m;
        b >>= 1;
    }
    result
}
