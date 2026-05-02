#![cfg_attr(test, allow(clippy::unwrap_used))]
//! UPLC (Untyped Plutus Lambda Calculus) evaluator for Cardano scripts.
//!
//! This crate implements a pure-Rust CEK machine that evaluates Plutus
//! scripts as used on the Cardano blockchain. It supports:
//!
//! - **PlutusBinary decoding**: parse on-chain script bytes into a UPLC `Program`
//! - **CEK evaluation**: evaluate terms with de Bruijn indices, closures,
//!   and partial application of built-in functions
//! - **All PlutusV1 builtins**: integer, bytestring, string, bool, list,
//!   pair, data, crypto (SHA-256, Blake2b-256, Ed25519), and tracing
//! - **Budget tracking**: CPU/memory cost accounting with configurable models
//!
//! ## Quick Start
//!
//! ```rust
//! use yggdrasil_plutus::{evaluate_term, Term, Constant, Value, ExBudget, CostModel};
//!
//! // Build a simple program: (\x -> x) 42
//! let term = Term::Apply(
//!     Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
//!     Box::new(Term::Constant(Constant::Integer(42))),
//! );
//!
//! let (result, _logs) = evaluate_term(
//!     term,
//!     ExBudget::new(10_000_000, 10_000_000),
//!     CostModel::default(),
//! ).expect("evaluation should succeed");
//!
//! match result {
//!     Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
//!     _ => panic!("unexpected result"),
//! }
//! ```
//!
//! ## Architecture
//!
//! - [`types`] — UPLC term language, constants, builtins, runtime values
//! - [`flat`] — PlutusBinary and Flat binary format decoder for on-chain scripts
//! - [`machine`] — CEK machine evaluator
//! - [`builtins`] — Built-in function implementations
//! - [`cost_model`] — Execution budget and cost tracking
//! - [`error`] — Error types
//!
//! Reference: <https://github.com/IntersectMBO/plutus>

pub mod builtins;
pub mod cost_model;
pub mod error;
pub mod flat;
pub mod machine;
pub mod types;

// Re-exports for convenience.
pub use cost_model::{BuiltinSemanticsVariant, CostModel, CostModelError};
pub use error::MachineError;
pub use flat::{decode_flat_program, decode_script_bytes, decode_script_bytes_allowing_remainder};
pub use machine::CekMachine;
pub use types::{Constant, DefaultFun, ExBudget, Program, Term, Type, Value};

/// Evaluate a UPLC term with the given budget and cost model.
///
/// Returns the final value and any trace log messages.
pub fn evaluate_term(
    term: Term,
    budget: ExBudget,
    cost_model: CostModel,
) -> Result<(Value, Vec<String>), MachineError> {
    let mut machine = CekMachine::new(budget, cost_model);
    let result = machine.evaluate(term)?;
    Ok((result, machine.logs))
}

/// Evaluate a UPLC program with the given budget and cost model.
///
/// Returns the final value and any trace log messages.
pub fn evaluate_program(
    program: Program,
    budget: ExBudget,
    cost_model: CostModel,
) -> Result<(Value, Vec<String>), MachineError> {
    evaluate_term(program.term, budget, cost_model)
}

