//! Plutus budgeting and script-data helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/PlutusContext.hs`.
//! Ports the script-data loading, `scriptDataModifyNumber`, and Plutus
//! auto-budget fitting helpers consumed by `Script/Core.makePlutusContext`.

use std::fmt;
use std::path::Path;

use num_bigint::BigInt;
use serde_json::{Value, json};
use yggdrasil_ledger::{PlutusData, ProtocolParameters, eras::alonzo::ExUnits};

use crate::setup::plutus::pre_execute_plutus_script;
use crate::tx_generator::utxo::ScriptInAnyLang;
use crate::types::ExecutionUnits;

/// Mirror of upstream `PlutusAutoLimitingFactor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlutusAutoLimitingFactor {
    /// The candidate loop count exceeded the target memory budget.
    ExceededMemoryLimit,
    /// The candidate loop count exceeded the target execution-step budget.
    ExceededStepLimit,
}

impl PlutusAutoLimitingFactor {
    fn as_upstream_str(self) -> &'static str {
        match self {
            Self::ExceededMemoryLimit => "ExceededMemoryLimit",
            Self::ExceededStepLimit => "ExceededStepLimit",
        }
    }
}

/// Mirror of upstream `PlutusBudgetFittingStrategy`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlutusBudgetFittingStrategy {
    /// Fit against the per-transaction execution budget.
    TargetTxExpenditure,
    /// Fit against the per-block budget, optionally using a scaling factor.
    TargetBlockExpenditure(Option<f64>),
    /// Fit against a fixed target number of transactions per block.
    TargetTxsPerBlock(usize),
}

impl fmt::Display for PlutusBudgetFittingStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetTxExpenditure => f.write_str("txbudget"),
            Self::TargetBlockExpenditure(None) => f.write_str("blockbudget"),
            Self::TargetBlockExpenditure(Some(factor)) => {
                write!(f, "blockbudget_{factor}")
            }
            Self::TargetTxsPerBlock(count) => write!(f, "txperblock_{count}"),
        }
    }
}

/// Mirror of upstream `PlutusAutoBudget`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlutusAutoBudget {
    /// Target execution units used by the fitted redeemer.
    pub auto_budget_units: ExecutionUnits,
    /// Datum passed to the Plutus loop script.
    pub auto_budget_datum: PlutusData,
    /// Redeemer containing the fitted loop counter.
    pub auto_budget_redeemer: PlutusData,
    /// Optional upper bound for the binary search.
    pub auto_budget_upper_bound_hint: Option<usize>,
}

/// Mirror of upstream `PlutusBudgetSummary`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlutusBudgetSummary {
    /// Maximum execution units per block.
    pub budget_per_block: ExecutionUnits,
    /// Maximum execution units per transaction.
    pub budget_per_tx: ExecutionUnits,
    /// Maximum execution units per transaction input.
    pub budget_per_tx_input: ExecutionUnits,
    /// Strategy used to fit the loop budget.
    pub budget_strategy: PlutusBudgetFittingStrategy,
    /// Target budget selected for each transaction input.
    pub budget_target: ExecutionUnits,
    /// Script identifier from upstream `TxGenPlutusResolvedTo`.
    pub script_id: String,
    /// Datum argument used during fitting.
    pub script_arg_datum: PlutusData,
    /// Redeemer argument used during fitting.
    pub script_arg_redeemer: PlutusData,
    /// Fitted loop counter.
    pub loop_counter: usize,
    /// Limiting factor(s) observed immediately above the fitted loop counter.
    pub loop_limiting_factors: Vec<PlutusAutoLimitingFactor>,
    /// Measured execution units for one script input.
    pub budget_used_per_tx_input: ExecutionUnits,
    /// Projected unused execution units per block.
    pub projected_budget_unused_per_block: ExecutionUnits,
    /// Projected unused execution units per transaction.
    pub projected_budget_unused_per_tx: ExecutionUnits,
    /// Projected transaction count per block.
    pub projected_tx_per_block: usize,
    /// Projected script-loop count per block.
    pub projected_loops_per_block: usize,
    /// Transaction size projection, filled by the preview path when available.
    pub projected_tx_size: Option<usize>,
    /// Transaction fee projection, filled by the preview path when available.
    pub projected_tx_fee: Option<u64>,
    /// Human-readable strategy selection note.
    pub message_strategy: Option<String>,
    /// Stable summary identifier built from the selected strategy.
    pub message_id: String,
}

