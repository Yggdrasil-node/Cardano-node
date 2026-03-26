//! CEK machine — the UPLC evaluator.
//!
//! Implements a standard CEK (Control-Environment-Continuation) machine for
//! the Untyped Plutus Lambda Calculus. Built-in functions are saturated via
//! partial application with arity tracking.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek.hs>

use crate::builtins::evaluate_builtin;
use crate::cost_model::{CostModel, StepKind};
use crate::error::MachineError;
use crate::types::{Environment, ExBudget, Term, Value};

// ---------------------------------------------------------------------------
// Continuation frames
// ---------------------------------------------------------------------------

/// Continuation frames kept on the CEK machine's stack.
#[derive(Debug)]
enum Frame {
    /// Evaluating the function part of an `Apply`; the argument term and
    /// its environment are saved for later.
    ApplyArg(Environment, Term),

    /// The function value has been evaluated; now evaluating the argument.
    /// When the argument value arrives, `apply_fun(fun, arg)` is called.
    ApplyFun(Value),

    /// A `Force` is waiting for the inner term to evaluate.
    Force,

    /// Evaluating constructor fields left-to-right (UPLC 1.1.0+).
    ConstrFields {
        tag: u64,
        evaluated: Vec<Value>,
        remaining: Vec<Term>,
        env: Environment,
    },

    /// Waiting for the scrutinee of a `Case` to evaluate.
    /// Carries the branch terms and the environment at the `Case` site.
    CaseBranches {
        branches: Vec<Term>,
        env: Environment,
    },

    /// Applying a case branch result to remaining constructor field values.
    CaseApply {
        remaining: Vec<Value>,
    },
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

enum State {
    /// Evaluating a term in an environment.
    Computing(Term, Environment),
    /// Delivering a value to the continuation stack.
    Returning(Value),
    /// Evaluation complete.
    Done(Value),
}

// ---------------------------------------------------------------------------
// CekMachine
// ---------------------------------------------------------------------------

/// CEK machine state.
pub struct CekMachine {
    frames: Vec<Frame>,
    budget: ExBudget,
    cost_model: CostModel,
    steps: u64,
    max_steps: u64,
    /// Trace log messages emitted by the `trace` builtin.
    pub logs: Vec<String>,
}

impl CekMachine {
    /// Create a new machine with the given budget.
    pub fn new(budget: ExBudget, cost_model: CostModel) -> Self {
        Self {
            frames: Vec::with_capacity(64),
            budget,
            cost_model,
            steps: 0,
            max_steps: 10_000_000_000,
            logs: Vec::new(),
        }
    }

    /// Evaluate a term to a value.
    pub fn evaluate(&mut self, term: Term) -> Result<Value, MachineError> {
        // Upstream charges a one-time startup cost before evaluation begins.
        self.budget.spend(self.cost_model.startup_cost)?;

        let env = Environment::new();
        let mut state = State::Computing(term, env);

        loop {
            match state {
                State::Computing(term, env) => {
                    state = self.step_compute(term, env)?;
                }
                State::Returning(value) => {
                    state = self.step_return(value)?;
                }
                State::Done(value) => {
                    return Ok(value);
                }
            }
        }
    }

    /// Remaining budget after evaluation.
    pub fn remaining_budget(&self) -> ExBudget {
        self.budget
    }

    /// Number of machine steps executed.
    pub fn steps_taken(&self) -> u64 {
        self.steps
    }

    // -- Compute --------------------------------------------------------

