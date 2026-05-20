//! Convert parsed script `Action` values into state transitions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Action.hs`.
//! Ports the `action` dispatch boundary. As in upstream, most action
//! bodies delegate to `Script/Core`; this module keeps the local
//! `startProtocol` bridge because upstream defines that helper here to
//! avoid circular imports.

use std::path::{Path, PathBuf};

use crate::script::core::{
    add_fund, cancel_benchmark, define_signing_key, delay, init_wallet, read_signing_key, reserved,
    set_protocol_parameters, submit_action, wait_benchmark, wait_for_era,
};
use crate::script::env::{
    Env, Error, GenesisHandle, ProtocolHandle, lift_tx_gen_error, set_env_genesis,
    set_env_network_id, set_env_protocol, set_env_socket_path, trace_debug,
};
use crate::script::types::Action;

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

    let protocol = ProtocolHandle {
        config_file: PathBuf::from(config_file),
        tracer_socket: tracer_socket.map(PathBuf::from),
    };
    let genesis = GenesisHandle {
        config_file: PathBuf::from(config_file),
    };
    set_env_protocol(env, protocol);
    set_env_genesis(env, genesis);
    Err(lift_tx_gen_error(
        "startProtocol: mkConsensusProtocol/getGenesis runtime wiring is not yet implemented \
         (pending Script/Core runtime slice)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::env::{get_env_keys, get_env_network_id, get_env_wallets};
    use crate::script::types::{NetworkId, SigningKeyEnvelope};
    use crate::types::AnyCardanoEra;

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
}