/// Decode and evaluate a Plutus script from raw on-chain `PlutusBinary` bytes.
///
/// This is the primary entry point for evaluating scripts extracted from
/// transaction witness sets.
pub fn evaluate_script(
    script_bytes: &[u8],
    budget: ExBudget,
    cost_model: CostModel,
) -> Result<(Value, Vec<String>), MachineError> {
    let program = decode_script_bytes(script_bytes)?;
    evaluate_program(program, budget, cost_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big_budget() -> ExBudget {
        ExBudget::new(10_000_000, 10_000_000)
    }

    // -- evaluate_term -------------------------------------------------

    #[test]
    fn evaluate_term_constant() {
        let (val, logs) = evaluate_term(
            Term::Constant(Constant::Integer(99)),
            big_budget(),
            CostModel::default(),
        )
        .unwrap();
        assert!(matches!(val, Value::Constant(Constant::Integer(99))));
        assert!(logs.is_empty());
    }

    #[test]
    fn evaluate_term_identity_application() {
        let term = Term::Apply(
            Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
            Box::new(Term::Constant(Constant::Bool(true))),
        );
        let (val, _) = evaluate_term(term, big_budget(), CostModel::default()).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Bool(true))));
    }

    #[test]
    fn evaluate_term_trace_logs() {
        // (force trace) "hello" ()
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::Trace)))),
                Box::new(Term::Constant(Constant::String("hello".into()))),
            )),
            Box::new(Term::Constant(Constant::Unit)),
        );
        let (_, logs) = evaluate_term(term, big_budget(), CostModel::default()).unwrap();
        assert_eq!(logs, vec!["hello".to_string()]);
    }

    #[test]
    fn evaluate_term_error() {
        let err = evaluate_term(Term::Error, big_budget(), CostModel::default());
        assert!(err.is_err());
    }

    #[test]
    fn evaluate_term_budget_exhaustion() {
        let tiny = ExBudget::new(1, 1);
        // Apply requires several steps, should exhaust quickly.
        let term = Term::Apply(
            Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
            Box::new(Term::Constant(Constant::Integer(1))),
        );
        let err = evaluate_term(term, tiny, CostModel::default());
        assert!(err.is_err());
    }

    // -- evaluate_program ----------------------------------------------

    #[test]
    fn evaluate_program_basic() {
        let prog = Program {
            major: 1,
            minor: 0,
            patch: 0,
            term: Term::Constant(Constant::Integer(42)),
        };
        let (val, _) = evaluate_program(prog, big_budget(), CostModel::default()).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Integer(42))));
    }

    #[test]
    fn evaluate_program_add_integers() {
        let add = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::AddInteger)),
                Box::new(Term::Constant(Constant::Integer(3))),
            )),
            Box::new(Term::Constant(Constant::Integer(7))),
        );
        let prog = Program {
            major: 1,
            minor: 0,
            patch: 0,
            term: add,
        };
        let (val, _) = evaluate_program(prog, big_budget(), CostModel::default()).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Integer(10))));
    }

    // -- evaluate_script -----------------------------------------------

    #[test]
    fn evaluate_script_error_term() {
        // Build flat program: version 1.0.0, body = Error.
        let flat_bytes: Vec<u8> = vec![0x01, 0x00, 0x00, 0x60];
        // Single CBOR wrap.
        let mut cbor = vec![0x44u8]; // 4-byte bytestring
        cbor.extend_from_slice(&flat_bytes);

        let result = evaluate_script(&cbor, big_budget(), CostModel::default());
        assert!(result.is_err());
    }

    #[test]
    fn evaluate_script_constant_unit() {
        // Build flat program: version 1.0.0, body = Constant Unit.
        // Constant tag=4 (0100), type tag list [Unit=3].
        let flat_bytes: Vec<u8> = vec![0x01, 0x00, 0x00, 0x49, 0x80];
        // Single CBOR wrap.
        let mut cbor = vec![0x45u8]; // 5-byte bytestring
        cbor.extend_from_slice(&flat_bytes);

        let (val, _) = evaluate_script(&cbor, big_budget(), CostModel::default()).unwrap();
        assert!(matches!(val, Value::Constant(Constant::Unit)));
    }

    // -- Re-exports exist ----------------------------------------------

    #[test]
    fn reexports_are_accessible() {
        // Confirm the re-exports compile.
        let _budget = ExBudget::default();
        let _model = CostModel::default();
        let _err_variant = MachineError::EvaluationFailure;
        let _fun = DefaultFun::AddInteger;
        let _typ = Type::Integer;
    }
}
