//! Nix-service high-level configuration for `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Setup/NixService.hs`.
//! Ports `NixServiceOptions`, `NodeDescription`, keepalive helpers,
//! node-config overrides, and the `txGen*` projection helpers needed
//! before `Compiler.hs` can compile high-level options to scripts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::types::{
    AnyCardanoEra, DEFAULT_TX_GEN_TX_PARAMS, Lovelace, NumberOfInputsPerTx, NumberOfOutputsPerTx,
    NumberOfTxs, TpsRate, TxAdditionalSize, TxGenConfig, TxGenPlutusParams, TxGenTxParams,
};

/// Mirror of upstream `defaultKeepaliveTimeout`.
pub const DEFAULT_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(30);

/// Mirror of upstream `NodeDescription` JSON shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NodeDescription {
    /// IPv4 address string from the `addr` JSON field.
    pub addr: String,
    /// Node-to-node port.
    pub port: u16,
    /// Node alias from the optional `name` JSON field.
    pub name: String,
}

impl<'de> Deserialize<'de> for NodeDescription {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawNodeDescription {
            addr: String,
            port: u16,
            name: Option<String>,
        }

        let raw = RawNodeDescription::deserialize(deserializer)?;
        let name = raw
            .name
            .unwrap_or_else(|| format!("{}:{}", raw.addr, raw.port));
        Ok(Self {
            addr: raw.addr,
            port: raw.port,
            name,
        })
    }
}

/// Mirror of upstream `NixServiceOptions`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NixServiceOptions {
    /// Upstream `_nix_debugMode`.
    #[serde(rename = "debugMode")]
    pub nix_debug_mode: bool,
    /// Upstream `_nix_tx_count`.
    #[serde(rename = "tx_count")]
    pub nix_tx_count: NumberOfTxs,
    /// Upstream `_nix_tps`.
    #[serde(rename = "tps")]
    pub nix_tps: TpsRate,
    /// Upstream `_nix_inputs_per_tx`.
    #[serde(rename = "inputs_per_tx")]
    pub nix_inputs_per_tx: NumberOfInputsPerTx,
    /// Upstream `_nix_outputs_per_tx`.
    #[serde(rename = "outputs_per_tx")]
    pub nix_outputs_per_tx: NumberOfOutputsPerTx,
    /// Upstream `_nix_tx_fee`.
    #[serde(rename = "tx_fee")]
    pub nix_tx_fee: Lovelace,
    /// Upstream `_nix_min_utxo_value`.
    #[serde(rename = "min_utxo_value")]
    pub nix_min_utxo_value: Lovelace,
    /// Upstream `_nix_add_tx_size`.
    #[serde(rename = "add_tx_size")]
    pub nix_add_tx_size: TxAdditionalSize,
    /// Upstream `_nix_init_cooldown`.
    #[serde(rename = "init_cooldown")]
    pub nix_init_cooldown: f64,
    /// Upstream `_nix_era`.
    #[serde(rename = "era")]
    pub nix_era: AnyCardanoEra,
    /// Upstream `_nix_plutus`.
    #[serde(rename = "plutus")]
    pub nix_plutus: Option<TxGenPlutusParams>,
    /// Upstream `_nix_keepalive`.
    #[serde(rename = "keepalive")]
    pub nix_keepalive: Option<u64>,
    /// Upstream `_nix_nodeConfigFile`.
    #[serde(rename = "nodeConfigFile")]
    pub nix_node_config_file: Option<PathBuf>,
    /// Upstream `_nix_cardanoTracerSocket`.
    #[serde(rename = "cardanoTracerSocket")]
    pub nix_cardano_tracer_socket: Option<PathBuf>,
    /// Upstream `_nix_sigKey`.
    #[serde(rename = "sigKey")]
    pub nix_sig_key: PathBuf,
    /// Upstream `_nix_localNodeSocketPath`.
    #[serde(rename = "localNodeSocketPath")]
    pub nix_local_node_socket_path: PathBuf,
    /// Upstream `_nix_targetNodes`.
    #[serde(
        rename = "targetNodes",
        deserialize_with = "deserialize_non_empty_nodes"
    )]
    pub nix_target_nodes: Vec<NodeDescription>,
}

/// Errors from Nix-service option handling.
#[derive(Debug, thiserror::Error)]
pub enum NixServiceError {
    /// High-level config JSON did not match `NixServiceOptions`.
    #[error("NixServiceOptions JSON parse failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Neither config JSON nor CLI override supplied a node config file.
    #[error("No node-configFile set")]
    MissingNodeConfigFile,
}

/// Parse high-level config JSON from a raw string.
pub fn parse_nix_service_options_str(raw: &str) -> Result<NixServiceOptions, NixServiceError> {
    Ok(serde_json::from_str(raw)?)
}

