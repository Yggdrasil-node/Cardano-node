//! Core transaction-generator script operations.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`.
//! Ports the state/query/runtime helper boundary consumed by
//! `Cardano.Benchmarking.Script.Action.action`. This slice owns the
//! deterministic state-only operations; protocol queries, transaction
//! stream evaluation, Plutus context construction, and submission still
//! return explicit `TxGenError` boundaries until their downstream
//! `GeneratorTx` and node-runtime mirrors land.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::script::aeson;
use crate::script::env::{
    Env, Error, Fund, ProtocolParameterMode, WalletRef, get_env_keys, get_env_network_id,
    get_env_socket_path, get_env_threads, get_env_threads_mut, get_env_wallets_mut,
    get_proto_param_mode, lift_tx_gen_error, set_env_keys, set_env_threads, set_env_wallets,
    set_proto_param_mode, trace_debug,
};
use crate::script::types::{Generator, ProtocolParametersSource, SigningKeyEnvelope, SubmitMode};
use crate::types::{AnyCardanoEra, Lovelace, TxGenTxParams};

/// Mirror of upstream `withEra`.
pub fn with_era<T>(
    era: AnyCardanoEra,
    action: impl FnOnce(AnyCardanoEra) -> Result<T, Error>,
) -> Result<T, Error> {
    match era {
        AnyCardanoEra::Byron => Err(lift_tx_gen_error("byron not supported")),
        _ => action(era),
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
    era: AnyCardanoEra,
    wallet: &str,
    tx_in: &str,
    lovelace: Lovelace,
    key_name: &str,
) -> Result<(), Error> {
    with_era(era, |era| {
        let _fund_key = get_env_keys(env, key_name)?;
        add_fund_to_wallet(env, wallet, era, tx_in, lovelace, key_name)
    })
}

/// Mirror of upstream `addFundToWallet`.
pub fn add_fund_to_wallet(
    env: &mut Env,
    wallet: &str,
    era: AnyCardanoEra,
    tx_in: &str,
    lovelace: Lovelace,
    key_name: &str,
) -> Result<(), Error> {
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

/// Mirror of upstream `waitBenchmarkCore`.
pub fn wait_benchmark_core(_env: &Env) -> Result<(), Error> {
    Ok(())
}

/// Mirror of upstream `waitBenchmark`.
pub fn wait_benchmark(env: &Env) -> Result<(), Error> {
    if get_env_threads(env).is_some() {
        wait_benchmark_core(env)
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

/// Rust carrier for upstream `LocalNodeConnectInfo`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalConnectInfo {
    /// Network ID used by local node-to-client calls.
    pub network_id: crate::script::types::NetworkId,
    /// Node socket path.
    pub socket_path: std::path::PathBuf,
}

/// Mirror of upstream `getLocalConnectInfo`.
pub fn get_local_connect_info(env: &Env) -> Result<LocalConnectInfo, Error> {
    Ok(LocalConnectInfo {
        network_id: get_env_network_id(env)?.clone(),
        socket_path: get_env_socket_path(env)?.clone(),
    })
}

/// Mirror of upstream `queryEra`.
pub fn query_era(env: &Env) -> Result<AnyCardanoEra, Error> {
    let _connect_info = get_local_connect_info(env)?;
    Err(lift_tx_gen_error(
        "queryEra: local-state query is not yet implemented \
         (pending node-to-client Script/Core runtime slice)",
    ))
}

/// Mirror of upstream `queryRemoteProtocolParameters`.
pub fn query_remote_protocol_parameters(env: &Env) -> Result<Value, Error> {
    let _connect_info = get_local_connect_info(env)?;
    Err(lift_tx_gen_error(
        "queryRemoteProtocolParameters: local-state protocol-parameters query is not yet implemented \
         (pending node-to-client Script/Core runtime slice)",
    ))
}

/// Mirror of upstream `getProtocolParameters`.
pub fn get_protocol_parameters(env: &Env) -> Result<Value, Error> {
    match get_proto_param_mode(env)? {
        ProtocolParameterMode::ProtocolParameterQuery => query_remote_protocol_parameters(env),
        ProtocolParameterMode::ProtocolParameterLocal(parameters) => Ok(parameters.clone()),
    }
}

/// Mirror of upstream `waitForEra`.
pub fn wait_for_era(env: &mut Env, era: AnyCardanoEra) -> Result<(), Error> {
    let current_era = query_era(env)?;
    if current_era == era {
        Ok(())
    } else {
        crate::script::env::trace_error(
            env,
            &format!("Current era: {current_era:?} Waiting for: {era:?}"),
        );
        delay(1.0)?;
        wait_for_era(env, era)
    }
}

/// Mirror of upstream `submitAction`.
pub fn submit_action(
    _env: &mut Env,
    era: AnyCardanoEra,
    submit_mode: &SubmitMode,
    generator: &Generator,
    tx_params: &TxGenTxParams,
) -> Result<(), Error> {
    with_era(era, |_era| submit_in_era(submit_mode, generator, tx_params))
}

/// Mirror of upstream `submitInEra`.
pub fn submit_in_era(
    _submit_mode: &SubmitMode,
    _generator: &Generator,
    _tx_params: &TxGenTxParams,
) -> Result<(), Error> {
    Err(lift_tx_gen_error(
        "submitInEra: transaction generation is not yet implemented \
         (pending GeneratorTx runtime slice)",
    ))
}

/// Mirror of upstream `initWallet`.
pub fn init_wallet(env: &mut Env, name: &str) -> Result<(), Error> {
    set_env_wallets(env, name, WalletRef::default());
    Ok(())
}

/// Mirror of upstream `traceTxGeneratorVersion`.
pub fn trace_tx_generator_version(env: &mut Env) {
    trace_debug(env, "tx-generator version tracing is not yet wired");
}

/// Mirror of upstream `reserved`.
pub fn reserved(_options: &[String]) -> Result<(), Error> {
    Err(Error::UserError("no dirty hack is implemented".to_string()))
}

/// Test helper for later slices that need to seed async state.
pub fn set_dummy_benchmark_control(env: &mut Env) {
    set_env_threads(env, crate::script::env::AsyncBenchmarkControl::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::env::{Env, get_env_wallets};

    #[test]
    fn with_era_rejects_byron_and_accepts_shelley_based_eras() {
        assert_eq!(
            with_era(AnyCardanoEra::Byron, Ok::<_, Error>),
            Err(Error::TxGenError("byron not supported".to_string()))
        );
        assert_eq!(
            with_era(AnyCardanoEra::Dijkstra, Ok::<_, Error>),
            Ok(AnyCardanoEra::Dijkstra)
        );
    }

    #[test]
    fn add_fund_requires_existing_wallet_and_key() {
        let mut env = Env::empty_env();

        let err = add_fund(&mut env, AnyCardanoEra::Conway, "wallet", "abc#0", 1, "key")
            .expect_err("missing key");

        assert_eq!(err, Error::UserError("Lookup of key failed".to_string()));
    }

    #[test]
    fn add_fund_inserts_into_wallet_when_key_exists() {
        let mut env = Env::empty_env();
        init_wallet(&mut env, "wallet").expect("wallet");
        define_signing_key(
            &mut env,
            "key",
            SigningKeyEnvelope::payment_signing_key_shelley("5820abcd"),
        );

        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "wallet",
            "abc#0",
            12,
            "key",
        )
        .expect("fund");

        assert_eq!(
            get_env_wallets(&env, "wallet")
                .expect("wallet")
                .funds()
                .len(),
            1
        );
    }

    #[test]
    fn local_protocol_parameters_are_returned_without_querying_node() {
        let mut env = Env::empty_env();
        let params = serde_json::json!({"protocolVersion": {"major": 10, "minor": 0}});
        set_proto_param_mode(
            &mut env,
            ProtocolParameterMode::ProtocolParameterLocal(params.clone()),
        );

        assert_eq!(get_protocol_parameters(&env), Ok(params));
    }

    #[test]
    fn query_protocol_parameters_requires_local_connect_info() {
        let mut env = Env::empty_env();
        set_proto_param_mode(&mut env, ProtocolParameterMode::ProtocolParameterQuery);

        assert_eq!(
            get_protocol_parameters(&env),
            Err(Error::UserError("Unset Genesis".to_string()))
        );
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
