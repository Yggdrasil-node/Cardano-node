//! Types used within transaction generator scripts.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Types.hs`.
//! Ports the script action/generator IR consumed by
//! `Cardano.Benchmarking.Compiler.compileOptions`. Concrete
//! `runScript` execution lands in later `Script` / `GeneratorTx`
//! slices.

use std::path::PathBuf;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::setup::nix_service::NodeDescription;
use crate::types::{
    AnyCardanoEra, ExecutionUnits, Lovelace, NumberOfInputsPerTx, NumberOfOutputsPerTx,
    NumberOfTxs, PlutusScriptRef, TpsRate, TxGenPlutusType, TxGenTxParams,
};

/// Mirror of upstream `type Script = [Action]`.
pub type Script = Vec<Action>;

/// Text-envelope representation of a payment signing key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SigningKeyEnvelope {
    /// TextEnvelope type tag.
    #[serde(rename = "type")]
    pub envelope_type: String,
    /// TextEnvelope description.
    pub description: String,
    /// TextEnvelope raw CBOR bytes encoded as hex.
    #[serde(rename = "cborHex")]
    pub cbor_hex: String,
}

impl SigningKeyEnvelope {
    /// Construct an upstream-shaped Shelley payment signing-key envelope.
    pub fn payment_signing_key_shelley(cbor_hex: impl Into<String>) -> Self {
        Self {
            envelope_type: "PaymentSigningKeyShelley_ed25519".to_string(),
            description: "Payment Signing Key".to_string(),
            cbor_hex: cbor_hex.into(),
        }
    }
}

/// Mirror of upstream `Action`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `SetNetworkId`.
    SetNetworkId(String),
    /// `SetSocketPath`.
    SetSocketPath(PathBuf),
    /// `InitWallet`.
    InitWallet(String),
    /// `StartProtocol`.
    StartProtocol(PathBuf, Option<PathBuf>),
    /// `Delay`.
    Delay(f64),
    /// `ReadSigningKey`.
    ReadSigningKey(String, PathBuf),
    /// `DefineSigningKey`.
    DefineSigningKey(String, SigningKeyEnvelope),
    /// `AddFund`.
    AddFund(AnyCardanoEra, String, String, Lovelace, String),
    /// `WaitBenchmark`.
    WaitBenchmark,
    /// `Submit`.
    Submit(AnyCardanoEra, SubmitMode, TxGenTxParams, Generator),
    /// `CancelBenchmark`.
    CancelBenchmark,
    /// `Reserved`.
    Reserved(Vec<String>),
    /// `WaitForEra`.
    WaitForEra(AnyCardanoEra),
    /// `SetProtocolParameters`.
    SetProtocolParameters(ProtocolParametersSource),
    /// `LogMsg`.
    LogMsg(String),
}

impl Serialize for Action {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::SetNetworkId(network_id) => {
                serialize_single(serializer, "SetNetworkId", network_id)
            }
            Self::SetSocketPath(socket_path) => {
                serialize_single(serializer, "SetSocketPath", socket_path)
            }
            Self::InitWallet(wallet) => serialize_single(serializer, "InitWallet", wallet),
            Self::StartProtocol(config, tracer) => {
                serialize_single(serializer, "StartProtocol", &(config, tracer))
            }
            Self::Delay(seconds) => serialize_single(serializer, "Delay", seconds),
            Self::ReadSigningKey(name, file) => {
                serialize_single(serializer, "ReadSigningKey", &(name, file))
            }
            Self::DefineSigningKey(name, key) => {
                serialize_single(serializer, "DefineSigningKey", &(name, key))
            }
            Self::AddFund(era, wallet, tx_in, lovelace, key_name) => serialize_single(
                serializer,
                "AddFund",
                &(era, wallet, tx_in, lovelace, key_name),
            ),
            Self::WaitBenchmark => serialize_unit(serializer, "WaitBenchmark"),
            Self::Submit(era, submit_mode, tx_params, generator) => serialize_single(
                serializer,
                "Submit",
                &(era, submit_mode, tx_params, generator),
            ),
            Self::CancelBenchmark => serialize_unit(serializer, "CancelBenchmark"),
            Self::Reserved(options) => serialize_single(serializer, "Reserved", options),
            Self::WaitForEra(era) => serialize_single(serializer, "WaitForEra", era),
            Self::SetProtocolParameters(source) => {
                serialize_single(serializer, "SetProtocolParameters", source)
            }
            Self::LogMsg(message) => serialize_single(serializer, "LogMsg", message),
        }
    }
}

