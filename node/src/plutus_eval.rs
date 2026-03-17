//! CEK-machine `PlutusEvaluator` implementation for the node.
//!
//! Bridges [`yggdrasil_ledger::plutus_validation::PlutusEvaluator`] to the
//! actual [`yggdrasil_plutus`] CEK machine.
//!
//! ## Argument application
//!
//! Cardano Plutus scripts are curried functions:
//! - Spending validator:   `datum -> redeemer -> context -> result`
//! - All other validators: `redeemer -> context -> result`
//!
//! For PlutusV1/V2 the result is discarded — any non-error outcome is
//! accepted. For PlutusV3 the result must be `Constant(Bool(true))`.
//!
//! ## ScriptContext (current limitation)
//!
//! A full `ScriptContext` / `TxInfo` construction requires access to the
//! full transaction body (inputs, outputs, fee, validity range, etc.), which
//! is not yet threaded through `PlutusScriptEval`. Until that milestone,
//! the context is approximated as an empty constructor `Constr(0, [])` so
//! that the argument-application plumbing is correct and scripts that do
//! not inspect the context (or always-succeed scripts) pass correctly.
//!
//! Full ScriptContext construction is tracked as a future milestone in
//! `crates/ledger/src/AGENTS.md`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core>
//! Reference: <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/PlutusScripts.hs>

use yggdrasil_ledger::{
    LedgerError,
    plutus::PlutusData,
    plutus_validation::{PlutusEvaluator, PlutusScriptEval, PlutusVersion},
};
use yggdrasil_plutus::{
    decode_script_bytes,
    types::{Constant, Term},
    CostModel, ExBudget, MachineError, Value,
};

// ---------------------------------------------------------------------------
// CekPlutusEvaluator
// ---------------------------------------------------------------------------

/// A [`PlutusEvaluator`] backed by the `yggdrasil-plutus` CEK machine.
///
/// Decodes each script from its on-chain Flat bytes, applies datum (if
/// spending), redeemer, and a placeholder ScriptContext, then evaluates
/// within the budget declared by the transaction.
#[derive(Clone, Debug, Default)]
pub struct CekPlutusEvaluator {
    /// Cost model to use. Defaults to `CostModel::default()`.
    pub cost_model: CostModel,
}

impl CekPlutusEvaluator {
    /// Create an evaluator with the default cost model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluator with a custom cost model.
    pub fn with_cost_model(cost_model: CostModel) -> Self {
        Self { cost_model }
    }
}

impl PlutusEvaluator for CekPlutusEvaluator {
    fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
        // 1. Decode the on-chain script bytes (Flat / CBOR-unwrap).
        let program = decode_script_bytes(&eval.script_bytes).map_err(|e| {
            LedgerError::PlutusScriptDecodeError {
                hash: eval.script_hash,
                reason: e.to_string(),
            }
        })?;

        // 2. Build Term::Constant wrappers for datum, redeemer, and context.
        let redeemer_term = data_term(eval.redeemer.clone());
        // Placeholder context: Constr(0, []) — an empty constructor that
        // satisfies the arity requirement without encoding any real TxInfo.
        // Scripts that inspect the context will fail; scripts that ignore it
        // (or are always-succeed stubs) will pass correctly.
        let context_term = Term::Constant(Constant::Data(PlutusData::Constr(0, vec![])));

        // 3. Apply arguments in the order specified by the Plutus script ABI.
        //    spending validator: script datum redeemer context
        //    all others:         script redeemer context
        let applied = match &eval.datum {
            Some(datum) => Term::Apply(
                Box::new(Term::Apply(
                    Box::new(Term::Apply(
                        Box::new(program.term),
                        Box::new(data_term(datum.clone())),
                    )),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
            None => Term::Apply(
                Box::new(Term::Apply(
                    Box::new(program.term),
                    Box::new(redeemer_term),
                )),
                Box::new(context_term),
            ),
        };

        // 4. Build execution budget from the transaction's declared ExUnits.
        //    ExUnits.steps → cpu; ExUnits.mem → mem.
        let budget = ExBudget::new(
            eval.ex_units.steps as i64,
            eval.ex_units.mem as i64,
        );

        // 5. Evaluate the applied term.
        let (result, _logs) =
            yggdrasil_plutus::evaluate_term(applied, budget, self.cost_model.clone())
                .map_err(|e| map_machine_error(&eval.script_hash, e))?;

        // 6. PlutusV3 scripts must explicitly return Bool(true).
        //    PlutusV1/V2 accept any non-error result.
        if eval.version == PlutusVersion::V3 {
            match result {
                Value::Constant(Constant::Bool(true)) => Ok(()),
                other => Err(LedgerError::PlutusScriptFailed {
                    hash: eval.script_hash,
                    reason: format!(
                        "PlutusV3 script must return Bool(true), got: {:?}",
                        other
                    ),
                }),
            }
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Wrap a [`PlutusData`] value in a `Term::Constant`.
fn data_term(data: PlutusData) -> Term {
    Term::Constant(Constant::Data(data))
}

/// Convert a [`MachineError`] into a [`LedgerError::PlutusScriptFailed`].
fn map_machine_error(hash: &[u8; 28], err: MachineError) -> LedgerError {
    LedgerError::PlutusScriptFailed {
        hash: *hash,
        reason: err.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::plutus_validation::{PlutusScriptEval, PlutusVersion, ScriptPurpose};
    use yggdrasil_ledger::{eras::alonzo::ExUnits, plutus::PlutusData};

    fn dummy_hash() -> [u8; 28] {
        [0xab; 28]
    }

    fn mint_eval(script_bytes: Vec<u8>, version: PlutusVersion) -> PlutusScriptEval {
        PlutusScriptEval {
            script_hash: dummy_hash(),
            version,
            script_bytes,
            purpose: ScriptPurpose::Minting {
                policy_id: dummy_hash(),
            },
            datum: None,
            redeemer: PlutusData::Integer(42),
            ex_units: ExUnits {
                mem: 10_000_000,
                steps: 10_000_000,
            },
        }
    }

    #[test]
    fn decode_error_on_empty_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        // Empty script bytes → decode failure.
        let eval = PlutusScriptEval {
            script_bytes: vec![],
            ..mint_eval(vec![], PlutusVersion::V1)
        };
        let result = evaluator.evaluate(&eval);
        assert!(
            result.is_err(),
            "empty script bytes must produce a decode error"
        );
        match result {
            Err(LedgerError::PlutusScriptDecodeError { .. }) => {}
            Err(other) => panic!("expected PlutusScriptDecodeError, got: {:?}", other),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn decode_error_on_garbage_bytes() {
        let evaluator = CekPlutusEvaluator::new();
        let eval = mint_eval(vec![0xff, 0xfe, 0xfd, 0xfc], PlutusVersion::V1);
        let result = evaluator.evaluate(&eval);
        assert!(
            result.is_err(),
            "garbage bytes must produce a decode or evaluation error"
        );
    }
}
