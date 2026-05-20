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
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use yggdrasil_ledger::{Decoder, Encoder};
use yggdrasil_network::protocols::{HardForkBlockQuery, QueryHardFork, UpstreamQuery};
#[cfg(unix)]
use yggdrasil_network::{AcquireTarget, LocalStateQueryClient, MiniProtocolNum, ntc_connect};

use crate::generator_tx::sized_metadata::{TxMetadata, mk_metadata};
use crate::script::aeson;
use crate::script::env::{
    Env, Error, Fund, ProtocolParameterMode, WalletRef, get_env_keys, get_env_network_id,
    get_env_socket_path, get_env_threads, get_env_threads_mut, get_env_wallets_mut,
    get_proto_param_mode, lift_tx_gen_error, set_env_keys, set_env_threads, set_env_wallets,
    set_proto_param_mode, trace_debug,
};
use crate::script::types::{Generator, ProtocolParametersSource, SigningKeyEnvelope, SubmitMode};
use crate::types::{AnyCardanoEra, Lovelace, TxGenTxParams};

/// Mainnet network magic used by node-to-client handshakes.
///
/// Mirrors the canonical `NetworkMagic 764824073` value used by
/// upstream Cardano tools when a script selects `Mainnet`.
pub const MAINNET_NETWORK_MAGIC: u32 = 764_824_073;

const PROTOCOL_PARAMETERS_QUERIED_FILE: &str = "protocol-parameters-queried.json";

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
    /// Network magic used by the node-to-client handshake.
    pub network_magic: u32,
    /// Node socket path.
    pub socket_path: PathBuf,
}

/// Mirror of upstream `getLocalConnectInfo`.
pub fn get_local_connect_info(env: &Env) -> Result<LocalConnectInfo, Error> {
    let network_id = get_env_network_id(env)?.clone();
    Ok(LocalConnectInfo {
        network_magic: network_id_to_magic(&network_id)?,
        network_id,
        socket_path: get_env_socket_path(env)?.clone(),
    })
}

/// Convert upstream `NetworkId` to the NtC protocol magic.
pub fn network_id_to_magic(network_id: &crate::script::types::NetworkId) -> Result<u32, Error> {
    match network_id {
        crate::script::types::NetworkId::Mainnet => Ok(MAINNET_NETWORK_MAGIC),
        crate::script::types::NetworkId::Testnet(magic) => u32::try_from(*magic)
            .map_err(|_| Error::UserError(format!("NetworkMagic out of u32 range: {magic}"))),
    }
}

/// Mirror of upstream `queryEra`.
pub fn query_era(env: &Env) -> Result<AnyCardanoEra, Error> {
    let connect_info = get_local_connect_info(env)?;
    query_era_with_connect_info(&connect_info)
}

/// Mirror of upstream `queryRemoteProtocolParameters`.
pub fn query_remote_protocol_parameters(env: &mut Env) -> Result<Value, Error> {
    let connect_info = get_local_connect_info(env)?;
    let era = query_era_with_connect_info(&connect_info)?;
    let query = encode_protocol_parameters_query(era)?;
    let result = run_local_state_query(
        &connect_info,
        query,
        "QueryInShelleyBasedEra QueryProtocolParameters",
    )?;
    let era_native_pparams = decode_query_if_current_match(&result, era)?;
    let parameters = protocol_parameters_value(era, &era_native_pparams)?;
    let rendered = serde_json::to_string_pretty(&parameters)
        .map_err(|err| lift_tx_gen_error(format!("prettyPrintOrdered: {err}")))?;
    fs::write(PROTOCOL_PARAMETERS_QUERIED_FILE, format!("{rendered}\n")).map_err(|err| {
        lift_tx_gen_error(format!(
            "queryRemoteProtocolParameters: write {PROTOCOL_PARAMETERS_QUERIED_FILE}: {err}"
        ))
    })?;
    trace_debug(
        env,
        &format!(
            "queryRemoteProtocolParameters : query result saved in: {PROTOCOL_PARAMETERS_QUERIED_FILE}"
        ),
    );
    Ok(parameters)
}