    fn step_compute(&mut self, term: Term, env: Environment) -> Result<State, MachineError> {
        match term {
            Term::Var(index) => {
                self.spend_step(StepKind::Var)?;
                let val = env.lookup(index)?.clone();
                Ok(State::Returning(val))
            }
            Term::LamAbs(body) => {
                self.spend_step(StepKind::LamAbs)?;
                Ok(State::Returning(Value::Lambda(*body, env)))
            }
            Term::Apply(fun, arg) => {
                self.spend_step(StepKind::Apply)?;
                self.frames.push(Frame::ApplyArg(env.clone(), *arg));
                Ok(State::Computing(*fun, env))
            }
            Term::Delay(body) => {
                self.spend_step(StepKind::Delay)?;
                Ok(State::Returning(Value::Delay(*body, env)))
            }
            Term::Force(inner) => {
                self.spend_step(StepKind::Force)?;
                self.frames.push(Frame::Force);
                Ok(State::Computing(*inner, env))
            }
            Term::Constant(c) => {
                self.spend_step(StepKind::Constant)?;
                Ok(State::Returning(Value::Constant(c)))
            }
            Term::Builtin(fun) => {
                self.spend_step(StepKind::Builtin)?;
                Ok(State::Returning(Value::BuiltinApp {
                    fun,
                    forces: 0,
                    args: Vec::new(),
                }))
            }
            Term::Error => Err(MachineError::EvaluationFailure),
            Term::Constr(tag, fields) => {
                self.spend_step(StepKind::Constr)?;
                if fields.is_empty() {
                    Ok(State::Returning(Value::Constr(tag, Vec::new())))
                } else {
                    let mut remaining = fields;
                    let first = remaining.remove(0);
                    self.frames.push(Frame::ConstrFields {
                        tag,
                        evaluated: Vec::new(),
                        remaining,
                        env: env.clone(),
                    });
                    Ok(State::Computing(first, env))
                }
            }
            Term::Case(scrutinee, branches) => {
                self.spend_step(StepKind::Case)?;
                self.frames.push(Frame::CaseBranches {
                    branches,
                    env: env.clone(),
                });
                Ok(State::Computing(*scrutinee, env))
            }
        }
    }

    // -- Return ---------------------------------------------------------

    fn step_return(&mut self, value: Value) -> Result<State, MachineError> {
        match self.frames.pop() {
            None => Ok(State::Done(value)),

            Some(Frame::ApplyArg(env, arg_term)) => {
                self.frames.push(Frame::ApplyFun(value));
                Ok(State::Computing(arg_term, env))
            }

            Some(Frame::ApplyFun(fun_val)) => self.apply_fun(fun_val, value),

            Some(Frame::Force) => self.force_value(value),

            Some(Frame::ConstrFields {
                tag,
                mut evaluated,
                mut remaining,
                env,
            }) => {
                evaluated.push(value);
                if remaining.is_empty() {
                    Ok(State::Returning(Value::Constr(tag, evaluated)))
                } else {
                    let next = remaining.remove(0);
                    self.frames.push(Frame::ConstrFields {
                        tag,
                        evaluated,
                        remaining,
                        env: env.clone(),
                    });
                    Ok(State::Computing(next, env))
                }
            }

            Some(Frame::CaseBranches { branches, env }) => {
                // Scrutinee must be a Constr value.
                match value {
                    Value::Constr(tag, fields) => {
                        let tag_usize = tag as usize;
                        if tag_usize >= branches.len() {
                            return Err(MachineError::UnexpectedConstructorTag {
                                tag,
                                branches: branches.len(),
                            });
                        }
                        let branch = branches.into_iter().nth(tag_usize)
                            .ok_or(MachineError::UnexpectedConstructorTag {
                                tag,
                                branches: 0,
                            })?;
                        // Push CaseApply to apply branch to field values
                        // once the branch term evaluates.
                        if !fields.is_empty() {
                            self.frames.push(Frame::CaseApply {
                                remaining: fields,
                            });
                        }
                        Ok(State::Computing(branch, env))
                    }
                    _ => Err(MachineError::NonConstrScrutinized),
                }
            }

            Some(Frame::CaseApply { mut remaining }) => {
                // `value` is the branch function or partial application result.
                if remaining.is_empty() {
                    Ok(State::Returning(value))
                } else {
                    let next_arg = remaining.remove(0);
                    if !remaining.is_empty() {
                        self.frames.push(Frame::CaseApply { remaining });
                    }
                    self.apply_fun(value, next_arg)
                }
            }
        }
    }

    // -- Apply / Force --------------------------------------------------

    fn apply_fun(&mut self, fun: Value, arg: Value) -> Result<State, MachineError> {
        match fun {
            Value::Lambda(body, env) => {
                let new_env = env.extend(arg);
                Ok(State::Computing(body, new_env))
            }
            Value::BuiltinApp {
                fun: builtin,
                forces,
                mut args,
            } => {
                let (needed_forces, needed_args) = builtin.arity();

                // Upstream enforces: all forces first, then args.
                if forces < needed_forces {
                    return Err(MachineError::BuiltinTermArgumentExpected {
                        expected: "force",
                        received: "argument",
                    });
                }

                args.push(arg);

                if forces >= needed_forces && args.len() >= needed_args {
                    let result = evaluate_builtin(
                        builtin, &args, &self.cost_model, &mut self.logs,
                    )?;
                    let cost = self.cost_model.builtin_cost(builtin, &args);
                    self.budget.spend(cost)?;
                    Ok(State::Returning(result))
                } else {
                    Ok(State::Returning(Value::BuiltinApp {
                        fun: builtin,
                        forces,
                        args,
                    }))
                }
            }
            _ => Err(MachineError::NonFunctionApplication),
        }
    }

