//! Convert parsed script `Action` values into state transitions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Action.hs`.
//! Ports the `action` dispatch boundary. As in upstream, most action
//! bodies delegate to `Script/Core`; this module keeps the local
//! `startProtocol` bridge because upstream defines that helper here to
//! avoid circular imports.

use std::fs;
use std::path::{Path, PathBuf};

use crate::script::core::{
    add_fund, cancel_benchmark, define_signing_key, delay, init_wallet, read_signing_key, reserved,
    set_protocol_parameters, submit_action, wait_benchmark, wait_for_era,
};
use crate::script::env::{
    BenchTracers, Env, Error, GenesisHandle, ProtocolHandle, lift_tx_gen_error, set_bench_tracers,
    set_env_genesis, set_env_network_id, set_env_protocol, set_env_socket_path, trace_debug,
};
use crate::script::types::{Action, NetworkId};
use yggdrasil_node_config::NodeConfigFile;

/// Mirror of upstream `action`.
pub fn action(env: &mut Env, script_action: &Action) -> Result<(), Error> {
    match script_action {
        Action::SetNetworkId(value) => {
            set_env_network_id(env, value.clone());
            Ok(())
        }
        Action::SetSocketPath(value) => {
            set_env_socket_path(env, value.clone());
            Ok(())
        }
        Action::InitWallet(name) => init_wallet(env, name),
        Action::SetProtocolParameters(source) => set_protocol_parameters(env, source),
        Action::StartProtocol(config_file, tracer_socket) => {
            start_protocol(env, config_file, tracer_socket.as_deref())
        }
        Action::ReadSigningKey(name, file_path) => read_signing_key(env, name, file_path),
        Action::DefineSigningKey(name, key) => {
            define_signing_key(env, name, key.clone());
            Ok(())
        }
        Action::AddFund(era, wallet, tx_in, lovelace, key_name) => {
            add_fund(env, *era, wallet, tx_in, *lovelace, key_name)
        }
        Action::Delay(seconds) => delay(*seconds),
        Action::Submit(era, submit_mode, tx_params, generator) => {
            submit_action(env, *era, submit_mode, generator, tx_params)
        }
        Action::WaitBenchmark => wait_benchmark(env),
        Action::CancelBenchmark => cancel_benchmark(env),
        Action::WaitForEra(era) => wait_for_era(env, *era),
        Action::LogMsg(message) => {
            trace_debug(env, message);
            Ok(())
        }
        Action::Reserved(options) => reserved(options),
    }
}

/// Mirror of upstream `startProtocol`.
pub fn start_protocol(
    env: &mut Env,
    config_file: &Path,
    tracer_socket: Option<&Path>,
) -> Result<(), Error> {
    if !config_file.exists() {
        return Err(lift_tx_gen_error(format!(
            "mkNodeConfig: config file does not exist: {}",
            config_file.display()
        )));
    }

    let node_config = mk_node_config(config_file)?;
    let protocol_name = node_config
        .protocol
        .clone()
        .unwrap_or_else(|| "Cardano".to_string());
    if protocol_name != "Cardano" {
        return Err(lift_tx_gen_error(format!(
            "mkConsensusProtocol: unsupported Protocol {protocol_name:?}; expected \"Cardano\""
        )));
    }
    let network_magic = node_config.network_magic;
    let config_dir = config_file.parent();
    let protocol = ProtocolHandle {
        config_file: PathBuf::from(config_file),
        tracer_socket: tracer_socket.map(PathBuf::from),
        protocol: protocol_name,
        network_magic,
    };
    let genesis = GenesisHandle {
        config_file: PathBuf::from(config_file),
        shelley_genesis_file: node_config
            .shelley_genesis_file
            .as_deref()
            .map(Path::new)
            .map(|path| resolve_config_relative(config_dir, path)),
        shelley_genesis_hash: node_config.shelley_genesis_hash,
        network_magic,
    };
    set_env_protocol(env, protocol);
    set_env_genesis(env, genesis);
    set_env_network_id(env, NetworkId::Testnet(u64::from(network_magic)));
    set_bench_tracers(env, BenchTracers::default());
    Ok(())
}

fn mk_node_config(config_file: &Path) -> Result<NodeConfigFile, Error> {
    let raw = fs::read_to_string(config_file).map_err(|err| {
        lift_tx_gen_error(format!(
            "mkNodeConfig: failed to read config file {}: {err}",
            config_file.display()
        ))
    })?;
    match serde_json::from_str(&raw) {
        Ok(parsed) => Ok(parsed),
        Err(json_err) => serde_norway::from_str(&raw).map_err(|yaml_err| {
            lift_tx_gen_error(format!(
                "mkNodeConfig: failed to parse config file {} as JSON ({json_err}) or YAML ({yaml_err})",
                config_file.display()
            ))
        }),
    }
}

