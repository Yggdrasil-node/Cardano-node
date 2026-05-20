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

use serde::de::{DeserializeOwned, Error as DeError};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

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

/// Mirror of upstream `NetworkId` JSON used by tx-generator scripts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkId {
    /// Upstream `Mainnet`.
    Mainnet,
    /// Upstream `Testnet (NetworkMagic n)`.
    Testnet(u64),
}

impl Serialize for NetworkId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Mainnet => serializer.serialize_str("Mainnet"),
            Self::Testnet(magic) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Testnet", magic)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for NetworkId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(value) if value == "Mainnet" => Ok(Self::Mainnet),
            Value::Object(mut fields) if fields.len() == 1 => {
                if let Some(magic) = fields.remove("Testnet") {
                    return u64::deserialize(magic)
                        .map(Self::Testnet)
                        .map_err(D::Error::custom);
                }
                Err(D::Error::custom(format!(
                    "could not parse NetworkId: {}",
                    Value::Object(fields)
                )))
            }
            other => Err(D::Error::custom(format!(
                "could not parse NetworkId: {other}"
            ))),
        }
    }
}

/// Mirror of upstream `Action`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `SetNetworkId`.
    SetNetworkId(NetworkId),
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

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) = object_with_single_field::<D::Error>(value, "Action")?;
        match tag.as_str() {
            "SetNetworkId" => Ok(Self::SetNetworkId(parse_payload(payload, &tag)?)),
            "SetSocketPath" => Ok(Self::SetSocketPath(parse_payload(payload, &tag)?)),
            "InitWallet" => Ok(Self::InitWallet(parse_payload(payload, &tag)?)),
            "StartProtocol" => {
                let (config, tracer) = parse_payload(payload, &tag)?;
                Ok(Self::StartProtocol(config, tracer))
            }
            "Delay" => Ok(Self::Delay(parse_payload(payload, &tag)?)),
            "ReadSigningKey" => {
                let (name, file) = parse_payload(payload, &tag)?;
                Ok(Self::ReadSigningKey(name, file))
            }
            "DefineSigningKey" => {
                let (name, key) = parse_payload(payload, &tag)?;
                Ok(Self::DefineSigningKey(name, key))
            }
            "AddFund" => {
                let (era, wallet, tx_in, lovelace, key_name) = parse_payload(payload, &tag)?;
                Ok(Self::AddFund(era, wallet, tx_in, lovelace, key_name))
            }
            "WaitBenchmark" => {
                expect_unit_payload(payload, &tag)?;
                Ok(Self::WaitBenchmark)
            }
            "Submit" => {
                let (era, submit_mode, tx_params, generator) = parse_payload(payload, &tag)?;
                Ok(Self::Submit(era, submit_mode, tx_params, generator))
            }
            "CancelBenchmark" => {
                expect_unit_payload(payload, &tag)?;
                Ok(Self::CancelBenchmark)
            }
            "Reserved" => Ok(Self::Reserved(parse_payload(payload, &tag)?)),
            "WaitForEra" => Ok(Self::WaitForEra(parse_payload(payload, &tag)?)),
            "SetProtocolParameters" => {
                Ok(Self::SetProtocolParameters(parse_payload(payload, &tag)?))
            }
            "LogMsg" => Ok(Self::LogMsg(parse_payload(payload, &tag)?)),
            other => Err(D::Error::custom(format!("unknown Action tag `{other}`"))),
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

impl<'de> Deserialize<'de> for Generator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) = object_with_single_field::<D::Error>(value, "Generator")?;
        match tag.as_str() {
            "SecureGenesis" => {
                let (wallet, genesis_key, fund_key) = parse_payload(payload, &tag)?;
                Ok(Self::SecureGenesis(wallet, genesis_key, fund_key))
            }
            "Split" => {
                let (source, pay_mode, change_mode, lovelaces) = parse_payload(payload, &tag)?;
                Ok(Self::Split(source, pay_mode, change_mode, lovelaces))
            }
            "SplitN" => {
                let (source, pay_mode, count) = parse_payload(payload, &tag)?;
                Ok(Self::SplitN(source, pay_mode, count))
            }
            "NtoM" => {
                let (source, pay_mode, inputs, outputs, add_size, collateral) =
                    parse_payload(payload, &tag)?;
                Ok(Self::NtoM(
                    source, pay_mode, inputs, outputs, add_size, collateral,
                ))
            }
            "Sequence" => Ok(Self::Sequence(parse_payload(payload, &tag)?)),
            "Cycle" => Ok(Self::Cycle(Box::new(parse_payload(payload, &tag)?))),
            "Take" => {
                let (count, generator) = parse_payload(payload, &tag)?;
                Ok(Self::Take(count, Box::new(generator)))
            }
            "RoundRobin" => Ok(Self::RoundRobin(parse_payload(payload, &tag)?)),
            "OneOf" => Ok(Self::OneOf(parse_payload(payload, &tag)?)),
            other => Err(D::Error::custom(format!("unknown Generator tag `{other}`"))),
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

impl<'de> Deserialize<'de> for ProtocolParametersSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) =
            object_with_single_field::<D::Error>(value, "ProtocolParametersSource")?;
        match tag.as_str() {
            "QueryLocalNode" => {
                expect_unit_payload(payload, &tag)?;
                Ok(Self::QueryLocalNode)
            }
            "UseLocalProtocolFile" => Ok(Self::UseLocalProtocolFile(parse_payload(payload, &tag)?)),
            other => Err(D::Error::custom(format!(
                "unknown ProtocolParametersSource tag `{other}`"
            ))),
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

impl<'de> Deserialize<'de> for SubmitMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) = object_with_single_field::<D::Error>(value, "SubmitMode")?;
        match tag.as_str() {
            "LocalSocket" => {
                expect_unit_payload(payload, &tag)?;
                Ok(Self::LocalSocket)
            }
            "Benchmark" => {
                let (nodes, tps, tx_count): (Vec<NodeDescription>, TpsRate, NumberOfTxs) =
                    parse_payload(payload, &tag)?;
                if nodes.is_empty() {
                    return Err(D::Error::custom("Benchmark target node list is empty"));
                }
                Ok(Self::Benchmark(nodes, tps, tx_count))
            }
            "DumpToFile" => Ok(Self::DumpToFile(parse_payload(payload, &tag)?)),
            "DiscardTX" => {
                expect_unit_payload(payload, &tag)?;
                Ok(Self::DiscardTx)
            }
            "NodeToNode" => {
                let addresses: Vec<String> = parse_payload(payload, &tag)?;
                if addresses.is_empty() {
                    return Err(D::Error::custom("NodeToNode address list is empty"));
                }
                Ok(Self::NodeToNode(addresses))
            }
            other => Err(D::Error::custom(format!(
                "unknown SubmitMode tag `{other}`"
            ))),
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

impl<'de> Deserialize<'de> for PayMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) = object_with_single_field::<D::Error>(value, "PayMode")?;
        match tag.as_str() {
            "PayToAddr" => {
                let (key, wallet) = parse_payload(payload, &tag)?;
                Ok(Self::PayToAddr(key, wallet))
            }
            "PayToScript" => {
                let (spec, wallet) = parse_payload(payload, &tag)?;
                Ok(Self::PayToScript(spec, wallet))
            }
            other => Err(D::Error::custom(format!("unknown PayMode tag `{other}`"))),
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

impl<'de> Deserialize<'de> for ScriptBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let (tag, payload) = object_with_single_field::<D::Error>(value, "ScriptBudget")?;
        match tag.as_str() {
            "StaticScriptBudget" => {
                let (datum, redeemer, units, debug) = parse_payload(payload, &tag)?;
                Ok(Self::StaticScriptBudget(datum, redeemer, units, debug))
            }
            "AutoScript" => {
                let (redeemer, inputs) = parse_payload(payload, &tag)?;
                Ok(Self::AutoScript(redeemer, inputs))
            }
            other => Err(D::Error::custom(format!(
                "unknown ScriptBudget tag `{other}`"
            ))),
        }
    }
}

/// Mirror of upstream `ScriptSpec`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

fn object_with_single_field<E>(value: Value, type_name: &str) -> Result<(String, Value), E>
where
    E: DeError,
{
    match value {
        Value::Object(fields) if fields.len() == 1 => {
            let (tag, payload) = fields.into_iter().next().expect("one field");
            Ok((tag, payload))
        }
        Value::Object(fields) => Err(E::custom(format!(
            "{type_name} must be an ObjectWithSingleField, got {} fields",
            fields.len()
        ))),
        other => Err(E::custom(format!(
            "{type_name} must be an ObjectWithSingleField object, got {other}"
        ))),
    }
}

fn parse_payload<T, E>(payload: Value, tag: &str) -> Result<T, E>
where
    T: DeserializeOwned,
    E: DeError,
{
    serde_json::from_value(payload).map_err(|err| E::custom(format!("{tag}: {err}")))
}

fn expect_unit_payload<E>(payload: Value, tag: &str) -> Result<(), E>
where
    E: DeError,
{
    match payload {
        Value::Array(items) if items.is_empty() => Ok(()),
        other => Err(E::custom(format!(
            "{tag}: expected empty array for nullary constructor, got {other}"
        ))),
    }
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