/// Mirror of upstream `Generator`.
#[derive(Clone, Debug, PartialEq)]
pub enum Generator {
    /// `SecureGenesis`.
    SecureGenesis(String, String, String),
    /// `Split`.
    Split(String, PayMode, PayMode, Vec<Lovelace>),
    /// `SplitN`.
    SplitN(String, PayMode, usize),
    /// `NtoM`.
    NtoM(
        String,
        PayMode,
        NumberOfInputsPerTx,
        NumberOfOutputsPerTx,
        Option<usize>,
        Option<String>,
    ),
    /// `Sequence`.
    Sequence(Vec<Generator>),
    /// `Cycle`.
    Cycle(Box<Generator>),
    /// `Take`.
    Take(usize, Box<Generator>),
    /// `RoundRobin`.
    RoundRobin(Vec<Generator>),
    /// `OneOf`.
    OneOf(Vec<(Generator, f64)>),
}

impl Serialize for Generator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::SecureGenesis(wallet, genesis_key, fund_key) => serialize_single(
                serializer,
                "SecureGenesis",
                &(wallet, genesis_key, fund_key),
            ),
            Self::Split(source, pay_mode, change_mode, lovelaces) => serialize_single(
                serializer,
                "Split",
                &(source, pay_mode, change_mode, lovelaces),
            ),
            Self::SplitN(source, pay_mode, n) => {
                serialize_single(serializer, "SplitN", &(source, pay_mode, n))
            }
            Self::NtoM(source, pay_mode, inputs, outputs, add_size, collateral) => {
                serialize_single(
                    serializer,
                    "NtoM",
                    &(source, pay_mode, inputs, outputs, add_size, collateral),
                )
            }
            Self::Sequence(generators) => serialize_single(serializer, "Sequence", generators),
            Self::Cycle(generator) => serialize_single(serializer, "Cycle", generator),
            Self::Take(count, generator) => {
                serialize_single(serializer, "Take", &(count, generator))
            }
            Self::RoundRobin(generators) => serialize_single(serializer, "RoundRobin", generators),
            Self::OneOf(generators) => serialize_single(serializer, "OneOf", generators),
        }
    }
}

/// Mirror of upstream `ProtocolParametersSource`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolParametersSource {
    /// `QueryLocalNode`.
    QueryLocalNode,
    /// `UseLocalProtocolFile`.
    UseLocalProtocolFile(PathBuf),
}

impl Serialize for ProtocolParametersSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::QueryLocalNode => serialize_unit(serializer, "QueryLocalNode"),
            Self::UseLocalProtocolFile(path) => {
                serialize_single(serializer, "UseLocalProtocolFile", path)
            }
        }
    }
}

/// Mirror of upstream `SubmitMode`.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitMode {
    /// `LocalSocket`.
    LocalSocket,
    /// `Benchmark`.
    Benchmark(Vec<NodeDescription>, TpsRate, NumberOfTxs),
    /// `DumpToFile`.
    DumpToFile(PathBuf),
    /// `DiscardTX`.
    DiscardTx,
    /// Deprecated upstream `NodeToNode`.
    NodeToNode(Vec<String>),
}

impl Serialize for SubmitMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::LocalSocket => serialize_unit(serializer, "LocalSocket"),
            Self::Benchmark(nodes, tps, tx_count) => {
                serialize_single(serializer, "Benchmark", &(nodes, tps, tx_count))
            }
            Self::DumpToFile(path) => serialize_single(serializer, "DumpToFile", path),
            Self::DiscardTx => serialize_unit(serializer, "DiscardTX"),
            Self::NodeToNode(addresses) => serialize_single(serializer, "NodeToNode", addresses),
        }
    }
}