/// Parse high-level config JSON from an already-scanned Aeson value.
pub fn parse_nix_service_options_value(value: Value) -> Result<NixServiceOptions, NixServiceError> {
    Ok(serde_json::from_value(value)?)
}

/// Mirror of upstream `getKeepaliveTimeout`.
pub fn get_keepalive_timeout(opts: &NixServiceOptions) -> Duration {
    opts.nix_keepalive
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_KEEPALIVE_TIMEOUT)
}

/// Mirror of upstream `getNodeAlias`, comparing only host addresses.
pub fn get_node_alias(opts: &NixServiceOptions, addr: &str) -> Option<String> {
    opts.nix_target_nodes
        .iter()
        .find(|node| node.addr == addr)
        .map(|node| node.name.clone())
}

/// Mirror of upstream `getNodeConfigFile`.
pub fn get_node_config_file(opts: &NixServiceOptions) -> Option<&Path> {
    opts.nix_node_config_file.as_deref()
}

/// Mirror of upstream `setNodeConfigFile`.
pub fn set_node_config_file(opts: &mut NixServiceOptions, file_path: PathBuf) {
    opts.nix_node_config_file = Some(file_path);
}

/// Apply the upstream `mangleNodeConfig` command-line override rule.
pub fn mangle_node_config(
    mut opts: NixServiceOptions,
    override_file: Option<PathBuf>,
) -> Result<NixServiceOptions, NixServiceError> {
    match (opts.nix_node_config_file.as_ref(), override_file) {
        (_, Some(file_path)) => {
            set_node_config_file(&mut opts, file_path);
            Ok(opts)
        }
        (Some(_), None) => Ok(opts),
        (None, None) => Err(NixServiceError::MissingNodeConfigFile),
    }
}

/// Apply the upstream `mangleTracerConfig` Maybe-semigroup rule.
pub fn mangle_tracer_config(
    mut opts: NixServiceOptions,
    trace_socket: Option<PathBuf>,
) -> NixServiceOptions {
    opts.nix_cardano_tracer_socket = match (trace_socket, opts.nix_cardano_tracer_socket.take()) {
        (Some(new), Some(old)) => Some(PathBuf::from(format!(
            "{}{}",
            new.to_string_lossy(),
            old.to_string_lossy()
        ))),
        (Some(new), None) => Some(new),
        (None, existing) => existing,
    };
    opts
}

/// Mirror of upstream `txGenTxParams`.
pub fn tx_gen_tx_params(opts: &NixServiceOptions) -> TxGenTxParams {
    TxGenTxParams {
        tx_param_fee: opts.nix_tx_fee,
        tx_param_add_tx_size: opts.nix_add_tx_size,
        tx_param_ttl: DEFAULT_TX_GEN_TX_PARAMS.tx_param_ttl,
    }
}

/// Mirror of upstream `txGenConfig`.
pub fn tx_gen_config(opts: &NixServiceOptions) -> TxGenConfig {
    TxGenConfig {
        conf_min_utxo_value: opts.nix_min_utxo_value,
        conf_txs_per_second: opts.nix_tps,
        conf_init_cooldown: opts.nix_init_cooldown,
        conf_txs_inputs: opts.nix_inputs_per_tx,
        conf_txs_outputs: opts.nix_outputs_per_tx,
    }
}

/// Mirror of upstream `txGenPlutusParams`.
pub fn tx_gen_plutus_params(opts: &NixServiceOptions) -> TxGenPlutusParams {
    opts.nix_plutus
        .clone()
        .unwrap_or(TxGenPlutusParams::PlutusOff)
}

