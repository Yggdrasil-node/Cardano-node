//! UPLC (Untyped Plutus Lambda Calculus) types.
//!
//! Defines the term language, constant values, built-in function enumeration,
//! runtime values, and evaluation environment for the CEK machine.
//!
//! Sub-modules:
//!
//! - [`term`] — `Program`, `Term`, `Type`, `Constant`.
//! - [`default_fun`] — `DefaultFun` (built-in operation enumeration).
//! - [`runtime`] — `ExBudget`, `Value`, `Environment`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core>

pub mod cek_internal;
pub mod core_type;
pub mod default_builtins;

pub use cek_internal::{Environment, ExBudget, Value};
pub use core_type::{Constant, Program, Term, Type};
pub use default_builtins::DefaultFun;

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MachineError;
    use num_bigint::BigInt;
    use yggdrasil_ledger::plutus::PlutusData;

    // -- Program ----------------------------------------------------------

    #[test]
    fn program_clone_and_eq() {
        let p = Program {
            major: 1,
            minor: 1,
            patch: 0,
            term: Term::Constant(Constant::integer(42)),
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
        let a = Program {
            major: 1,
            minor: 0,
            patch: 0,
            term: Term::Error,
        };
        let b = Program {
            major: 2,
            minor: 0,
            patch: 0,
            term: Term::Error,
        };
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
        let a = Term::Constant(Constant::integer(10));
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
        let t = Term::Constant(Constant::integer(i128::MAX));
        assert_eq!(t, Term::Constant(Constant::integer(i128::MAX)));
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
        let t = Term::Constr(
            1,
            vec![
                Term::Constant(Constant::integer(1)),
                Term::Constant(Constant::integer(2)),
            ],
        );
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
        let branch = Term::Constant(Constant::integer(42));
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
        assert_eq!(
            t.clone(),
            Type::Pair(Box::new(Type::Integer), Box::new(Type::ByteString))
        );
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
        assert_eq!(Constant::integer(0), Constant::integer(0));
        assert_ne!(Constant::integer(1), Constant::integer(2));
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
        assert_eq!(
            Constant::String("abc".into()),
            Constant::String("abc".into())
        );
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
            vec![Constant::integer(1), Constant::integer(2)],
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
            Box::new(Constant::integer(42)),
            Box::new(Constant::ByteString(vec![1])),
        );
        if let Constant::ProtoPair(t1, t2, a, b) = &c {
            assert_eq!(*t1, Type::Integer);
            assert_eq!(*t2, Type::ByteString);
            assert_eq!(**a, Constant::integer(42));
            assert_eq!(**b, Constant::ByteString(vec![1]));
        } else {
            panic!("expected ProtoPair");
        }
    }

    #[test]
    fn constant_data() {
        let d = PlutusData::integer(99);
        let c = Constant::Data(d.clone());
        assert_eq!(c, Constant::Data(PlutusData::integer(99)));
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
        assert_eq!(
            DefaultFun::LessThanEqualsByteString.name(),
            "lessThanEqualsByteString"
        );
        assert_eq!(DefaultFun::IfThenElse.name(), "ifThenElse");
        assert_eq!(DefaultFun::HeadList.name(), "headList");
        assert_eq!(DefaultFun::ConstrData.name(), "constrData");
        assert_eq!(DefaultFun::EqualsData.name(), "equalsData");
        assert_eq!(
            DefaultFun::VerifyEcdsaSecp256k1Signature.name(),
            "verifyEcdsaSecp256k1Signature"
        );
        assert_eq!(DefaultFun::Bls12_381_G1_Add.name(), "bls12_381_G1_add");
        assert_eq!(DefaultFun::Keccak_256.name(), "keccak_256");
        assert_eq!(
            DefaultFun::IntegerToByteString.name(),
            "integerToByteString"
        );
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
        let v = Value::Constant(Constant::integer(1));
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
        let v = Value::Constant(Constant::integer(42));
        let c = v.as_constant().unwrap();
        assert_eq!(*c, Constant::integer(42));
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
        let env = env.extend(Value::Constant(Constant::integer(10)));
        let val = env.lookup(1).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Integer(n)) if n == &BigInt::from(10)));
    }

    #[test]
    fn env_debruijn_ordering() {
        // Index 1 = most recent, 2 = next, etc.
        let env = Environment::new();
        let env = env.extend(Value::Constant(Constant::integer(1)));
        let env = env.extend(Value::Constant(Constant::integer(2)));
        let env = env.extend(Value::Constant(Constant::integer(3)));

        // Index 1 = most recent = 3
        if let Value::Constant(Constant::Integer(n)) = env.lookup(1).unwrap() {
            assert_eq!(*n, BigInt::from(3));
        } else {
            panic!("expected integer");
        }

        // Index 2 = 2
        if let Value::Constant(Constant::Integer(n)) = env.lookup(2).unwrap() {
            assert_eq!(*n, BigInt::from(2));
        } else {
            panic!("expected integer");
        }

        // Index 3 = oldest = 1
        if let Value::Constant(Constant::Integer(n)) = env.lookup(3).unwrap() {
            assert_eq!(*n, BigInt::from(1));
        } else {
            panic!("expected integer");
        }
    }

    #[test]
    fn env_lookup_zero_is_error() {
        let env = Environment::new().extend(Value::Constant(Constant::Unit));
        assert!(env.lookup(0).is_err());
    }

    #[test]
    fn env_lookup_out_of_range() {
        let env = Environment::new().extend(Value::Constant(Constant::Unit));
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
        let env1 = Environment::new().extend(Value::Constant(Constant::integer(1)));
        let env2 = env1.extend(Value::Constant(Constant::integer(2)));

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
            env = env.extend(Value::Constant(Constant::integer(i)));
        }
        // Index 1 = 99, Index 100 = 0.
        if let Value::Constant(Constant::Integer(n)) = env.lookup(1).unwrap() {
            assert_eq!(*n, BigInt::from(99));
        } else {
            panic!("expected integer");
        }
        if let Value::Constant(Constant::Integer(n)) = env.lookup(100).unwrap() {
            assert_eq!(*n, BigInt::from(0));
        } else {
            panic!("expected integer");
        }
    }

    // -- Constant::ProtoList nested -----------------------------------------

    #[test]
    fn constant_proto_list_of_pairs() {
        let c = Constant::ProtoList(
            Type::Pair(Box::new(Type::Data), Box::new(Type::Data)),
            vec![Constant::ProtoPair(
                Type::Data,
                Type::Data,
                Box::new(Constant::Data(PlutusData::integer(1))),
                Box::new(Constant::Data(PlutusData::integer(2))),
            )],
        );
        if let Constant::ProtoList(ty, items) = &c {
            assert_eq!(*ty, Type::Pair(Box::new(Type::Data), Box::new(Type::Data)));
            assert_eq!(items.len(), 1);
        } else {
            panic!("expected ProtoList");
        }
    }

    // -- DefaultFun drift guards -------------------------------------------
    //
    // These tests pin the canonical `DefaultFun` set against silent drift
    // between the discriminant assignments (lines ~134..234), the
    // `from_tag` cascade, and the `all()` helper. A mismatch in any of
    // these three independent hand-written statements would corrupt
    // on-chain Flat decoding (`unknown builtin tag` for a real builtin,
    // OR — worse — a tag mapping to the wrong builtin and silently
    // executing an unintended on-chain operation).
    //
    // Reference: `PlutusCore.Default.Builtins.DefaultFun` ordering.

    #[test]
    fn default_fun_all_covers_every_tag_in_canonical_order() {
        // Pin the slice length to the current upstream surface (88 builtins:
        // 52 base, 2 V2 secp, 17 BLS12-381, 2 extra hashes, 2 conversions,
        // 11 bitwise, 1 modular). Adding a builtin upstream MUST extend
        // `all()` AND the discriminant list AND `from_tag` — this test
        // catches the case where someone adds a new variant but forgets
        // to extend `all()`.
        let all = DefaultFun::all();
        assert_eq!(
            all.len(),
            88,
            "DefaultFun::all() must cover every variant (88 as of PlutusV3 + Plomin)",
        );

        // Cross-assert each `all()[i] as u8 == i`. A copy-paste reorder in
        // `all()` (e.g. accidentally swapping two entries) fails here with
        // the offending index — and because the discriminant ordering
        // matches the on-chain Flat tag ordering, this test guards the
        // correctness of the slice as an iteration surface for future
        // drift-guard tests (cost-model coverage, arity coverage, etc.).
        for (i, &v) in all.iter().enumerate() {
            assert_eq!(
                v as u8, i as u8,
                "DefaultFun::all()[{i}] = {v:?} but its discriminant is {}",
                v as u8,
            );
        }
    }

    #[test]
    fn default_fun_from_tag_round_trips_for_every_variant() {
        // For every variant `v` in `all()`, `from_tag(v as u8)` must
        // return `Ok(v)`. This catches typos in the `from_tag` cascade
        // (e.g. `60 => Ok(Self::Bls12_381_G1_Compress)` accidentally
        // typed as `60 => Ok(Self::Bls12_381_G1_Uncompress)`) — the
        // worst-case bug because handshake-level decoding succeeds but
        // the script silently executes the wrong builtin.
        for &v in DefaultFun::all() {
            let tag = v as u8;
            let decoded = DefaultFun::from_tag(tag).unwrap_or_else(|e| {
                panic!("from_tag({tag}) failed for {v:?}: {e:?}");
            });
            assert_eq!(
                decoded, v,
                "from_tag({tag}) returned {decoded:?}, expected {v:?}",
            );
        }
    }

    #[test]
    fn default_fun_from_tag_rejects_tags_outside_canonical_range() {
        // First out-of-range tag (one past `ExpModInteger = 87`) and the
        // saturated `u8::MAX` must both fail with a `FlatDecodeError`,
        // not silently map to a placeholder. A future builtin slot at
        // tag 88 will require extending `all()` AND `from_tag` — at
        // which point this test should be updated to bump the rejection
        // boundary, surfacing the new contract explicitly.
        let next = DefaultFun::all().len() as u8;
        assert_eq!(next, 88, "next-unused tag should be one past last variant");

        for bogus in [next, 100, 200, u8::MAX] {
            let err =
                DefaultFun::from_tag(bogus).expect_err(&format!("tag {bogus} must be rejected"));
            match err {
                MachineError::FlatDecodeError(msg) => assert!(
                    msg.contains(&format!("unknown builtin tag {bogus}")),
                    "rejection message must name the offending tag, got: {msg}",
                ),
                other => panic!("expected FlatDecodeError, got {other:?}"),
            }
        }
    }
}
