//! State for the transaction-generator action interpreter.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Env.hs`.
//! Ports the `Env` state shape, `ProtocolParameterMode`, `Error`
//! constructors, and accessor semantics needed by
//! `Cardano.Benchmarking.Script.Action.action`. Consensus protocol,
//! genesis, wallet queue internals, tracers, and async benchmark
//! handles are represented by Rust-side typed carriers until the later
//! `Script/Core` and `GeneratorTx` runtime slices wire the real node
//! machinery.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::script::types::{NetworkId, SigningKeyEnvelope};
pub use crate::tx_generator::fund::Fund;
pub use crate::wallet::WalletRef;

/// Mirror of upstream `ProtocolParameterMode`.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolParameterMode {
    /// `ProtocolParameterQuery`.
    ProtocolParameterQuery,
    /// `ProtocolParameterLocal`.
    ProtocolParameterLocal(Value),
}

/// Placeholder for upstream `SomeConsensusProtocol`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolHandle {
    /// Node configuration file used to initialise the protocol.
    pub config_file: PathBuf,
    /// Optional cardano-tracer socket path.
    pub tracer_socket: Option<PathBuf>,
}

/// Placeholder for upstream `ShelleyGenesis`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisHandle {
    /// Node configuration file that led to the genesis.
    pub config_file: PathBuf,
}

/// Placeholder for upstream `BenchTracers`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BenchTracers {
    messages: Vec<String>,
}

impl BenchTracers {
    /// Append a trace message.
    pub fn push(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
    }

    /// Return accumulated trace messages.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

/// Placeholder for upstream `AsyncBenchmarkControl`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AsyncBenchmarkControl {
    /// Whether shutdown has been requested.
    pub shutdown_requested: bool,
}

/// Mirror of upstream `Env`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Env {
    /// Upstream `protoParams`.
    pub proto_params: Option<ProtocolParameterMode>,
    /// Upstream `envGenesis`.
    pub env_genesis: Option<GenesisHandle>,
    /// Upstream `envProtocol`.
    pub env_protocol: Option<ProtocolHandle>,
    /// Upstream `envNetworkId`.
    pub env_network_id: Option<NetworkId>,
    /// Upstream `envSocketPath`.
    pub env_socket_path: Option<PathBuf>,
    /// Upstream `envKeys`.
    pub env_keys: BTreeMap<String, SigningKeyEnvelope>,
    /// Upstream `envWallets`.
    pub env_wallets: BTreeMap<String, WalletRef>,
    /// Upstream `envSummary`.
    pub env_summary: Option<Value>,
    /// Upstream `benchTracers` reader-state slot.
    pub bench_tracers: Option<BenchTracers>,
    /// Upstream `envThreads` reader-state slot.
    pub env_threads: Option<AsyncBenchmarkControl>,
}

impl Env {
    /// Mirror of upstream `emptyEnv`.
    pub fn empty_env() -> Self {
        Self::default()
    }
}

/// Mirror of upstream `Error`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// Upstream `TxGenError`.
    #[error("TxGenError: {0}")]
    TxGenError(String),
    /// Upstream `UserError`.
    #[error("UserError: {0}")]
    UserError(String),
    /// Upstream `WalletError`.
    #[error("WalletError: {0}")]
    WalletError(String),
}

/// Mirror of upstream `liftTxGenError`.
pub fn lift_tx_gen_error(message: impl Into<String>) -> Error {
    Error::TxGenError(message.into())
}

/// Mirror of upstream `setProtoParamMode`.
pub fn set_proto_param_mode(env: &mut Env, value: ProtocolParameterMode) {
    env.proto_params = Some(value);
}

/// Mirror of upstream `getProtoParamMode`.
pub fn get_proto_param_mode(env: &Env) -> Result<&ProtocolParameterMode, Error> {
    get_env_val(env.proto_params.as_ref(), "ProtocolParameterMode")
}

/// Mirror of upstream `setBenchTracers`.
pub fn set_bench_tracers(env: &mut Env, value: BenchTracers) {
    env.bench_tracers = Some(value);
}

/// Mirror of upstream `getBenchTracers`.
pub fn get_bench_tracers(env: &Env) -> Result<&BenchTracers, Error> {
    get_env_val(env.bench_tracers.as_ref(), "BenchTracers")
}

/// Mirror of upstream `setEnvGenesis`.
pub fn set_env_genesis(env: &mut Env, value: GenesisHandle) {
    env.env_genesis = Some(value);
}

/// Mirror of upstream `getEnvGenesis`.
pub fn get_env_genesis(env: &Env) -> Result<&GenesisHandle, Error> {
    get_env_val(env.env_genesis.as_ref(), "Genesis")
}

/// Mirror of upstream `setEnvProtocol`.
pub fn set_env_protocol(env: &mut Env, value: ProtocolHandle) {
    env.env_protocol = Some(value);
}

/// Mirror of upstream `getEnvProtocol`.
pub fn get_env_protocol(env: &Env) -> Result<&ProtocolHandle, Error> {
    get_env_val(env.env_protocol.as_ref(), "Protocol")
}

/// Mirror of upstream `setEnvNetworkId`.
pub fn set_env_network_id(env: &mut Env, value: NetworkId) {
    env.env_network_id = Some(value);
}

/// Mirror of upstream `getEnvNetworkId`.
pub fn get_env_network_id(env: &Env) -> Result<&NetworkId, Error> {
    get_env_val(env.env_network_id.as_ref(), "Genesis")
}