    fn force_value(&mut self, value: Value) -> Result<State, MachineError> {
        match value {
            Value::Delay(body, env) => Ok(State::Computing(body, env)),
            Value::BuiltinApp {
                fun,
                forces,
                args,
            } => {
                let (needed_forces, needed_args) = fun.arity();

                // Upstream enforces: all forces first, then args.
                // Forcing after all forces are done is an error.
                if forces >= needed_forces {
                    return Err(MachineError::BuiltinTermArgumentExpected {
                        expected: "argument",
                        received: "force",
                    });
                }

                let new_forces = forces + 1;

                if new_forces >= needed_forces && args.len() >= needed_args {
                    let result = evaluate_builtin(
                        fun, &args, &self.cost_model, &mut self.logs,
                    )?;
                    let cost = self.cost_model.builtin_cost(fun, &args);
                    self.budget.spend(cost)?;
                    Ok(State::Returning(result))
                } else {
                    Ok(State::Returning(Value::BuiltinApp {
                        fun,
                        forces: new_forces,
                        args,
                    }))
                }
            }
            _ => Err(MachineError::NonPolymorphicForce),
        }
    }

    // -- Budget ---------------------------------------------------------

    fn spend_step(&mut self, kind: StepKind) -> Result<(), MachineError> {
        self.steps += 1;
        if self.steps > self.max_steps {
            return Err(MachineError::OutOfBudget(format!(
                "exceeded max steps ({})",
                self.max_steps
            )));
        }
        let cost = self.cost_model.step_cost(kind);
        self.budget.spend(cost)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost_model::CostModel;
    use crate::types::{Constant, DefaultFun, ExBudget, Term};

    /// Helper: create a machine with generous budget.
    fn machine() -> CekMachine {
        CekMachine::new(
            ExBudget::new(10_000_000, 10_000_000),
            CostModel::default(),
        )
    }

    // -- Constant evaluation ----------------------------------------------

    #[test]
    fn eval_constant_integer() {
        let mut m = machine();
        let result = m.evaluate(Term::Constant(Constant::Integer(42))).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer"),
        }
    }

    #[test]
    fn eval_constant_bytestring() {
        let mut m = machine();
        let result = m
            .evaluate(Term::Constant(Constant::ByteString(vec![1, 2, 3])))
            .unwrap();
        match result {
            Value::Constant(Constant::ByteString(bs)) => assert_eq!(bs, vec![1, 2, 3]),
            _ => panic!("expected bytestring"),
        }
    }

    #[test]
    fn eval_constant_string() {
        let mut m = machine();
        let result = m
            .evaluate(Term::Constant(Constant::String("hello".into())))
            .unwrap();
        match result {
            Value::Constant(Constant::String(s)) => assert_eq!(s, "hello"),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn eval_constant_unit() {
        let mut m = machine();
        let result = m.evaluate(Term::Constant(Constant::Unit)).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Unit)));
    }

    #[test]
    fn eval_constant_bool() {
        let mut m = machine();
        let r = m.evaluate(Term::Constant(Constant::Bool(true))).unwrap();
        assert!(matches!(r, Value::Constant(Constant::Bool(true))));
    }

    // -- Identity function (\x -> x) applied to a value -------------------