/// Mirror of upstream `PayMode`.
#[derive(Clone, Debug, PartialEq)]
pub enum PayMode {
    /// `PayToAddr`.
    PayToAddr(String, String),
    /// `PayToScript`.
    PayToScript(ScriptSpec, String),
}

impl Serialize for PayMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::PayToAddr(key, wallet) => {
                serialize_single(serializer, "PayToAddr", &(key, wallet))
            }
            Self::PayToScript(spec, wallet) => {
                serialize_single(serializer, "PayToScript", &(spec, wallet))
            }
        }
    }
}

/// Mirror of upstream `ScriptBudget`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptBudget {
    /// `StaticScriptBudget`.
    StaticScriptBudget(PathBuf, PathBuf, ExecutionUnits, bool),
    /// `AutoScript`.
    AutoScript(PathBuf, NumberOfInputsPerTx),
}

impl Serialize for ScriptBudget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::StaticScriptBudget(datum, redeemer, units, debug) => serialize_single(
                serializer,
                "StaticScriptBudget",
                &(datum, redeemer, units, debug),
            ),
            Self::AutoScript(redeemer, inputs) => {
                serialize_single(serializer, "AutoScript", &(redeemer, inputs))
            }
        }
    }
}

/// Mirror of upstream `ScriptSpec`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScriptSpec {
    /// Upstream `scriptSpecFile`.
    #[serde(rename = "scriptSpecFile")]
    pub script_spec_file: PlutusScriptRef,
    /// Upstream `scriptSpecBudget`.
    #[serde(rename = "scriptSpecBudget")]
    pub script_spec_budget: ScriptBudget,
    /// Upstream `scriptSpecPlutusType`.
    #[serde(rename = "scriptSpecPlutusType")]
    pub script_spec_plutus_type: TxGenPlutusType,
}

/// Pretty-print a script using a deterministic JSON layout.
pub fn pretty_print(script: &[Action]) -> Result<String, serde_json::Error> {
    let mut rendered = serde_json::to_string_pretty(script)?;
    rendered.push('\n');
    Ok(rendered)
}

fn serialize_single<S, T>(serializer: S, tag: &'static str, payload: &T) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + ?Sized,
{
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry(tag, payload)?;
    map.end()
}

fn serialize_unit<S>(serializer: S, tag: &'static str) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let empty: [(); 0] = [];
    serialize_single(serializer, tag, &empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signing_key_envelope_matches_text_envelope_shape() {
        let envelope = SigningKeyEnvelope::payment_signing_key_shelley("5820abcd");

        assert_eq!(
            serde_json::to_value(envelope).expect("envelope serializes"),
            json!({
                "type": "PaymentSigningKeyShelley_ed25519",
                "description": "Payment Signing Key",
                "cborHex": "5820abcd"
            })
        );
    }

    #[test]
    fn action_serialization_uses_object_with_single_field_shape() {
        let action = Action::Submit(
            AnyCardanoEra::Conway,
            SubmitMode::LocalSocket,
            TxGenTxParams {
                tx_param_fee: 1,
                tx_param_add_tx_size: 2,
                tx_param_ttl: 3,
            },
            Generator::SecureGenesis(
                "wallet".to_string(),
                "GenesisInputFund".to_string(),
                "TxGenFunds".to_string(),
            ),
        );

        assert_eq!(
            serde_json::to_value(action).expect("action serializes"),
            json!({
                "Submit": [
                    "Conway",
                    { "LocalSocket": [] },
                    {
                        "txParamFee": 1,
                        "txParamAddTxSize": 2,
                        "txParamTTL": 3
                    },
                    {
                        "SecureGenesis": [
                            "wallet",
                            "GenesisInputFund",
                            "TxGenFunds"
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn pretty_print_adds_trailing_newline() {
        let rendered = pretty_print(&[Action::Delay(1.5)]).expect("pretty print");

        assert!(rendered.ends_with('\n'));
        assert!(rendered.contains("\"Delay\": 1.5"));
    }
}
