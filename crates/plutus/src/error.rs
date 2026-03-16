//! Error types for UPLC evaluation.

use thiserror::Error;

/// Errors that can occur during UPLC program evaluation.
#[derive(Debug, Error)]
pub enum MachineError {
    /// Execution exceeded the allotted CPU or memory budget.
    #[error("out of budget: {0}")]
    OutOfBudget(String),

    /// Builtin received argument of unexpected type.
    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        expected: &'static str,
        actual: String,
    },

    /// Error in a built-in function.
    #[error("builtin {builtin}: {message}")]
    BuiltinError { builtin: String, message: String },

    /// De Bruijn index refers to non-existent binding.
    #[error("unbound variable: index {0}")]
    UnboundVariable(u64),

    /// Attempted `Apply` on a non-function value.
    #[error("non-function application")]
    NonFunctionApplication,

    /// Attempted `Force` on a non-polymorphic / non-delayed value.
    #[error("non-polymorphic force")]
    NonPolymorphicForce,

    /// The program explicitly called `Error`.
    #[error("evaluation failure (user error)")]
    EvaluationFailure,

    /// Builtin has not been implemented yet.
    #[error("unimplemented builtin: {0}")]
    UnimplementedBuiltin(String),

    /// Division or modulus by zero.
    #[error("division by zero")]
    DivisionByZero,

    /// Integer value exceeded i128 range.
    #[error("integer overflow")]
    IntegerOverflow,

    /// Index into byte string or list was out of range.
    #[error("index out of bounds: {index} (length {length})")]
    IndexOutOfBounds { index: i128, length: usize },

    /// Operation on an empty list.
    #[error("empty list")]
    EmptyList,

    /// Byte string was not valid UTF-8.
    #[error("invalid UTF-8")]
    InvalidUtf8,

    /// Constructor tag not matched by any case branch.
    #[error("unexpected constructor tag {tag}, only {branches} branches")]
    UnexpectedConstructorTag { tag: u64, branches: usize },

    /// Flat binary decoding error.
    #[error("flat decode: {0}")]
    FlatDecodeError(String),
}
