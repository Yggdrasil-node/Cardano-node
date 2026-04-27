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

    /// A builtin received a Force when it expected an argument, or vice-versa.
    ///
    /// Upstream: `BuiltinTermArgumentExpectedMachineError`.
    /// All type forces must be applied before any value arguments.
    #[error("builtin expected {expected} but received {received}")]
    BuiltinTermArgumentExpected {
        expected: &'static str,
        received: &'static str,
    },

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

    /// `Case` scrutinee did not reduce to a `Constr` value.
    ///
    /// Upstream: `NonConstrScrutinizedMachineError`.
    #[error("non-constr scrutinized")]
    NonConstrScrutinized,

    /// Flat binary decoding error.
    #[error("flat decode: {0}")]
    FlatDecodeError(String),

    /// Cryptographic operation failed (e.g. invalid BLS point).
    #[error("crypto error: {0}")]
    CryptoError(String),

    /// Cost model is missing an entry for a builtin invoked at runtime.
    ///
    /// Upstream cost models always cover every builtin available at the
    /// active language version; a missing entry indicates an incomplete or
    /// malformed cost model rather than a script-level failure. Surfaced as
    /// a structural error so it cannot be collapsed to opaque
    /// `EvaluationFailure`.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Plutus.CostModels` —
    /// `mkCostModel` requires complete coverage of `DefaultFun`.
    #[error("cost model missing entry for builtin: {0}")]
    MissingBuiltinCost(String),
}

impl MachineError {
    /// Whether this is an *operational* error (runtime failure inside a
    /// well-formed program) rather than a *structural* error (malformed
    /// program or budget exhaustion).
    ///
    /// Upstream collapses operational errors to opaque `EvaluationFailure`
    /// when reporting to the ledger, preventing internal details from leaking.
    pub fn is_operational(&self) -> bool {
        matches!(
            self,
            Self::TypeMismatch { .. }
                | Self::BuiltinError { .. }
                | Self::DivisionByZero
                | Self::IntegerOverflow
                | Self::IndexOutOfBounds { .. }
                | Self::EmptyList
                | Self::InvalidUtf8
                | Self::CryptoError(_)
                | Self::NonConstrScrutinized
                | Self::NonFunctionApplication
                | Self::NonPolymorphicForce
                | Self::BuiltinTermArgumentExpected { .. }
                | Self::UnexpectedConstructorTag { .. }
                | Self::EvaluationFailure
        )
    }

