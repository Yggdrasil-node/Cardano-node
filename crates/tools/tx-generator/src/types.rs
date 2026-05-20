//! Types internal to the transaction generator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Types.hs`.
//! Ports the high-level configuration types needed by
//! `Setup/NixService.hs`. Transaction construction types land in later
//! `GeneratorTx` slices.

use std::path::PathBuf;

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

/// Mirror of upstream `NumberOfInputsPerTx`.
pub type NumberOfInputsPerTx = usize;

/// Mirror of upstream `NumberOfOutputsPerTx`.
pub type NumberOfOutputsPerTx = usize;

/// Mirror of upstream `NumberOfTxs`.
pub type NumberOfTxs = usize;

/// Mirror of upstream `TxAdditionalSize`.
pub type TxAdditionalSize = usize;

/// Mirror of upstream `TPSRate`.
pub type TpsRate = f64;

/// Lovelace amount used where upstream stores `Coin`.
pub type Lovelace = u64;

/// Slot number used by upstream `TxGenTxParams`.
pub type SlotNo = u64;

/// Mirror of upstream `TxGenError`.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TxGenError {
    /// Upstream `ApiError`.
    #[error("ApiError ({0})")]
    ApiError(String),
    /// Upstream `ProtocolError`.
    #[error("ProtocolError ({0})")]
    ProtocolError(String),
    /// Upstream `PlutusError`.
    #[error("ProtocolError ({0})")]
    PlutusError(String),
    /// Upstream `TxGenError`.
    #[error("ApiError ({0:?})")]
    TxGenError(String),
}

/// Mirror of upstream `PayWithChange`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PayWithChange {
    /// Upstream `PayExact [Coin]`.
    PayExact(Vec<Lovelace>),
    /// Upstream `PayWithChange Coin [Coin]`.
    PayWithChange(Lovelace, Vec<Lovelace>),
}

/// Cardano era accepted by the upstream high-level JSON config.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AnyCardanoEra {
    /// Byron era.
    Byron,
    /// Shelley era.
    Shelley,
    /// Allegra era.
    Allegra,
    /// Mary era.
    Mary,
    /// Alonzo era.
    Alonzo,
    /// Babbage era.
    Babbage,
    /// Conway era.
    Conway,
    /// Dijkstra era.
    Dijkstra,
}

/// Mirror of upstream `TxGenTxParams`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TxGenTxParams {
    /// Transaction fee in lovelace.
    #[serde(rename = "txParamFee")]
    pub tx_param_fee: Lovelace,
    /// Additional payload size in bytes.
    #[serde(rename = "txParamAddTxSize")]
    pub tx_param_add_tx_size: TxAdditionalSize,
    /// Transaction time-to-live slot.
    #[serde(rename = "txParamTTL")]
    pub tx_param_ttl: SlotNo,
}

/// Defaults taken from upstream `defaultTxGenTxParams`.
pub const DEFAULT_TX_GEN_TX_PARAMS: TxGenTxParams = TxGenTxParams {
    tx_param_fee: 10_000_000,
    tx_param_add_tx_size: 100,
    tx_param_ttl: 1_000_000,
};

/// Mirror of upstream `TxGenConfig`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TxGenConfig {
    /// Minimum lovelace required per UTxO entry.
    #[serde(rename = "confMinUtxoValue")]
    pub conf_min_utxo_value: Lovelace,
    /// Target transactions per second.
    #[serde(rename = "confTxsPerSecond")]
    pub conf_txs_per_second: TpsRate,
    /// Initial cooldown in seconds.
    #[serde(rename = "confInitCooldown")]
    pub conf_init_cooldown: f64,
    /// Inputs per transaction.
    #[serde(rename = "confTxsInputs")]
    pub conf_txs_inputs: NumberOfInputsPerTx,
    /// Outputs per transaction.
    #[serde(rename = "confTxsOutputs")]
    pub conf_txs_outputs: NumberOfOutputsPerTx,
}

/// Mirror of upstream `TxGenPlutusType`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TxGenPlutusType {
    /// Generate Plutus loop transactions that saturate per-transaction limits.
    LimitSaturationLoop,
    /// Generate Plutus loop transactions that fit eight transactions per block.
    #[serde(rename = "LimitTxPerBlock_8")]
    LimitTxPerBlock8,
    /// Generate Plutus loop transactions that fit four transactions per block.
    #[serde(rename = "LimitTxPerBlock_4")]
    LimitTxPerBlock4,
    /// Built-in custom-call benchmark script.
    BenchCustomCall,
    /// Operator-provided Plutus script.
    CustomScript,
}

