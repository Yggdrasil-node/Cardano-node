//! CEK machine — the UPLC evaluator.
//!
//! Implements a standard CEK (Control-Environment-Continuation) machine for
//! the Untyped Plutus Lambda Calculus. Built-in functions are saturated via
//! partial application with arity tracking.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek.hs>

use crate::builtins::evaluate_builtin;
use crate::cost_model::CostModel;
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
        self.tick()?;
        match term {
            Term::Var(index) => {
                let val = env.lookup(index)?.clone();
                Ok(State::Returning(val))
            }
            Term::LamAbs(body) => Ok(State::Returning(Value::Lambda(*body, env))),
            Term::Apply(fun, arg) => {
                self.frames.push(Frame::ApplyArg(env.clone(), *arg));
                Ok(State::Computing(*fun, env))
            }
            Term::Delay(body) => Ok(State::Returning(Value::Delay(*body, env))),
            Term::Force(inner) => {
                self.frames.push(Frame::Force);
                Ok(State::Computing(*inner, env))
            }
            Term::Constant(c) => Ok(State::Returning(Value::Constant(c))),
            Term::Builtin(fun) => Ok(State::Returning(Value::BuiltinApp {
                fun,
                forces: 0,
                args: Vec::new(),
            })),
            Term::Error => Err(MachineError::EvaluationFailure),
            Term::Constr(tag, fields) => {
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
                    _ => Err(MachineError::TypeMismatch {
                        expected: "constr",
                        actual: value.type_name().to_string(),
                    }),
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
        self.tick()?;
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
        self.tick()?;
        match value {
            Value::Delay(body, env) => Ok(State::Computing(body, env)),
            Value::BuiltinApp {
                fun,
                forces,
                args,
            } => {
                let (needed_forces, needed_args) = fun.arity();
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

    fn tick(&mut self) -> Result<(), MachineError> {
        self.steps += 1;
        if self.steps > self.max_steps {
            return Err(MachineError::OutOfBudget(format!(
                "exceeded max steps ({})",
                self.max_steps
            )));
        }
        let step_cost = self.cost_model.machine_step_cost();
        self.budget.spend(step_cost)
    }
}