    /// Collapse an operational error to the opaque `EvaluationFailure`
    /// variant, matching upstream's opacity guarantee.
    ///
    /// Structural errors (budget exhaustion, unbound variables, decode
    /// failures) are returned unchanged.
    pub fn into_ledger_error(self) -> Self {
        if self.is_operational() {
            Self::EvaluationFailure
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_budget_display() {
        let e = MachineError::OutOfBudget("cpu=-5, mem=10".into());
        let msg = format!("{e}");
        assert!(msg.contains("budget"));
        assert!(msg.contains("cpu=-5"));
    }

    #[test]
    fn type_mismatch_display() {
        let e = MachineError::TypeMismatch {
            expected: "integer",
            actual: "bytestring".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("integer"));
        assert!(msg.contains("bytestring"));
    }

    #[test]
    fn builtin_error_display() {
        let e = MachineError::BuiltinError {
            builtin: "addInteger".into(),
            message: "overflow".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("addInteger"));
        assert!(msg.contains("overflow"));
    }

    #[test]
    fn unbound_variable_display() {
        let e = MachineError::UnboundVariable(42);
        let msg = format!("{e}");
        assert!(msg.contains("42"));
    }

    #[test]
    fn non_function_application_display() {
        let e = MachineError::NonFunctionApplication;
        let msg = format!("{e}");
        assert!(msg.contains("non-function"));
    }

    #[test]
    fn non_polymorphic_force_display() {
        let e = MachineError::NonPolymorphicForce;
        let msg = format!("{e}");
        assert!(msg.contains("non-polymorphic"));
    }

    #[test]
    fn evaluation_failure_display() {
        let e = MachineError::EvaluationFailure;
        let msg = format!("{e}");
        assert!(msg.contains("evaluation failure"));
    }

    #[test]
    fn unimplemented_builtin_display() {
        let e = MachineError::UnimplementedBuiltin("fooBar".into());
        let msg = format!("{e}");
        assert!(msg.contains("fooBar"));
    }

    #[test]
    fn division_by_zero_display() {
        let e = MachineError::DivisionByZero;
        let msg = format!("{e}");
        assert!(msg.contains("division by zero"));
    }

    #[test]
    fn integer_overflow_display() {
        let e = MachineError::IntegerOverflow;
        let msg = format!("{e}");
        assert!(msg.contains("integer overflow"));
    }

    #[test]
    fn index_out_of_bounds_display() {
        let e = MachineError::IndexOutOfBounds {
            index: 10,
            length: 5,
        };
        let msg = format!("{e}");
        assert!(msg.contains("10"));
        assert!(msg.contains("5"));
    }

    #[test]
    fn empty_list_display() {
        let e = MachineError::EmptyList;
        let msg = format!("{e}");
        assert!(msg.contains("empty list"));
    }

    #[test]
    fn invalid_utf8_display() {
        let e = MachineError::InvalidUtf8;
        let msg = format!("{e}");
        assert!(msg.contains("UTF-8"));
    }

    #[test]
    fn unexpected_constructor_tag_display() {
        let e = MachineError::UnexpectedConstructorTag {
            tag: 5,
            branches: 3,
        };
        let msg = format!("{e}");
        assert!(msg.contains("5"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn flat_decode_error_display() {
        let e = MachineError::FlatDecodeError("bad bits".into());
        let msg = format!("{e}");
        assert!(msg.contains("bad bits"));
    }

    #[test]
    fn crypto_error_display() {
        let e = MachineError::CryptoError("invalid point".into());
        let msg = format!("{e}");
        assert!(msg.contains("invalid point"));
    }

    #[test]
    fn missing_builtin_cost_display() {
        // Structural error emitted when the active cost model lacks a
        // requested builtin. Upstream enforces cost-model completeness at
        // construction time, so reaching this in Yggdrasil indicates an
        // incomplete local CostModel mapping — the operator needs to see
        // WHICH builtin is missing.
        let e = MachineError::MissingBuiltinCost("bls12_381_G1_neg".into());
        let msg = format!("{e}");
        assert!(msg.contains("cost model"), "rule name: {msg}");
        assert!(
            msg.contains("bls12_381_G1_neg"),
            "must name the builtin: {msg}"
        );
    }

    #[test]
    fn non_constr_scrutinized_display() {
        let e = MachineError::NonConstrScrutinized;
        let msg = format!("{e}");
        assert!(msg.contains("non-constr"));
    }

    #[test]
    fn builtin_term_argument_expected_display() {
        let e = MachineError::BuiltinTermArgumentExpected {
            expected: "term argument",
            received: "type force",
        };
        let msg = format!("{e}");
        assert!(msg.contains("term argument"), "must name expected: {msg}");
        assert!(msg.contains("type force"), "must name received: {msg}");
    }

    #[test]
    fn error_is_debug() {
        let e = MachineError::DivisionByZero;
        let dbg = format!("{:?}", e);
        assert!(dbg.contains("DivisionByZero"));
    }

    #[test]
    fn error_implements_std_error() {
        let e = MachineError::DivisionByZero;
        let _: &dyn std::error::Error = &e;
    }

    // ---- is_operational / into_ledger_error ----

    #[test]
    fn operational_errors_are_classified_correctly() {
        let ops = vec![
            MachineError::TypeMismatch {
                expected: "Bool",
                actual: "Int".into(),
            },
            MachineError::BuiltinError {
                builtin: "AddInteger".into(),
                message: "x".into(),
            },
            MachineError::DivisionByZero,
            MachineError::IntegerOverflow,
            MachineError::IndexOutOfBounds {
                index: 0,
                length: 0,
            },
            MachineError::EmptyList,
            MachineError::InvalidUtf8,
            MachineError::CryptoError("bad".into()),
            MachineError::NonConstrScrutinized,
            MachineError::NonFunctionApplication,
            MachineError::NonPolymorphicForce,
            MachineError::BuiltinTermArgumentExpected {
                expected: "term argument",
                received: "type force",
            },
            MachineError::UnexpectedConstructorTag {
                tag: 0,
                branches: 0,
            },
            MachineError::EvaluationFailure,
        ];
        for e in &ops {
            assert!(e.is_operational(), "{e:?} should be operational");
        }
    }

    #[test]
    fn structural_errors_are_classified_correctly() {
        let structs = vec![
            MachineError::OutOfBudget("cpu overrun".into()),
            MachineError::UnboundVariable(42),
            MachineError::FlatDecodeError("trailing bits".into()),
            // Missing cost-model entries are structural (malformed cost
            // model, not runtime script failure) — upstream enforces this
            // at cost-model construction via `mkCostModel` completeness.
            MachineError::MissingBuiltinCost("bls12_381_G1_neg".into()),
        ];
        for e in &structs {
            assert!(!e.is_operational(), "{e:?} should be structural");
        }
    }

    /// Exhaustiveness drift guard: every `MachineError` variant must have
    /// an explicit operational/structural classification decision. The
    /// exhaustive `match` below forces any new variant to receive an
    /// explicit classification call — without this, a new variant would
    /// silently default to "structural" via the `matches!` fall-through
    /// in `is_operational`, which could let operational errors (expected
    /// to collapse to opaque `EvaluationFailure` for ledger safety) leak
    /// their internal diagnostic through the outer error surface.
    #[test]
    fn every_machine_error_variant_has_explicit_operational_decision() {
        // Representative of every variant. The match guarantees compile-
        // time exhaustiveness; each arm hard-codes the expected boolean.
        // A classification drift between this map and `is_operational`
        // will fail assertion and name the mismatched variant.
        let all: Vec<MachineError> = vec![
            MachineError::OutOfBudget("x".into()),
            MachineError::TypeMismatch {
                expected: "a",
                actual: "b".into(),
            },
            MachineError::BuiltinError {
                builtin: "x".into(),
                message: "y".into(),
            },
            MachineError::UnboundVariable(0),
            MachineError::NonFunctionApplication,
            MachineError::NonPolymorphicForce,
            MachineError::BuiltinTermArgumentExpected {
                expected: "a",
                received: "b",
            },
            MachineError::EvaluationFailure,
            MachineError::UnimplementedBuiltin("x".into()),
            MachineError::DivisionByZero,
            MachineError::IntegerOverflow,
            MachineError::IndexOutOfBounds {
                index: 0,
                length: 0,
            },
            MachineError::EmptyList,
            MachineError::InvalidUtf8,
            MachineError::UnexpectedConstructorTag {
                tag: 0,
                branches: 0,
            },
            MachineError::NonConstrScrutinized,
            MachineError::FlatDecodeError("x".into()),
            MachineError::CryptoError("x".into()),
            MachineError::MissingBuiltinCost("x".into()),
        ];
        for e in &all {
            // Exhaustive match forces explicit classification of new variants.
            let expected_op = match e {
                MachineError::OutOfBudget(_) => false,
                MachineError::TypeMismatch { .. } => true,
                MachineError::BuiltinError { .. } => true,
                MachineError::UnboundVariable(_) => false,
                MachineError::NonFunctionApplication => true,
                MachineError::NonPolymorphicForce => true,
                MachineError::BuiltinTermArgumentExpected { .. } => true,
                MachineError::EvaluationFailure => true,
                MachineError::UnimplementedBuiltin(_) => false,
                MachineError::DivisionByZero => true,
                MachineError::IntegerOverflow => true,
                MachineError::IndexOutOfBounds { .. } => true,
                MachineError::EmptyList => true,
                MachineError::InvalidUtf8 => true,
                MachineError::UnexpectedConstructorTag { .. } => true,
                MachineError::NonConstrScrutinized => true,
                MachineError::FlatDecodeError(_) => false,
                MachineError::CryptoError(_) => true,
                MachineError::MissingBuiltinCost(_) => false,
            };
            assert_eq!(
                e.is_operational(),
                expected_op,
                "classification mismatch for {e:?}: test expected \
                 {expected_op}, impl returned {}. Review the \
                 `matches!` list in `is_operational` against this test.",
                e.is_operational(),
            );
        }
    }

    #[test]
    fn into_ledger_error_collapses_operational() {
        let e = MachineError::DivisionByZero.into_ledger_error();
        assert!(matches!(e, MachineError::EvaluationFailure));
    }

    #[test]
    fn into_ledger_error_preserves_structural() {
        let e = MachineError::OutOfBudget("cpu".into()).into_ledger_error();
        assert!(matches!(e, MachineError::OutOfBudget(_)));
    }

    #[test]
    fn into_ledger_error_collapses_all_operational_variants() {
        let ops = vec![
            MachineError::TypeMismatch {
                expected: "A",
                actual: "B".into(),
            },
            MachineError::CryptoError("x".into()),
            MachineError::NonConstrScrutinized,
            MachineError::BuiltinTermArgumentExpected {
                expected: "term argument",
                received: "type force",
            },
        ];
        for e in ops {
            assert!(
                matches!(e.into_ledger_error(), MachineError::EvaluationFailure),
                "operational error should collapse to EvaluationFailure"
            );
        }
    }

    #[test]
    fn into_ledger_error_preserves_all_structural_variants() {
        let e1 = MachineError::UnboundVariable(7).into_ledger_error();
        assert!(matches!(e1, MachineError::UnboundVariable(7)));
        let e2 = MachineError::FlatDecodeError("bad".into()).into_ledger_error();
        assert!(matches!(e2, MachineError::FlatDecodeError(_)));
    }
}
