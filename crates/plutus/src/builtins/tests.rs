// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

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

fn eval_with_model(fun: DefaultFun, args: &[Value], cm: &CostModel) -> Result<Value, MachineError> {
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
//
// Phase 5.4: `int_to_scalar_bytes` is a BLS-only helper, gated behind
// the `bls12-381` Cargo feature — these direct-call tests carry the
// same gate so `cargo test --no-default-features` still compiles.
// ===================================================================

#[cfg(feature = "bls12-381")]
#[test]
fn int_to_scalar_bytes_zero() {
    let (bytes, neg) = int_to_scalar_bytes(0);
    assert_eq!(bytes, vec![0]);
    assert!(!neg);
}

#[cfg(feature = "bls12-381")]
#[test]
fn int_to_scalar_bytes_positive() {
    let (bytes, neg) = int_to_scalar_bytes(256);
    assert!(!neg);
    assert_eq!(bytes, vec![1, 0]); // 256 big-endian
}

#[cfg(feature = "bls12-381")]
#[test]
fn int_to_scalar_bytes_negative() {
    let (bytes, neg) = int_to_scalar_bytes(-42);
    assert!(neg);
    assert_eq!(bytes, vec![42]);
}

#[cfg(feature = "bls12-381")]
#[test]
fn int_to_scalar_bytes_min() {
    let (_, neg) = int_to_scalar_bytes(i128::MIN);
    assert!(neg);
}