/// Mirror of upstream `setEnvSocketPath`.
pub fn set_env_socket_path(env: &mut Env, value: PathBuf) {
    env.env_socket_path = Some(value);
}

/// Mirror of upstream `getEnvSocketPath`.
pub fn get_env_socket_path(env: &Env) -> Result<&PathBuf, Error> {
    get_env_val(env.env_socket_path.as_ref(), "SocketPath")
}

/// Mirror of upstream `setEnvKeys`.
pub fn set_env_keys(env: &mut Env, key: impl Into<String>, value: SigningKeyEnvelope) {
    env.env_keys.insert(key.into(), value);
}

/// Mirror of upstream `getEnvKeys`.
pub fn get_env_keys<'a>(env: &'a Env, key: &str) -> Result<&'a SigningKeyEnvelope, Error> {
    get_env_map(&env.env_keys, key)
}

/// Mirror of upstream `setEnvWallets`.
pub fn set_env_wallets(env: &mut Env, key: impl Into<String>, value: WalletRef) {
    env.env_wallets.insert(key.into(), value);
}

/// Mirror of upstream `getEnvWallets`.
pub fn get_env_wallets<'a>(env: &'a Env, key: &str) -> Result<&'a WalletRef, Error> {
    get_env_map(&env.env_wallets, key)
}

/// Mutable mirror of upstream `getEnvWallets` for Rust wallet updates.
pub fn get_env_wallets_mut<'a>(env: &'a mut Env, key: &str) -> Result<&'a mut WalletRef, Error> {
    if let Some(value) = env.env_wallets.get_mut(key) {
        Ok(value)
    } else {
        Err(Error::UserError(format!("Lookup of {key} failed")))
    }
}

/// Mirror of upstream `setEnvSummary`.
pub fn set_env_summary(env: &mut Env, value: Value) {
    env.env_summary = Some(value);
}

/// Mirror of upstream `getEnvSummary`.
pub fn get_env_summary(env: &Env) -> Option<&Value> {
    env.env_summary.as_ref()
}

/// Mirror of upstream `setEnvThreads`.
pub fn set_env_threads(env: &mut Env, value: AsyncBenchmarkControl) {
    env.env_threads = Some(value);
}

/// Mirror of upstream `getEnvThreads`.
pub fn get_env_threads(env: &Env) -> Option<&AsyncBenchmarkControl> {
    env.env_threads.as_ref()
}

/// Mutable mirror of upstream `getEnvThreads` for cancellation.
pub fn get_env_threads_mut(env: &mut Env) -> Option<&mut AsyncBenchmarkControl> {
    env.env_threads.as_mut()
}

/// Mirror of upstream `traceBenchTxSubmit` for debug/error text.
pub fn trace_bench_tx_submit(env: &mut Env, message: impl Into<String>) {
    let mut tracers = env.bench_tracers.take().unwrap_or_default();
    tracers.push(message);
    set_bench_tracers(env, tracers);
}

/// Mirror of upstream `traceError`.
pub fn trace_error(env: &mut Env, message: &str) {
    trace_bench_tx_submit(env, format!("ERROR: {message}"));
}

/// Mirror of upstream `traceDebug`.
pub fn trace_debug(env: &mut Env, message: &str) {
    trace_bench_tx_submit(env, message.to_string());
}

fn get_env_val<'a, T>(value: Option<&'a T>, name: &str) -> Result<&'a T, Error> {
    value.ok_or_else(|| Error::UserError(format!("Unset {name}")))
}

fn get_env_map<'a, T>(map: &'a BTreeMap<String, T>, key: &str) -> Result<&'a T, Error> {
    map.get(key)
        .ok_or_else(|| Error::UserError(format!("Lookup of {key} failed")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnyCardanoEra;

    #[test]
    fn empty_env_matches_upstream_maybe_and_map_defaults() {
        let env = Env::empty_env();

        assert_eq!(
            get_env_network_id(&env),
            Err(Error::UserError("Unset Genesis".to_string()))
        );
        assert!(env.env_keys.is_empty());
        assert!(env.env_wallets.is_empty());
        assert!(env.env_threads.is_none());
    }

    #[test]
    fn map_accessors_match_upstream_lookup_error_shape() {
        let env = Env::empty_env();

        assert_eq!(
            get_env_keys(&env, "missing"),
            Err(Error::UserError("Lookup of missing failed".to_string()))
        );
    }

    #[test]
    fn wallet_ref_preserves_insert_order() {
        let mut wallet = WalletRef::default();
        wallet.insert_fund(Fund {
            era: AnyCardanoEra::Conway,
            tx_in: "a#0".to_string(),
            lovelace: 1,
            key_name: "key-a".to_string(),
        });
        wallet.insert_fund(Fund {
            era: AnyCardanoEra::Conway,
            tx_in: "b#1".to_string(),
            lovelace: 2,
            key_name: "key-b".to_string(),
        });

        let funds = wallet.funds();
        assert_eq!(funds[0].tx_in, "a#0");
        assert_eq!(funds[1].tx_in, "b#1");
    }

    #[test]
    fn trace_helpers_initialize_and_append_messages() {
        let mut env = Env::empty_env();

        trace_debug(&mut env, "debug");
        trace_error(&mut env, "bad");

        assert_eq!(
            env.bench_tracers.expect("tracers").messages(),
            ["debug", "ERROR: bad"]
        );
    }
}
