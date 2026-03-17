//! UPLC (Untyped Plutus Lambda Calculus) evaluator for Cardano scripts.
//!
//! This crate implements a pure-Rust CEK machine that evaluates Plutus
//! scripts as used on the Cardano blockchain. It supports:
//!
//! - **Flat decoding**: parse on-chain script bytes into a UPLC `Program`
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
//! - [`flat`] — Flat binary format decoder for on-chain scripts
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
pub use cost_model::{CostModel, CostModelError};
pub use error::MachineError;
pub use flat::{decode_flat_program, decode_script_bytes};
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

/// Decode and evaluate a Plutus script from on-chain CBOR-wrapped bytes.
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