impl PlutusBudgetSummary {
    /// Render the summary with upstream `PlutusBudgetSummary` field names.
    pub fn to_json_value(&self) -> Value {
        json!({
            "budgetPerBlock": self.budget_per_block,
            "budgetPerTx": self.budget_per_tx,
            "budgetPerTxInput": self.budget_per_tx_input,
            "budgetStrategy": strategy_to_json(self.budget_strategy),
            "budgetTarget": self.budget_target,
            "scriptId": self.script_id,
            "scriptArgDatum": script_data_to_json_detailed_schema(&self.script_arg_datum),
            "scriptArgRedeemer": script_data_to_json_detailed_schema(&self.script_arg_redeemer),
            "loopCounter": self.loop_counter,
            "loopLimitingFactors": self.loop_limiting_factors
                .iter()
                .map(|factor| Value::String(factor.as_upstream_str().to_string()))
                .collect::<Vec<_>>(),
            "budgetUsedPerTxInput": self.budget_used_per_tx_input,
            "projectedBudgetUnusedPerBlock": self.projected_budget_unused_per_block,
            "projectedBudgetUnusedPerTx": self.projected_budget_unused_per_tx,
            "projectedTxPerBlock": self.projected_tx_per_block,
            "projectedLoopsPerBlock": self.projected_loops_per_block,
            "projectedTxSize": self.projected_tx_size,
            "projectedTxFee": self.projected_tx_fee,
            "messageStrategy": self.message_strategy,
            "messageId": self.message_id,
        })
    }

    fn with_message_strategy(mut self, message: impl Into<String>) -> Self {
        self.message_strategy = Some(message.into());
        self
    }
}