/// JSON representation of upstream `Either String FilePath` for Plutus scripts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlutusScriptRef {
    /// Upstream `Left String`.
    Named(String),
    /// Upstream `Right FilePath`.
    File(PathBuf),
}

/// Mirror of upstream `TxGenPlutusResolvedTo`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxGenPlutusResolvedTo {
    /// `ResolvedToLibrary`.
    ResolvedToLibrary(String),
    /// `ResolvedToFallback`.
    ResolvedToFallback(PathBuf),
    /// `ResolvedToFileName`.
    ResolvedToFileName(PathBuf),
}

impl fmt::Display for TxGenPlutusResolvedTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResolvedToLibrary(name) => write!(f, "builtin: {name}"),
            Self::ResolvedToFallback(path) => write!(f, "fallback: {}", path.display()),
            Self::ResolvedToFileName(path) => write!(f, "file: {}", path.display()),
        }
    }
}

impl<'de> Deserialize<'de> for PlutusScriptRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Object(mut obj) => {
                if let Some(left) = obj.remove("Left") {
                    return String::deserialize(left)
                        .map(Self::Named)
                        .map_err(serde::de::Error::custom);
                }
                if let Some(right) = obj.remove("Right") {
                    return PathBuf::deserialize(right)
                        .map(Self::File)
                        .map_err(serde::de::Error::custom);
                }
                Err(serde::de::Error::custom(
                    "expected Either String FilePath object with Left or Right",
                ))
            }
            other => Err(serde::de::Error::custom(format!(
                "expected Either String FilePath object, got {other}"
            ))),
        }
    }
}

impl Serialize for PlutusScriptRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Named(name) => {
                let mut obj = serde_json::Map::new();
                obj.insert("Left".to_string(), Value::String(name.clone()));
                Value::Object(obj).serialize(serializer)
            }
            Self::File(path) => {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "Right".to_string(),
                    Value::String(path.to_string_lossy().into_owned()),
                );
                Value::Object(obj).serialize(serializer)
            }
        }
    }
}

/// Execution unit budget used by static Plutus script settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionUnits {
    /// Execution steps.
    #[serde(rename = "executionSteps")]
    pub execution_steps: u64,
    /// Execution memory.
    #[serde(rename = "executionMemory")]
    pub execution_memory: u64,
}

/// Mirror of upstream `TxGenPlutusParams`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxGenPlutusParams {
    /// Generate Plutus transactions for a given script.
    PlutusOn {
        /// Plutus benchmark mode.
        plutus_type: TxGenPlutusType,
        /// Built-in script name or script file path.
        plutus_script: PlutusScriptRef,
        /// Optional datum JSON file.
        plutus_datum: Option<PathBuf>,
        /// Optional redeemer JSON file.
        plutus_redeemer: Option<PathBuf>,
        /// Optional memory limit override.
        plutus_exec_memory: Option<u64>,
        /// Optional step limit override.
        plutus_exec_steps: Option<u64>,
    },
    /// Do not generate Plutus transactions.
    PlutusOff,
}

#[derive(Deserialize, Serialize)]
struct PlutusOnJson {
    #[serde(rename = "type")]
    plutus_type: TxGenPlutusType,
    #[serde(rename = "script")]
    plutus_script: PlutusScriptRef,
    #[serde(rename = "datum")]
    plutus_datum: Option<PathBuf>,
    #[serde(rename = "redeemer")]
    plutus_redeemer: Option<PathBuf>,
    #[serde(rename = "limitExecutionMem")]
    plutus_exec_memory: Option<u64>,
    #[serde(rename = "limitExecutionSteps")]
    plutus_exec_steps: Option<u64>,
}

impl<'de> Deserialize<'de> for TxGenPlutusParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<PlutusOnJson>::deserialize(deserializer)?;
        Ok(match value {
            Some(on) => Self::PlutusOn {
                plutus_type: on.plutus_type,
                plutus_script: on.plutus_script,
                plutus_datum: on.plutus_datum,
                plutus_redeemer: on.plutus_redeemer,
                plutus_exec_memory: on.plutus_exec_memory,
                plutus_exec_steps: on.plutus_exec_steps,
            },
            None => Self::PlutusOff,
        })
    }
}