    #[test]
    fn eval_identity_function() {
        let mut m = machine();
        // (\x -> x) 42
        let term = Term::Apply(
            Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
    }

    // -- Nested lambda (const function K = \x.\y.x) ----------------------

    #[test]
    fn eval_const_function() {
        let mut m = machine();
        // (\x.\y. x) 10 20  →  10
        let k = Term::LamAbs(Box::new(Term::LamAbs(Box::new(Term::Var(2)))));
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(k),
                Box::new(Term::Constant(Constant::Integer(10))),
            )),
            Box::new(Term::Constant(Constant::Integer(20))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 10),
            _ => panic!("expected integer 10"),
        }
    }

    // -- Delay / Force ----------------------------------------------------

    #[test]
    fn eval_delay_force() {
        let mut m = machine();
        // Force (Delay 42)  →  42
        let term = Term::Force(Box::new(Term::Delay(Box::new(Term::Constant(
            Constant::Integer(42),
        )))));
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
    }

    #[test]
    fn eval_delay_produces_thunk() {
        let mut m = machine();
        // Delay <body> produces a Delay value (not evaluated).
        let term = Term::Delay(Box::new(Term::Constant(Constant::Integer(99))));
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Delay(..)));
    }

    // -- Error term -------------------------------------------------------

    #[test]
    fn eval_error_term() {
        let mut m = machine();
        let err = m.evaluate(Term::Error).unwrap_err();
        assert!(matches!(err, MachineError::EvaluationFailure));
    }

    // -- Unbound variable -------------------------------------------------

    #[test]
    fn eval_unbound_variable() {
        let mut m = machine();
        let err = m.evaluate(Term::Var(1)).unwrap_err();
        assert!(matches!(err, MachineError::UnboundVariable(1)));
    }

    #[test]
    fn eval_unbound_variable_zero() {
        let mut m = machine();
        // Var(0) is invalid de Bruijn index.
        let body = Term::LamAbs(Box::new(Term::Var(0)));
        let term = Term::Apply(
            Box::new(body),
            Box::new(Term::Constant(Constant::Unit)),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::UnboundVariable(0)));
    }

    // -- Non-function application -----------------------------------------

    #[test]
    fn eval_apply_non_function() {
        let mut m = machine();
        // (42) 10  →  NonFunctionApplication
        let term = Term::Apply(
            Box::new(Term::Constant(Constant::Integer(42))),
            Box::new(Term::Constant(Constant::Integer(10))),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::NonFunctionApplication));
    }

    // -- Force on non-polymorphic value -----------------------------------

    #[test]
    fn eval_force_non_polymorphic() {
        let mut m = machine();
        // Force 42  →  NonPolymorphicForce
        let term = Term::Force(Box::new(Term::Constant(Constant::Integer(42))));
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::NonPolymorphicForce));
    }

    // -- Builtin: AddInteger -----------------------------------------------

    #[test]
    fn eval_add_integer() {
        let mut m = machine();
        // addInteger 3 4  →  7
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::AddInteger)),
                Box::new(Term::Constant(Constant::Integer(3))),
            )),
            Box::new(Term::Constant(Constant::Integer(4))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 7),
            _ => panic!("expected integer 7"),
        }
    }

    #[test]
    fn eval_subtract_integer() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::SubtractInteger)),
                Box::new(Term::Constant(Constant::Integer(10))),
            )),
            Box::new(Term::Constant(Constant::Integer(3))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 7),
            _ => panic!("expected integer 7"),
        }
    }

    #[test]
    fn eval_multiply_integer() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::MultiplyInteger)),
                Box::new(Term::Constant(Constant::Integer(6))),
            )),
            Box::new(Term::Constant(Constant::Integer(7))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
    }

    // -- Builtin: EqualsInteger -----------

    #[test]
    fn eval_equals_integer_true() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::EqualsInteger)),
                Box::new(Term::Constant(Constant::Integer(42))),
            )),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(true))));
    }

    #[test]
    fn eval_equals_integer_false() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::EqualsInteger)),
                Box::new(Term::Constant(Constant::Integer(1))),
            )),
            Box::new(Term::Constant(Constant::Integer(2))),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(false))));
    }

    // -- Polymorphic builtin: IfThenElse ----------------------------------

    #[test]
    fn eval_if_then_else_true() {
        let mut m = machine();
        // Force (IfThenElse) True 1 2  →  1
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Apply(
                    Box::new(Term::Force(Box::new(Term::Builtin(
                        DefaultFun::IfThenElse,
                    )))),
                    Box::new(Term::Constant(Constant::Bool(true))),
                )),
                Box::new(Term::Constant(Constant::Integer(1))),
            )),
            Box::new(Term::Constant(Constant::Integer(2))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 1),
            _ => panic!("expected integer 1"),
        }
    }

    #[test]
    fn eval_if_then_else_false() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Apply(
                    Box::new(Term::Force(Box::new(Term::Builtin(
                        DefaultFun::IfThenElse,
                    )))),
                    Box::new(Term::Constant(Constant::Bool(false))),
                )),
                Box::new(Term::Constant(Constant::Integer(1))),
            )),
            Box::new(Term::Constant(Constant::Integer(2))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 2),
            _ => panic!("expected integer 2"),
        }
    }

    // -- Constr / Case (UPLC 1.1.0+) -------------------------------------

    #[test]
    fn eval_constr_empty() {
        let mut m = machine();
        let term = Term::Constr(0, vec![]);
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constr(0, ref fields) if fields.is_empty()));
    }

    #[test]
    fn eval_constr_with_fields() {
        let mut m = machine();
        let term = Term::Constr(1, vec![
            Term::Constant(Constant::Integer(10)),
            Term::Constant(Constant::Integer(20)),
        ]);
        let result = m.evaluate(term).unwrap();
        if let Value::Constr(tag, fields) = &result {
            assert_eq!(*tag, 1);
            assert_eq!(fields.len(), 2);
        } else {
            panic!("expected Constr");
        }
    }

    #[test]
    fn eval_case_branch_zero() {
        let mut m = machine();
        // Case (Constr 0 []) [42, 99]
        let term = Term::Case(
            Box::new(Term::Constr(0, vec![])),
            vec![
                Term::Constant(Constant::Integer(42)),
                Term::Constant(Constant::Integer(99)),
            ],
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
    }

    #[test]
    fn eval_case_branch_one() {
        let mut m = machine();
        let term = Term::Case(
            Box::new(Term::Constr(1, vec![])),
            vec![
                Term::Constant(Constant::Integer(42)),
                Term::Constant(Constant::Integer(99)),
            ],
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 99),
            _ => panic!("expected integer 99"),
        }
    }

    #[test]
    fn eval_case_tag_out_of_range() {
        let mut m = machine();
        let term = Term::Case(
            Box::new(Term::Constr(5, vec![])),
            vec![Term::Constant(Constant::Integer(1))],
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(
            err,
            MachineError::UnexpectedConstructorTag { tag: 5, .. }
        ));
    }

    #[test]
    fn eval_case_non_constr_scrutinee() {
        let mut m = machine();
        let term = Term::Case(
            Box::new(Term::Constant(Constant::Integer(0))),
            vec![Term::Constant(Constant::Integer(1))],
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::NonConstrScrutinized));
    }

    // -- Trace builtin ----------------------------------------------------

    #[test]
    fn eval_trace_logs_message() {
        let mut m = machine();
        // Force Trace "hello" 42  →  42
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::Trace)))),
                Box::new(Term::Constant(Constant::String("hello".into()))),
            )),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
        assert_eq!(m.logs, vec!["hello".to_string()]);
    }

    // -- Budget exhaustion ------------------------------------------------

    #[test]
    fn eval_budget_exhaustion() {
        // Tiny budget should run out.
        let mut m = CekMachine::new(
            ExBudget::new(50, 50),
            CostModel::default(),
        );
        // Need multiple steps to exhaust.
        let term = Term::Apply(
            Box::new(Term::LamAbs(Box::new(Term::Apply(
                Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
                Box::new(Term::Var(1)),
            )))),
            Box::new(Term::Constant(Constant::Integer(1))),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::OutOfBudget(_)));
    }

    // -- remaining_budget / steps_taken -----------------------------------

    #[test]
    fn eval_remaining_budget_decreases() {
        let initial = ExBudget::new(10_000_000, 10_000_000);
        let mut m = CekMachine::new(initial, CostModel::default());
        m.evaluate(Term::Constant(Constant::Unit)).unwrap();
        let remaining = m.remaining_budget();
        assert!(remaining.cpu < initial.cpu);
        assert!(remaining.mem < initial.mem);
    }

    #[test]
    fn eval_steps_taken_positive() {
        let mut m = machine();
        assert_eq!(m.steps_taken(), 0);
        m.evaluate(Term::Constant(Constant::Unit)).unwrap();
        assert!(m.steps_taken() > 0);
    }

    // -- Partial builtin application --------------------------------------

    #[test]
    fn eval_partial_application_returns_builtin_app() {
        let mut m = machine();
        // AddInteger 3  →  BuiltinApp (partially applied)
        let term = Term::Apply(
            Box::new(Term::Builtin(DefaultFun::AddInteger)),
            Box::new(Term::Constant(Constant::Integer(3))),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(
            result,
            Value::BuiltinApp {
                fun: DefaultFun::AddInteger,
                forces: 0,
                ..
            }
        ));
    }

    #[test]
    fn force_apply_order_arg_before_force_errors() {
        // Apply an arg to IfThenElse before forcing it — upstream rejects this.
        // IfThenElse has arity (1 force, 3 args). Applying before forcing is wrong.
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Builtin(DefaultFun::IfThenElse)),
            Box::new(Term::Constant(Constant::Bool(true))),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(
            err,
            MachineError::BuiltinTermArgumentExpected {
                expected: "force",
                received: "argument",
            }
        ));
    }

    #[test]
    fn force_apply_order_excess_force_errors() {
        // Force AddInteger — it needs 0 forces, so any force is excess.
        let mut m = machine();
        let term = Term::Force(Box::new(Term::Builtin(DefaultFun::AddInteger)));
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(
            err,
            MachineError::BuiltinTermArgumentExpected {
                expected: "argument",
                received: "force",
            }
        ));
    }

    #[test]
    fn force_apply_order_double_force_on_single_force_builtin_errors() {
        // IfThenElse needs 1 force. Forcing twice is wrong.
        let mut m = machine();
        let term = Term::Force(Box::new(Term::Force(Box::new(
            Term::Builtin(DefaultFun::IfThenElse),
        ))));
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(
            err,
            MachineError::BuiltinTermArgumentExpected {
                expected: "argument",
                received: "force",
            }
        ));
    }

    // -- Lambda closure ---------------------------------------------------

    #[test]
    fn eval_lambda_returns_closure() {
        let mut m = machine();
        let term = Term::LamAbs(Box::new(Term::Var(1)));
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Lambda(..)));
    }

    // -- Nested apply -----------------------------------------------------

    #[test]
    fn eval_nested_apply() {
        let mut m = machine();
        // (\f.\x. f x) (\y. y) 42  →  42
        let f_body = Term::Apply(
            Box::new(Term::Var(2)), // f
            Box::new(Term::Var(1)), // x
        );
        let outer = Term::LamAbs(Box::new(Term::LamAbs(Box::new(f_body))));
        let id = Term::LamAbs(Box::new(Term::Var(1)));
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(outer),
                Box::new(id),
            )),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected 42"),
        }
    }

    // -- LessThanInteger --------------------------------------------------

    #[test]
    fn eval_less_than_integer_true() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::LessThanInteger)),
                Box::new(Term::Constant(Constant::Integer(1))),
            )),
            Box::new(Term::Constant(Constant::Integer(2))),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(true))));
    }

    #[test]
    fn eval_less_than_integer_false() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::LessThanInteger)),
                Box::new(Term::Constant(Constant::Integer(5))),
            )),
            Box::new(Term::Constant(Constant::Integer(3))),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(false))));
    }

    // -- DivideInteger (div towards -inf) / DivisionByZero ----

    #[test]
    fn eval_divide_integer() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::DivideInteger)),
                Box::new(Term::Constant(Constant::Integer(7))),
            )),
            Box::new(Term::Constant(Constant::Integer(2))),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 3),
            _ => panic!("expected 3"),
        }
    }

    #[test]
    fn eval_divide_by_zero() {
        let mut m = machine();
        let term = Term::Apply(
            Box::new(Term::Apply(
                Box::new(Term::Builtin(DefaultFun::DivideInteger)),
                Box::new(Term::Constant(Constant::Integer(10))),
            )),
            Box::new(Term::Constant(Constant::Integer(0))),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::DivisionByZero));
    }

    // -- Case with field application --------------------------------------

    #[test]
    fn eval_case_with_field_application() {
        let mut m = machine();
        // Case (Constr 0 [10]) [\x. x]
        // Branch \x. x receives field 10.
        let term = Term::Case(
            Box::new(Term::Constr(0, vec![Term::Constant(Constant::Integer(10))])),
            vec![Term::LamAbs(Box::new(Term::Var(1)))],
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 10),
            _ => panic!("expected integer 10"),
        }
    }

    // -- HeadList / TailList / NullList via full CEK evaluation -------------

    #[test]
    fn eval_head_list() {
        let mut m = machine();
        // Force HeadList [1, 2, 3]
        let list = Term::Constant(Constant::ProtoList(
            crate::types::Type::Integer,
            vec![
                Constant::Integer(1),
                Constant::Integer(2),
                Constant::Integer(3),
            ],
        ));
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::HeadList)))),
            Box::new(list),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 1),
            _ => panic!("expected 1"),
        }
    }

    #[test]
    fn eval_head_list_empty() {
        let mut m = machine();
        let list = Term::Constant(Constant::ProtoList(
            crate::types::Type::Integer,
            vec![],
        ));
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::HeadList)))),
            Box::new(list),
        );
        let err = m.evaluate(term).unwrap_err();
        assert!(matches!(err, MachineError::EmptyList));
    }

    #[test]
    fn eval_null_list_true() {
        let mut m = machine();
        let list = Term::Constant(Constant::ProtoList(
            crate::types::Type::Integer,
            vec![],
        ));
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::NullList)))),
            Box::new(list),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(true))));
    }

    #[test]
    fn eval_null_list_false() {
        let mut m = machine();
        let list = Term::Constant(Constant::ProtoList(
            crate::types::Type::Integer,
            vec![Constant::Integer(1)],
        ));
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Builtin(DefaultFun::NullList)))),
            Box::new(list),
        );
        let result = m.evaluate(term).unwrap();
        assert!(matches!(result, Value::Constant(Constant::Bool(false))));
    }

    // -- FstPair / SndPair ------------------------------------------------

    #[test]
    fn eval_fst_pair() {
        let mut m = machine();
        let pair = Term::Constant(Constant::ProtoPair(
            crate::types::Type::Integer,
            crate::types::Type::ByteString,
            Box::new(Constant::Integer(1)),
            Box::new(Constant::ByteString(vec![2])),
        ));
        // Force Force FstPair pair
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Force(Box::new(
                Term::Builtin(DefaultFun::FstPair),
            ))))),
            Box::new(pair),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 1),
            _ => panic!("expected integer 1"),
        }
    }

    #[test]
    fn eval_snd_pair() {
        let mut m = machine();
        let pair = Term::Constant(Constant::ProtoPair(
            crate::types::Type::Integer,
            crate::types::Type::ByteString,
            Box::new(Constant::Integer(1)),
            Box::new(Constant::ByteString(vec![2])),
        ));
        let term = Term::Apply(
            Box::new(Term::Force(Box::new(Term::Force(Box::new(
                Term::Builtin(DefaultFun::SndPair),
            ))))),
            Box::new(pair),
        );
        let result = m.evaluate(term).unwrap();
        match result {
            Value::Constant(Constant::ByteString(bs)) => assert_eq!(bs, vec![2]),
            _ => panic!("expected bytestring"),
        }
    }

    // -- Multiple trace calls collect all logs ----------------------------

    #[test]
    fn eval_multiple_traces() {
        let mut m = machine();
        // let trace_ = Force Trace
        // trace_ "a" (trace_ "b" 42)
        let trace_forced = Term::Force(Box::new(Term::Builtin(DefaultFun::Trace)));
        let inner = Term::Apply(
            Box::new(Term::Apply(
                Box::new(trace_forced.clone()),
                Box::new(Term::Constant(Constant::String("b".into()))),
            )),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let outer = Term::Apply(
            Box::new(Term::Apply(
                Box::new(trace_forced),
                Box::new(Term::Constant(Constant::String("a".into()))),
            )),
            Box::new(inner),
        );
        let result = m.evaluate(outer).unwrap();
        match result {
            Value::Constant(Constant::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected integer 42"),
        }
        // Inner trace fires first (argument is evaluated before outer trace completes).
        assert_eq!(m.logs, vec!["b".to_string(), "a".to_string()]);
    }

    // -- Per-step-kind cost differentiation -----------------------------------

    fn custom_model(step_costs: crate::cost_model::StepCosts) -> CostModel {
        CostModel {
            step_costs,
            startup_cost: ExBudget::new(0, 0), // zero for precise step-cost tests
            builtin_cpu: 1_000,
            builtin_mem: 1_000,
            builtin_costs: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn startup_cost_charged_once() {
        let model = CostModel {
            startup_cost: ExBudget::new(500, 250),
            ..CostModel::default()
        };
        let initial = ExBudget::new(10_000_000, 10_000_000);
        let mut m = CekMachine::new(initial, model);
        // Evaluating a constant: startup(500,250) + constant_step(100,100)
        m.evaluate(Term::Constant(Constant::Unit)).unwrap();
        assert_eq!(m.remaining_budget().cpu, initial.cpu - 500 - 100);
        assert_eq!(m.remaining_budget().mem, initial.mem - 250 - 100);
    }

    #[test]
    fn step_costs_differentiated_by_term_type() {
        use crate::cost_model::StepCosts;

        let step_costs = StepCosts {
            var_cpu: 10, var_mem: 1,
            constant_cpu: 20, constant_mem: 2,
            lam_cpu: 30, lam_mem: 3,
            apply_cpu: 40, apply_mem: 4,
            delay_cpu: 50, delay_mem: 5,
            force_cpu: 60, force_mem: 6,
            builtin_cpu: 70, builtin_mem: 7,
            constr_cpu: 80, constr_mem: 8,
            case_cpu: 90, case_mem: 9,
        };
        let model = custom_model(step_costs);
        let initial = ExBudget::new(10_000_000, 10_000_000);

        // Single Constant(42): 1 step, charges constant_cpu=20
        let mut m = CekMachine::new(initial, model.clone());
        m.evaluate(Term::Constant(Constant::Integer(42))).unwrap();
        assert_eq!(m.steps_taken(), 1);
        assert_eq!(m.remaining_budget().cpu, initial.cpu - 20);
        assert_eq!(m.remaining_budget().mem, initial.mem - 2);

        // (\x -> x) 42: Apply + LamAbs + Constant + Var = 4 steps
        let term = Term::Apply(
            Box::new(Term::LamAbs(Box::new(Term::Var(1)))),
            Box::new(Term::Constant(Constant::Integer(42))),
        );
        let mut m2 = CekMachine::new(initial, model.clone());
        m2.evaluate(term).unwrap();
        assert_eq!(m2.steps_taken(), 4);
        assert_eq!(m2.remaining_budget().cpu, initial.cpu - (40 + 30 + 20 + 10));
        assert_eq!(m2.remaining_budget().mem, initial.mem - (4 + 3 + 2 + 1));

        // Delay(Const): Delay wraps a thunk – only 1 step
        let mut m3 = CekMachine::new(initial, model);
        m3.evaluate(Term::Delay(Box::new(Term::Constant(Constant::Integer(42))))).unwrap();
        assert_eq!(m3.steps_taken(), 1);
        assert_eq!(m3.remaining_budget().cpu, initial.cpu - 50);
    }

    #[test]
    fn force_charges_force_step_cost_only() {
        use crate::cost_model::StepCosts;

        let model = custom_model(StepCosts {
            force_cpu: 60, force_mem: 6,
            delay_cpu: 50, delay_mem: 5,
            constant_cpu: 20, constant_mem: 2,
            ..StepCosts::default()
        });
        let initial = ExBudget::new(10_000_000, 10_000_000);

        // Force(Delay(Constant(42))): Force + Delay + Constant = 3 steps
        let term = Term::Force(Box::new(
            Term::Delay(Box::new(Term::Constant(Constant::Integer(42)))),
        ));
        let mut m = CekMachine::new(initial, model);
        m.evaluate(term).unwrap();
        assert_eq!(m.steps_taken(), 3);
        assert_eq!(m.remaining_budget().cpu, initial.cpu - (60 + 50 + 20));
    }

    #[test]
    fn constr_and_case_charge_distinct_costs() {
        use crate::cost_model::StepCosts;

        let model = custom_model(StepCosts {
            constr_cpu: 80, constr_mem: 8,
            case_cpu: 90, case_mem: 9,
            constant_cpu: 20, constant_mem: 2,
            ..StepCosts::default()
        });
        let initial = ExBudget::new(10_000_000, 10_000_000);

        // Constr(0, [42]): Constr + Constant = 2 steps
        let mut m = CekMachine::new(initial, model.clone());
        m.evaluate(Term::Constr(0, vec![Term::Constant(Constant::Integer(42))])).unwrap();
        assert_eq!(m.remaining_budget().cpu, initial.cpu - (80 + 20));

        // Case(Constr(0, []), [Constant(42)]): Case + Constr + Constant = 3 steps
        let term = Term::Case(
            Box::new(Term::Constr(0, vec![])),
            vec![Term::Constant(Constant::Integer(42))],
        );
        let mut m2 = CekMachine::new(initial, model);
        m2.evaluate(term).unwrap();
        assert_eq!(m2.remaining_budget().cpu, initial.cpu - (90 + 80 + 20));
    }
}