fn deserialize_non_empty_nodes<'de, D>(deserializer: D) -> Result<Vec<NodeDescription>, D::Error>
where
    D: Deserializer<'de>,
{
    let nodes = Vec::<NodeDescription>::deserialize(deserializer)?;
    if nodes.is_empty() {
        Err(serde::de::Error::custom(
            "targetNodes must contain at least one NodeDescription",
        ))
    } else {
        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnyCardanoEra, PlutusScriptRef, TxGenPlutusType};
    use serde_json::json;

    fn full_config() -> Value {
        json!({
            "debugMode": false,
            "tx_count": 100,
            "tps": 10.0,
            "inputs_per_tx": 2,
            "outputs_per_tx": 3,
            "tx_fee": 212345,
            "min_utxo_value": 1000000,
            "add_tx_size": 39,
            "init_cooldown": 50.0,
            "era": "Conway",
            "keepalive": 45,
            "localNodeSocketPath": "/tmp/node.socket",
            "nodeConfigFile": "/tmp/config.json",
            "cardanoTracerSocket": "/tmp/tracer.sock",
            "sigKey": "/tmp/genesis-utxo.skey",
            "targetNodes": [
                {"addr": "127.0.0.1", "port": 30000, "name": "node0"},
                {"addr": "127.0.0.1", "port": 30001, "name": "node1"}
            ],
            "plutus": null
        })
    }

    #[test]
    fn parses_high_level_options_from_upstream_service_shape() {
        let opts = parse_nix_service_options_value(full_config()).expect("config parses");

        assert!(!opts.nix_debug_mode);
        assert_eq!(opts.nix_tx_count, 100);
        assert_eq!(opts.nix_tps, 10.0);
        assert_eq!(opts.nix_inputs_per_tx, 2);
        assert_eq!(opts.nix_outputs_per_tx, 3);
        assert_eq!(opts.nix_tx_fee, 212345);
        assert_eq!(opts.nix_min_utxo_value, 1_000_000);
        assert_eq!(opts.nix_add_tx_size, 39);
        assert_eq!(opts.nix_init_cooldown, 50.0);
        assert_eq!(opts.nix_era, AnyCardanoEra::Conway);
        assert_eq!(get_keepalive_timeout(&opts), Duration::from_secs(45));
        assert_eq!(
            get_node_config_file(&opts),
            Some(Path::new("/tmp/config.json"))
        );
        assert_eq!(
            get_node_alias(&opts, "127.0.0.1"),
            Some("node0".to_string())
        );
        assert_eq!(tx_gen_plutus_params(&opts), TxGenPlutusParams::PlutusOff);
    }

    #[test]
    fn node_description_name_defaults_when_absent() {
        let node: NodeDescription =
            serde_json::from_value(json!({"addr": "10.0.0.1", "port": 30000}))
                .expect("node parses");

        assert_eq!(
            node,
            NodeDescription {
                addr: "10.0.0.1".to_string(),
                port: 30000,
                name: "10.0.0.1:30000".to_string(),
            }
        );
    }

    #[test]
    fn rejects_empty_target_nodes_like_non_empty() {
        let mut config = full_config();
        config["targetNodes"] = json!([]);

        let err = parse_nix_service_options_value(config).expect_err("empty targetNodes fails");
        assert!(err.to_string().contains("targetNodes"));
    }

    #[test]
    fn projection_helpers_match_upstream_nix_service() {
        let opts = parse_nix_service_options_value(full_config()).expect("config parses");

        assert_eq!(
            tx_gen_tx_params(&opts),
            TxGenTxParams {
                tx_param_fee: 212345,
                tx_param_add_tx_size: 39,
                tx_param_ttl: 1_000_000,
            }
        );
        assert_eq!(
            tx_gen_config(&opts),
            TxGenConfig {
                conf_min_utxo_value: 1_000_000,
                conf_txs_per_second: 10.0,
                conf_init_cooldown: 50.0,
                conf_txs_inputs: 2,
                conf_txs_outputs: 3,
            }
        );
    }

    #[test]
    fn mangle_node_config_requires_existing_or_override() {
        let mut opts = parse_nix_service_options_value(full_config()).expect("config parses");
        opts.nix_node_config_file = None;

        let err = mangle_node_config(opts.clone(), None).expect_err("missing config fails");
        assert!(matches!(err, NixServiceError::MissingNodeConfigFile));

        let opts = mangle_node_config(opts, Some(PathBuf::from("override.yaml")))
            .expect("override supplies config");
        assert_eq!(
            opts.nix_node_config_file,
            Some(PathBuf::from("override.yaml"))
        );
    }

    #[test]
    fn mangle_tracer_config_matches_maybe_semigroup_order() {
        let opts = parse_nix_service_options_value(full_config()).expect("config parses");
        let opts = mangle_tracer_config(opts, Some(PathBuf::from("/tmp/override.sock")));

        assert_eq!(
            opts.nix_cardano_tracer_socket,
            Some(PathBuf::from("/tmp/override.sock/tmp/tracer.sock"))
        );
    }

    #[test]
    fn parses_plutus_options_from_service_shape() {
        let mut config = full_config();
        config["plutus"] = json!({
            "type": "LimitSaturationLoop",
            "script": { "Left": "Loop" },
            "datum": "datum.json",
            "redeemer": "redeemer.json",
            "limitExecutionMem": 7,
            "limitExecutionSteps": 11
        });

        let opts = parse_nix_service_options_value(config).expect("config parses");
        assert_eq!(
            tx_gen_plutus_params(&opts),
            TxGenPlutusParams::PlutusOn {
                plutus_type: TxGenPlutusType::LimitSaturationLoop,
                plutus_script: PlutusScriptRef::Named("Loop".to_string()),
                plutus_datum: Some(PathBuf::from("datum.json")),
                plutus_redeemer: Some(PathBuf::from("redeemer.json")),
                plutus_exec_memory: Some(7),
                plutus_exec_steps: Some(11),
            }
        );
    }
}