/// Mirror of upstream `getProtocolParameters`.
pub fn get_protocol_parameters(env: &mut Env) -> Result<Value, Error> {
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

/// Mirror of upstream `toMetadata`.
pub fn to_metadata(
    era: AnyCardanoEra,
    payload_size: Option<usize>,
) -> Result<Option<TxMetadata>, Error> {
    match payload_size {
        None => Ok(None),
        Some(size) => mk_metadata(era, size).map_err(lift_tx_gen_error),
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
    with_era(era, |era| {
        submit_in_era(era, submit_mode, generator, tx_params)
    })
}

/// Mirror of upstream `submitInEra`.
pub fn submit_in_era(
    era: AnyCardanoEra,
    _submit_mode: &SubmitMode,
    generator: &Generator,
    _tx_params: &TxGenTxParams,
) -> Result<(), Error> {
    preflight_generator_metadata(era, generator)?;
    Err(lift_tx_gen_error(
        "submitInEra: transaction generation is not yet implemented \
         (pending GeneratorTx transaction/runtime slice after R541 SizedMetadata)",
    ))
}

fn preflight_generator_metadata(era: AnyCardanoEra, generator: &Generator) -> Result<(), Error> {
    match generator {
        Generator::NtoM(_, _, _, _, metadata_size, _) => {
            let _ = to_metadata(era, *metadata_size)?;
            Ok(())
        }
        Generator::Sequence(generators) | Generator::RoundRobin(generators) => {
            for generator in generators {
                preflight_generator_metadata(era, generator)?;
            }
            Ok(())
        }
        Generator::Cycle(generator) | Generator::Take(_, generator) => {
            preflight_generator_metadata(era, generator)
        }
        Generator::OneOf(generators) => {
            for (generator, _weight) in generators {
                preflight_generator_metadata(era, generator)?;
            }
            Ok(())
        }
        Generator::SecureGenesis(_, _, _)
        | Generator::Split(_, _, _, _)
        | Generator::SplitN(_, _, _) => Ok(()),
    }
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

fn query_era_with_connect_info(connect_info: &LocalConnectInfo) -> Result<AnyCardanoEra, Error> {
    let result = run_local_state_query(
        connect_info,
        encode_current_era_query(),
        "QueryHardFork GetCurrentEra",
    )?;
    decode_current_era_result(&result)
}

fn run_local_state_query(
    connect_info: &LocalConnectInfo,
    query_bytes: Vec<u8>,
    query_label: &str,
) -> Result<Vec<u8>, Error> {
    #[cfg(not(unix))]
    {
        let _ = (connect_info, query_bytes, query_label);
        Err(lift_tx_gen_error(
            "LocalStateQuery over node-to-client sockets requires Unix-domain socket support",
        ))
    }

    #[cfg(unix)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| lift_tx_gen_error(format!("LocalStateQuery runtime: {err}")))?
        .block_on(run_local_state_query_async(
            connect_info,
            query_bytes,
            query_label,
        ))
}

#[cfg(unix)]
async fn run_local_state_query_async(
    connect_info: &LocalConnectInfo,
    query_bytes: Vec<u8>,
    query_label: &str,
) -> Result<Vec<u8>, Error> {
    let mut conn = ntc_connect(&connect_info.socket_path, connect_info.network_magic, true)
        .await
        .map_err(|err| {
            lift_tx_gen_error(format!(
                "LocalStateQuery connect {} (network_magic={}): {err}",
                connect_info.socket_path.display(),
                connect_info.network_magic
            ))
        })?;
    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .ok_or_else(|| lift_tx_gen_error("NTC_LOCAL_STATE_QUERY mini-protocol handle missing"))?;
    let mut client = LocalStateQueryClient::new(sq_handle);
    client
        .acquire(AcquireTarget::VolatileTip)
        .await
        .map_err(|err| lift_tx_gen_error(format!("LocalStateQuery acquire failed: {err}")))?;
    let result = client.query(query_bytes).await.map_err(|err| {
        lift_tx_gen_error(format!(
            "LocalStateQuery `{query_label}` query failed: {err}"
        ))
    })?;
    let _ = client.release().await;
    let _ = client.done().await;
    Ok(result)
}

fn encode_current_era_query() -> Vec<u8> {
    UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(
        QueryHardFork::GetCurrentEra,
    ))
    .encode()
}

fn encode_protocol_parameters_query(era: AnyCardanoEra) -> Result<Vec<u8>, Error> {
    let mut inner = Encoder::new();
    inner.array(2);
    inner.unsigned(era_to_lsq_ordinal(era)?);
    inner.array(1);
    inner.unsigned(3);
    Ok(
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent {
            inner_cbor: inner.into_bytes(),
        })
        .encode(),
    )
}

fn decode_current_era_result(result: &[u8]) -> Result<AnyCardanoEra, Error> {
    let mut bare = Decoder::new(result);
    if let Ok(ordinal) = bare.unsigned() {
        return era_from_lsq_ordinal(ordinal);
    }

    let mut dec = Decoder::new(result);
    let len = dec
        .array()
        .map_err(|err| lift_tx_gen_error(format!("decode QueryCurrentEra result: {err}")))?;
    if len != 1 {
        return Err(lift_tx_gen_error(format!(
            "decode QueryCurrentEra result: expected 1-element array or bare ordinal, got len={len}"
        )));
    }
    let ordinal = dec
        .unsigned()
        .map_err(|err| lift_tx_gen_error(format!("decode QueryCurrentEra ordinal: {err}")))?;
    era_from_lsq_ordinal(ordinal)
}

fn decode_query_if_current_match(result: &[u8], era: AnyCardanoEra) -> Result<Vec<u8>, Error> {
    let mut dec = Decoder::new(result);
    let len = dec
        .array()
        .map_err(|err| lift_tx_gen_error(format!("decode QueryIfCurrent result: {err}")))?;
    match len {
        1 => {
            let start = dec.position();
            dec.skip().map_err(|err| {
                lift_tx_gen_error(format!("decode QueryIfCurrent payload: {err}"))
            })?;
            let end = dec.position();
            Ok(result[start..end].to_vec())
        }
        2 => Err(lift_tx_gen_error(format!(
            "queryRemoteProtocolParameters: era mismatch for {era:?}: {}",
            hex::encode(result)
        ))),
        other => Err(lift_tx_gen_error(format!(
            "queryRemoteProtocolParameters: expected QueryIfCurrent match/mismatch, got array len={other}"
        ))),
    }
}

fn protocol_parameters_value(era: AnyCardanoEra, era_native_cbor: &[u8]) -> Result<Value, Error> {
    Ok(serde_json::json!({
        "source": "QueryLocalNode",
        "query": "GetCurrentPParams",
        "era": format!("{era:?}"),
        "eraOrdinal": era_to_lsq_ordinal(era)?,
        "eraNativeCborHex": hex::encode(era_native_cbor),
    }))
}

fn era_from_lsq_ordinal(ordinal: u64) -> Result<AnyCardanoEra, Error> {
    match ordinal {
        0 => Err(lift_tx_gen_error("queryEra Byron not supported")),
        1 => Ok(AnyCardanoEra::Shelley),
        2 => Ok(AnyCardanoEra::Allegra),
        3 => Ok(AnyCardanoEra::Mary),
        4 => Ok(AnyCardanoEra::Alonzo),
        5 => Ok(AnyCardanoEra::Babbage),
        6 => Ok(AnyCardanoEra::Conway),
        7 => Ok(AnyCardanoEra::Dijkstra),
        other => Err(lift_tx_gen_error(format!(
            "queryEra: unknown era ordinal {other}"
        ))),
    }
}

fn era_to_lsq_ordinal(era: AnyCardanoEra) -> Result<u64, Error> {
    match era {
        AnyCardanoEra::Byron => Err(lift_tx_gen_error("queryEra Byron not supported")),
        AnyCardanoEra::Shelley => Ok(1),
        AnyCardanoEra::Allegra => Ok(2),
        AnyCardanoEra::Mary => Ok(3),
        AnyCardanoEra::Alonzo => Ok(4),
        AnyCardanoEra::Babbage => Ok(5),
        AnyCardanoEra::Conway => Ok(6),
        AnyCardanoEra::Dijkstra => Ok(7),
    }
}

/// Test helper for later slices that need to seed async state.
pub fn set_dummy_benchmark_control(env: &mut Env) {
    set_env_threads(env, crate::script::env::AsyncBenchmarkControl::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::env::{Env, get_env_wallets, set_env_network_id, set_env_socket_path};
    use crate::script::types::{NetworkId, PayMode};

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

        assert_eq!(get_protocol_parameters(&mut env), Ok(params));
    }

    #[test]
    fn query_protocol_parameters_requires_local_connect_info() {
        let mut env = Env::empty_env();
        set_proto_param_mode(&mut env, ProtocolParameterMode::ProtocolParameterQuery);

        assert_eq!(
            get_protocol_parameters(&mut env),
            Err(Error::UserError("Unset Genesis".to_string()))
        );
    }

    #[test]
    fn local_connect_info_carries_network_magic() {
        let mut env = Env::empty_env();
        set_env_network_id(&mut env, NetworkId::Mainnet);
        set_env_socket_path(&mut env, PathBuf::from("/tmp/node.socket"));

        let info = get_local_connect_info(&env).expect("connect info");

        assert_eq!(info.network_magic, MAINNET_NETWORK_MAGIC);
        assert_eq!(info.socket_path, PathBuf::from("/tmp/node.socket"));
    }

    #[test]
    fn network_id_to_magic_accepts_u32_testnet_magic() {
        assert_eq!(network_id_to_magic(&NetworkId::Testnet(42)), Ok(42));
        assert_eq!(
            network_id_to_magic(&NetworkId::Testnet(u64::from(u32::MAX) + 1)),
            Err(Error::UserError(format!(
                "NetworkMagic out of u32 range: {}",
                u64::from(u32::MAX) + 1
            )))
        );
    }

    #[test]
    fn current_era_query_uses_upstream_hardfork_shape() {
        assert_eq!(
            encode_current_era_query(),
            vec![0x82, 0x00, 0x82, 0x02, 0x81, 0x01]
        );
    }

    #[test]
    fn to_metadata_preserves_upstream_sized_metadata_boundary() {
        let metadata = to_metadata(AnyCardanoEra::Conway, Some(39))
            .expect("metadata")
            .expect("some metadata");

        assert_eq!(metadata.to_cbor_bytes(), vec![0xa1, 0x00, 0x40]);
        assert_eq!(
            to_metadata(AnyCardanoEra::Conway, None).expect("metadata none"),
            None
        );
    }

    #[test]
    fn submit_in_era_preflights_ntom_metadata_size() {
        let generator = Generator::NtoM(
            "wallet".to_string(),
            PayMode::PayToAddr("key".to_string(), "wallet".to_string()),
            1,
            1,
            Some(38),
            None,
        );
        let err = submit_in_era(
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 1,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect_err("metadata size rejected");

        assert_eq!(
            err,
            Error::TxGenError(
                "Error : metadata must be 0 or at least 39 bytes in this era.".to_string()
            )
        );
    }

    #[test]
    fn protocol_parameters_query_uses_query_if_current_shape() {
        assert_eq!(
            encode_protocol_parameters_query(AnyCardanoEra::Conway),
            Ok(vec![0x82, 0x00, 0x82, 0x00, 0x82, 0x06, 0x81, 0x03])
        );
    }

    #[test]
    fn current_era_result_accepts_bare_and_legacy_array_ordinals() {
        assert_eq!(
            decode_current_era_result(&[0x06]),
            Ok(AnyCardanoEra::Conway)
        );
        assert_eq!(
            decode_current_era_result(&[0x81, 0x05]),
            Ok(AnyCardanoEra::Babbage)
        );
        assert_eq!(
            decode_current_era_result(&[0x00]),
            Err(Error::TxGenError(
                "queryEra Byron not supported".to_string()
            ))
        );
    }

    #[test]
    fn query_if_current_match_extracts_raw_payload() {
        assert_eq!(
            decode_query_if_current_match(&[0x81, 0x83, 0x01, 0x02, 0x03], AnyCardanoEra::Conway),
            Ok(vec![0x83, 0x01, 0x02, 0x03])
        );
    }

    #[test]
    fn protocol_parameters_value_preserves_era_native_cbor() {
        assert_eq!(
            protocol_parameters_value(AnyCardanoEra::Conway, &[0x83, 0x01, 0x02, 0x03])
                .expect("value"),
            serde_json::json!({
                "source": "QueryLocalNode",
                "query": "GetCurrentPParams",
                "era": "Conway",
                "eraOrdinal": 6,
                "eraNativeCborHex": "83010203",
            })
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
