//! Convert parsed script `Action` values into state transitions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Action.hs`.
//! Ports the `action` dispatch boundary and the deterministic
//! state-only action cases. Protocol initialisation, local-state
//! queries, transaction generation, and async submission return
//! explicit `TxGenError` values until the later `Script/Core` and
//! `GeneratorTx` slices wire the real runtime machinery.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::script::aeson;
use crate::script::env::{
    AsyncBenchmarkControl, Env, Error, Fund, GenesisHandle, ProtocolHandle, ProtocolParameterMode,
    WalletRef, get_env_keys, get_env_threads, get_env_threads_mut, get_env_wallets_mut,
    lift_tx_gen_error, set_bench_tracers, set_env_genesis, set_env_keys, set_env_network_id,
    set_env_protocol, set_env_socket_path, set_env_wallets, set_proto_param_mode,
};
use crate::script::types::{Action, ProtocolParametersSource, SigningKeyEnvelope};

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
        Action::Submit(_, _, _, _) => Err(lift_tx_gen_error(
            "submitAction: transaction generation is not yet implemented \
             (pending GeneratorTx runtime slice)",
        )),
        Action::WaitBenchmark => wait_benchmark(env),
        Action::CancelBenchmark => cancel_benchmark(env),
        Action::WaitForEra(_) => Err(lift_tx_gen_error(
            "waitForEra: local-state era query is not yet implemented \
             (pending Script/Core runtime slice)",
        )),
        Action::LogMsg(message) => {
            trace_debug(env, message);
            Ok(())
        }
        Action::Reserved(options) => reserved(options),
    }
}

/// Mirror of upstream `setProtocolParameters`.
pub fn set_protocol_parameters(
    env: &mut Env,
    source: &ProtocolParametersSource,
) -> Result<(), Error> {
    match source {
        ProtocolParametersSource::QueryLocalNode => {
            set_proto_param_mode(env, ProtocolParameterMode::ProtocolParameterQuery);
            Ok(())
        }
        ProtocolParametersSource::UseLocalProtocolFile(file) => {
            let parameters: Value = aeson::parse_json_file(file)
                .map_err(|err| lift_tx_gen_error(format!("readProtocolParametersFile: {err}")))?;
            set_proto_param_mode(
                env,
                ProtocolParameterMode::ProtocolParameterLocal(parameters),
            );
            Ok(())
        }
    }
}

/// Mirror of upstream `readSigningKey`.
pub fn read_signing_key(env: &mut Env, name: &str, file_path: &Path) -> Result<(), Error> {
    let raw = fs::read_to_string(file_path)
        .map_err(|err| lift_tx_gen_error(format!("readSigningKeyFile: {err}")))?;
    let key: SigningKeyEnvelope = serde_json::from_str(&raw)
        .map_err(|err| lift_tx_gen_error(format!("readSigningKeyFile: {err}")))?;
    set_env_keys(env, name, key);
    Ok(())
}

/// Mirror of upstream `defineSigningKey`.
pub fn define_signing_key(env: &mut Env, name: &str, key: SigningKeyEnvelope) {
    set_env_keys(env, name, key);
}

/// Mirror of upstream `addFund`.
pub fn add_fund(
    env: &mut Env,
    era: crate::types::AnyCardanoEra,
    wallet: &str,
    tx_in: &str,
    lovelace: crate::types::Lovelace,
    key_name: &str,
) -> Result<(), Error> {
    let _fund_key = get_env_keys(env, key_name)?;
    let wallet_ref = get_env_wallets_mut(env, wallet)?;
    wallet_ref.insert_fund(Fund {
        era,
        tx_in: tx_in.to_string(),
        lovelace,
        key_name: key_name.to_string(),
    });
    Ok(())
}

/// Mirror of upstream `delay`.
pub fn delay(seconds: f64) -> Result<(), Error> {
    if seconds.is_sign_negative() {
        return Err(Error::UserError(format!(
            "Delay must be non-negative: {seconds}"
        )));
    }
    std::thread::sleep(Duration::from_micros((1_000_000.0 * seconds).floor() as u64));
    Ok(())
}

/// Mirror of upstream `waitBenchmark`.
pub fn wait_benchmark(env: &Env) -> Result<(), Error> {
    if get_env_threads(env).is_some() {
        Ok(())
    } else {
        Err(lift_tx_gen_error(
            "waitBenchmark: missing AsyncBenchmarkControl",
        ))
    }
}

/// Mirror of upstream `cancelBenchmark`.
pub fn cancel_benchmark(env: &mut Env) -> Result<(), Error> {
    let Some(control) = get_env_threads_mut(env) else {
        return Err(lift_tx_gen_error(
            "cancelBenchmark: missing AsyncBenchmarkControl",
        ));
    };
    control.shutdown_requested = true;
    wait_benchmark(env)
}

/// Mirror of upstream `initWallet`.
pub fn init_wallet(env: &mut Env, name: &str) -> Result<(), Error> {
    set_env_wallets(env, name, WalletRef::default());
    Ok(())
}

/// Mirror of upstream `traceDebug`.
pub fn trace_debug(env: &mut Env, message: &str) {
    let mut tracers = env.bench_tracers.take().unwrap_or_default();
    tracers.push(message);
    set_bench_tracers(env, tracers);
}

/// Mirror of upstream `reserved`.
pub fn reserved(_options: &[String]) -> Result<(), Error> {
    Err(Error::UserError("no dirty hack is implemented".to_string()))
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

/// Test helper for later slices that need to seed async state.
pub fn set_dummy_benchmark_control(env: &mut Env) {
    crate::script::env::set_env_threads(env, AsyncBenchmarkControl::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::env::{get_env_network_id, get_env_wallets};
    use crate::script::types::NetworkId;
    use crate::types::AnyCardanoEra;

    #[test]
    fn state_actions_update_env_like_upstream_accessors() {
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
    fn add_fund_requires_existing_wallet_and_key() {
        let mut env = Env::empty_env();

        let err = add_fund(&mut env, AnyCardanoEra::Conway, "wallet", "abc#0", 1, "key")
            .expect_err("missing key");

        assert_eq!(err, Error::UserError("Lookup of key failed".to_string()));
    }

    #[test]
    fn wait_and_cancel_match_missing_async_control_boundary() {
        let mut env = Env::empty_env();

        assert_eq!(
            wait_benchmark(&env),
            Err(Error::TxGenError(
                "waitBenchmark: missing AsyncBenchmarkControl".to_string()
            ))
        );
        assert_eq!(
            cancel_benchmark(&mut env),
            Err(Error::TxGenError(
                "cancelBenchmark: missing AsyncBenchmarkControl".to_string()
            ))
        );
    }

    #[test]
    fn cancel_marks_seeded_async_control_for_shutdown() {
        let mut env = Env::empty_env();
        set_dummy_benchmark_control(&mut env);

        cancel_benchmark(&mut env).expect("cancel");

        assert!(
            crate::script::env::get_env_threads(&env)
                .expect("control")
                .shutdown_requested
        );
    }
}