fn resolve_config_relative(config_dir: Option<&Path>, path: &Path) -> PathBuf {
    if path.is_absolute() {
        PathBuf::from(path)
    } else if let Some(config_dir) = config_dir {
        config_dir.join(path)
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::env::{
        get_bench_tracers, get_env_genesis, get_env_keys, get_env_network_id, get_env_protocol,
        get_env_wallets,
    };
    use crate::script::types::{NetworkId, SigningKeyEnvelope};
    use crate::types::AnyCardanoEra;
    use std::path::PathBuf;
    use yggdrasil_node_config::default_config;

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-action-{name}-{}-{nanos}",
            std::process::id(),
        ))
    }

    fn write_node_config(path: &Path, network_magic: u32) {
        let mut config = default_config();
        config.network_magic = network_magic;
        config.protocol = Some("Cardano".to_string());
        fs::write(
            path,
            serde_json::to_string(&config).expect("serialize node config"),
        )
        .expect("write node config");
    }

    #[test]
    fn action_dispatch_updates_env_like_upstream_accessors() {
        let mut env = Env::empty_env();
        let key = SigningKeyEnvelope::payment_signing_key_shelley("5820abcd");

        action(&mut env, &Action::SetNetworkId(NetworkId::Testnet(42))).expect("network id");
        action(&mut env, &Action::InitWallet("wallet".to_string())).expect("wallet");
        action(
            &mut env,
            &Action::DefineSigningKey("key".to_string(), key.clone()),
        )
        .expect("key");
        action(
            &mut env,
            &Action::AddFund(
                AnyCardanoEra::Conway,
                "wallet".to_string(),
                "abc#0".to_string(),
                12,
                "key".to_string(),
            ),
        )
        .expect("add fund");

        assert_eq!(get_env_network_id(&env), Ok(&NetworkId::Testnet(42)));
        assert_eq!(
            get_env_wallets(&env, "wallet")
                .expect("wallet")
                .funds()
                .len(),
            1
        );
        assert_eq!(get_env_keys(&env, "key"), Ok(&key));
    }

    #[test]
    fn log_msg_dispatches_through_env_trace_debug() {
        let mut env = Env::empty_env();

        action(&mut env, &Action::LogMsg("hello".to_string())).expect("log");

        assert_eq!(env.bench_tracers.expect("tracers").messages(), ["hello"]);
    }

    #[test]
    fn start_protocol_sets_protocol_genesis_network_and_tracers() {
        let config_file = unique_temp_path("node-config.json");
        let tracer_socket = unique_temp_path("trace-forwarder.sock");
        write_node_config(&config_file, 42);
        let mut env = Env::empty_env();

        start_protocol(&mut env, &config_file, Some(&tracer_socket)).expect("start protocol");

        assert_eq!(get_env_network_id(&env), Ok(&NetworkId::Testnet(42)));
        let protocol = get_env_protocol(&env).expect("protocol");
        assert_eq!(protocol.config_file, config_file);
        assert_eq!(protocol.tracer_socket, Some(tracer_socket));
        assert_eq!(protocol.protocol, "Cardano");
        assert_eq!(protocol.network_magic, 42);
        let genesis = get_env_genesis(&env).expect("genesis");
        assert_eq!(genesis.network_magic, 42);
        assert_eq!(
            genesis
                .shelley_genesis_file
                .as_deref()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some("shelley-genesis.json")
        );
        assert!(get_bench_tracers(&env).is_ok());

        let _ = fs::remove_file(config_file);
    }

    #[test]
    fn start_protocol_rejects_non_cardano_protocol() {
        let config_file = unique_temp_path("node-config-non-cardano.json");
        let mut config = default_config();
        config.protocol = Some("Other".to_string());
        fs::write(
            &config_file,
            serde_json::to_string(&config).expect("serialize node config"),
        )
        .expect("write node config");
        let mut env = Env::empty_env();

        let err = start_protocol(&mut env, &config_file, None).expect_err("unsupported protocol");

        assert!(err.to_string().contains("mkConsensusProtocol"));
        assert!(env.env_protocol.is_none());
        let _ = fs::remove_file(config_file);
    }
}