impl Serialize for TxGenPlutusParams {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::PlutusOff => serializer.serialize_none(),
            Self::PlutusOn {
                plutus_type,
                plutus_script,
                plutus_datum,
                plutus_redeemer,
                plutus_exec_memory,
                plutus_exec_steps,
            } => PlutusOnJson {
                plutus_type: *plutus_type,
                plutus_script: plutus_script.clone(),
                plutus_datum: plutus_datum.clone(),
                plutus_redeemer: plutus_redeemer.clone(),
                plutus_exec_memory: *plutus_exec_memory,
                plutus_exec_steps: *plutus_exec_steps,
            }
            .serialize(serializer),
        }
    }
}

/// Returns true when Plutus mode is active.
pub fn is_plutus_mode(params: &TxGenPlutusParams) -> bool {
    !matches!(params, TxGenPlutusParams::PlutusOff)
}

/// Mirrors upstream `hasLoopCalibration`.
pub fn has_loop_calibration(plutus_type: TxGenPlutusType) -> bool {
    matches!(
        plutus_type,
        TxGenPlutusType::LimitTxPerBlock8
            | TxGenPlutusType::LimitTxPerBlock4
            | TxGenPlutusType::LimitSaturationLoop
    )
}

/// Mirrors upstream `hasStaticBudget`.
pub fn has_static_budget(params: &TxGenPlutusParams) -> Option<ExecutionUnits> {
    match params {
        TxGenPlutusParams::PlutusOn {
            plutus_exec_memory: Some(execution_memory),
            plutus_exec_steps: Some(execution_steps),
            ..
        } => Some(ExecutionUnits {
            execution_steps: *execution_steps,
            execution_memory: *execution_memory,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_tx_gen_tx_params_match_upstream() {
        assert_eq!(DEFAULT_TX_GEN_TX_PARAMS.tx_param_fee, 10_000_000);
        assert_eq!(DEFAULT_TX_GEN_TX_PARAMS.tx_param_add_tx_size, 100);
        assert_eq!(DEFAULT_TX_GEN_TX_PARAMS.tx_param_ttl, 1_000_000);
    }

    #[test]
    fn parses_plutus_off_from_null() {
        let params: TxGenPlutusParams = serde_json::from_value(Value::Null).expect("null parses");
        assert_eq!(params, TxGenPlutusParams::PlutusOff);
        assert!(!is_plutus_mode(&params));
    }

    #[test]
    fn parses_plutus_on_from_service_shape() {
        let params: TxGenPlutusParams = serde_json::from_value(json!({
            "type": "CustomScript",
            "script": { "Right": "scripts/custom.plutus" },
            "datum": "datum.json",
            "redeemer": "redeemer.json",
            "limitExecutionMem": 12,
            "limitExecutionSteps": 34
        }))
        .expect("plutus object parses");

        assert_eq!(
            params,
            TxGenPlutusParams::PlutusOn {
                plutus_type: TxGenPlutusType::CustomScript,
                plutus_script: PlutusScriptRef::File(PathBuf::from("scripts/custom.plutus")),
                plutus_datum: Some(PathBuf::from("datum.json")),
                plutus_redeemer: Some(PathBuf::from("redeemer.json")),
                plutus_exec_memory: Some(12),
                plutus_exec_steps: Some(34),
            }
        );
        assert!(is_plutus_mode(&params));
        assert_eq!(
            has_static_budget(&params),
            Some(ExecutionUnits {
                execution_steps: 34,
                execution_memory: 12,
            })
        );
    }

    #[test]
    fn rejects_untagged_plutus_script_reference_like_upstream_either() {
        let err = serde_json::from_value::<TxGenPlutusParams>(json!({
            "type": "CustomScript",
            "script": "scripts/custom.plutus",
            "datum": null,
            "redeemer": null,
            "limitExecutionMem": null,
            "limitExecutionSteps": null
        }))
        .expect_err("bare string is not upstream Either JSON");

        assert!(err.to_string().contains("Either String FilePath"));
    }

    #[test]
    fn loop_calibration_modes_match_upstream_predicate() {
        assert!(has_loop_calibration(TxGenPlutusType::LimitTxPerBlock8));
        assert!(has_loop_calibration(TxGenPlutusType::LimitTxPerBlock4));
        assert!(has_loop_calibration(TxGenPlutusType::LimitSaturationLoop));
        assert!(!has_loop_calibration(TxGenPlutusType::BenchCustomCall));
        assert!(!has_loop_calibration(TxGenPlutusType::CustomScript));
    }
}
