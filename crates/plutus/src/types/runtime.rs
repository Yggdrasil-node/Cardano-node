//! CEK machine runtime types: budget, value, environment.
//!
//! Mirrors upstream `UntypedPlutusCore.Evaluation.Machine.Cek.Internal`
//! (`Value`, `Env`) and `PlutusCore.Evaluation.Machine.ExBudget`
//! (`ExBudget`).
//!
//! Four types:
//!
//! - `ExBudget` — execution budget tracking CPU steps + memory units
//!   (mirrors ledger `ExUnits` for the evaluator-internal accounting).
//! - `Value` — closed values produced by reduction (Constant, LamAbs,
//!   DelayClosure, Builtin, Constr).
//! - `Environment` — Arc-backed cons-list mapping de Bruijn indices
//!   to closure values.
//! - `EnvNode` (private) — internal cons-cell linking `Environment` chains.
//!
//! Extracted from `types.rs` in R273g (Phase γ §R273 seventh slice).

use std::sync::Arc;

use crate::error::MachineError;

use super::default_fun::DefaultFun;
use super::term::{Constant, Term};

// ---------------------------------------------------------------------------
// ExBudget
// ---------------------------------------------------------------------------

/// Execution budget tracking CPU steps and memory units.
///
/// Mirrors `ExUnits` from the ledger but used within the evaluator to
/// track consumption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExBudget {
    pub cpu: i64,
    pub mem: i64,
}

impl ExBudget {
    pub fn new(cpu: i64, mem: i64) -> Self {
        Self { cpu, mem }
    }

    /// Returns `true` if both components are non-negative.
    pub fn is_within_limit(&self) -> bool {
        self.cpu >= 0 && self.mem >= 0
    }

    /// Spend some budget. Returns an error if the budget is exceeded.
    ///
    /// Uses `checked_sub` so a malformed cost-model constant carrying
    /// `i64::MIN` cannot wrap-around past zero in release mode.
    pub fn spend(&mut self, cost: ExBudget) -> Result<(), MachineError> {
        let cpu_after = self.cpu.checked_sub(cost.cpu).ok_or_else(|| {
            MachineError::OutOfBudget(format!(
                "cpu subtract overflow: {} - {}",
                self.cpu, cost.cpu
            ))
        })?;
        let mem_after = self.mem.checked_sub(cost.mem).ok_or_else(|| {
            MachineError::OutOfBudget(format!(
                "mem subtract overflow: {} - {}",
                self.mem, cost.mem
            ))
        })?;
        self.cpu = cpu_after;
        self.mem = mem_after;
        if self.cpu < 0 || self.mem < 0 {
            Err(MachineError::OutOfBudget(format!(
                "remaining cpu={}, mem={}",
                self.cpu, self.mem
            )))
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Value — CEK machine runtime values
// ---------------------------------------------------------------------------

/// Runtime value produced by the CEK machine.
#[derive(Clone, Debug)]
pub enum Value {
    /// A constant.
    Constant(Constant),
    /// A lambda closure capturing its environment.
    Lambda(Term, Environment),
    /// A delayed computation capturing its environment.
    Delay(Term, Environment),
    /// A partially applied built-in function.
    BuiltinApp {
        fun: DefaultFun,
        /// Number of `Force` (type) arguments received so far.
        forces: usize,
        /// Value arguments received so far (in application order).
        args: Vec<Value>,
    },
    /// A constructed value (UPLC 1.1.0+).
    Constr(u64, Vec<Value>),
}

impl Value {
    /// Extract as a constant, or return a type mismatch error.
    pub fn as_constant(&self) -> Result<&Constant, MachineError> {
        match self {
            Self::Constant(c) => Ok(c),
            other => Err(MachineError::TypeMismatch {
                expected: "constant",
                actual: other.type_name().to_string(),
            }),
        }
    }

    /// Human-readable type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Constant(_) => "constant",
            Self::Lambda(..) => "lambda",
            Self::Delay(..) => "delay",
            Self::BuiltinApp { .. } => "builtin",
            Self::Constr(..) => "constr",
        }
    }
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// CEK environment mapping de Bruijn indices to values.
///
/// Index 1 refers to the most recently bound variable. The representation is
/// persistent so closures and continuation frames can share captured tails
/// without cloning large values such as `ScriptContext`.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    head: Option<Arc<EnvNode>>,
    len: usize,
}

#[derive(Debug)]
struct EnvNode {
    value: Value,
    parent: Option<Arc<EnvNode>>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new environment with `val` as the most recent binding.
    pub fn extend(&self, val: Value) -> Self {
        Self {
            head: Some(Arc::new(EnvNode {
                value: val,
                parent: self.head.clone(),
            })),
            len: self.len + 1,
        }
    }

    /// Look up a 1-based de Bruijn index.
    pub fn lookup(&self, index: u64) -> Result<&Value, MachineError> {
        let depth = usize::try_from(index).map_err(|_| MachineError::UnboundVariable(index))?;
        if depth == 0 || depth > self.len {
            return Err(MachineError::UnboundVariable(index));
        }

        let mut current = self.head.as_deref();
        for _ in 1..depth {
            current = current.and_then(|node| node.parent.as_deref());
        }

        current
            .map(|node| &node.value)
            .ok_or(MachineError::UnboundVariable(index))
    }
}