/// Mirror of upstream `readScriptData`.
pub fn read_script_data(path: &Path) -> Result<PlutusData, String> {
    if path.as_os_str().is_empty() {
        return Ok(PlutusData::integer(0));
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("readScriptData: {}: {err}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| format!("readScriptData: {}: {err}", path.display()))?;
    script_data_from_json_detailed_schema(&value)
}

/// Parse Cardano API's `ScriptDataJsonDetailedSchema` representation.
pub fn script_data_from_json_detailed_schema(value: &Value) -> Result<PlutusData, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "ScriptDataJsonDetailedSchema: expected object".to_string())?;

    if let Some(int) = object.get("int") {
        return parse_script_data_integer(int).map(PlutusData::integer);
    }
    if let Some(bytes) = object.get("bytes") {
        let hex = bytes
            .as_str()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.bytes: expected string".to_string())?;
        return hex::decode(hex)
            .map(PlutusData::Bytes)
            .map_err(|err| format!("ScriptDataJsonDetailedSchema.bytes: invalid hex: {err}"));
    }
    if let Some(list) = object.get("list") {
        let values = list
            .as_array()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.list: expected array".to_string())?
            .iter()
            .map(script_data_from_json_detailed_schema)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(PlutusData::List(values));
    }
    if let Some(map) = object.get("map") {
        let entries = map
            .as_array()
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: expected array".to_string())?
            .iter()
            .map(|entry| {
                let entry = entry.as_object().ok_or_else(|| {
                    "ScriptDataJsonDetailedSchema.map: expected object entry".to_string()
                })?;
                let key = entry
                    .get("k")
                    .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: missing k".to_string())?;
                let value = entry
                    .get("v")
                    .ok_or_else(|| "ScriptDataJsonDetailedSchema.map: missing v".to_string())?;
                Ok((
                    script_data_from_json_detailed_schema(key)?,
                    script_data_from_json_detailed_schema(value)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(PlutusData::Map(entries));
    }

    if let Some(constructor) = object.get("constructor") {
        let alt = parse_u64_field(constructor, "ScriptDataJsonDetailedSchema.constructor")?;
        let fields = object
            .get("fields")
            .ok_or_else(|| "ScriptDataJsonDetailedSchema.constructor: missing fields".to_string())?
            .as_array()
            .ok_or_else(|| {
                "ScriptDataJsonDetailedSchema.constructor.fields: expected array".to_string()
            })?
            .iter()
            .map(script_data_from_json_detailed_schema)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(PlutusData::Constr(alt, fields));
    }

    Err(
        "ScriptDataJsonDetailedSchema: expected one of int, bytes, list, map, constructor"
            .to_string(),
    )
}

/// Mirror of upstream `scriptDataModifyNumber`.
pub fn script_data_modify_number(data: &PlutusData, f: impl Fn(&BigInt) -> BigInt) -> PlutusData {
    fn go(data: &PlutusData, f: &dyn Fn(&BigInt) -> BigInt) -> PlutusData {
        match data {
            PlutusData::Integer(value) => PlutusData::Integer(f(value)),
            PlutusData::Constr(alt, fields) => PlutusData::Constr(*alt, go_list(fields, f)),
            PlutusData::List(values) => PlutusData::List(go_list(values, f)),
            PlutusData::Map(entries) => {
                let values = entries
                    .iter()
                    .map(|(_key, value)| value.clone())
                    .collect::<Vec<_>>();
                let changed_values = go_list(&values, f);
                PlutusData::Map(
                    entries
                        .iter()
                        .zip(changed_values)
                        .map(|((key, _), value)| (key.clone(), value))
                        .collect(),
                )
            }
            PlutusData::Bytes(bytes) => PlutusData::Bytes(bytes.clone()),
        }
    }

    fn go_list(values: &[PlutusData], f: &dyn Fn(&BigInt) -> BigInt) -> Vec<PlutusData> {
        let mut out = Vec::with_capacity(values.len());
        for (idx, value) in values.iter().enumerate() {
            let changed = go(value, f);
            if changed == *value {
                out.push(value.clone());
            } else {
                out.push(changed);
                out.extend_from_slice(&values[idx + 1..]);
                return out;
            }
        }
        out
    }

    go(data, &f)
}

/// Mirror of upstream `plutusAutoScaleBlockfit`.
pub fn plutus_auto_scale_blockfit(
    protocol_parameters: &ProtocolParameters,
    script_info: (String, String),
    script: &ScriptInAnyLang,
    auto_budget: PlutusAutoBudget,
    strategy: PlutusBudgetFittingStrategy,
    tx_inputs: usize,
) -> Result<(PlutusBudgetSummary, PlutusAutoBudget, ExecutionUnits), String> {
    let scaling_strategies = match strategy {
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(None) => {
            vec![1.0, 1.25, 1.5, 1.75, 2.0]
                .into_iter()
                .map(|factor| PlutusBudgetFittingStrategy::TargetBlockExpenditure(Some(factor)))
                .collect::<Vec<_>>()
        }
        other => vec![other],
    };

    let mut results = Vec::with_capacity(scaling_strategies.len());
    for scaling_strategy in scaling_strategies {
        let result = plutus_auto_budget_max_out(
            protocol_parameters,
            script,
            auto_budget.clone(),
            scaling_strategy,
            tx_inputs,
        )?;
        let pre_run = pre_execute_plutus_script(
            protocol_parameters,
            script,
            &result.0.auto_budget_datum,
            &result.0.auto_budget_redeemer,
        )?;
        let summary = plutus_budget_summary(
            protocol_parameters,
            script_info.clone(),
            scaling_strategy,
            (&result.0, result.1, &result.2),
            &pre_run,
            tx_inputs,
        )?;
        results.push((summary, result.0, pre_run));
    }

    let (max_index, max_loops) = results
        .iter()
        .enumerate()
        .max_by_key(|(_, (summary, _, _))| summary.projected_loops_per_block)
        .ok_or_else(|| "plutusAutoScaleBlockfit: no scaling strategies".to_string())?;
    let (min_index, _) = results
        .iter()
        .enumerate()
        .min_by_key(|(_, (summary, _, _))| {
            summary.projected_budget_unused_per_block.execution_steps
        })
        .ok_or_else(|| "plutusAutoScaleBlockfit: no scaling strategies".to_string())?;

    let message = match strategy {
        PlutusBudgetFittingStrategy::TargetTxExpenditure => {
            "maxing out loops for tx budget was indicated".to_string()
        }
        PlutusBudgetFittingStrategy::TargetTxsPerBlock(target) => {
            format!("a fixed {target} txs per block was specified")
        }
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(_) if max_index == min_index => {
            format!(
                "{} maximizes loops per block AND minimizes unused execution steps per block",
                max_loops.0.budget_strategy
            )
        }
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(_) => {
            format!(
                "{} maximizes loops per block BUT DOES NOT minimize unused execution steps per block",
                max_loops.0.budget_strategy
            )
        }
    };

    let mut selected = results.swap_remove(max_index);
    selected.0 = selected.0.with_message_strategy(message);
    Ok(selected)
}

/// Mirror of upstream `plutusAutoBudgetMaxOut`.
pub fn plutus_auto_budget_max_out(
    protocol_parameters: &ProtocolParameters,
    script: &ScriptInAnyLang,
    auto_budget: PlutusAutoBudget,
    strategy: PlutusBudgetFittingStrategy,
    tx_inputs: usize,
) -> Result<(PlutusAutoBudget, usize, Vec<PlutusAutoLimitingFactor>), String> {
    if matches!(
        strategy,
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(None)
    ) {
        return Err(
            "plutusAutoBudgetMaxOut : a scaling factor is required for TargetBlockExpenditure"
                .to_string(),
        );
    }
    if tx_inputs == 0 {
        return Err("plutusAutoBudgetMaxOut : txInputs must be positive".to_string());
    }

    let (budget_per_block, budget_per_tx) = max_execution_unit_budgets(protocol_parameters)?;
    let target_budget = match strategy {
        PlutusBudgetFittingStrategy::TargetTxExpenditure => {
            execution_units_div(budget_per_tx, tx_inputs, "TargetTxExpenditure")?
        }
        PlutusBudgetFittingStrategy::TargetTxsPerBlock(target) => {
            if target == 0 {
                return Err(
                    "plutusAutoBudgetMaxOut : TargetTxsPerBlock must be positive".to_string(),
                );
            }
            let divisor = target.checked_mul(tx_inputs).ok_or_else(|| {
                "plutusAutoBudgetMaxOut : target input count overflow".to_string()
            })?;
            let per_input = execution_units_div(budget_per_block, divisor, "TargetTxsPerBlock")?;
            execution_units_min(per_input, budget_per_tx)
        }
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(Some(scaling_factor)) => {
            let target_txs = target_tx_per_block(budget_per_block, budget_per_tx, scaling_factor)?;
            let divisor = target_txs.checked_mul(tx_inputs).ok_or_else(|| {
                "plutusAutoBudgetMaxOut : target input count overflow".to_string()
            })?;
            execution_units_div(budget_per_block, divisor, "TargetBlockExpenditure")?
        }
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(None) => {
            return Err(
                "plutusAutoBudgetMaxOut : TargetBlockExpenditure Nothing should be unreachable. This is an implementation error in tx-generator."
                    .to_string(),
            );
        }
    };
    let upper_bound = auto_budget.auto_budget_upper_bound_hint.unwrap_or(16_000);

    let (fitted_loop_count, limiting_factors) = binary_search(
        |loop_count| {
            let candidate_redeemer = to_loop_argument(&auto_budget, loop_count);
            let used = match pre_execute_plutus_script(
                protocol_parameters,
                script,
                &auto_budget.auto_budget_datum,
                &candidate_redeemer,
            ) {
                Ok(used) => used,
                Err(err) if err.contains("out of budget") => {
                    return Ok(vec![
                        PlutusAutoLimitingFactor::ExceededStepLimit,
                        PlutusAutoLimitingFactor::ExceededMemoryLimit,
                    ]);
                }
                Err(err) => return Err(err),
            };
            let mut factors = Vec::new();
            if used.execution_steps > target_budget.execution_steps {
                factors.push(PlutusAutoLimitingFactor::ExceededStepLimit);
            }
            if used.execution_memory > target_budget.execution_memory {
                factors.push(PlutusAutoLimitingFactor::ExceededMemoryLimit);
            }
            Ok(factors)
        },
        0,
        upper_bound,
    )?;

    let fitted_redeemer = to_loop_argument(&auto_budget, fitted_loop_count);

    Ok((
        PlutusAutoBudget {
            auto_budget_units: target_budget,
            auto_budget_datum: auto_budget.auto_budget_datum,
            auto_budget_redeemer: fitted_redeemer,
            auto_budget_upper_bound_hint: auto_budget.auto_budget_upper_bound_hint,
        },
        fitted_loop_count,
        limiting_factors,
    ))
}

/// Mirror of upstream `plutusBudgetSummary`.
pub fn plutus_budget_summary(
    protocol_parameters: &ProtocolParameters,
    script_info: (String, String),
    strategy: PlutusBudgetFittingStrategy,
    result: (&PlutusAutoBudget, usize, &[PlutusAutoLimitingFactor]),
    pre_run: &ExecutionUnits,
    tx_inputs: usize,
) -> Result<PlutusBudgetSummary, String> {
    if tx_inputs == 0 {
        return Err("plutusBudgetSummary : txInputs must be positive".to_string());
    }
    let (budget_per_block, budget_per_tx) = max_execution_unit_budgets(protocol_parameters)?;
    let budget_per_tx_input = execution_units_div(budget_per_tx, tx_inputs, "plutusBudgetSummary")?;
    let used_per_tx = execution_units_mul(*pre_run, tx_inputs, "plutusBudgetSummary")?;
    let projected_tx_per_block = projected_tx_per_block(budget_per_block, used_per_tx)?;
    let projected_loops_per_block = result
        .1
        .checked_mul(tx_inputs)
        .and_then(|value| value.checked_mul(projected_tx_per_block))
        .ok_or_else(|| "plutusBudgetSummary : projected loop count overflow".to_string())?;
    let projected_budget_unused_per_tx =
        execution_units_minus(budget_per_tx, used_per_tx, "plutusBudgetSummary")?;
    let used_per_block =
        execution_units_mul(used_per_tx, projected_tx_per_block, "plutusBudgetSummary")?;
    let projected_budget_unused_per_block =
        execution_units_minus(budget_per_block, used_per_block, "plutusBudgetSummary")?;

    Ok(PlutusBudgetSummary {
        budget_per_block,
        budget_per_tx,
        budget_per_tx_input,
        budget_strategy: strategy,
        budget_target: result.0.auto_budget_units,
        script_id: script_info.0,
        script_arg_datum: result.0.auto_budget_datum.clone(),
        script_arg_redeemer: result.0.auto_budget_redeemer.clone(),
        loop_counter: result.1,
        loop_limiting_factors: result.2.to_vec(),
        budget_used_per_tx_input: *pre_run,
        projected_budget_unused_per_block,
        projected_budget_unused_per_tx,
        projected_tx_per_block,
        projected_loops_per_block,
        projected_tx_size: None,
        projected_tx_fee: None,
        message_strategy: None,
        message_id: script_info.1,
    })
}

/// Render Cardano API's `ScriptDataJsonDetailedSchema` representation.
pub fn script_data_to_json_detailed_schema(data: &PlutusData) -> Value {
    match data {
        PlutusData::Integer(value) => json!({ "int": value.to_string() }),
        PlutusData::Bytes(bytes) => json!({ "bytes": hex::encode(bytes) }),
        PlutusData::List(values) => json!({
            "list": values.iter().map(script_data_to_json_detailed_schema).collect::<Vec<_>>()
        }),
        PlutusData::Map(entries) => json!({
            "map": entries
                .iter()
                .map(|(key, value)| {
                    json!({
                        "k": script_data_to_json_detailed_schema(key),
                        "v": script_data_to_json_detailed_schema(value),
                    })
                })
                .collect::<Vec<_>>()
        }),
        PlutusData::Constr(alt, fields) => json!({
            "constructor": alt,
            "fields": fields.iter().map(script_data_to_json_detailed_schema).collect::<Vec<_>>()
        }),
    }
}

fn parse_script_data_integer(value: &Value) -> Result<BigInt, String> {
    match value {
        Value::Number(number) => {
            if let Some(n) = number.as_i64() {
                Ok(BigInt::from(n))
            } else if let Some(n) = number.as_u64() {
                Ok(BigInt::from(n))
            } else {
                Err("ScriptDataJsonDetailedSchema.int: floating values are not valid".to_string())
            }
        }
        Value::String(text) => text
            .parse::<BigInt>()
            .map_err(|err| format!("ScriptDataJsonDetailedSchema.int: invalid integer: {err}")),
        _ => {
            Err("ScriptDataJsonDetailedSchema.int: expected integer or decimal string".to_string())
        }
    }
}

fn parse_u64_field(value: &Value, field: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("{field}: expected unsigned integer"))
}

fn strategy_to_json(strategy: PlutusBudgetFittingStrategy) -> Value {
    match strategy {
        PlutusBudgetFittingStrategy::TargetTxExpenditure => {
            json!({ "tag": "TargetTxExpenditure" })
        }
        PlutusBudgetFittingStrategy::TargetBlockExpenditure(factor) => {
            json!({ "tag": "TargetBlockExpenditure", "contents": factor })
        }
        PlutusBudgetFittingStrategy::TargetTxsPerBlock(count) => {
            json!({ "tag": "TargetTxsPerBlock", "contents": count })
        }
    }
}

fn max_execution_unit_budgets(
    protocol_parameters: &ProtocolParameters,
) -> Result<(ExecutionUnits, ExecutionUnits), String> {
    let budget_per_block = protocol_parameters.max_block_ex_units.ok_or_else(|| {
        "plutusAutoBudgetMaxOut : call to function in pre-Alonzo era. This is an implementation error in tx-generator.".to_string()
    })?;
    let budget_per_tx = protocol_parameters.max_tx_ex_units.ok_or_else(|| {
        "plutusAutoBudgetMaxOut : call to function in pre-Alonzo era. This is an implementation error in tx-generator.".to_string()
    })?;
    Ok((
        execution_units_from_ex_units(budget_per_block),
        execution_units_from_ex_units(budget_per_tx),
    ))
}

fn execution_units_from_ex_units(units: ExUnits) -> ExecutionUnits {
    ExecutionUnits {
        execution_steps: units.steps,
        execution_memory: units.mem,
    }
}

fn execution_units_div(
    units: ExecutionUnits,
    divisor: usize,
    context: &str,
) -> Result<ExecutionUnits, String> {
    if divisor == 0 {
        return Err(format!(
            "{context} : execution unit divisor must be positive"
        ));
    }
    let divisor = u64::try_from(divisor)
        .map_err(|_| format!("{context} : execution unit divisor overflow"))?;
    Ok(ExecutionUnits {
        execution_steps: units.execution_steps / divisor,
        execution_memory: units.execution_memory / divisor,
    })
}

fn execution_units_mul(
    units: ExecutionUnits,
    factor: usize,
    context: &str,
) -> Result<ExecutionUnits, String> {
    let factor =
        u64::try_from(factor).map_err(|_| format!("{context} : execution unit factor overflow"))?;
    Ok(ExecutionUnits {
        execution_steps: units
            .execution_steps
            .checked_mul(factor)
            .ok_or_else(|| format!("{context} : execution steps overflow"))?,
        execution_memory: units
            .execution_memory
            .checked_mul(factor)
            .ok_or_else(|| format!("{context} : execution memory overflow"))?,
    })
}

fn execution_units_minus(
    left: ExecutionUnits,
    right: ExecutionUnits,
    context: &str,
) -> Result<ExecutionUnits, String> {
    Ok(ExecutionUnits {
        execution_steps: left
            .execution_steps
            .checked_sub(right.execution_steps)
            .ok_or_else(|| format!("{context} : execution steps underflow"))?,
        execution_memory: left
            .execution_memory
            .checked_sub(right.execution_memory)
            .ok_or_else(|| format!("{context} : execution memory underflow"))?,
    })
}

fn execution_units_min(left: ExecutionUnits, right: ExecutionUnits) -> ExecutionUnits {
    ExecutionUnits {
        execution_steps: left.execution_steps.min(right.execution_steps),
        execution_memory: left.execution_memory.min(right.execution_memory),
    }
}

fn target_tx_per_block(
    budget_per_block: ExecutionUnits,
    budget_per_tx: ExecutionUnits,
    scaling_factor: f64,
) -> Result<usize, String> {
    if !scaling_factor.is_finite() || scaling_factor <= 0.0 {
        return Err("plutusAutoBudgetMaxOut : scaling factor must be positive".to_string());
    }
    if budget_per_tx.execution_steps == 0 || budget_per_tx.execution_memory == 0 {
        return Err("plutusAutoBudgetMaxOut : per-transaction budget must be positive".to_string());
    }
    let steps_ratio =
        budget_per_block.execution_steps as f64 / budget_per_tx.execution_steps as f64;
    let memory_ratio =
        budget_per_block.execution_memory as f64 / budget_per_tx.execution_memory as f64;
    let txs = (scaling_factor * steps_ratio.max(memory_ratio)).ceil();
    if !txs.is_finite() || txs <= 0.0 || txs > usize::MAX as f64 {
        return Err(
            "plutusAutoBudgetMaxOut : target transactions per block out of range".to_string(),
        );
    }
    Ok(txs as usize)
}

fn projected_tx_per_block(
    budget_per_block: ExecutionUnits,
    used_per_tx: ExecutionUnits,
) -> Result<usize, String> {
    if used_per_tx.execution_steps == 0 || used_per_tx.execution_memory == 0 {
        return Err("plutusBudgetSummary : used execution units must be positive".to_string());
    }
    let by_steps = budget_per_block.execution_steps / used_per_tx.execution_steps;
    let by_memory = budget_per_block.execution_memory / used_per_tx.execution_memory;
    usize::try_from(by_steps.min(by_memory))
        .map_err(|_| "plutusBudgetSummary : projected tx count overflow".to_string())
}

fn to_loop_argument(auto_budget: &PlutusAutoBudget, loop_count: usize) -> PlutusData {
    let loop_count = BigInt::from(loop_count);
    script_data_modify_number(&auto_budget.auto_budget_redeemer, |value| {
        value + &loop_count
    })
}

fn binary_search<F>(
    mut predicate: F,
    low: usize,
    high: usize,
) -> Result<(usize, Vec<PlutusAutoLimitingFactor>), String>
where
    F: FnMut(usize) -> Result<Vec<PlutusAutoLimitingFactor>, String>,
{
    let low_factors = predicate(low)?;
    let high_factors = predicate(high)?;
    let low_inside_limits = low_factors.is_empty();
    let high_inside_limits = high_factors.is_empty();
    if !low_inside_limits || high_inside_limits {
        return Err(format!(
            "binarySearch: bad initial bounds: ({low},{},{high},{})",
            bool_haskell(low_inside_limits),
            bool_haskell(high_inside_limits)
        ));
    }

    let mut a = low;
    let mut b = high;
    let mut b_factors = high_factors;
    while a + 1 < b {
        let midpoint = (a + b) / 2;
        let midpoint_factors = predicate(midpoint)?;
        if midpoint_factors.is_empty() {
            a = midpoint;
        } else {
            b = midpoint;
            b_factors = midpoint_factors;
        }
    }
    Ok((a, b_factors))
}

fn bool_haskell(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::setup::plutus::read_plutus_script;
    use crate::tx_generator::utxo::ScriptLanguage;
    use crate::types::PlutusScriptRef;
    use serde_json::{Value, json};

    #[test]
    fn empty_script_data_path_is_integer_zero() {
        assert_eq!(
            read_script_data(Path::new("")).expect("empty value"),
            PlutusData::integer(0)
        );
    }

    #[test]
    fn detailed_schema_parses_constructor_map_list_bytes_and_int() {
        let value = json!({
            "constructor": 1,
            "fields": [
                {"int": "18446744073709551616"},
                {"bytes": "aabb"},
                {"list": [{"int": -1}]},
                {"map": [{"k": {"bytes": "00"}, "v": {"int": 7}}]}
            ]
        });

        assert_eq!(
            script_data_from_json_detailed_schema(&value).expect("script data"),
            PlutusData::Constr(
                1,
                vec![
                    PlutusData::integer("18446744073709551616".parse::<BigInt>().expect("big int")),
                    PlutusData::Bytes(vec![0xaa, 0xbb]),
                    PlutusData::List(vec![PlutusData::integer(-1)]),
                    PlutusData::Map(vec![(PlutusData::Bytes(vec![0]), PlutusData::integer(7))]),
                ],
            )
        );
    }

    #[test]
    fn script_data_modify_number_updates_first_changed_value_only() {
        let data = PlutusData::Map(vec![
            (PlutusData::integer(1), PlutusData::Bytes(vec![1])),
            (
                PlutusData::integer(2),
                PlutusData::List(vec![PlutusData::integer(3)]),
            ),
            (PlutusData::integer(4), PlutusData::integer(5)),
        ]);

        let changed = script_data_modify_number(&data, |n| n + 10);

        assert_eq!(
            changed,
            PlutusData::Map(vec![
                (PlutusData::integer(1), PlutusData::Bytes(vec![1])),
                (
                    PlutusData::integer(2),
                    PlutusData::List(vec![PlutusData::integer(13)])
                ),
                (PlutusData::integer(4), PlutusData::integer(5)),
            ])
        );
    }

    #[test]
    fn fitting_strategy_display_matches_upstream_show() {
        assert_eq!(
            PlutusBudgetFittingStrategy::TargetTxExpenditure.to_string(),
            "txbudget"
        );
        assert_eq!(
            PlutusBudgetFittingStrategy::TargetBlockExpenditure(None).to_string(),
            "blockbudget"
        );
        assert_eq!(
            PlutusBudgetFittingStrategy::TargetBlockExpenditure(Some(1.25)).to_string(),
            "blockbudget_1.25"
        );
        assert_eq!(
            PlutusBudgetFittingStrategy::TargetTxsPerBlock(8).to_string(),
            "txperblock_8"
        );
    }

    #[test]
    fn binary_search_returns_largest_in_limit_value() {
        let (value, factors) = binary_search(
            |candidate| {
                if candidate > 7 {
                    Ok(vec![PlutusAutoLimitingFactor::ExceededStepLimit])
                } else {
                    Ok(Vec::new())
                }
            },
            0,
            16,
        )
        .expect("binary search");

        assert_eq!(value, 7);
        assert_eq!(factors, vec![PlutusAutoLimitingFactor::ExceededStepLimit]);
    }

    #[test]
    fn binary_search_reports_haskell_shaped_bad_bounds() {
        let err = binary_search(|_| Ok(Vec::new()), 0, 16).expect_err("bad bounds");

        assert_eq!(err, "binarySearch: bad initial bounds: (0,True,16,True)");
    }

    #[test]
    fn auto_budget_max_out_rejects_missing_block_scaling_factor_first() {
        let err = plutus_auto_budget_max_out(
            &ProtocolParameters::alonzo_defaults(),
            &ScriptInAnyLang::new(ScriptLanguage::PlutusV1, Vec::new()),
            PlutusAutoBudget {
                auto_budget_units: ExecutionUnits {
                    execution_steps: 1,
                    execution_memory: 1,
                },
                auto_budget_datum: PlutusData::integer(0),
                auto_budget_redeemer: PlutusData::integer(0),
                auto_budget_upper_bound_hint: None,
            },
            PlutusBudgetFittingStrategy::TargetBlockExpenditure(None),
            1,
        )
        .expect_err("missing scaling factor");

        assert_eq!(
            err,
            "plutusAutoBudgetMaxOut : a scaling factor is required for TargetBlockExpenditure"
        );
    }

    #[test]
    fn budget_summary_json_uses_upstream_field_names() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.max_block_ex_units = Some(ExUnits {
            mem: 100,
            steps: 1000,
        });
        params.max_tx_ex_units = Some(ExUnits {
            mem: 20,
            steps: 200,
        });
        let auto_budget = PlutusAutoBudget {
            auto_budget_units: ExecutionUnits {
                execution_steps: 100,
                execution_memory: 10,
            },
            auto_budget_datum: PlutusData::integer(0),
            auto_budget_redeemer: PlutusData::integer(42),
            auto_budget_upper_bound_hint: None,
        };

        let summary = plutus_budget_summary(
            &params,
            (
                "ResolvedToFallback \"Loop\"".to_string(),
                "txbudget".to_string(),
            ),
            PlutusBudgetFittingStrategy::TargetTxExpenditure,
            (
                &auto_budget,
                42,
                &[PlutusAutoLimitingFactor::ExceededStepLimit],
            ),
            &ExecutionUnits {
                execution_steps: 25,
                execution_memory: 5,
            },
            2,
        )
        .expect("summary");
        let value = summary.to_json_value();

        assert_eq!(value["budgetPerBlock"]["executionSteps"], 1000);
        assert_eq!(value["budgetPerTxInput"]["executionMemory"], 10);
        assert_eq!(value["scriptArgRedeemer"], json!({ "int": "42" }));
        assert_eq!(value["projectedTxPerBlock"], 10);
        assert_eq!(value["projectedLoopsPerBlock"], 840);
        assert_eq!(value["loopLimitingFactors"], json!(["ExceededStepLimit"]));
    }

    #[test]
    fn auto_scale_blockfit_fits_named_loop_budget() {
        let params = protocol_parameters_with_v1_cost_model();
        let (script, resolved_to) =
            read_plutus_script(&PlutusScriptRef::Named("Loop".to_string())).expect("loop script");
        let max_tx_units = params.max_tx_ex_units.expect("max tx units");
        let auto_budget = PlutusAutoBudget {
            auto_budget_units: ExecutionUnits {
                execution_steps: max_tx_units.steps,
                execution_memory: max_tx_units.mem,
            },
            auto_budget_datum: PlutusData::integer(0),
            auto_budget_redeemer: PlutusData::integer(1_000_000),
            auto_budget_upper_bound_hint: None,
        };

        let (summary, fitted_budget, pre_run) = plutus_auto_scale_blockfit(
            &params,
            (
                resolved_to.to_string(),
                PlutusBudgetFittingStrategy::TargetTxsPerBlock(8).to_string(),
            ),
            &script,
            auto_budget,
            PlutusBudgetFittingStrategy::TargetTxsPerBlock(8),
            1,
        )
        .expect("fit loop budget");

        assert!(summary.loop_counter > 0);
        assert_eq!(
            fitted_budget.auto_budget_redeemer,
            summary.script_arg_redeemer
        );
        assert_eq!(summary.budget_used_per_tx_input, pre_run);
        assert_eq!(
            summary.message_strategy.as_deref(),
            Some("a fixed 8 txs per block was specified")
        );
    }

    fn protocol_parameters_with_v1_cost_model() -> ProtocolParameters {
        let raw: Value = serde_json::from_str(include_str!("../../data/protocol-parameters.json"))
            .expect("protocol parameters JSON");
        let v1_model: Vec<i64> =
            serde_json::from_value(raw["costModels"]["PlutusV1"].clone()).expect("V1 model");
        let mut cost_models = BTreeMap::new();
        cost_models.insert(0, v1_model);
        let mut params = ProtocolParameters::alonzo_defaults();
        params.protocol_version = Some((6, 0));
        params.cost_models = Some(cost_models);
        params.max_tx_ex_units = Some(ExUnits {
            mem: raw["maxTxExecutionUnits"]["memory"]
                .as_u64()
                .expect("max tx mem"),
            steps: raw["maxTxExecutionUnits"]["steps"]
                .as_u64()
                .expect("max tx steps"),
        });
        params.max_block_ex_units = Some(ExUnits {
            mem: raw["maxBlockExecutionUnits"]["memory"]
                .as_u64()
                .expect("max block mem"),
            steps: raw["maxBlockExecutionUnits"]["steps"]
                .as_u64()
                .expect("max block steps"),
        });
        params
    }
}
