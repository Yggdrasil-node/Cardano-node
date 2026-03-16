//! Built-in function implementations for the UPLC evaluator.
//!
//! All PlutusV1 builtins are implemented. PlutusV2/V3-only builtins
//! (secp256k1 signatures, BLS12-381, bitwise) return
//! `MachineError::UnimplementedBuiltin` until their dependencies are added.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>

use yggdrasil_crypto::blake2b;
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
            // SHA3-256 not yet available in our crypto crate.
            Err(MachineError::UnimplementedBuiltin("sha3_256".into()))
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
        // PlutusV2 — secp256k1 (not yet implemented)
        // ---------------------------------------------------------------
        VerifyEcdsaSecp256k1Signature | VerifySchnorrSecp256k1Signature => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
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

        Keccak_256 | Ripemd_160 => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
        }

        Blake2b_224 => {
            let bs = get_bytestring(&args[0])?;
            let hash = blake2b::hash_bytes_224(bs);
            Ok(Value::Constant(Constant::ByteString(hash.0.to_vec())))
        }

        IntegerToByteString | ByteStringToInteger => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
        }

        AndByteString | OrByteString | XorByteString | ComplementByteString
        | ReadBit | WriteBits | ReplicateByte | ShiftByteString
        | RotateByteString | CountSetBits | FindFirstSetBit => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
        }

        ExpModInteger => {
            Err(MachineError::UnimplementedBuiltin(fun.name().into()))
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
    let vk = yggdrasil_crypto::ed25519::VerificationKey::from_bytes(
        vkey.try_into().unwrap(),
    );
    let sig = yggdrasil_crypto::ed25519::Signature::from_bytes(
        sig.try_into().unwrap(),
    );
    vk.verify(msg, &sig).is_ok()
}
