//! Core transaction-generator script operations.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`.
//! Ports the state/query/runtime helper boundary consumed by
//! `Cardano.Benchmarking.Script.Action.action`. This slice owns the
//! deterministic state-only operations, Plutus context construction,
//! finite transaction-stream evaluation, LocalSocket submission,
//! Benchmark submission control, Shelley-through-Conway key-witnessed
//! `DumpToFile` rendering, and budget-summary projection. The remaining
//! Plutus-bearing Alonzo-family `DumpToFile` witness shapes still return
//! explicit `TxGenError` boundaries until their downstream mirrors land.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use num_bigint::BigInt;
use serde_json::Value;
use yggdrasil_crypto::{hash_bytes_224, hash_bytes_256};
use yggdrasil_ledger::{
    Address, AllegraTxBody, AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, BabbageTxBody,
    BabbageTxOut, CborDecode, CborEncode, ConwayTxBody, DatumOption, Decoder, Encoder, MaryTxBody,
    MaryTxOut, PlutusData, ProtocolParameters, ScriptRef, ShelleyCompatibleSubmittedTx,
    ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyVkeyWitness, ShelleyWitnessSet,
    StakeCredential,
    eras::alonzo::{ExUnits, Redeemer},
    total_min_fee,
};
use yggdrasil_network::protocols::{HardForkBlockQuery, QueryHardFork, UpstreamQuery};
#[cfg(unix)]
use yggdrasil_network::{
    AcquireTarget, LocalStateQueryClient, LocalTxSubmissionClient, MiniProtocolNum, ntc_connect,
};

use crate::benchmarking::types::SubmissionErrorPolicy;
use crate::generator_tx::sized_metadata::{TxMetadata, mk_metadata};
use crate::generator_tx::{WalletBenchmarkError, wallet_benchmark};
use crate::script::aeson;
use crate::script::env::{
    AsyncBenchmarkControl, Env, Error, Fund, ProtocolParameterMode, WalletRef, get_env_genesis,
    get_env_keys, get_env_network_id, get_env_socket_path, get_env_summary, get_env_threads,
    get_env_threads_mut, get_env_wallets, get_env_wallets_mut, get_proto_param_mode,
    lift_tx_gen_error, set_env_keys, set_env_summary, set_env_threads, set_env_wallets,
    set_proto_param_mode, trace_bench_tx_submit, trace_debug,
};
use crate::script::types::{
    Generator, PayMode, ProtocolParametersSource, ScriptBudget, ScriptSpec, SigningKeyEnvelope,
    SubmitMode,
};
use crate::setup::nix_service::NodeDescription;
use crate::setup::plutus::{pre_execute_plutus_script, read_plutus_script};
use crate::tx_generator::fund::{
    FundWitness, ScriptWitnessForSpending, get_fund_coin, get_fund_tx_in,
};
use crate::tx_generator::genesis::genesis_secure_initial_fund;
use crate::tx_generator::plutus_context::{
    PlutusAutoBudget, PlutusBudgetFittingStrategy, plutus_auto_scale_blockfit, read_script_data,
    script_data_modify_number,
};
use crate::tx_generator::tx::{GeneratedTx, gen_tx, source_transaction_preview, tx_size_in_bytes};
use crate::tx_generator::utils::{include_change, inputs_to_outputs_with_fee};
use crate::tx_generator::utxo::{
    ScriptInAnyLang, ToUtxo, ToUtxoList, key_address, mk_utxo_script, mk_utxo_variant,
    script_address,
};
use crate::types::{
    AnyCardanoEra, ExecutionUnits, Lovelace, NumberOfTxs, PayWithChange, TpsRate, TxGenPlutusType,
    TxGenTxParams,
};
use crate::wallet::{mangle_repeat, mangle_with_change, wallet_preview, wallet_source};

/// Mainnet network magic used by node-to-client handshakes.
///
/// Mirrors the canonical `NetworkMagic 764824073` value used by
/// upstream Cardano tools when a script selects `Mainnet`.
pub const MAINNET_NETWORK_MAGIC: u32 = 764_824_073;

const PROTOCOL_PARAMETERS_QUERIED_FILE: &str = "protocol-parameters-queried.json";
const PLUTUS_BUDGET_SUMMARY_FILE: &str = "plutus-budget-summary.json";

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
    wallet_ref.insert_fund(Fund::key_fund(era, tx_in, lovelace, key_name));
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
pub fn wait_benchmark_core(env: &mut Env) -> Result<(), Error> {
    let summary = {
        let control = get_env_threads_mut(env)
            .ok_or_else(|| lift_tx_gen_error("waitBenchmark: missing AsyncBenchmarkControl"))?;
        control.wait_summary().map_err(wallet_benchmark_error)?
    };
    if let Some(summary) = summary {
        let rendered = serde_json::to_string(&summary)
            .map_err(|err| lift_tx_gen_error(format!("TraceBenchTxSubSummary: {err}")))?;
        trace_bench_tx_submit(env, format!("TraceBenchTxSubSummary {rendered}"));
    }
    Ok(())
}

/// Mirror of upstream `waitBenchmark`.
pub fn wait_benchmark(env: &mut Env) -> Result<(), Error> {
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
    control.shutdown();
    wait_benchmark_core(env)
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

/// Rust carrier for upstream `TxInsCollateral era, [Fund]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedCollateral {
    /// Collateral transaction inputs in upstream `getFundTxIn` order.
    pub tx_ins: Vec<String>,
    /// Funds retained beside the generated transaction stream.
    pub funds: Vec<Fund>,
}

/// Mirror of upstream `selectCollateralFunds`.
pub fn select_collateral_funds(
    env: &Env,
    era: AnyCardanoEra,
    collateral_wallet: Option<&str>,
) -> Result<SelectedCollateral, Error> {
    let Some(wallet_name) = collateral_wallet else {
        return Ok(SelectedCollateral {
            tx_ins: Vec::new(),
            funds: Vec::new(),
        });
    };

    let collateral_funds = get_env_wallets(env, wallet_name)?.funds();
    if collateral_funds.is_empty() {
        return Err(Error::WalletError(
            "selectCollateralFunds: emptylist".to_string(),
        ));
    }
    if !collateral_supported_in_era(era) {
        return Err(Error::WalletError(format!(
            "selectCollateralFunds: collateral: era not supported :{era:?}"
        )));
    }

    Ok(SelectedCollateral {
        tx_ins: collateral_funds
            .iter()
            .map(|fund| get_fund_tx_in(fund).to_string())
            .collect(),
        funds: collateral_funds,
    })
}

/// Result returned by upstream `interpretPayMode`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterpretedPayMode {
    /// Destination output/fund builder for the selected pay mode.
    pub to_utxo: ToUtxo,
    /// Destination wallet that receives generated funds.
    pub destination_wallet: String,
    /// Raw address bytes rendered as hex until the Bech32 surface lands.
    pub address_hex: String,
}

/// Result returned by upstream `makePlutusContext`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlutusContext {
    /// Spending witness for generated script funds.
    pub witness: FundWitness,
    /// Plutus script in an upstream language wrapper.
    pub script: ScriptInAnyLang,
    /// Datum placed on generated script outputs.
    pub script_data: PlutusData,
    /// Script fee computed from execution-unit prices.
    pub script_fee: Lovelace,
}

#[derive(Clone, Debug, PartialEq)]
struct ScriptProtocolParameters {
    execution_unit_prices: ExecutionUnitPrices,
    max_tx_execution_units: ExecutionUnits,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ExecutionUnitPrices {
    price_execution_memory: f64,
    price_execution_steps: f64,
}

/// Mirror of upstream `makePlutusContext`.
pub fn make_plutus_context(
    env: &mut Env,
    _era: AnyCardanoEra,
    script_spec: &ScriptSpec,
) -> Result<PlutusContext, Error> {
    let protocol_parameters = get_protocol_parameters(env)?;
    let script_parameters = script_protocol_parameters(&protocol_parameters)?;
    let (script, resolved_to) =
        read_plutus_script(&script_spec.script_spec_file).map_err(lift_tx_gen_error)?;

    trace_debug(
        env,
        &format!(
            "Plutus auto mode : Available budget per TX: {:?}",
            script_parameters.max_tx_execution_units
        ),
    );

    let (script_data, script_redeemer, execution_units) = match &script_spec.script_spec_budget {
        ScriptBudget::StaticScriptBudget(data_file, redeemer_file, units, with_check) => {
            let script_data = read_script_data(data_file).map_err(lift_tx_gen_error)?;
            let redeemer = read_script_data(redeemer_file).map_err(lift_tx_gen_error)?;
            if *with_check {
                let pre_execution_parameters =
                    ledger_protocol_parameters(&protocol_parameters, "makePlutusContext")?
                        .ok_or_else(|| {
                            Error::WalletError(format!(
                                "makePlutusContext preExecuteScript failed: preExecutePlutusScript: cost model unavailable for: {:?}",
                                script.language
                            ))
                        })?;
                let pre_execution_units = pre_execute_plutus_script(
                    &pre_execution_parameters,
                    &script,
                    &script_data,
                    &redeemer,
                )
                .map_err(|err| {
                    Error::WalletError(format!("makePlutusContext preExecuteScript failed: {err}"))
                })?;
                if units != &pre_execution_units {
                    return Err(Error::WalletError(format!(
                        " Stated execution Units do not match result of pre execution.  Stated value : {:?} PreExecution result : {:?}",
                        units, pre_execution_units
                    )));
                }
            }
            (script_data, redeemer, *units)
        }
        ScriptBudget::AutoScript(redeemer_file, tx_inputs) => {
            let redeemer = read_script_data(redeemer_file).map_err(lift_tx_gen_error)?;
            let strategy = match script_spec.script_spec_plutus_type {
                TxGenPlutusType::LimitTxPerBlock8 => {
                    PlutusBudgetFittingStrategy::TargetTxsPerBlock(8)
                }
                TxGenPlutusType::LimitTxPerBlock4 => {
                    PlutusBudgetFittingStrategy::TargetTxsPerBlock(4)
                }
                _ => PlutusBudgetFittingStrategy::TargetTxExpenditure,
            };
            let auto_budget = PlutusAutoBudget {
                auto_budget_units: script_parameters.max_tx_execution_units,
                auto_budget_datum: PlutusData::integer(0),
                auto_budget_redeemer: script_data_modify_number(&redeemer, |_| {
                    BigInt::from(1_000_000u64)
                }),
                auto_budget_upper_bound_hint: None,
            };
            let pre_execution_parameters =
                ledger_protocol_parameters(&protocol_parameters, "makePlutusContext")?
                    .ok_or_else(|| {
                        Error::WalletError(format!(
                            "makePlutusContext preExecuteScript failed: preExecutePlutusScript: cost model unavailable for: {:?}",
                            script.language
                        ))
                    })?;
            let script_info = (resolved_to.to_string(), strategy.to_string());
            trace_debug(
                env,
                &format!(
                    "Plutus auto mode : Available budget per Tx: {:?} -- split between inputs per Tx: {}",
                    script_parameters.max_tx_execution_units, tx_inputs
                ),
            );

            let (summary, auto_budget, pre_run) = plutus_auto_scale_blockfit(
                &pre_execution_parameters,
                script_info,
                &script,
                auto_budget,
                strategy,
                *tx_inputs,
            )
            .map_err(lift_tx_gen_error)?;
            set_env_summary(env, summary.to_json_value());
            dump_budget_summary_if_existing(env)?;
            (
                auto_budget.auto_budget_datum,
                auto_budget.auto_budget_redeemer,
                pre_run,
            )
        }
    };

    trace_debug(
        env,
        &format!(
            "Plutus Benchmark : Script: {:?}, ResolvedTo: {}, Datum: {:?}, Redeemer: {:?}, StatedBudget: {:?}",
            script_spec.script_spec_file,
            resolved_to,
            script_data,
            script_redeemer,
            execution_units
        ),
    );

    let script_fee =
        script_fee_from_prices(&script_parameters.execution_unit_prices, &execution_units)?;
    let witness = FundWitness::ScriptWitness(ScriptWitnessForSpending {
        language: format!("{:?}", script.language),
        script_bytes: script.bytes.clone(),
        datum: script_data.clone(),
        redeemer: script_redeemer,
        execution_units,
    });

    Ok(PlutusContext {
        witness,
        script,
        script_data,
        script_fee,
    })
}

/// Mirror of upstream `dumpBudgetSummaryIfExisting`.
fn dump_budget_summary_if_existing(env: &mut Env) -> Result<(), Error> {
    let summary = get_env_summary(env)
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let rendered = serde_json::to_string_pretty(&summary)
        .map_err(|err| lift_tx_gen_error(format!("prettyPrintOrdered: {err}")))?;
    fs::write(PLUTUS_BUDGET_SUMMARY_FILE, format!("{rendered}\n")).map_err(|err| {
        lift_tx_gen_error(format!(
            "dumpBudgetSummaryIfExisting: write {PLUTUS_BUDGET_SUMMARY_FILE}: {err}"
        ))
    })?;
    trace_debug(
        env,
        &format!(
            "dumpBudgetSummaryIfExisting : budget summary created/updated in: {PLUTUS_BUDGET_SUMMARY_FILE}"
        ),
    );
    Ok(())
}

/// Mirror of upstream `interpretPayMode`.
pub fn interpret_pay_mode(
    env: &mut Env,
    era: AnyCardanoEra,
    pay_mode: &PayMode,
) -> Result<InterpretedPayMode, Error> {
    let network_id = get_env_network_id(env)?.clone();
    match pay_mode {
        PayMode::PayToAddr(key_name, dest_wallet) => {
            let fund_key = get_env_keys(env, key_name)?.clone();
            let _wallet_ref = get_env_wallets(env, dest_wallet)?;
            let to_utxo =
                mk_utxo_variant(era, network_id.clone(), key_name.clone(), fund_key.clone())
                    .map_err(lift_tx_gen_error)?;
            let address_hex =
                hex::encode(key_address(&network_id, &fund_key).map_err(lift_tx_gen_error)?);
            Ok(InterpretedPayMode {
                to_utxo,
                destination_wallet: dest_wallet.clone(),
                address_hex,
            })
        }
        PayMode::PayToScript(script_spec, dest_wallet) => {
            let _wallet_ref = get_env_wallets(env, dest_wallet)?;
            let context = make_plutus_context(env, era, script_spec)?;
            let address_hex = hex::encode(script_address(&network_id, &context.script));
            let to_utxo = mk_utxo_script(
                era,
                network_id,
                context.script.clone(),
                context.script_data,
                context.witness,
            )
            .map_err(lift_tx_gen_error)?;
            Ok(InterpretedPayMode {
                address_hex,
                to_utxo,
                destination_wallet: dest_wallet.clone(),
            })
        }
    }
}

fn script_protocol_parameters(value: &Value) -> Result<ScriptProtocolParameters, Error> {
    if let Some(cbor_hex) = value.get("eraNativeCborHex").and_then(Value::as_str) {
        let bytes = hex::decode(cbor_hex).map_err(|err| {
            lift_tx_gen_error(format!(
                "makePlutusContext: eraNativeCborHex is not valid hex: {err}"
            ))
        })?;
        let params = ProtocolParameters::from_cbor_bytes(&bytes).map_err(|err| {
            lift_tx_gen_error(format!(
                "makePlutusContext: could not decode era-native protocol parameters: {err}"
            ))
        })?;
        let price_mem = params.price_mem.ok_or_else(|| {
            Error::WalletError(
                "unexpected protocolParamPrices == Nothing in runPlutusBenchmark".to_string(),
            )
        })?;
        let price_step = params.price_step.ok_or_else(|| {
            Error::WalletError(
                "unexpected protocolParamPrices == Nothing in runPlutusBenchmark".to_string(),
            )
        })?;
        let max_tx_ex_units = params
            .max_tx_ex_units
            .ok_or_else(|| lift_tx_gen_error("Cannot determine protocolParamMaxTxExUnits"))?;
        return Ok(ScriptProtocolParameters {
            execution_unit_prices: ExecutionUnitPrices {
                price_execution_memory: unit_interval_to_f64(price_mem)?,
                price_execution_steps: unit_interval_to_f64(price_step)?,
            },
            max_tx_execution_units: ExecutionUnits {
                execution_steps: max_tx_ex_units.steps,
                execution_memory: max_tx_ex_units.mem,
            },
        });
    }

    let prices = value.get("executionUnitPrices").ok_or_else(|| {
        Error::WalletError(
            "unexpected protocolParamPrices == Nothing in runPlutusBenchmark".to_string(),
        )
    })?;
    let max_tx = value
        .get("maxTxExecutionUnits")
        .ok_or_else(|| lift_tx_gen_error("Cannot determine protocolParamMaxTxExUnits"))?;

    Ok(ScriptProtocolParameters {
        execution_unit_prices: ExecutionUnitPrices {
            price_execution_memory: parse_json_f64_field(
                prices,
                &["priceMemory", "priceMem", "memory", "executionMemory"],
                "executionUnitPrices.priceMemory",
            )?,
            price_execution_steps: parse_json_f64_field(
                prices,
                &["priceSteps", "priceStep", "steps", "executionSteps"],
                "executionUnitPrices.priceSteps",
            )?,
        },
        max_tx_execution_units: ExecutionUnits {
            execution_steps: parse_json_u64_field(
                max_tx,
                &["steps", "executionSteps"],
                "maxTxExecutionUnits.steps",
            )?,
            execution_memory: parse_json_u64_field(
                max_tx,
                &["memory", "mem", "executionMemory"],
                "maxTxExecutionUnits.memory",
            )?,
        },
    })
}

fn tx_protocol_parameters(value: &Value) -> Result<Option<ProtocolParameters>, Error> {
    ledger_protocol_parameters(value, "genTx")
}

fn ledger_protocol_parameters(
    value: &Value,
    error_context: &str,
) -> Result<Option<ProtocolParameters>, Error> {
    if let Some(cbor_hex) = value.get("eraNativeCborHex").and_then(Value::as_str) {
        let bytes = hex::decode(cbor_hex).map_err(|err| {
            lift_tx_gen_error(format!(
                "{error_context}: eraNativeCborHex is not valid hex: {err}"
            ))
        })?;
        return ProtocolParameters::from_cbor_bytes(&bytes)
            .map(Some)
            .map_err(|err| {
                lift_tx_gen_error(format!(
                    "{error_context}: could not decode era-native protocol parameters: {err}"
                ))
            });
    }

    let Some(cost_models_value) = value
        .get("costModels")
        .or_else(|| value.get("costmodels"))
        .or_else(|| value.get("cost_models"))
    else {
        return Ok(None);
    };
    let cost_model_object = cost_models_value.as_object().ok_or_else(|| {
        lift_tx_gen_error(format!("{error_context}: costModels: expected object"))
    })?;
    let mut cost_models = BTreeMap::new();
    for (language, model) in cost_model_object {
        let key = cost_model_language_key(language, error_context)?;
        let entries = model.as_array().ok_or_else(|| {
            lift_tx_gen_error(format!(
                "{error_context}: costModels.{language}: expected array"
            ))
        })?;
        let mut values = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            values.push(cost_model_entry_i64(entry).map_err(|err| {
                lift_tx_gen_error(format!(
                    "{error_context}: costModels.{language}[{index}]: {err}"
                ))
            })?);
        }
        cost_models.insert(key, values);
    }

    let mut parameters = ProtocolParameters::alonzo_defaults();
    parameters.cost_models = Some(cost_models);
    parameters.protocol_version = parse_optional_protocol_version(value, error_context)?;
    if let Some(max_tx_ex_units) = parse_optional_max_tx_ex_units(value, error_context)? {
        parameters.max_tx_ex_units = Some(max_tx_ex_units);
    }
    if let Some(max_block_ex_units) = parse_optional_max_block_ex_units(value, error_context)? {
        parameters.max_block_ex_units = Some(max_block_ex_units);
    }
    Ok(Some(parameters))
}

fn cost_model_language_key(language: &str, error_context: &str) -> Result<u8, Error> {
    match language {
        "PlutusV1" | "PlutusScriptV1" | "0" => Ok(0),
        "PlutusV2" | "PlutusScriptV2" | "1" => Ok(1),
        "PlutusV3" | "PlutusScriptV3" | "2" => Ok(2),
        other => Err(lift_tx_gen_error(format!(
            "{error_context}: unsupported cost model language `{other}`"
        ))),
    }
}

fn parse_optional_protocol_version(
    value: &Value,
    error_context: &str,
) -> Result<Option<(u64, u64)>, Error> {
    let Some(protocol_version) = value
        .get("protocolVersion")
        .or_else(|| value.get("protocol_version"))
    else {
        return Ok(ProtocolParameters::alonzo_defaults().protocol_version);
    };

    if let Some(array) = protocol_version.as_array() {
        if array.len() != 2 {
            return Err(lift_tx_gen_error(format!(
                "{error_context}: protocolVersion: expected 2-element array"
            )));
        }
        return Ok(Some((
            json_u64_value(&array[0], &format!("{error_context}: protocolVersion[0]"))?,
            json_u64_value(&array[1], &format!("{error_context}: protocolVersion[1]"))?,
        )));
    }

    Ok(Some((
        parse_json_u64_field(
            protocol_version,
            &["major", "protocolMajor"],
            "protocolVersion.major",
        )?,
        parse_json_u64_field(
            protocol_version,
            &["minor", "protocolMinor"],
            "protocolVersion.minor",
        )?,
    )))
}

fn parse_optional_max_tx_ex_units(
    value: &Value,
    error_context: &str,
) -> Result<Option<ExUnits>, Error> {
    parse_optional_ex_units(
        value,
        error_context,
        &[
            "maxTxExecutionUnits",
            "max_tx_execution_units",
            "maxTxExUnits",
            "max_tx_ex_units",
        ],
        "maxTxExecutionUnits",
    )
}

fn parse_optional_max_block_ex_units(
    value: &Value,
    error_context: &str,
) -> Result<Option<ExUnits>, Error> {
    parse_optional_ex_units(
        value,
        error_context,
        &[
            "maxBlockExecutionUnits",
            "max_block_execution_units",
            "maxBlockExUnits",
            "max_block_ex_units",
        ],
        "maxBlockExecutionUnits",
    )
}

fn parse_optional_ex_units(
    value: &Value,
    error_context: &str,
    names: &[&str],
    label: &str,
) -> Result<Option<ExUnits>, Error> {
    let Some(ex_units) = names.iter().find_map(|name| value.get(*name)) else {
        return Ok(None);
    };
    let steps_field = format!("{error_context}: {label}.steps");
    let memory_field = format!("{error_context}: {label}.memory");
    Ok(Some(ExUnits {
        steps: parse_json_u64_field(
            ex_units,
            &["steps", "executionSteps", "exUnitsSteps"],
            &steps_field,
        )?,
        mem: parse_json_u64_field(
            ex_units,
            &["memory", "mem", "executionMemory", "exUnitsMem"],
            &memory_field,
        )?,
    }))
}

fn json_u64_value(value: &Value, field: &str) -> Result<u64, Error> {
    value
        .as_u64()
        .ok_or_else(|| lift_tx_gen_error(format!("{field}: expected unsigned integer")))
}

fn cost_model_entry_i64(value: &Value) -> Result<i64, String> {
    let number = value
        .as_number()
        .ok_or_else(|| "expected integer".to_string())?;
    if let Some(value) = number.as_i64() {
        return Ok(value);
    }
    let value = number
        .as_u64()
        .ok_or_else(|| "expected signed 64-bit integer".to_string())?;
    i64::try_from(value).map_err(|_| "expected signed 64-bit integer".to_string())
}

fn script_fee_from_prices(
    prices: &ExecutionUnitPrices,
    execution_units: &ExecutionUnits,
) -> Result<Lovelace, Error> {
    let fee = (execution_units.execution_steps as f64 * prices.price_execution_steps)
        + (execution_units.execution_memory as f64 * prices.price_execution_memory);
    if !fee.is_finite() || fee < 0.0 {
        return Err(lift_tx_gen_error(
            "makePlutusContext: script fee calculation produced a non-finite value",
        ));
    }
    if fee.ceil() > u64::MAX as f64 {
        return Err(lift_tx_gen_error(
            "makePlutusContext: script fee exceeds Lovelace range",
        ));
    }
    Ok(fee.ceil() as Lovelace)
}

fn parse_json_f64_field(value: &Value, names: &[&str], label: &str) -> Result<f64, Error> {
    let field = find_json_field(value, names, label)?;
    match field {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| lift_tx_gen_error(format!("{label}: expected finite number"))),
        Value::Object(object) => {
            let numerator = object
                .get("numerator")
                .and_then(Value::as_f64)
                .ok_or_else(|| lift_tx_gen_error(format!("{label}.numerator: expected number")))?;
            let denominator = object
                .get("denominator")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    lift_tx_gen_error(format!("{label}.denominator: expected number"))
                })?;
            if denominator == 0.0 {
                Err(lift_tx_gen_error(format!(
                    "{label}.denominator: expected non-zero number"
                )))
            } else {
                Ok(numerator / denominator)
            }
        }
        _ => Err(lift_tx_gen_error(format!("{label}: expected number"))),
    }
}

fn parse_json_u64_field(value: &Value, names: &[&str], label: &str) -> Result<u64, Error> {
    find_json_field(value, names, label)?
        .as_u64()
        .ok_or_else(|| lift_tx_gen_error(format!("{label}: expected unsigned integer")))
}

fn find_json_field<'a>(value: &'a Value, names: &[&str], label: &str) -> Result<&'a Value, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| lift_tx_gen_error(format!("{label}: expected object")))?;
    names
        .iter()
        .find_map(|name| object.get(*name))
        .ok_or_else(|| lift_tx_gen_error(format!("{label}: missing field")))
}

fn unit_interval_to_f64(value: yggdrasil_ledger::UnitInterval) -> Result<f64, Error> {
    if value.denominator == 0 {
        return Err(lift_tx_gen_error(
            "makePlutusContext: execution unit price denominator is zero",
        ));
    }
    Ok(value.numerator as f64 / value.denominator as f64)
}

/// Mirror of upstream `submitAction`.
pub fn submit_action(
    env: &mut Env,
    era: AnyCardanoEra,
    submit_mode: &SubmitMode,
    generator: &Generator,
    tx_params: &TxGenTxParams,
) -> Result<(), Error> {
    with_era(era, |era| {
        submit_in_era(env, era, submit_mode, generator, tx_params)
    })
}

/// Mirror of upstream `submitInEra`.
pub fn submit_in_era(
    env: &mut Env,
    era: AnyCardanoEra,
    submit_mode: &SubmitMode,
    generator: &Generator,
    tx_params: &TxGenTxParams,
) -> Result<(), Error> {
    let protocol_parameters = get_protocol_parameters(env)?;
    let tx_protocol_parameters = tx_protocol_parameters(&protocol_parameters)?;
    match submit_mode {
        SubmitMode::NodeToNode(_) => Err(lift_tx_gen_error("NodeToNode deprecated: ToDo: remove")),
        SubmitMode::Benchmark(nodes, tps_rate, tx_count) => {
            let txs = eval_generator(
                env,
                era,
                generator,
                tx_params,
                tx_protocol_parameters.as_ref(),
                None,
            )?;
            benchmark_tx_stream(env, nodes.clone(), *tps_rate, *tx_count, txs)
        }
        SubmitMode::DumpToFile(file_path) => {
            let txs = eval_generator(
                env,
                era,
                generator,
                tx_params,
                tx_protocol_parameters.as_ref(),
                None,
            )?;
            dump_txs_to_file(file_path, &txs)
        }
        SubmitMode::DiscardTx => {
            let _txs = eval_generator(
                env,
                era,
                generator,
                tx_params,
                tx_protocol_parameters.as_ref(),
                None,
            )?;
            Ok(())
        }
        SubmitMode::LocalSocket => {
            let txs = eval_generator(
                env,
                era,
                generator,
                tx_params,
                tx_protocol_parameters.as_ref(),
                None,
            )?;
            submit_generated_txs_local_socket(env, &txs)
        }
    }
}

fn benchmark_tx_stream(
    env: &mut Env,
    target_nodes: Vec<NodeDescription>,
    tps_rate: TpsRate,
    tx_count: NumberOfTxs,
    txs: Vec<GeneratedTx>,
) -> Result<(), Error> {
    trace_debug(
        env,
        "******* Tx generator, phase 2: pay to recipients *******",
    );
    trace_debug(
        env,
        &format!(
            "******* Tx generator, launching Tx peers:  {} of them",
            target_nodes.len()
        ),
    );
    let network_magic = network_id_to_magic(get_env_network_id(env)?)?;
    let worker_threads = std::cmp::max(2, target_nodes.len() + 1);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| lift_tx_gen_error(format!("walletBenchmark runtime: {err}")))?;
    let control = runtime
        .block_on(wallet_benchmark(
            target_nodes,
            tps_rate,
            SubmissionErrorPolicy::LogErrors,
            network_magic,
            tx_count,
            txs,
        ))
        .map_err(wallet_benchmark_error)?;
    set_env_threads(
        env,
        AsyncBenchmarkControl::from_wallet_benchmark(runtime, control),
    );
    Ok(())
}

fn wallet_benchmark_error(err: WalletBenchmarkError) -> Error {
    lift_tx_gen_error(err.to_string())
}

fn dump_txs_to_file(file_path: &Path, txs: &[GeneratedTx]) -> Result<(), Error> {
    let mut rendered = String::new();
    for tx in txs {
        rendered.push_str(&show_tx_for_dump(tx)?);
    }
    fs::write(file_path, rendered).map_err(|err| {
        lift_tx_gen_error(format!(
            "DumpToFile: failed to write {}: {err}",
            file_path.display()
        ))
    })
}

fn show_tx_for_dump(generated: &GeneratedTx) -> Result<String, Error> {
    match &generated.tx {
        yggdrasil_ledger::MultiEraSubmittedTx::Shelley(tx) => show_shelley_tx_for_dump(tx),
        yggdrasil_ledger::MultiEraSubmittedTx::Allegra(tx) => show_allegra_tx_for_dump(tx),
        yggdrasil_ledger::MultiEraSubmittedTx::Mary(tx) => show_mary_tx_for_dump(tx),
        yggdrasil_ledger::MultiEraSubmittedTx::Alonzo(tx) => show_alonzo_tx_for_dump(tx),
        yggdrasil_ledger::MultiEraSubmittedTx::Babbage(tx) => show_babbage_tx_for_dump(tx),
        yggdrasil_ledger::MultiEraSubmittedTx::Conway(tx) => show_conway_tx_for_dump(tx),
    }
}

fn show_shelley_tx_for_dump(
    tx: &ShelleyCompatibleSubmittedTx<ShelleyTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Shelley", "stbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    ensure_absent(tx.body.update.as_ref(), "Shelley", "stbrUpdate")?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_absent(tx.auxiliary_data.as_ref(), "Shelley", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_shelley_tx_out_list(&tx.body.outputs, "Shelley")?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Shelley")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraShelley (ShelleyTx {{stBody = MkShelleyTxBody ShelleyTxBodyRaw {{stbrInputs = fromList [{inputs}], stbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, stbrCerts = StrictSeq {{fromStrict = fromList []}}, stbrWithdrawals = {withdrawals}, stbrFee = Coin {}, stbrTtl = SlotNo {}, stbrUpdate = SNothing, stbrAuxDataHash = {aux_data_hash}}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee, tx.body.ttl,
    ))
}

fn show_allegra_tx_for_dump(
    tx: &ShelleyCompatibleSubmittedTx<AllegraTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Allegra", "atbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    ensure_absent(tx.body.update.as_ref(), "Allegra", "atbrUpdate")?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_absent(tx.auxiliary_data.as_ref(), "Allegra", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_shelley_tx_out_list(&tx.body.outputs, "Allegra")?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Allegra")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraAllegra (ShelleyTx {{stBody = MkAllegraTxBody AllegraTxBodyRaw {{atbrInputs = fromList [{inputs}], atbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, atbrCerts = StrictSeq {{fromStrict = fromList []}}, atbrWithdrawals = {withdrawals}, atbrFee = Coin {}, atbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, atbrUpdate = SNothing, atbrAuxDataHash = {aux_data_hash}, atbrMint = ()}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

fn show_mary_tx_for_dump(tx: &ShelleyCompatibleSubmittedTx<MaryTxBody>) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Mary", "atbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    ensure_absent(tx.body.update.as_ref(), "Mary", "atbrUpdate")?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_empty_mint(tx.body.mint.as_ref(), "Mary", "atbrMint")?;
    ensure_absent(tx.auxiliary_data.as_ref(), "Mary", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_mary_tx_out_list(&tx.body.outputs)?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Mary")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraMary (ShelleyTx {{stBody = MkMaryTxBody AllegraTxBodyRaw {{atbrInputs = fromList [{inputs}], atbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, atbrCerts = StrictSeq {{fromStrict = fromList []}}, atbrWithdrawals = {withdrawals}, atbrFee = Coin {}, atbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, atbrUpdate = SNothing, atbrAuxDataHash = {aux_data_hash}, atbrMint = MultiAsset (fromList [])}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

fn show_alonzo_tx_for_dump(
    tx: &AlonzoCompatibleSubmittedTx<AlonzoTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Alonzo", "atbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    ensure_absent(tx.body.update.as_ref(), "Alonzo", "atbrUpdate")?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_empty_mint(tx.body.mint.as_ref(), "Alonzo", "atbrMint")?;
    let script_integrity_hash = show_strict_maybe_script_integrity_hash(tx.body.script_data_hash);
    let collateral = show_tx_in_list(tx.body.collateral.as_deref().unwrap_or_default());
    ensure_absent(tx.auxiliary_data.as_ref(), "Alonzo", "atAuxData")?;

    let req_signer_hashes = show_req_signer_hashes(tx.body.required_signers.as_deref());
    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_alonzo_tx_out_list(&tx.body.outputs)?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_alonzo_witness_set(&tx.witness_set)?;
    let is_valid = show_haskell_bool(tx.is_valid);
    let network_id = show_strict_maybe_network(tx.body.network_id)?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraAlonzo (AlonzoTx {{atBody = MkAlonzoTxBody AlonzoTxBodyRaw {{atbrInputs = fromList [{inputs}], atbrCollateral = fromList [{collateral}], atbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, atbrCerts = StrictSeq {{fromStrict = fromList []}}, atbrWithdrawals = {withdrawals}, atbrTxFee = Coin {}, atbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, atbrUpdate = SNothing, atbrReqSignerHashes = {req_signer_hashes}, atbrMint = MultiAsset (fromList []), atbrScriptIntegrityHash = {script_integrity_hash}, atbrAuxDataHash = {aux_data_hash}, atbrTxNetworkId = {network_id}}} (blake2b_256: SafeHash \"{body_hash}\"), atWits = {witnesses}, atIsValid = IsValid {is_valid}, atAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

fn show_babbage_tx_for_dump(
    tx: &AlonzoCompatibleSubmittedTx<BabbageTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Babbage", "btbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    ensure_absent(tx.body.update.as_ref(), "Babbage", "btbrUpdate")?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_empty_mint(tx.body.mint.as_ref(), "Babbage", "btbrMint")?;
    let script_integrity_hash = show_strict_maybe_script_integrity_hash(tx.body.script_data_hash);
    let collateral = show_tx_in_list(tx.body.collateral.as_deref().unwrap_or_default());
    ensure_absent(
        tx.body.collateral_return.as_ref(),
        "Babbage",
        "btbrCollateralReturn",
    )?;
    ensure_absent(
        tx.body.total_collateral.as_ref(),
        "Babbage",
        "btbrTotalCollateral",
    )?;
    ensure_empty_or_absent(
        tx.body.reference_inputs.as_deref(),
        "Babbage",
        "btbrReferenceInputs",
    )?;
    ensure_absent(tx.auxiliary_data.as_ref(), "Babbage", "atAuxData")?;

    let req_signer_hashes = show_req_signer_hashes(tx.body.required_signers.as_deref());
    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_babbage_tx_out_list(&tx.body.outputs)?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_alonzo_witness_set(&tx.witness_set)?;
    let is_valid = show_haskell_bool(tx.is_valid);
    let network_id = show_strict_maybe_network(tx.body.network_id)?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraBabbage (AlonzoTx {{atBody = MkBabbageTxBody BabbageTxBodyRaw {{btbrInputs = fromList [{inputs}], btbrCollateralInputs = fromList [{collateral}], btbrReferenceInputs = fromList [], btbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, btbrCollateralReturn = SNothing, btbrTotalCollateral = SNothing, btbrCerts = StrictSeq {{fromStrict = fromList []}}, btbrWithdrawals = {withdrawals}, btbrFee = Coin {}, btbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, btbrUpdate = SNothing, btbrReqSignerHashes = {req_signer_hashes}, btbrMint = MultiAsset (fromList []), btbrScriptIntegrityHash = {script_integrity_hash}, btbrAuxDataHash = {aux_data_hash}, btbrNetworkId = {network_id}}} (blake2b_256: SafeHash \"{body_hash}\"), atWits = {witnesses}, atIsValid = IsValid {is_valid}, atAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

fn show_conway_tx_for_dump(
    tx: &AlonzoCompatibleSubmittedTx<ConwayTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Conway", "ctbrCerts")?;
    let withdrawals = show_withdrawals(tx.body.withdrawals.as_ref())?;
    let aux_data_hash = show_strict_maybe_aux_data_hash(tx.body.auxiliary_data_hash);
    ensure_empty_mint(tx.body.mint.as_ref(), "Conway", "ctbrMint")?;
    let script_integrity_hash = show_strict_maybe_script_integrity_hash(tx.body.script_data_hash);
    let collateral = show_tx_in_list(tx.body.collateral.as_deref().unwrap_or_default());
    ensure_absent(
        tx.body.collateral_return.as_ref(),
        "Conway",
        "ctbrCollateralReturn",
    )?;
    ensure_absent(
        tx.body.total_collateral.as_ref(),
        "Conway",
        "ctbrTotalCollateral",
    )?;
    ensure_empty_or_absent(
        tx.body.reference_inputs.as_deref(),
        "Conway",
        "ctbrReferenceInputs",
    )?;
    let voting_procedures = show_conway_voting_procedures(tx.body.voting_procedures.as_ref());
    let proposal_procedures =
        show_conway_proposal_procedures(tx.body.proposal_procedures.as_deref())?;
    let current_treasury_value = show_strict_maybe_coin(tx.body.current_treasury_value);
    let treasury_donation = show_coin(tx.body.treasury_donation.unwrap_or(0));
    ensure_absent(tx.auxiliary_data.as_ref(), "Conway", "atAuxData")?;

    let req_signer_hashes = show_req_signer_hashes(tx.body.required_signers.as_deref());
    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_babbage_tx_out_list(&tx.body.outputs)?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_alonzo_witness_set(&tx.witness_set)?;
    let is_valid = show_haskell_bool(tx.is_valid);
    let network_id = show_strict_maybe_network(tx.body.network_id)?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraConway (AlonzoTx {{atBody = MkConwayTxBody ConwayTxBodyRaw {{ctbrSpendInputs = fromList [{inputs}], ctbrCollateralInputs = fromList [{collateral}], ctbrReferenceInputs = fromList [], ctbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, ctbrCollateralReturn = SNothing, ctbrTotalCollateral = SNothing, ctbrCerts = OSet {{osSSeq = StrictSeq {{fromStrict = fromList []}}, osSet = fromList []}}, ctbrWithdrawals = {withdrawals}, ctbrFee = Coin {}, ctbrVldt = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, ctbrReqSignerHashes = {req_signer_hashes}, ctbrMint = MultiAsset (fromList []), ctbrScriptIntegrityHash = {script_integrity_hash}, ctbrAuxDataHash = {aux_data_hash}, ctbrNetworkId = {network_id}, ctbrVotingProcedures = {voting_procedures}, ctbrProposalProcedures = {proposal_procedures}, ctbrCurrentTreasuryValue = {current_treasury_value}, ctbrTreasuryDonation = {treasury_donation}}} (blake2b_256: SafeHash \"{body_hash}\"), atWits = {witnesses}, atIsValid = IsValid {is_valid}, atAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

/// Render `Coin <n>` matching upstream `Show Coin` (via `Quiet Coin` —
/// suppresses the record syntax around the unCoin field but keeps the
/// `Coin` constructor name).
fn show_coin(coin: u64) -> String {
    format!("Coin {coin}")
}

/// Render `StrictMaybe Coin` matching upstream stock-derived
/// `Show (StrictMaybe Coin)`: `SNothing` or `SJust (Coin <n>)`. The
/// inner `Coin` is wrapped in parens because `SJust` shows its argument
/// at precedence 11.
fn show_strict_maybe_coin(value: Option<u64>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(coin) => format!("SJust ({})", show_coin(coin)),
    }
}

/// Render `ctbrVotingProcedures = VotingProcedures {unVotingProcedures =
/// fromList [(Voter, fromList [(GovActionId, VotingProcedure)])]}` matching
/// upstream stock-derived Show through the record-newtype `VotingProcedures
/// { unVotingProcedures :: Map Voter (Map GovActionId (VotingProcedure era))
/// }`.
///
/// Outer-map entries are ordered by `Voter` byte-lex (matching upstream
/// `Data.Map` ordering on the derived `Ord Voter` — which follows
/// constructor index then inner hash bytes); inner-map entries by
/// `GovActionId` (matching upstream `Ord` over `(TxId, GovActionIx)`).
/// `BTreeMap` iteration order in Rust matches because the yggdrasil
/// `Voter` and `GovActionId` types `derive Ord` over the same field
/// order.
fn show_conway_voting_procedures(
    procedures: Option<&yggdrasil_ledger::VotingProcedures>,
) -> String {
    let entries = match procedures {
        None => String::new(),
        Some(vp) => vp
            .procedures
            .iter()
            .map(|(voter, inner)| {
                let inner_entries = inner
                    .iter()
                    .map(|(gaid, vp_inner)| {
                        format!(
                            "({},{})",
                            show_conway_gov_action_id(gaid),
                            show_conway_voting_procedure(vp_inner)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("({},fromList [{inner_entries}])", show_conway_voter(voter))
            })
            .collect::<Vec<_>>()
            .join(","),
    };
    format!("VotingProcedures {{unVotingProcedures = fromList [{entries}]}}")
}

/// Render upstream `Show Vote`: `VoteNo`, `VoteYes`, or `Abstain`.
fn show_conway_vote(vote: yggdrasil_ledger::Vote) -> &'static str {
    match vote {
        yggdrasil_ledger::Vote::No => "VoteNo",
        yggdrasil_ledger::Vote::Yes => "VoteYes",
        yggdrasil_ledger::Vote::Abstain => "Abstain",
    }
}

/// Render upstream `Show Voter`. Upstream lifts hash bytes through
/// `Credential HotCommitteeRole` (KeyHashObj / ScriptHashObj) for
/// committee and DRep voters and uses a raw `KeyHash` for stake pool
/// voters.
fn show_conway_voter(voter: &yggdrasil_ledger::Voter) -> String {
    use yggdrasil_ledger::Voter;
    match voter {
        Voter::CommitteeKeyHash(h) => format!(
            "CommitteeVoter (KeyHashObj (KeyHash {{unKeyHash = \"{}\"}}))",
            hex::encode(h)
        ),
        Voter::CommitteeScript(h) => format!(
            "CommitteeVoter (ScriptHashObj (ScriptHash \"{}\"))",
            hex::encode(h)
        ),
        Voter::DRepKeyHash(h) => format!(
            "DRepVoter (KeyHashObj (KeyHash {{unKeyHash = \"{}\"}}))",
            hex::encode(h)
        ),
        Voter::DRepScript(h) => format!(
            "DRepVoter (ScriptHashObj (ScriptHash \"{}\"))",
            hex::encode(h)
        ),
        Voter::StakePool(h) => format!(
            "StakePoolVoter (KeyHash {{unKeyHash = \"{}\"}})",
            hex::encode(h)
        ),
    }
}

/// Render upstream `Show GovActionId`: stock-derived record Show with
/// `gaidTxId :: TxId` (newtype over `SafeHash EraIndependentTxBody`) and
/// `gaidGovActionIx :: GovActionIx` (newtype over `Word16`).
fn show_conway_gov_action_id(id: &yggdrasil_ledger::GovActionId) -> String {
    format!(
        "GovActionId {{gaidTxId = TxId {{unTxId = SafeHash \"{}\"}}, gaidGovActionIx = GovActionIx {{unGovActionIx = {}}}}}",
        hex::encode(id.transaction_id),
        id.gov_action_index
    )
}

/// Render upstream `Show (VotingProcedure era)`: stock-derived record
/// with `vProcVote :: Vote` and `vProcAnchor :: StrictMaybe Anchor`.
fn show_conway_voting_procedure(vp: &yggdrasil_ledger::VotingProcedure) -> String {
    let anchor = match &vp.anchor {
        None => "SNothing".to_string(),
        Some(a) => format!("SJust ({})", show_anchor(a)),
    };
    format!(
        "VotingProcedure {{vProcVote = {}, vProcAnchor = {anchor}}}",
        show_conway_vote(vp.vote)
    )
}

/// Render upstream `Show Anchor`: stock-derived record `Anchor {anchorUrl
/// = Url {urlToText = "..."}, anchorDataHash = SafeHash "<hex>"}`.
fn show_anchor(anchor: &yggdrasil_ledger::Anchor) -> String {
    format!(
        "Anchor {{anchorUrl = {}, anchorDataHash = SafeHash \"{}\"}}",
        show_url(&anchor.url),
        hex::encode(anchor.data_hash)
    )
}

/// Render upstream `Show Url` via record-newtype `Url {urlToText = "..."}`.
fn show_url(url: &str) -> String {
    format!("Url {{urlToText = {url:?}}}")
}

/// Render `ctbrProposalProcedures = OSet {osSSeq = StrictSeq {fromStrict =
/// fromList [...]}, osSet = fromList [...]}` matching upstream stock-derived
/// `Show (OSet (ProposalProcedure era))`.
///
/// All seven `GovAction` variants are rendered (see
/// [`show_conway_gov_action`]).
fn show_conway_proposal_procedures(
    procedures: Option<&[yggdrasil_ledger::ProposalProcedure]>,
) -> Result<String, Error> {
    let body = match procedures {
        None | Some(&[]) => String::new(),
        Some(items) => items
            .iter()
            .map(show_conway_proposal_procedure)
            .collect::<Result<Vec<_>, _>>()?
            .join(","),
    };
    Ok(format!(
        "OSet {{osSSeq = StrictSeq {{fromStrict = fromList [{body}]}}, osSet = fromList [{body}]}}"
    ))
}

/// Render upstream `Show (ProposalProcedure era)`: stock-derived record with
/// `pProcDeposit :: Coin`, `pProcReturnAddr :: AccountAddress`, `pProcGovAction
/// :: GovAction era`, `pProcAnchor :: Anchor`.
fn show_conway_proposal_procedure(
    proc: &yggdrasil_ledger::ProposalProcedure,
) -> Result<String, Error> {
    let return_addr = show_account_address(&proc.reward_account)?;
    let gov_action = show_conway_gov_action(&proc.gov_action)?;
    Ok(format!(
        "ProposalProcedure {{pProcDeposit = {}, pProcReturnAddr = {return_addr}, pProcGovAction = {gov_action}, pProcAnchor = {}}}",
        show_coin(proc.deposit),
        show_anchor(&proc.anchor),
    ))
}

/// Render upstream `Show AccountAddress`: stock-derived record with
/// `aaNetworkId :: Network` (Testnet / Mainnet) and `aaId :: AccountId`
/// (newtype-derived Show over `Credential Staking` — `KeyHashObj` /
/// `ScriptHashObj`).
fn show_account_address(reward_account_bytes: &[u8]) -> Result<String, Error> {
    use yggdrasil_ledger::RewardAccount;
    let ra = RewardAccount::from_bytes(reward_account_bytes).ok_or_else(|| {
        lift_tx_gen_error(
            "DumpToFile: Conway Show(Tx) renderer received invalid reward-account bytes",
        )
    })?;
    show_account_address_from_record(&ra)
}

/// Render upstream `Show AccountAddress` from a typed `RewardAccount` (no
/// byte-decoding needed). Use when the carrier already has structured data
/// (e.g. inside `TreasuryWithdrawals` whose map is keyed by `RewardAccount`).
fn show_account_address_from_record(ra: &yggdrasil_ledger::RewardAccount) -> Result<String, Error> {
    let network = show_network(ra.network)?;
    let inner = match ra.credential {
        yggdrasil_ledger::StakeCredential::AddrKeyHash(h) => format!(
            "KeyHashObj (KeyHash {{unKeyHash = \"{}\"}})",
            hex::encode(h)
        ),
        yggdrasil_ledger::StakeCredential::ScriptHash(h) => {
            format!("ScriptHashObj (ScriptHash \"{}\")", hex::encode(h))
        }
    };
    Ok(format!(
        "AccountAddress {{aaNetworkId = {network}, aaId = {inner}}}"
    ))
}

/// Render upstream `Show (GovAction era)` for all seven variants:
/// `InfoAction`, `NoConfidence`, `HardForkInitiation`,
/// `NewConstitution`, `ParameterChange`, `TreasuryWithdrawals`, and
/// `UpdateCommittee`.
///
/// `ParameterChange` delegates to [`show_conway_pparams_update`],
/// which returns a typed `TxGenError` only when the carried
/// `ProtocolParameterUpdate` sets a Shelley-era-only field that has
/// no Conway `PParamsUpdate` representation (a malformed input, not
/// a missing port).
fn show_conway_gov_action(action: &yggdrasil_ledger::GovAction) -> Result<String, Error> {
    use yggdrasil_ledger::GovAction;
    match action {
        GovAction::InfoAction => Ok("InfoAction".to_string()),
        GovAction::NoConfidence { prev_action_id } => Ok(format!(
            "NoConfidence {}",
            show_strict_maybe_gov_purpose_id(prev_action_id.as_ref())
        )),
        GovAction::HardForkInitiation {
            prev_action_id,
            protocol_version,
        } => Ok(format!(
            "HardForkInitiation {} (ProtVer {{pvMajor = Version {}, pvMinor = {}}})",
            show_strict_maybe_gov_purpose_id(prev_action_id.as_ref()),
            protocol_version.0,
            protocol_version.1,
        )),
        GovAction::NewConstitution {
            prev_action_id,
            constitution,
        } => {
            let guardrails = match constitution.guardrails_script_hash {
                None => "SNothing".to_string(),
                Some(h) => format!("SJust (ScriptHash \"{}\")", hex::encode(h)),
            };
            Ok(format!(
                "NewConstitution {} (Constitution {{constitutionAnchor = {}, constitutionGuardrailsScriptHash = {guardrails}}})",
                show_strict_maybe_gov_purpose_id(prev_action_id.as_ref()),
                show_anchor(&constitution.anchor),
            ))
        }
        GovAction::ParameterChange {
            prev_action_id,
            protocol_param_update,
            guardrails_script_hash,
        } => {
            let ppu = show_conway_pparams_update(protocol_param_update)?;
            let guardrails = match guardrails_script_hash {
                None => "SNothing".to_string(),
                Some(h) => format!("(SJust (ScriptHash \"{}\"))", hex::encode(h)),
            };
            Ok(format!(
                "ParameterChange {} {ppu} {guardrails}",
                show_strict_maybe_gov_purpose_id(prev_action_id.as_ref()),
            ))
        }
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash,
        } => {
            let entries: Result<Vec<String>, Error> = withdrawals
                .iter()
                .map(|(account, coin)| {
                    let addr = show_account_address_from_record(account)?;
                    Ok(format!("({addr},{})", show_coin(*coin)))
                })
                .collect();
            let body = entries?.join(",");
            // StrictMaybe ScriptHash at showsPrec 11: SNothing has no
            // parens (nullary), SJust wraps to `(SJust (ScriptHash "..."))`.
            let guardrails = match guardrails_script_hash {
                None => "SNothing".to_string(),
                Some(h) => format!("(SJust (ScriptHash \"{}\"))", hex::encode(h)),
            };
            Ok(format!(
                "TreasuryWithdrawals (fromList [{body}]) {guardrails}"
            ))
        }
        GovAction::UpdateCommittee {
            prev_action_id,
            members_to_remove,
            members_to_add,
            quorum,
        } => {
            let mut remove_sorted: Vec<&yggdrasil_ledger::StakeCredential> =
                members_to_remove.iter().collect();
            remove_sorted.sort();
            let remove_body = remove_sorted
                .iter()
                .map(|c| show_stake_credential(c))
                .collect::<Vec<_>>()
                .join(",");
            let add_body = members_to_add
                .iter()
                .map(|(c, epoch)| format!("({},EpochNo {epoch})", show_stake_credential(c)))
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!(
                "UpdateCommittee {} (fromList [{remove_body}]) (fromList [{add_body}]) {}",
                show_strict_maybe_gov_purpose_id(prev_action_id.as_ref()),
                show_unit_interval(*quorum),
            ))
        }
    }
}

/// Render upstream `Show (Credential r)`:
///   `KeyHashObj (KeyHash {unKeyHash = "<hex>"})`
///   `ScriptHashObj (ScriptHash "<hex>")`
///
/// Used for Conway committee credentials. Yggdrasil's StakeCredential
/// matches upstream's Credential for these roles structurally (the
/// phantom role tag does not affect Show output).
fn show_stake_credential(credential: &yggdrasil_ledger::StakeCredential) -> String {
    match credential {
        yggdrasil_ledger::StakeCredential::AddrKeyHash(h) => format!(
            "KeyHashObj (KeyHash {{unKeyHash = \"{}\"}})",
            hex::encode(h)
        ),
        yggdrasil_ledger::StakeCredential::ScriptHash(h) => {
            format!("ScriptHashObj (ScriptHash \"{}\")", hex::encode(h))
        }
    }
}

/// Render upstream `Show (PParamsUpdate ConwayEra)` as the
/// 30-field `ConwayPParams` record with all-SNothing field values for
/// the empty-update path. Non-empty updates return a typed `TxGenError`
/// naming the first set field whose per-type Show is not yet ported —
/// these wrap rich domain types (`CoinPerByte`, `CompactForm Coin`,
/// `EpochInterval`, `NonNegativeInterval`, `Prices`, `OrdExUnits`,
/// `PoolVotingThresholds`, `DRepVotingThresholds`, `CostModels`) that
/// each need dedicated Show ports.
///
/// Field order matches upstream `Cardano.Ledger.Conway.PParams.ConwayPParams`
/// — 30 fields starting at `cppTxFeePerByte` and ending at
/// `cppMinFeeRefScriptCostPerByte`. THKD wrapper is transparent
/// (`instance Show (HKD f a) => Show (THKD t f a) where show = show . unTHKD`),
/// so each field renders as the underlying `StrictMaybe value` directly.
fn show_conway_pparams_update(
    ppu: &yggdrasil_ledger::ProtocolParameterUpdate,
) -> Result<String, Error> {
    // The yggdrasil ProtocolParameterUpdate carries a superset of
    // Conway's PParams — it preserves Shelley-era fields (`d`,
    // `extra_entropy`, `min_utxo_value`, `protocol_version`) that
    // Conway dropped. A Conway `ParameterChange` carrying any of
    // those is a malformed input: the field has no Conway
    // `PParamsUpdate` representation, so it is surfaced as a typed
    // rejection rather than rendered.
    let mut set_fields: Vec<&'static str> = Vec::new();
    if ppu.d.is_some() {
        set_fields.push("d (Shelley-era only — no Conway field)");
    }
    if ppu.extra_entropy.is_some() {
        set_fields.push("extra_entropy (Shelley-era only — no Conway field)");
    }
    if ppu.protocol_version.is_some() {
        set_fields.push("protocol_version (HKDNoUpdate — not in PParamsUpdate)");
    }
    if ppu.min_utxo_value.is_some() {
        set_fields.push("min_utxo_value (Shelley-era only — no Conway field)");
    }
    if ppu.price_mem.is_some() != ppu.price_step.is_some() {
        set_fields.push("cppPrices (price_mem and price_step must be set together)");
    }
    if !set_fields.is_empty() {
        return Err(lift_tx_gen_error(format!(
            "DumpToFile: Conway ParameterChange carries field(s) with no Conway PParamsUpdate representation: {}",
            set_fields.join(", ")
        )));
    }

    // Render each field at p=0 inside the record. Coin-family fields
    // (CompactForm Coin / CoinPerByte) render as `SJust (CompactCoin
    // {unCompactCoin = <n>})` matching upstream stock-derived
    // `Show (CompactForm Coin)` and `Show (CoinPerByte)` (newtype-
    // delegated). Plain Word16/Word32/Word64 fields render as `SJust
    // <n>` because their newtypes (Word16/Word32/Word64) use stock
    // primitive Show. Committee size, gov action deposit, etc match
    // similarly.
    Ok(format!(
        "(ConwayPParams {{cppTxFeePerByte = {}, cppTxFeeFixed = {}, cppMaxBBSize = {}, cppMaxTxSize = {}, cppMaxBHSize = {}, cppKeyDeposit = {}, cppPoolDeposit = {}, cppEMax = {}, cppNOpt = {}, cppA0 = {}, cppRho = {}, cppTau = {}, cppProtocolVersion = NoUpdate, cppMinPoolCost = {}, cppCoinsPerUTxOByte = {}, cppCostModels = {}, cppPrices = {}, cppMaxTxExUnits = {}, cppMaxBlockExUnits = {}, cppMaxValSize = {}, cppCollateralPercentage = {}, cppMaxCollateralInputs = {}, cppPoolVotingThresholds = {}, cppDRepVotingThresholds = {}, cppCommitteeMinSize = {}, cppCommitteeMaxTermLength = {}, cppGovActionLifetime = {}, cppGovActionDeposit = {}, cppDRepDeposit = {}, cppDRepActivity = {}, cppMinFeeRefScriptCostPerByte = {}}})",
        show_pparam_compact_coin(ppu.min_fee_a),
        show_pparam_compact_coin(ppu.min_fee_b),
        show_pparam_word(ppu.max_block_body_size),
        show_pparam_word(ppu.max_tx_size),
        show_pparam_word(ppu.max_block_header_size),
        show_pparam_compact_coin(ppu.key_deposit),
        show_pparam_compact_coin(ppu.pool_deposit),
        show_pparam_epoch_interval(ppu.e_max),
        show_pparam_word(ppu.n_opt),
        show_pparam_ratio_interval(ppu.a0),
        show_pparam_ratio_interval(ppu.rho),
        show_pparam_ratio_interval(ppu.tau),
        show_pparam_compact_coin(ppu.min_pool_cost),
        show_pparam_compact_coin(ppu.coins_per_utxo_byte),
        show_pparam_cost_models(ppu.cost_models.as_ref()),
        show_pparam_prices(ppu.price_mem, ppu.price_step),
        show_pparam_ex_units(ppu.max_tx_ex_units.as_ref()),
        show_pparam_ex_units(ppu.max_block_ex_units.as_ref()),
        show_pparam_word(ppu.max_val_size),
        show_pparam_word(ppu.collateral_percentage),
        show_pparam_word(ppu.max_collateral_inputs),
        show_pparam_pool_voting_thresholds(ppu.pool_voting_thresholds.as_ref()),
        show_pparam_drep_voting_thresholds(ppu.drep_voting_thresholds.as_ref()),
        show_pparam_word(ppu.min_committee_size),
        show_pparam_epoch_interval(ppu.committee_term_limit),
        show_pparam_epoch_interval(ppu.gov_action_lifetime),
        show_pparam_compact_coin(ppu.gov_action_deposit),
        show_pparam_compact_coin(ppu.drep_deposit),
        show_pparam_epoch_interval(ppu.drep_activity),
        show_pparam_ratio_interval(ppu.min_fee_ref_script_cost_per_byte),
    ))
}

/// Render a `StrictMaybe EpochInterval` PParamsUpdate field at showsPrec 0.
/// EpochInterval is `newtype EpochInterval = EpochInterval { unEpochInterval
/// :: Word32 }` with `deriving (Show) via Quiet EpochInterval` — Quiet
/// suppresses the record syntax but keeps the constructor name. Empty
/// renders as `SNothing`; set renders as `SJust (EpochInterval <n>)` (parens
/// because SJust wraps single-arg constructor at p=11).
fn show_pparam_epoch_interval<N: Into<u64> + Copy>(value: Option<N>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(n) => format!("SJust (EpochInterval {})", n.into()),
    }
}

/// Render a `StrictMaybe` over an interval newtype (`UnitInterval`,
/// `NonNegativeInterval`) that delegates Show through `BoundedRatio`
/// to `Show (Ratio Word64)` — output is `<num> % <den>` (no constructor
/// prefix because of `deriving newtype Show`). Empty renders as
/// `SNothing`; set renders as `SJust (<num> % <den>)` (parens because
/// Ratio Show wraps at p > 7, inside SJust at p=11).
fn show_pparam_ratio_interval(value: Option<yggdrasil_ledger::UnitInterval>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(ui) => format!("SJust {}", show_unit_interval(ui)),
    }
}

/// Render a `StrictMaybe Prices` PParamsUpdate field. Upstream `Prices` is
/// stock-derived record Show: `Prices {prMem = <num> % <den>, prSteps =
/// <num> % <den>}`. yggdrasil splits Prices into two `price_mem` and
/// `price_step` Option<UnitInterval> fields; both must be set together to
/// form a valid Prices value. Empty pair renders as `SNothing`, both Some
/// renders as `SJust (Prices {...})`. The mixed-Some/None case is caught
/// earlier by the field-rejection list.
fn show_pparam_prices(
    price_mem: Option<yggdrasil_ledger::UnitInterval>,
    price_step: Option<yggdrasil_ledger::UnitInterval>,
) -> String {
    match (price_mem, price_step) {
        (Some(mem), Some(step)) => format!(
            "SJust (Prices {{prMem = {}, prSteps = {}}})",
            // Strip the always-wrapping parens from show_unit_interval —
            // record fields show at p=0 so the inner ratio renders bare.
            strip_outer_parens(&show_unit_interval(mem)),
            strip_outer_parens(&show_unit_interval(step)),
        ),
        _ => "SNothing".to_string(),
    }
}

/// Render a `StrictMaybe OrdExUnits` PParamsUpdate field. `OrdExUnits` is
/// `newtype OrdExUnits = OrdExUnits ExUnits deriving newtype Show`, so its
/// Show delegates to `Show ExUnits` (record-shaped). Inside `SJust` at p=11
/// the ExUnits record wraps with parens: `SJust (ExUnits {exUnitsMem =
/// <m>, exUnitsSteps = <s>})`.
fn show_pparam_ex_units(value: Option<&yggdrasil_ledger::eras::alonzo::ExUnits>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(eu) => format!("SJust ({})", show_alonzo_ex_units(eu)),
    }
}

/// Strip a single layer of outer parens from a string. Used when a helper
/// renders a value wrapped in parens for constructor-argument safety but
/// a caller needs the bare form (e.g. inside a record field at p=0).
fn strip_outer_parens(s: &str) -> &str {
    if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Render `StrictMaybe CostModels` matching upstream stock-derived
/// `Show CostModels`: 2-field record `CostModels {_costModelsValid =
/// fromList [(<Language>, CostModel <Language> [<cost-array>]),...],
/// _costModelsUnknown = fromList [(<tag>, [<costs>]),...]}`. Yggdrasil's
/// `CostModels = BTreeMap<u8, Vec<i64>>` flattens valid + unknown; this
/// helper splits by language tag (0=PlutusV1, 1=PlutusV2, 2=PlutusV3)
/// into the upstream two-map shape, with `CostModel <Language>
/// <Int64-list>` per known entry (upstream `Show CostModel` is a custom
/// `"CostModel " <> show lang <> " " <> show cm` formatter).
fn show_pparam_cost_models(value: Option<&std::collections::BTreeMap<u8, Vec<i64>>>) -> String {
    let Some(models) = value else {
        return "SNothing".to_string();
    };
    let mut valid_entries: Vec<String> = Vec::new();
    let mut unknown_entries: Vec<String> = Vec::new();
    for (tag, costs) in models {
        let cost_list = format!(
            "[{}]",
            costs
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        match *tag {
            0 => valid_entries.push(format!("(PlutusV1,CostModel PlutusV1 {cost_list})")),
            1 => valid_entries.push(format!("(PlutusV2,CostModel PlutusV2 {cost_list})")),
            2 => valid_entries.push(format!("(PlutusV3,CostModel PlutusV3 {cost_list})")),
            other => unknown_entries.push(format!("({other},{cost_list})")),
        }
    }
    format!(
        "SJust (CostModels {{_costModelsValid = fromList [{}], _costModelsUnknown = fromList [{}]}})",
        valid_entries.join(","),
        unknown_entries.join(",")
    )
}
/// 5-field record. Each `UnitInterval` field renders bare (no outer
/// parens) inside the record at p=0. Inside `SJust` at p=11 the record
/// wraps with parens.
fn show_pparam_pool_voting_thresholds(
    value: Option<&yggdrasil_ledger::PoolVotingThresholds>,
) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(t) => format!(
            "SJust (PoolVotingThresholds {{pvtMotionNoConfidence = {}, pvtCommitteeNormal = {}, pvtCommitteeNoConfidence = {}, pvtHardForkInitiation = {}, pvtPPSecurityGroup = {}}})",
            strip_outer_parens(&show_unit_interval(t.motion_no_confidence)),
            strip_outer_parens(&show_unit_interval(t.committee_normal)),
            strip_outer_parens(&show_unit_interval(t.committee_no_confidence)),
            strip_outer_parens(&show_unit_interval(t.hard_fork_initiation)),
            strip_outer_parens(&show_unit_interval(t.pp_security_group)),
        ),
    }
}

/// Render `StrictMaybe DRepVotingThresholds`. Upstream is a stock-derived
/// 10-field record. Field order: motionNoConfidence, committeeNormal,
/// committeeNoConfidence, updateToConstitution, hardForkInitiation,
/// ppNetworkGroup, ppEconomicGroup, ppTechnicalGroup, ppGovGroup,
/// treasuryWithdrawal.
fn show_pparam_drep_voting_thresholds(
    value: Option<&yggdrasil_ledger::DRepVotingThresholds>,
) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(t) => format!(
            "SJust (DRepVotingThresholds {{dvtMotionNoConfidence = {}, dvtCommitteeNormal = {}, dvtCommitteeNoConfidence = {}, dvtUpdateToConstitution = {}, dvtHardForkInitiation = {}, dvtPPNetworkGroup = {}, dvtPPEconomicGroup = {}, dvtPPTechnicalGroup = {}, dvtPPGovGroup = {}, dvtTreasuryWithdrawal = {}}})",
            strip_outer_parens(&show_unit_interval(t.motion_no_confidence)),
            strip_outer_parens(&show_unit_interval(t.committee_normal)),
            strip_outer_parens(&show_unit_interval(t.committee_no_confidence)),
            strip_outer_parens(&show_unit_interval(t.update_to_constitution)),
            strip_outer_parens(&show_unit_interval(t.hard_fork_initiation)),
            strip_outer_parens(&show_unit_interval(t.pp_network_group)),
            strip_outer_parens(&show_unit_interval(t.pp_economic_group)),
            strip_outer_parens(&show_unit_interval(t.pp_technical_group)),
            strip_outer_parens(&show_unit_interval(t.pp_gov_group)),
            strip_outer_parens(&show_unit_interval(t.treasury_withdrawal)),
        ),
    }
}

/// Render a Coin-family `StrictMaybe (CompactForm Coin)` PParamsUpdate
/// field at showsPrec 0. CoinPerByte fields share this shape because
/// `CoinPerByte` newtype-delegates Show to `CompactForm Coin`. Empty
/// renders as `SNothing`; set renders as `SJust (CompactCoin
/// {unCompactCoin = <n>})` (the inner `CompactCoin` record is wrapped
/// in parens because `SJust` at showsPrec 0 wraps its arg at p=11).
fn show_pparam_compact_coin<N: Into<u64> + Copy>(value: Option<N>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(n) => format!("SJust (CompactCoin {{unCompactCoin = {}}})", n.into()),
    }
}

/// Render a plain numeric `StrictMaybe Word{16,32,64}` PParamsUpdate
/// field at showsPrec 0. Empty renders as `SNothing`; set renders as
/// `SJust <n>` because primitive Words have stock numeric Show without
/// constructor wrapping at p=0.
fn show_pparam_word<N: std::fmt::Display + Copy>(value: Option<N>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(n) => format!("SJust {n}"),
    }
}

/// Render upstream `Show UnitInterval`: `<num> % <den>` at p=0 (no parens
/// when shown at top-level inside a record field), but wrapped with parens
/// at showsPrec > 7 (constructor argument position).
///
/// `UnitInterval` is `newtype UnitInterval (BoundedRatio UnitInterval
/// Word64)` with `deriving newtype Show`, and `BoundedRatio b a = BoundedRatio
/// (Ratio a)` likewise. Both newtypes delegate to `Show (Ratio Word64)`
/// which produces `<num> % <den>`, wrapping with parens at ratioPrec > 7.
///
/// We always wrap so it can be used safely in constructor-argument
/// position. Callers who need an unwrapped form (e.g. inside a record
/// field) should strip the outer parens.
fn show_unit_interval(ui: yggdrasil_ledger::UnitInterval) -> String {
    format!("({} % {})", ui.numerator, ui.denominator)
}

/// Render `StrictMaybe (GovPurposeId p)`. `GovPurposeId` is a phantom-tagged
/// newtype over `GovActionId` with `deriving newtype Show`, so its Show
/// delegates directly to `GovActionId`'s record Show.
fn show_strict_maybe_gov_purpose_id(value: Option<&yggdrasil_ledger::GovActionId>) -> String {
    match value {
        None => "SNothing".to_string(),
        Some(id) => format!("(SJust {})", show_conway_gov_action_id(id)),
    }
}

fn ensure_absent<T>(value: Option<&T>, era: &str, field: &str) -> Result<(), Error> {
    if value.is_some() {
        return Err(lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer does not yet support non-empty {field}"
        )));
    }
    Ok(())
}

fn ensure_empty_or_absent<T>(value: Option<&[T]>, era: &str, field: &str) -> Result<(), Error> {
    if value.is_some_and(|items| !items.is_empty()) {
        return Err(lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer does not yet support non-empty {field}"
        )));
    }
    Ok(())
}

fn ensure_empty_mint(
    value: Option<&yggdrasil_ledger::MintAsset>,
    era: &str,
    field: &str,
) -> Result<(), Error> {
    if value.is_some_and(|items| !items.is_empty()) {
        return Err(lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer does not yet support non-empty {field}"
        )));
    }
    Ok(())
}

fn show_strict_maybe_slot(slot: Option<u64>) -> String {
    match slot {
        Some(slot) => format!("SJust (SlotNo {slot})"),
        None => "SNothing".to_string(),
    }
}

fn show_haskell_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

fn show_tx_in_list(inputs: &[ShelleyTxIn]) -> String {
    let mut sorted = inputs.to_vec();
    sorted.sort();
    sorted.iter().map(show_tx_in).collect::<Vec<_>>().join(",")
}

fn show_tx_in(input: &ShelleyTxIn) -> String {
    format!(
        "TxIn (TxId {{unTxId = SafeHash \"{}\"}}) (TxIx {{unTxIx = {}}})",
        hex::encode(input.transaction_id),
        input.index
    )
}

fn show_shelley_tx_out_list(outputs: &[ShelleyTxOut], era: &str) -> Result<String, Error> {
    outputs
        .iter()
        .map(|output| show_shelley_tx_out(output, era))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.join(","))
}

fn show_shelley_tx_out(output: &ShelleyTxOut, era: &str) -> Result<String, Error> {
    let address = Address::from_bytes(&output.address).ok_or_else(|| {
        lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer received invalid address bytes"
        ))
    })?;
    Ok(format!(
        "({},Coin {})",
        show_shelley_address(&address, era)?,
        output.amount
    ))
}

fn show_mary_tx_out_list(outputs: &[MaryTxOut]) -> Result<String, Error> {
    outputs
        .iter()
        .map(show_mary_tx_out)
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.join(","))
}

fn show_mary_tx_out(output: &MaryTxOut) -> Result<String, Error> {
    let address = Address::from_bytes(&output.address).ok_or_else(|| {
        lift_tx_gen_error("DumpToFile: Mary Show(Tx) renderer received invalid address bytes")
    })?;
    Ok(format!(
        "({},{})",
        show_shelley_address(&address, "Mary")?,
        show_mary_value(&output.amount)?
    ))
}

fn show_alonzo_tx_out_list(outputs: &[AlonzoTxOut]) -> Result<String, Error> {
    outputs
        .iter()
        .map(show_alonzo_tx_out)
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.join(","))
}

fn show_alonzo_tx_out(output: &AlonzoTxOut) -> Result<String, Error> {
    let address = Address::from_bytes(&output.address).ok_or_else(|| {
        lift_tx_gen_error("DumpToFile: Alonzo Show(Tx) renderer received invalid address bytes")
    })?;
    let datum_hash = output
        .datum_hash
        .as_ref()
        .map(|hash| format!("SJust (SafeHash \"{}\")", hex::encode(hash)))
        .unwrap_or_else(|| "SNothing".to_string());
    Ok(format!(
        "({},{},{datum_hash})",
        show_shelley_address(&address, "Alonzo")?,
        show_mary_value(&output.amount)?
    ))
}

fn show_babbage_tx_out_list(outputs: &[BabbageTxOut]) -> Result<String, Error> {
    outputs
        .iter()
        .map(show_babbage_tx_out)
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.join(","))
}

fn show_babbage_tx_out(output: &BabbageTxOut) -> Result<String, Error> {
    let address = Address::from_bytes(&output.address).ok_or_else(|| {
        lift_tx_gen_error("DumpToFile: Babbage Show(Tx) renderer received invalid address bytes")
    })?;
    let datum = show_babbage_datum(output.datum_option.as_ref())?;
    let script_ref = show_babbage_script_ref(output.script_ref.as_ref())?;
    let value = show_mary_value(&output.amount)?;
    let size = output.to_cbor_bytes().len();
    Ok(format!(
        "Sized {{sizedValue = ({addr},{value},{datum},{script_ref}), sizedSize = {size}}}",
        addr = show_shelley_address(&address, "Babbage")?,
    ))
}

fn show_babbage_datum(datum: Option<&DatumOption>) -> Result<String, Error> {
    match datum {
        None => Ok("NoDatum".to_string()),
        Some(DatumOption::Hash(hash)) => {
            Ok(format!("DatumHash (SafeHash \"{}\")", hex::encode(hash)))
        }
        Some(DatumOption::Inline(pd)) => {
            // Upstream Show for `Datum era`:
            //   `Datum (BinaryData "<latin1-escaped-bytes>")`
            //
            // `Datum era` is stock-derived Show on
            // `data Datum era = NoDatum | DatumHash DataHash | Datum (BinaryData era)`.
            // The single-arg `Datum` constructor wraps the `BinaryData` at
            // showsPrec 11; `BinaryData era = BinaryData ShortByteString
            // deriving newtype Show` so the value renders as `(BinaryData
            // "<bytestring-show>")`. The underlying SBS is the canonical
            // CBOR of the plutus data (yggdrasil PlutusData::to_cbor_bytes()).
            let bs_show = show_haskell_bytestring(&pd.to_cbor_bytes());
            Ok(format!("Datum (BinaryData {bs_show})"))
        }
    }
}

fn show_babbage_script_ref(script_ref: Option<&ScriptRef>) -> Result<String, Error> {
    match script_ref {
        None => Ok("SNothing".to_string()),
        Some(sr) => {
            // Upstream `Show (AlonzoScript era)` (Cardano.Ledger.Alonzo.Scripts):
            //     show (NativeScript x)        = "NativeScript " ++ show x
            //     show s@(PlutusScript plutus) = "PlutusScript " ++ show language
            //                                  ++ " " ++ show (hashScript @era s)
            //
            // `Show ScriptHash` at p=0 (default showsPrec ignores precedence for
            // instances defining only `show`) emits `ScriptHash "<hex>"`. Wrapped
            // in `SJust` at p=0 (tuple position), upstream produces
            // `SJust PlutusScript PlutusV{N} ScriptHash "<hex>"` without parens
            // — the AlonzoScript Show defines only `show`, so showsPrec 11
            // delegates to `show x` without adding parens.
            //
            // Script hash domain: Blake2b-224 over `prefix-tag ++ script_bytes`.
            // PlutusV1 = 0x01, PlutusV2 = 0x02, PlutusV3 = 0x03.
            let inner = match &sr.0 {
                yggdrasil_ledger::Script::PlutusV1(bytes) => format!(
                    "PlutusScript PlutusV1 ScriptHash \"{}\"",
                    hex::encode(plutus_script_hash(1, bytes))
                ),
                yggdrasil_ledger::Script::PlutusV2(bytes) => format!(
                    "PlutusScript PlutusV2 ScriptHash \"{}\"",
                    hex::encode(plutus_script_hash(2, bytes))
                ),
                yggdrasil_ledger::Script::PlutusV3(bytes) => format!(
                    "PlutusScript PlutusV3 ScriptHash \"{}\"",
                    hex::encode(plutus_script_hash(3, bytes))
                ),
                yggdrasil_ledger::Script::Native(ns) => {
                    format!("NativeScript {}", show_native_script(ns))
                }
            };
            Ok(format!("SJust {inner}"))
        }
    }
}

/// Compute the Plutus script hash for a given language tag and script bytes.
///
/// Matches upstream `hashScript` for Plutus scripts: `Blake2b_224 (lang_tag
/// ++ script_bytes)`. The language tag is 0x01 for PlutusV1, 0x02 for
/// PlutusV2, 0x03 for PlutusV3.
fn plutus_script_hash(language_tag: u8, script_bytes: &[u8]) -> [u8; 28] {
    let mut buf = Vec::with_capacity(1 + script_bytes.len());
    buf.push(language_tag);
    buf.extend_from_slice(script_bytes);
    yggdrasil_crypto::hash_bytes_224(&buf).0
}

/// Render upstream `Show (Timelock era)` for a yggdrasil native script.
///
/// The Timelock newtype wraps `MemoBytes (TimelockRaw era)`; upstream
/// stock-derived Show emits `MkTimelock <raw-show> (blake2b_256: SafeHash
/// "<hex>")` where the outer hash is the BLAKE2b-256 of the canonical
/// CBOR encoding of the raw timelock. Yggdrasil's
/// `NativeScript::encode_cbor` produces the identical CBOR shape, so
/// hashing its byte output is byte-equivalent to upstream's MemoBytes
/// hash.
fn show_native_script(script: &yggdrasil_ledger::NativeScript) -> String {
    let raw = show_timelock_raw(script);
    let cbor = script.to_cbor_bytes();
    let hash = hex::encode(hash_bytes_256(&cbor).0);
    format!("MkTimelock {raw} (blake2b_256: SafeHash \"{hash}\")")
}

/// Render the inner `TimelockRaw` constructor: matches upstream
/// stock-derived `Show (TimelockRaw era)` over the 6-variant ADT:
///   TimelockSignature (KeyHash {unKeyHash = "..."})
///   TimelockAllOf (StrictSeq {fromStrict = fromList [<Timelock>,...]})
///   TimelockAnyOf (StrictSeq {fromStrict = fromList [<Timelock>,...]})
///   TimelockMOf <n> (StrictSeq {fromStrict = fromList [<Timelock>,...]})
///   TimelockTimeStart (SlotNo <n>)
///   TimelockTimeExpire (SlotNo <n>)
fn show_timelock_raw(script: &yggdrasil_ledger::NativeScript) -> String {
    use yggdrasil_ledger::NativeScript;
    match script {
        NativeScript::ScriptPubkey(kh) => format!(
            "TimelockSignature (KeyHash {{unKeyHash = \"{}\"}})",
            hex::encode(kh)
        ),
        NativeScript::ScriptAll(scripts) => format!(
            "TimelockAllOf (StrictSeq {{fromStrict = fromList [{}]}})",
            scripts
                .iter()
                .map(show_native_script)
                .collect::<Vec<_>>()
                .join(",")
        ),
        NativeScript::ScriptAny(scripts) => format!(
            "TimelockAnyOf (StrictSeq {{fromStrict = fromList [{}]}})",
            scripts
                .iter()
                .map(show_native_script)
                .collect::<Vec<_>>()
                .join(",")
        ),
        NativeScript::ScriptNOfK(n, scripts) => format!(
            "TimelockMOf {n} (StrictSeq {{fromStrict = fromList [{}]}})",
            scripts
                .iter()
                .map(show_native_script)
                .collect::<Vec<_>>()
                .join(",")
        ),
        NativeScript::InvalidBefore(slot) => format!("TimelockTimeStart (SlotNo {slot})"),
        NativeScript::InvalidHereafter(slot) => format!("TimelockTimeExpire (SlotNo {slot})"),
    }
}

fn show_mary_value(value: &yggdrasil_ledger::Value) -> Result<String, Error> {
    let (coin, assets) = match value {
        yggdrasil_ledger::Value::Coin(coin) => (*coin, None),
        yggdrasil_ledger::Value::CoinAndAssets(coin, assets) => (*coin, Some(assets)),
    };
    let multi_asset = match assets {
        None => "fromList []".to_string(),
        Some(map) if map.is_empty() => "fromList []".to_string(),
        Some(map) => show_multi_asset_entries(map),
    };
    Ok(format!(
        "MaryValue (Coin {coin}) (MultiAsset ({multi_asset}))"
    ))
}

/// Render the inner `fromList [...]` body of a non-empty `MultiAsset` map,
/// mirroring upstream `Show (Map PolicyID (Map AssetName Integer))`:
/// `fromList [(PolicyID {policyID = ScriptHash "<hex>"},fromList [("<asset-hex>",<qty>),...]),...]`.
///
/// Entry order follows `BTreeMap` byte-lex ordering, which matches upstream
/// `Data.Map` `toAscList` ordering on `PolicyID` (Ord via `ScriptHash` ->
/// `Hash.Hash` byte order) and `AssetName` (Ord via `ShortByteString` byte
/// order).
fn show_multi_asset_entries(ma: &yggdrasil_ledger::eras::mary::MultiAsset) -> String {
    let policies: Vec<String> = ma
        .iter()
        .map(|(policy, assets)| {
            let inner: Vec<String> = assets
                .iter()
                .map(|(asset, qty)| format!("(\"{}\",{qty})", hex::encode(asset)))
                .collect();
            format!(
                "(PolicyID {{policyID = ScriptHash \"{}\"}},fromList [{}])",
                hex::encode(policy),
                inner.join(",")
            )
        })
        .collect();
    format!("fromList [{}]", policies.join(","))
}

fn show_shelley_address(address: &Address, era: &str) -> Result<String, Error> {
    match address {
        Address::Enterprise(enterprise) => Ok(format!(
            "Addr {} {} StakeRefNull",
            show_network(enterprise.network)?,
            show_payment_credential(&enterprise.payment)
        )),
        _ => Err(lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer does not yet support address shape {address:?}"
        ))),
    }
}

fn show_network(network: u8) -> Result<&'static str, Error> {
    match network {
        0 => Ok("Testnet"),
        1 => Ok("Mainnet"),
        other => Err(lift_tx_gen_error(format!(
            "DumpToFile: unsupported Shelley network id {other}"
        ))),
    }
}

/// Render a tx-body `StrictMaybe Network` field (`atbrTxNetworkId`
/// / `btbrNetworkId` / `ctbrNetworkId`). `Network` has a
/// stock-derived nullary `Show` (`Testnet` / `Mainnet`), so a set
/// value renders `SJust Testnet` with no inner parens.
fn show_strict_maybe_network(network_id: Option<u8>) -> Result<String, Error> {
    match network_id {
        None => Ok("SNothing".to_string()),
        Some(n) => Ok(format!("SJust {}", show_network(n)?)),
    }
}

/// Render a tx-body `Withdrawals` field — upstream `newtype
/// Withdrawals = Withdrawals { unWithdrawals :: Map AccountAddress
/// Coin }` with stock-derived record `Show`. Entries are sorted by
/// `AccountAddress` (the `BTreeMap` key order matches upstream
/// `Data.Map` Show).
fn show_withdrawals(
    withdrawals: Option<&BTreeMap<yggdrasil_ledger::RewardAccount, u64>>,
) -> Result<String, Error> {
    let body = match withdrawals {
        None => String::new(),
        Some(map) => map
            .iter()
            .map(|(account, coin)| {
                let addr = show_account_address_from_record(account)?;
                Ok(format!("({addr},{})", show_coin(*coin)))
            })
            .collect::<Result<Vec<_>, Error>>()?
            .join(","),
    };
    Ok(format!("Withdrawals {{unWithdrawals = fromList [{body}]}}"))
}

/// Render a tx-body required-signer-hash set
/// (`atbrReqSignerHashes` / `btbrReqSignerHashes` /
/// `ctbrReqSignerHashes`) — upstream `Set (KeyHash 'Witness)`,
/// each `KeyHash` rendered `KeyHash {unKeyHash = "..."}`. Hashes
/// are sorted to match upstream `Data.Set` Show ordering.
fn show_req_signer_hashes(hashes: Option<&[[u8; 28]]>) -> String {
    let mut sorted: Vec<&[u8; 28]> = hashes.unwrap_or_default().iter().collect();
    sorted.sort();
    let body = sorted
        .iter()
        .map(|h| format!("KeyHash {{unKeyHash = \"{}\"}}", hex::encode(h)))
        .collect::<Vec<_>>()
        .join(",");
    format!("fromList [{body}]")
}

/// Render a tx-body `StrictMaybe TxAuxDataHash` field
/// (`stbrAuxDataHash` / `atbrAuxDataHash` / `btbrAuxDataHash` /
/// `ctbrAuxDataHash`). `TxAuxDataHash` has a stock-derived record
/// `Show` over a `SafeHash`, so a set value renders `SJust
/// (TxAuxDataHash {unTxAuxDataHash = SafeHash "..."})`.
fn show_strict_maybe_aux_data_hash(hash: Option<[u8; 32]>) -> String {
    match hash {
        None => "SNothing".to_string(),
        Some(h) => format!(
            "SJust (TxAuxDataHash {{unTxAuxDataHash = SafeHash \"{}\"}})",
            hex::encode(h)
        ),
    }
}

/// Render a tx-body `StrictMaybe ScriptIntegrityHash` field
/// (`atbrScriptIntegrityHash` / `btbrScriptIntegrityHash` /
/// `ctbrScriptIntegrityHash`). Upstream `type ScriptIntegrityHash
/// = SafeHash EraIndependentScriptIntegrity` is a bare type alias,
/// so a set value renders `SJust (SafeHash "...")`.
fn show_strict_maybe_script_integrity_hash(hash: Option<[u8; 32]>) -> String {
    match hash {
        None => "SNothing".to_string(),
        Some(h) => format!("SJust (SafeHash \"{}\")", hex::encode(h)),
    }
}

fn show_payment_credential(credential: &StakeCredential) -> String {
    match credential {
        StakeCredential::AddrKeyHash(hash) => {
            format!(
                "(KeyHashObj (KeyHash {{unKeyHash = \"{}\"}}))",
                hex::encode(hash)
            )
        }
        StakeCredential::ScriptHash(hash) => {
            format!("(ScriptHashObj (ScriptHash \"{}\"))", hex::encode(hash))
        }
    }
}

fn show_shelley_witness_set(witness_set: &ShelleyWitnessSet, era: &str) -> Result<String, Error> {
    if !witness_set.native_scripts.is_empty()
        || !witness_set.bootstrap_witnesses.is_empty()
        || !witness_set.plutus_v1_scripts.is_empty()
        || !witness_set.plutus_data.is_empty()
        || !witness_set.redeemers.is_empty()
        || !witness_set.plutus_v2_scripts.is_empty()
        || !witness_set.plutus_v3_scripts.is_empty()
    {
        return Err(lift_tx_gen_error(format!(
            "DumpToFile: {era} Show(Tx) renderer does not yet support non-vkey witnesses"
        )));
    }

    let mut witnesses = witness_set.vkey_witnesses.clone();
    witnesses.sort_by_key(|witness| witness.vkey);
    let vkeys = witnesses
        .iter()
        .map(show_vkey_witness)
        .collect::<Vec<_>>()
        .join(",");
    let witness_hash = hex::encode(hash_bytes_256(&witness_set.to_cbor_bytes()).0);
    Ok(format!(
        "ShelleyTxWitsRaw {{stwrAddrTxWits = fromList [{vkeys}], stwrScriptTxWits = fromList [], stwrBootAddrTxWits = fromList []}} (blake2b_256: SafeHash \"{witness_hash}\")"
    ))
}

fn show_alonzo_witness_set(witness_set: &ShelleyWitnessSet) -> Result<String, Error> {
    let mut witnesses = witness_set.vkey_witnesses.clone();
    witnesses.sort_by_key(|witness| witness.vkey);
    let vkeys = witnesses
        .iter()
        .map(show_vkey_witness)
        .collect::<Vec<_>>()
        .join(",");
    let boot = show_alonzo_bootstrap_witnesses(&witness_set.bootstrap_witnesses);
    let scripts = show_alonzo_script_witnesses(witness_set);
    let dats = show_alonzo_tx_dats(&witness_set.plutus_data);
    let rdmrs = show_alonzo_redeemers(&witness_set.redeemers)?;
    let witness_hash = hex::encode(hash_bytes_256(&witness_set.to_cbor_bytes()).0);
    Ok(format!(
        "AlonzoTxWitsRaw {{atwrAddrTxWits = fromList [{vkeys}], atwrBootAddrTxWits = {boot}, atwrScriptTxWits = {scripts}, atwrDatsTxWits = {dats}, atwrRdmrsTxWits = {rdmrs}}} (blake2b_256: SafeHash \"{witness_hash}\")"
    ))
}

/// Render `atwrBootAddrTxWits = fromList [...]` matching upstream
/// `Show (Set BootstrapWitness)`.
///
/// Upstream `Ord BootstrapWitness = comparing bootstrapWitKeyHash`
/// where `bootstrapWitKeyHash` is `Blake2b-224 (SHA3-256 (<Byron
/// AddressInfo prefix> ++ key ++ chain_code ++ attributes))`. Yggdrasil
/// now implements this via `bootstrap_witness_key_hash`, so the sort
/// order is byte-equivalent to upstream for any number of witnesses.
fn show_alonzo_bootstrap_witnesses(witnesses: &[yggdrasil_ledger::BootstrapWitness]) -> String {
    let mut sorted: Vec<&yggdrasil_ledger::BootstrapWitness> = witnesses.iter().collect();
    sorted.sort_by_key(|bw| {
        bootstrap_witness_key_hash(&bw.public_key, &bw.chain_code, &bw.attributes)
    });
    let body = sorted
        .iter()
        .map(|bw| show_bootstrap_witness(bw))
        .collect::<Vec<_>>()
        .join(",");
    format!("fromList [{body}]")
}

/// Compute the upstream `bootstrapWitKeyHash`:
///
/// `Blake2b-224 (SHA3-256 (prefix ++ key32 ++ chain_code32 ++ attributes))`
///
/// where prefix is the constant 6-byte Byron `AddressInfo` header
/// `[0x83, 0x00, 0x82, 0x00, 0x58, 0x40]` (CBOR-shaped list-of-3-token,
/// addrType=0, list-of-2-token, type=0, bytestring-len-64-token —
/// matching `Cardano.Ledger.Keys.Bootstrap.bootstrapWitKeyHash`).
fn bootstrap_witness_key_hash(
    public_key: &[u8; 32],
    chain_code: &[u8; 32],
    attributes: &[u8],
) -> [u8; 28] {
    const PREFIX: [u8; 6] = [0x83, 0x00, 0x82, 0x00, 0x58, 0x40];
    let mut buf = Vec::with_capacity(PREFIX.len() + 32 + 32 + attributes.len());
    buf.extend_from_slice(&PREFIX);
    buf.extend_from_slice(public_key);
    buf.extend_from_slice(chain_code);
    buf.extend_from_slice(attributes);
    let sha3 = yggdrasil_crypto::sha3_256(&buf).0;
    yggdrasil_crypto::hash_bytes_224(&sha3).0
}

/// Render a single `BootstrapWitness` matching upstream stock-derived
/// `Show BootstrapWitness`: record with four fields wrapping VKey,
/// SignedDSIGN, ChainCode (`Quiet`-shown), and the raw attribute
/// ByteArray.
fn show_bootstrap_witness(bw: &yggdrasil_ledger::BootstrapWitness) -> String {
    format!(
        "BootstrapWitness {{bwKey = VKey (VerKeyEd25519DSIGN \"{}\"), bwSignature = SignedDSIGN (SigEd25519DSIGN \"{}\"), bwChainCode = ChainCode {}, bwAttributes = {}}}",
        hex::encode(bw.public_key),
        hex::encode(bw.signature),
        show_haskell_bytestring(&bw.chain_code),
        show_haskell_bytestring(&bw.attributes),
    )
}

/// Render `atwrScriptTxWits = fromList [...]` matching upstream
/// `Show (Map ScriptHash (AlonzoScript era))`.
///
/// Each script in the witness set becomes one entry keyed by its
/// `ScriptHash`:
/// - Plutus scripts: Blake2b-224 over `language-tag ++ script_bytes`
///   (tags 0x01/0x02/0x03 for V1/V2/V3), value rendered as `PlutusScript
///   PlutusV{N} ScriptHash "<hex>"`.
/// - Native scripts: Blake2b-224 over `0x00 ++ canonical CBOR`, value
///   rendered as `NativeScript MkTimelock <raw> (blake2b_256: SafeHash
///   "<hex>")`.
///
/// Entries sort by script-hash byte-lex order matching upstream
/// `Data.Map toAscList`.
fn show_alonzo_script_witnesses(witness_set: &ShelleyWitnessSet) -> String {
    enum Entry<'a> {
        Plutus(&'a str),
        Native(&'a yggdrasil_ledger::NativeScript),
    }
    let mut entries: Vec<([u8; 28], Entry<'_>)> = Vec::new();
    for bytes in &witness_set.plutus_v1_scripts {
        entries.push((plutus_script_hash(1, bytes), Entry::Plutus("PlutusV1")));
    }
    for bytes in &witness_set.plutus_v2_scripts {
        entries.push((plutus_script_hash(2, bytes), Entry::Plutus("PlutusV2")));
    }
    for bytes in &witness_set.plutus_v3_scripts {
        entries.push((plutus_script_hash(3, bytes), Entry::Plutus("PlutusV3")));
    }
    for ns in &witness_set.native_scripts {
        entries.push((yggdrasil_ledger::native_script_hash(ns), Entry::Native(ns)));
    }
    entries.sort_by_key(|(hash, _)| *hash);
    let body = entries
        .iter()
        .map(|(hash, entry)| {
            let hex_hash = hex::encode(hash);
            match entry {
                Entry::Plutus(lang) => format!(
                    "(ScriptHash \"{hex_hash}\",PlutusScript {lang} ScriptHash \"{hex_hash}\")"
                ),
                Entry::Native(ns) => format!(
                    "(ScriptHash \"{hex_hash}\",NativeScript {})",
                    show_native_script(ns)
                ),
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("fromList [{body}]")
}

/// Render `MkTxDats (TxDatsRaw {unTxDatsRaw = fromList [...]} (blake2b_256:
/// SafeHash "<hex>"))` matching upstream stock-derived Show through the
/// `MemoBytes` wrapper.
///
/// Entry order follows the sorted DataHash key (upstream `Map DataHash`
/// uses byte-lex ordering on `SafeHash` Hash bytes). The MemoBytes raw
/// CBOR is `tag 258` + variable-length array of each datum's CBOR, mirroring
/// upstream `encodeWithSetTag . Map.elems . unTxDatsRaw`.
fn show_alonzo_tx_dats(plutus_data: &[PlutusData]) -> String {
    let mut entries: Vec<(Hash256, &PlutusData)> = plutus_data
        .iter()
        .map(|pd| (hash_bytes_256(&pd.to_cbor_bytes()).0, pd))
        .collect();
    entries.sort_by_key(|a| a.0);

    let raw_bytes = alonzo_tx_dats_raw_cbor(&entries);
    let outer_hash = hex::encode(hash_bytes_256(&raw_bytes).0);

    let body = entries
        .iter()
        .map(|(hash, pd)| {
            let datum_hash_hex = hex::encode(hash);
            let pd_hash_hex = hex::encode(hash_bytes_256(&pd.to_cbor_bytes()).0);
            format!(
                "(SafeHash \"{datum_hash_hex}\",MkData {} (blake2b_256: SafeHash \"{pd_hash_hex}\"))",
                show_plutus_data(pd)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "MkTxDats (TxDatsRaw {{unTxDatsRaw = fromList [{body}]}} (blake2b_256: SafeHash \"{outer_hash}\"))"
    )
}

type Hash256 = [u8; 32];

/// CBOR encoding of `TxDatsRaw` — `tag 258` (set marker) plus a variable-length
/// array of each datum's CBOR, mirroring upstream
/// `encodeWithSetTag . Map.elems . unTxDatsRaw`. Empty input encodes to
/// `[0xd9, 0x01, 0x02, 0x80]` which matches the existing empty-dats fixture.
fn alonzo_tx_dats_raw_cbor(entries: &[(Hash256, &PlutusData)]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.tag(258);
    enc.array(entries.len() as u64);
    for (_, pd) in entries {
        pd.encode_cbor(&mut enc);
    }
    enc.into_bytes()
}

/// Render `PlutusData` matching upstream `Show (PV1.Data)` — the stock-derived
/// Show for `data Data = Constr Integer [Data] | Map [(Data,Data)] | List [Data] | I Integer | B ByteString`.
///
/// Children are rendered at showsPrec 11 because they are constructor
/// arguments; integers wrap with parens only when negative (showsPrec 11 of a
/// non-negative integer is the bare digit sequence). Byte strings render via
/// upstream `Show ByteString`, which interprets bytes as Latin1 and applies
/// standard Haskell `Show String` escapes (printable ASCII inline,
/// non-printable as `\NNN` decimal escapes).
///
/// Used to render `Data era` payloads inside `TxDatsRaw` and `RedeemersRaw`
/// dump output. R572 ships the renderer; R573 will lift the boundary in
/// `show_alonzo_witness_set` for non-empty `plutus_data` and `redeemers`.
fn show_plutus_data(data: &PlutusData) -> String {
    show_plutus_data_prec(data, 0)
}

fn show_plutus_data_prec(data: &PlutusData, prec: u8) -> String {
    match data {
        PlutusData::Constr(alt, fields) => {
            let items = fields
                .iter()
                .map(show_plutus_data)
                .collect::<Vec<_>>()
                .join(",");
            paren_if(prec > 10, &format!("Constr {alt} [{items}]"))
        }
        PlutusData::Map(entries) => {
            let rendered = entries
                .iter()
                .map(|(k, v)| format!("({},{})", show_plutus_data(k), show_plutus_data(v)))
                .collect::<Vec<_>>()
                .join(",");
            paren_if(prec > 10, &format!("Map [{rendered}]"))
        }
        PlutusData::List(items) => {
            let rendered = items
                .iter()
                .map(show_plutus_data)
                .collect::<Vec<_>>()
                .join(",");
            paren_if(prec > 10, &format!("List [{rendered}]"))
        }
        PlutusData::Integer(n) => {
            // `I` is a single-arg constructor: stock-derived Show wraps the
            // whole `I <n>` with parens at prec > 10. The inner integer Show
            // at showsPrec 11 wraps negatives in parens itself (because `-`
            // is unary minus and parses tighter at p > 6); non-negatives are
            // shown bare.
            let inner = if n.sign() == num_bigint::Sign::Minus {
                format!("({n})")
            } else {
                format!("{n}")
            };
            paren_if(prec > 10, &format!("I {inner}"))
        }
        PlutusData::Bytes(bs) => paren_if(prec > 10, &format!("B {}", show_haskell_bytestring(bs))),
    }
}

fn paren_if(condition: bool, inner: &str) -> String {
    if condition {
        format!("({inner})")
    } else {
        inner.to_string()
    }
}

/// Render a Rust `&[u8]` as Haskell's default `Show ByteString` produces.
///
/// Mirrors GHC's derived `Show String` over `Data.ByteString.unpackChars`,
/// which is the path upstream `instance Show ByteString` uses.
///
/// Escape table (matching GHC's `showLitChar` + `showLitString`):
/// - `"` (0x22) → `\"` (always, via showLitString)
/// - `\` (0x5C) → `\\`
/// - 0x20-0x7E except above → inline
/// - 0x07-0x0D → short `\a` `\b` `\t` `\n` `\v` `\f` `\r` aliases
/// - 0x0E `\SO` → with `\&` protection before a following `H` so
///   `\SOH` (Start Of Heading) and `\SO`+`H` stay distinguishable
/// - 0x00-0x06, 0x0F-0x1F → multi-letter mnemonic
///   (`\NUL`/`\SOH`/.../`\US`)
/// - 0x7F → `\DEL`
/// - 0x80-0xFF → `\NNN` decimal escape with `\&` separator before any
///   following ASCII digit so the escape boundary is unambiguous
const HASKELL_ASCII_TAB: &[&str] = &[
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US",
];

fn show_haskell_bytestring(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for (idx, byte) in bytes.iter().enumerate() {
        let next = bytes.get(idx + 1).copied();
        match *byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            // GHC `showLitChar` short-form aliases for 0x07-0x0D
            // (BEL/BS/HT/LF/VT/FF/CR).
            0x07 => out.push_str("\\a"),
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0A => out.push_str("\\n"),
            0x0B => out.push_str("\\v"),
            0x0C => out.push_str("\\f"),
            0x0D => out.push_str("\\r"),
            // 0x0E SO — disambiguate from `\SOH` (Start Of Heading).
            0x0E => {
                out.push_str("\\SO");
                if next == Some(b'H') {
                    out.push_str("\\&");
                }
            }
            // Multi-letter mnemonic for the remaining 0x00-0x1F controls.
            0x00..=0x06 | 0x0F..=0x1F => {
                out.push('\\');
                out.push_str(HASKELL_ASCII_TAB[*byte as usize]);
            }
            // 0x7F DEL — always emitted as `\DEL`.
            0x7F => out.push_str("\\DEL"),
            // Printable ASCII (0x20-0x7E except `"` and `\\`).
            0x20..=0x7E => out.push(*byte as char),
            // 0x80-0xFF: decimal escape with `\&` separator before a
            // following ASCII digit so the escape boundary is unambiguous
            // (e.g. `\200\&5`, not `\2005`).
            0x80..=0xFF => {
                out.push_str(&format!("\\{byte}"));
                if next.is_some_and(|n| n.is_ascii_digit()) {
                    out.push_str("\\&");
                }
            }
        }
    }
    out.push('"');
    out
}

/// Render `MkRedeemers (RedeemersRaw {unRedeemersRaw = fromList [...]}
/// (blake2b_256: SafeHash "<hex>"))` matching upstream stock-derived Show
/// through the `MemoBytes` wrapper.
///
/// Entries are sorted by `(tag, index)` to match upstream `Map (PlutusPurpose
/// AsIx era) (Data era, ExUnits)` ordering. The MemoBytes raw CBOR is the
/// Alonzo-era array-of-`[tag,index,data,ex_units]` encoding emitted by
/// `Redeemer::encode_cbor`; that's what feeds the BLAKE2b-256 hash.
///
/// `tag` is mapped to the upstream `PlutusPurpose` constructor: 0 →
/// `AlonzoSpending`, 1 → `AlonzoMinting`, 2 → `AlonzoCertifying`, 3 →
/// `AlonzoRewarding`.
fn show_alonzo_redeemers(redeemers: &[Redeemer]) -> Result<String, Error> {
    let mut sorted = redeemers.to_vec();
    sorted.sort_by_key(|r| (r.tag, r.index));

    let raw_bytes = alonzo_redeemers_raw_cbor(&sorted);
    let outer_hash = hex::encode(hash_bytes_256(&raw_bytes).0);

    let entries: Result<Vec<String>, Error> = sorted
        .iter()
        .map(|r| {
            let purpose = show_alonzo_plutus_purpose(r.tag, r.index)?;
            let data = show_plutus_data(&r.data);
            let ex_units = show_alonzo_ex_units(&r.ex_units);
            Ok(format!("({purpose},({data},{ex_units}))"))
        })
        .collect();
    let body = entries?.join(",");

    Ok(format!(
        "MkRedeemers (RedeemersRaw {{unRedeemersRaw = fromList [{body}]}} (blake2b_256: SafeHash \"{outer_hash}\"))"
    ))
}

/// CBOR encoding of `RedeemersRaw` for the Alonzo era: a definite-length array
/// of `[tag, index, data, ex_units]` redeemer entries, matching
/// `Redeemer::encode_cbor`'s 4-element array shape. Empty input encodes to
/// `[0x80]` (definite array of length 0), matching the prior empty-redeemers
/// fixture hash.
fn alonzo_redeemers_raw_cbor(redeemers: &[Redeemer]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(redeemers.len() as u64);
    for r in redeemers {
        r.encode_cbor(&mut enc);
    }
    enc.into_bytes()
}

/// Render the `PlutusPurpose AsIx era` constructor for an Alonzo redeemer
/// tag/index pair, mirroring stock-derived
/// `Show (AlonzoPlutusPurpose AsIx era)` and the `AsIx` newtype Show.
fn show_alonzo_plutus_purpose(tag: u8, index: u64) -> Result<String, Error> {
    let constructor = match tag {
        0 => "AlonzoSpending",
        1 => "AlonzoMinting",
        2 => "AlonzoCertifying",
        3 => "AlonzoRewarding",
        other => {
            return Err(lift_tx_gen_error(format!(
                "DumpToFile: Alonzo Show(Tx) renderer received unknown redeemer tag {other}"
            )));
        }
    };
    Ok(format!("{constructor} (AsIx {{unAsIx = {index}}})"))
}

/// Render `ExUnits {exUnitsMem = <m>, exUnitsSteps = <s>}` matching upstream
/// stock-derived Show on the two-field record.
fn show_alonzo_ex_units(ex: &ExUnits) -> String {
    format!(
        "ExUnits {{exUnitsMem = {}, exUnitsSteps = {}}}",
        ex.mem, ex.steps
    )
}

fn show_vkey_witness(witness: &ShelleyVkeyWitness) -> String {
    let key_hash = hex::encode(hash_bytes_224(&witness.vkey).0);
    format!(
        "WitVKeyInternal {{wvkKey = VKey (VerKeyEd25519DSIGN \"{}\"), wvkSignature = SignedDSIGN (SigEd25519DSIGN \"{}\"), wvkKeyHash = KeyHash {{unKeyHash = \"{key_hash}\"}}}}",
        hex::encode(witness.vkey),
        hex::encode(witness.signature)
    )
}

fn eval_generator(
    env: &mut Env,
    era: AnyCardanoEra,
    generator: &Generator,
    tx_params: &TxGenTxParams,
    protocol_parameters: Option<&ProtocolParameters>,
    limit: Option<usize>,
) -> Result<Vec<GeneratedTx>, Error> {
    if matches!(limit, Some(0)) {
        return Ok(Vec::new());
    }

    match generator {
        Generator::Split(wallet_name, pay_mode, pay_mode_change, coins) => {
            let output = interpret_pay_mode(env, era, pay_mode)?;
            trace_debug(
                env,
                &format!("split output address : {}", output.address_hex),
            );
            let change = interpret_pay_mode(env, era, pay_mode_change)?;
            trace_debug(
                env,
                &format!("split change address : {}", change.address_hex),
            );
            let input_funds = take_wallet_funds(env, wallet_name, 1)?;
            let have = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
            let split =
                include_change(tx_params.tx_param_fee, coins, &have).map_err(lift_tx_gen_error)?;
            let destinations = split_destinations(
                &split,
                &change.destination_wallet,
                &output.destination_wallet,
            );
            let to_utxo_list = mangle_with_change(&change.to_utxo, &output.to_utxo, split)
                .map_err(lift_tx_gen_error)?;
            let generated = generate_and_store(
                env,
                TxGenerationPlan {
                    era,
                    collateral_funds: &[],
                    fee: tx_params.tx_param_fee,
                    metadata: None,
                    input_funds: &input_funds,
                    protocol_parameters,
                    to_utxo_list,
                    destinations,
                },
            )?;
            Ok(vec![generated])
        }
        Generator::SplitN(wallet_name, pay_mode, count) => {
            let output = interpret_pay_mode(env, era, pay_mode)?;
            trace_debug(
                env,
                &format!("SplitN output address : {}", output.address_hex),
            );
            let input_funds = take_wallet_funds(env, wallet_name, 1)?;
            let have = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
            let values = inputs_to_outputs_with_fee(tx_params.tx_param_fee, *count, &have)
                .map_err(lift_tx_gen_error)?;
            let to_utxo_list =
                mangle_repeat(&output.to_utxo, &values).map_err(lift_tx_gen_error)?;
            let destinations = std::iter::repeat_n(output.destination_wallet.clone(), values.len())
                .collect::<Vec<_>>();
            let generated = generate_and_store(
                env,
                TxGenerationPlan {
                    era,
                    collateral_funds: &[],
                    fee: tx_params.tx_param_fee,
                    metadata: None,
                    input_funds: &input_funds,
                    protocol_parameters,
                    to_utxo_list,
                    destinations,
                },
            )?;
            Ok(vec![generated])
        }
        Generator::NtoM(wallet_name, pay_mode, inputs, outputs, metadata_size, collateral) => {
            let collaterals = select_collateral_funds(env, era, collateral.as_deref())?;
            let output = interpret_pay_mode(env, era, pay_mode)?;
            trace_debug(
                env,
                &format!("NtoM output address : {}", output.address_hex),
            );
            let metadata = to_metadata(era, *metadata_size)?;
            preview_ntom_transaction(
                env,
                NtoMPreviewPlan {
                    era,
                    wallet_name,
                    inputs: *inputs,
                    outputs: *outputs,
                    fee: tx_params.tx_param_fee,
                    collateral_funds: &collaterals.funds,
                    metadata: metadata.as_ref(),
                    output: &output,
                    protocol_parameters,
                },
            )?;

            let input_funds = take_wallet_funds(env, wallet_name, *inputs)?;
            let have = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
            let values = inputs_to_outputs_with_fee(tx_params.tx_param_fee, *outputs, &have)
                .map_err(lift_tx_gen_error)?;
            let to_utxo_list =
                mangle_repeat(&output.to_utxo, &values).map_err(lift_tx_gen_error)?;
            let destinations = std::iter::repeat_n(output.destination_wallet.clone(), values.len())
                .collect::<Vec<_>>();
            let generated = generate_and_store(
                env,
                TxGenerationPlan {
                    era,
                    collateral_funds: &collaterals.funds,
                    fee: tx_params.tx_param_fee,
                    metadata: metadata.as_ref(),
                    input_funds: &input_funds,
                    protocol_parameters,
                    to_utxo_list,
                    destinations,
                },
            )?;
            Ok(vec![generated])
        }
        Generator::Sequence(generators) => {
            let mut generated = Vec::new();
            for generator in generators {
                let remaining = limit.map(|max| max.saturating_sub(generated.len()));
                if matches!(remaining, Some(0)) {
                    break;
                }
                generated.extend(eval_generator(
                    env,
                    era,
                    generator,
                    tx_params,
                    protocol_parameters,
                    remaining,
                )?);
            }
            Ok(generated)
        }
        Generator::Cycle(generator) => {
            let Some(limit) = limit else {
                return Err(lift_tx_gen_error(
                    "Cycle: finite submit modes require an enclosing Take",
                ));
            };
            let mut generated = Vec::new();
            while generated.len() < limit {
                let remaining = limit - generated.len();
                let batch = eval_generator(
                    env,
                    era,
                    generator,
                    tx_params,
                    protocol_parameters,
                    Some(remaining),
                )?;
                if batch.is_empty() {
                    return Err(lift_tx_gen_error(
                        "Cycle: inner generator produced no transactions",
                    ));
                }
                generated.extend(batch);
            }
            Ok(generated)
        }
        Generator::Take(count, generator) => {
            let effective_limit = limit.map_or(*count, |max| max.min(*count));
            eval_generator(
                env,
                era,
                generator,
                tx_params,
                protocol_parameters,
                Some(effective_limit),
            )
        }
        Generator::SecureGenesis(wallet_name, genesis_key_name, fund_key_name) => {
            let network_id = get_env_network_id(env)?.clone();
            let genesis = get_env_genesis(env)?.initial_funds.clone();
            let src_key = get_env_keys(env, genesis_key_name)?.clone();
            let dest_key = get_env_keys(env, fund_key_name)?.clone();
            let (generated, fund) = genesis_secure_initial_fund(
                era,
                &network_id,
                &genesis,
                &src_key,
                fund_key_name,
                &dest_key,
                tx_params,
            )
            .map_err(|err| lift_tx_gen_error(err.to_string()))?;
            get_env_wallets_mut(env, wallet_name)?.insert_fund(fund);
            Ok(vec![generated])
        }
        Generator::RoundRobin(_) => Err(lift_tx_gen_error(
            "return $ foldr1 Streaming.interleaves gList",
        )),
        Generator::OneOf(_) => Err(lift_tx_gen_error(
            "todo: implement Quickcheck style oneOf generator",
        )),
    }
}

fn take_wallet_funds(env: &mut Env, wallet_name: &str, count: usize) -> Result<Vec<Fund>, Error> {
    let wallet = get_env_wallets_mut(env, wallet_name)?;
    wallet_source(wallet, count).map_err(lift_tx_gen_error)
}

fn split_destinations(
    split: &PayWithChange,
    change_wallet: &str,
    payment_wallet: &str,
) -> Vec<String> {
    match split {
        PayWithChange::PayExact(payments) => {
            std::iter::repeat_n(payment_wallet.to_string(), payments.len()).collect()
        }
        PayWithChange::PayWithChange(_, payments) => {
            let mut destinations = Vec::with_capacity(payments.len() + 1);
            destinations.push(change_wallet.to_string());
            destinations.extend(std::iter::repeat_n(
                payment_wallet.to_string(),
                payments.len(),
            ));
            destinations
        }
    }
}

struct TxGenerationPlan<'a> {
    era: AnyCardanoEra,
    collateral_funds: &'a [Fund],
    fee: Lovelace,
    metadata: Option<&'a TxMetadata>,
    input_funds: &'a [Fund],
    protocol_parameters: Option<&'a ProtocolParameters>,
    to_utxo_list: ToUtxoList,
    destinations: Vec<String>,
}

fn generate_and_store(env: &mut Env, plan: TxGenerationPlan<'_>) -> Result<GeneratedTx, Error> {
    let generated = gen_tx(
        plan.era,
        plan.protocol_parameters,
        &env.env_keys,
        plan.collateral_funds,
        plan.fee,
        plan.metadata,
        plan.input_funds,
        &plan.to_utxo_list.outputs,
    )
    .map_err(|err| lift_tx_gen_error(err.to_string()))?;
    store_generated_funds(env, &plan.to_utxo_list, &plan.destinations, &generated)?;
    Ok(generated)
}

fn store_generated_funds(
    env: &mut Env,
    to_utxo_list: &ToUtxoList,
    destinations: &[String],
    generated: &GeneratedTx,
) -> Result<(), Error> {
    let tx_id_hex = hex::encode(generated.tx_id.0);
    let funds = to_utxo_list.funds_for_tx_id(&tx_id_hex);
    if funds.len() != destinations.len() {
        return Err(lift_tx_gen_error(format!(
            "submitInEra: generated {} funds for {} destinations",
            funds.len(),
            destinations.len()
        )));
    }

    for (wallet_name, fund) in destinations.iter().zip(funds) {
        get_env_wallets_mut(env, wallet_name)?.insert_fund(fund);
    }
    Ok(())
}

struct NtoMPreviewPlan<'a> {
    era: AnyCardanoEra,
    fee: Lovelace,
    inputs: usize,
    outputs: usize,
    wallet_name: &'a str,
    collateral_funds: &'a [Fund],
    metadata: Option<&'a TxMetadata>,
    output: &'a InterpretedPayMode,
    protocol_parameters: Option<&'a ProtocolParameters>,
}

fn preview_ntom_transaction(env: &mut Env, plan: NtoMPreviewPlan<'_>) -> Result<(), Error> {
    let preview_funds = wallet_preview(get_env_wallets(env, plan.wallet_name)?, plan.inputs);
    let preview = source_transaction_preview(
        |funds, tx_outs| {
            gen_tx(
                plan.era,
                plan.protocol_parameters,
                &env.env_keys,
                plan.collateral_funds,
                plan.fee,
                plan.metadata,
                funds,
                tx_outs,
            )
        },
        &preview_funds,
        |coins| {
            inputs_to_outputs_with_fee(plan.fee, plan.outputs, coins)
                .map_err(crate::types::TxGenError::TxGenError)
        },
        |values: Vec<Lovelace>| {
            mangle_repeat(&plan.output.to_utxo, &values)
                .map_err(crate::types::TxGenError::TxGenError)
        },
    );

    match preview {
        Ok(tx) => {
            let tx_size = tx_size_in_bytes(&tx);
            let tx_fee = projected_tx_fee(plan.protocol_parameters, &tx, tx_size);
            trace_debug(env, &format!("Projected Tx size in bytes: {tx_size}"));
            trace_debug(
                env,
                &format!(
                    "Projected Tx fee in Coin: {}",
                    projected_tx_fee_trace(tx_fee)
                ),
            );
            update_env_summary_projection(env, tx_size, tx_fee);
            dump_budget_summary_if_existing(env)?;
        }
        Err(err) => {
            trace_debug(env, &format!("Error creating Tx preview: {err}"));
        }
    }
    Ok(())
}

fn projected_tx_fee(
    protocol_parameters: Option<&ProtocolParameters>,
    tx: &GeneratedTx,
    tx_size: usize,
) -> Option<Lovelace> {
    protocol_parameters.map(|parameters| {
        let total_ex_units = tx.tx.total_ex_units();
        total_min_fee(parameters, tx_size, total_ex_units.as_ref())
    })
}

fn projected_tx_fee_trace(fee: Option<Lovelace>) -> String {
    match fee {
        Some(fee) => format!("Just (Coin {fee})"),
        None => "Nothing".to_string(),
    }
}

fn update_env_summary_projection(env: &mut Env, tx_size: usize, tx_fee: Option<Lovelace>) {
    let Some(Value::Object(mut summary)) = get_env_summary(env).cloned() else {
        return;
    };
    summary.insert("projectedTxSize".to_string(), serde_json::json!(tx_size));
    summary.insert(
        "projectedTxFee".to_string(),
        match tx_fee {
            Some(fee) => serde_json::json!(fee),
            None => Value::Null,
        },
    );
    set_env_summary(env, Value::Object(summary));
}

fn collateral_supported_in_era(era: AnyCardanoEra) -> bool {
    matches!(
        era,
        AnyCardanoEra::Alonzo
            | AnyCardanoEra::Babbage
            | AnyCardanoEra::Conway
            | AnyCardanoEra::Dijkstra
    )
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

fn submit_generated_txs_local_socket(env: &Env, txs: &[GeneratedTx]) -> Result<(), Error> {
    let connect_info = get_local_connect_info(env)?;
    run_local_tx_submission(&connect_info, txs)
}

fn run_local_tx_submission(
    connect_info: &LocalConnectInfo,
    txs: &[GeneratedTx],
) -> Result<(), Error> {
    #[cfg(not(unix))]
    {
        let _ = (connect_info, txs);
        Err(lift_tx_gen_error(
            "LocalTxSubmission over node-to-client sockets requires Unix-domain socket support",
        ))
    }

    #[cfg(unix)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| lift_tx_gen_error(format!("LocalTxSubmission runtime: {err}")))?
        .block_on(run_local_tx_submission_async(connect_info, txs))
}

#[cfg(unix)]
async fn run_local_tx_submission_async(
    connect_info: &LocalConnectInfo,
    txs: &[GeneratedTx],
) -> Result<(), Error> {
    let mut conn = ntc_connect(&connect_info.socket_path, connect_info.network_magic, true)
        .await
        .map_err(|err| {
            lift_tx_gen_error(format!(
                "LocalTxSubmission connect {} (network_magic={}): {err}",
                connect_info.socket_path.display(),
                connect_info.network_magic
            ))
        })?;
    let tx_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .ok_or_else(|| lift_tx_gen_error("NTC_LOCAL_TX_SUBMISSION mini-protocol handle missing"))?;
    let mut client = LocalTxSubmissionClient::new(tx_handle);
    for tx in txs {
        client.submit(tx.tx.raw_cbor()).await.map_err(|err| {
            lift_tx_gen_error(format!(
                "LocalTxSubmission rejected {}: {err}",
                hex::encode(tx.tx_id.0)
            ))
        })?;
    }
    client
        .done()
        .await
        .map_err(|err| lift_tx_gen_error(format!("LocalTxSubmission done failed: {err}")))?;
    Ok(())
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
    use crate::script::env::{
        Env, GenesisHandle, GenesisInitialFund, get_env_wallets, set_env_genesis,
        set_env_network_id, set_env_socket_path,
    };
    use crate::script::types::{NetworkId, PayMode, ScriptBudget, ScriptSpec};
    use crate::setup::nix_service::NodeDescription;
    use crate::tx_generator::fund::get_fund_witness;
    use crate::tx_generator::utxo::script_data_hash;
    use crate::types::{PlutusScriptRef, TxGenPlutusType};
    use std::path::PathBuf;
    use tokio::net::TcpListener;
    use yggdrasil_network::{
        HandshakeVersion, MiniProtocolNum, TxIdsReply as ServerTxIdsReply, TxSubmissionServer,
        peer_accept,
    };

    const INPUT_TX_ID: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const THREE_ARG_UNIT_FLAT: &[u8] = &[0x01, 0x00, 0x00, 0x22, 0x24, 0x98, 0x00];

    fn signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::payment_signing_key_shelley(format!("5820{}", hex::encode([byte; 32])))
    }

    fn genesis_signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::genesis_utxo_signing_key(format!("5820{}", hex::encode([byte; 32])))
    }

    fn genesis_initial_fund(
        network_id: &NetworkId,
        key: &SigningKeyEnvelope,
        lovelace: Lovelace,
    ) -> GenesisInitialFund {
        let address = crate::tx_generator::utxo::key_address(network_id, key).expect("address");
        let tx_in = yggdrasil_node_genesis::initial_funds_pseudo_txin(&address);
        GenesisInitialFund {
            address,
            tx_in: format!("{}#{}", hex::encode(tx_in.transaction_id), tx_in.index),
            lovelace,
        }
    }

    fn seed_pay_to_addr_env(env: &mut Env) {
        set_env_network_id(env, NetworkId::Testnet(42));
        init_wallet(env, "source").expect("source wallet");
        init_wallet(env, "dest").expect("dest wallet");
        define_signing_key(env, "key", signing_key(7));
    }

    fn seed_static_plutus_protocol_parameters(env: &mut Env) {
        set_proto_param_mode(
            env,
            ProtocolParameterMode::ProtocolParameterLocal(serde_json::json!({
                "costModels": {
                    "PlutusV1": [1, 2, 3],
                    "PlutusV2": [4, 5, 6],
                    "PlutusV3": [7, 8, 9]
                },
                "executionUnitPrices": {
                    "priceMemory": 2.0,
                    "priceSteps": 0.5
                },
                "maxTxExecutionUnits": {
                    "memory": 14_000_000,
                    "steps": 10_000_000_000u64
                },
                "maxBlockExecutionUnits": {
                    "memory": 50_000_000,
                    "steps": 40_000_000_000u64
                }
            })),
        );
    }

    fn seed_real_plutus_protocol_parameters(env: &mut Env) {
        let parameters: Value =
            serde_json::from_str(include_str!("../../data/protocol-parameters.json"))
                .expect("protocol parameters JSON");
        set_proto_param_mode(
            env,
            ProtocolParameterMode::ProtocolParameterLocal(parameters),
        );
    }

    fn write_temp_v1_plutus_script(name: &str, flat_bytes: &[u8]) -> PathBuf {
        let script_bytes = cbor_bytes(flat_bytes);
        let envelope_bytes = cbor_bytes(&script_bytes);
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-{name}-{}-{:?}.plutus",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
        ));
        let text = serde_json::json!({
            "type": "PlutusScriptV1",
            "description": "",
            "cborHex": hex::encode(envelope_bytes)
        });
        fs::write(
            &path,
            serde_json::to_string(&text).expect("serialize envelope"),
        )
        .expect("write temp script");
        path
    }

    fn cbor_bytes(payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 24);
        let mut out = Vec::with_capacity(payload.len() + 1);
        out.push(0x40 | payload.len() as u8);
        out.extend_from_slice(payload);
        out
    }

    struct BudgetSummaryFileCleanup;

    impl Drop for BudgetSummaryFileCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(PLUTUS_BUDGET_SUMMARY_FILE);
        }
    }

    #[test]
    fn projected_tx_fee_trace_matches_upstream_maybe_coin_shape() {
        assert_eq!(projected_tx_fee_trace(None), "Nothing");
        assert_eq!(projected_tx_fee_trace(Some(123_456)), "Just (Coin 123456)");
    }

    #[test]
    fn env_summary_projection_updates_size_and_fee_fields() {
        let mut env = Env::empty_env();
        set_env_summary(
            &mut env,
            serde_json::json!({
                "perTxExecutionUnits": {
                    "memory": 10,
                    "steps": 20
                },
                "projectedTxSize": null,
                "projectedTxFee": null
            }),
        );

        update_env_summary_projection(&mut env, 1_024, Some(172_381));

        let summary = get_env_summary(&env).expect("summary");
        assert_eq!(summary["projectedTxSize"], serde_json::json!(1_024));
        assert_eq!(summary["projectedTxFee"], serde_json::json!(172_381));
        assert_eq!(
            summary["perTxExecutionUnits"]["memory"],
            serde_json::json!(10)
        );
        assert_eq!(
            summary["perTxExecutionUnits"]["steps"],
            serde_json::json!(20)
        );
    }

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
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            "abc#0",
            100,
            "key",
        )
        .expect("source fund");
        let generator = Generator::NtoM(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
            1,
            Some(38),
            None,
        );
        let err = submit_in_era(
            &mut env,
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
    fn submit_in_era_preflights_splitn_value_split() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(&mut env, AnyCardanoEra::Conway, "source", "abc#0", 9, "key").expect("fund");
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            2,
        );

        let err = submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect_err("value split rejected");

        assert_eq!(
            err,
            Error::TxGenError(
                "inputsToOutputsWithFee: insufficient funds, inputs=[9], fee=10".to_string()
            )
        );
    }

    #[test]
    fn discard_submit_generates_key_spend_tx_and_updates_destination_wallet() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("discard submit");

        assert!(
            get_env_wallets(&env, "source")
                .expect("source")
                .funds()
                .is_empty()
        );
        let dest_funds = get_env_wallets(&env, "dest").expect("dest").funds();
        assert_eq!(dest_funds.len(), 1);
        assert_eq!(dest_funds[0].lovelace, 90);
        assert_ne!(dest_funds[0].tx_in, format!("{INPUT_TX_ID}#0"));
    }

    #[test]
    fn dumptofile_submit_generates_mary_haskell_show_transaction() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Mary,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-mary-dump-{}.out",
            std::process::id()
        ));
        let _ = fs::remove_file(&output_path);
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Mary,
            &SubmitMode::DumpToFile(output_path.clone()),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("mary dump submit");

        let rendered = fs::read_to_string(&output_path).expect("mary dump output");
        let _ = fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraMary (ShelleyTx {stBody = MkMaryTxBody AllegraTxBodyRaw"
        ));
        assert!(rendered.contains("MaryValue (Coin 90) (MultiAsset (fromList []))"));
        assert!(rendered.contains("atbrMint = MultiAsset (fromList [])"));
        assert!(rendered.contains("stWits = ShelleyTxWitsRaw"));
        assert_eq!(rendered.lines().filter(|line| !line.is_empty()).count(), 1);
    }

    #[test]
    fn dumptofile_mary_value_accepts_empty_multi_asset() {
        let rendered =
            show_mary_value(&yggdrasil_ledger::Value::CoinAndAssets(90, BTreeMap::new()))
                .expect("empty multi-asset value");

        assert_eq!(rendered, "MaryValue (Coin 90) (MultiAsset (fromList []))");
    }

    #[test]
    fn dumptofile_mary_value_renders_single_asset() {
        let mut inner = BTreeMap::new();
        inner.insert(vec![0xDE, 0xAD, 0xBE, 0xEF], 7);
        let mut ma = BTreeMap::new();
        ma.insert([0x11; 28], inner);

        let rendered = show_mary_value(&yggdrasil_ledger::Value::CoinAndAssets(42, ma))
            .expect("single multi-asset value");

        assert_eq!(
            rendered,
            "MaryValue (Coin 42) (MultiAsset (fromList [(PolicyID {policyID = ScriptHash \"11111111111111111111111111111111111111111111111111111111\"},fromList [(\"deadbeef\",7)])]))"
        );
    }

    #[test]
    fn dumptofile_mary_value_renders_empty_asset_name() {
        let mut inner = BTreeMap::new();
        inner.insert(Vec::new(), 1);
        let mut ma = BTreeMap::new();
        ma.insert([0xAB; 28], inner);

        let rendered = show_mary_value(&yggdrasil_ledger::Value::CoinAndAssets(1, ma))
            .expect("empty-asset-name multi-asset value");

        assert_eq!(
            rendered,
            "MaryValue (Coin 1) (MultiAsset (fromList [(PolicyID {policyID = ScriptHash \"abababababababababababababababababababababababababababab\"},fromList [(\"\",1)])]))"
        );
    }

    #[test]
    fn dumptofile_mary_value_sorts_policies_by_byte_lex_order() {
        let mut ma = BTreeMap::new();
        let mut inner_b = BTreeMap::new();
        inner_b.insert(vec![0x01], 9);
        ma.insert([0xFF; 28], inner_b);

        let mut inner_a = BTreeMap::new();
        inner_a.insert(vec![0x02], 5);
        ma.insert([0x00; 28], inner_a);

        let rendered = show_mary_value(&yggdrasil_ledger::Value::CoinAndAssets(0, ma))
            .expect("multi-policy multi-asset value");

        // 0x00... policy must precede 0xFF... policy (BTreeMap iter is
        // byte-lex ordered, matching upstream Data.Map toAscList on
        // PolicyID / ScriptHash byte ordering). 28 bytes = 56 hex chars.
        let zero_hex: String = std::iter::repeat_n('0', 56).collect();
        let ff_hex: String = std::iter::repeat_n('f', 56).collect();
        let zero_pos = rendered
            .find(&format!("ScriptHash \"{zero_hex}\""))
            .expect("zero-prefix policy rendered");
        let ff_pos = rendered
            .find(&format!("ScriptHash \"{ff_hex}\""))
            .expect("ff-prefix policy rendered");
        assert!(
            zero_pos < ff_pos,
            "policies must be rendered in ascending byte-lex order: rendered={rendered}"
        );
    }

    #[test]
    fn dumptofile_plutus_data_renders_integer() {
        assert_eq!(show_plutus_data(&PlutusData::integer(0)), "I 0");
        assert_eq!(show_plutus_data(&PlutusData::integer(42)), "I 42");
        assert_eq!(show_plutus_data(&PlutusData::integer(-7)), "I (-7)");
    }

    #[test]
    fn dumptofile_plutus_data_renders_bytes() {
        // Printable ASCII inline, non-printable as Haskell decimal escapes.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(b"abc".to_vec())),
            "B \"abc\""
        );
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])),
            "B \"\\222\\173\\190\\239\""
        );
        // `\&` separator inserted before a following digit so the escape
        // boundary is unambiguous (decimal 200 followed by ASCII '5').
        // Bytes >= 0x80 always use the decimal `\NNN` form; the renderer
        // does not yet emit mnemonic escapes for the 0x00-0x1F range (those
        // are matched against the printable-ASCII branch in upstream Show
        // and need named escapes like `\NUL`, `\SOH`, ... — a future
        // round may close that for full byte parity).
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0xC8, b'5'])),
            "B \"\\200\\&5\""
        );
        // Backslash and double-quote escape paths.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(b"a\\b\"c".to_vec())),
            "B \"a\\\\b\\\"c\""
        );
    }

    #[test]
    fn dumptofile_plutus_data_renders_bytes_full_mnemonic_escapes() {
        // 0x07-0x0D short-form aliases.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x07, 0x08, 0x0B, 0x0C])),
            "B \"\\a\\b\\v\\f\""
        );
        // 0x00-0x06 multi-letter mnemonics.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x00, 0x01, 0x02, 0x03])),
            "B \"\\NUL\\SOH\\STX\\ETX\""
        );
        // 0x0E SO with H lookahead: needs `\&` separator so `\SOH` (Start
        // Of Heading) and `\SO`+`H` stay distinguishable.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x0E, b'H'])),
            "B \"\\SO\\&H\""
        );
        // 0x0E SO without H lookahead: no separator.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x0E, b'I'])),
            "B \"\\SOI\""
        );
        // 0x0F-0x1F multi-letter mnemonics.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x0F, 0x1F])),
            "B \"\\SI\\US\""
        );
        // 0x7F DEL.
        assert_eq!(
            show_plutus_data(&PlutusData::Bytes(vec![0x7F])),
            "B \"\\DEL\""
        );
    }

    #[test]
    fn dumptofile_plutus_data_renders_list() {
        let list = PlutusData::List(vec![
            PlutusData::integer(1),
            PlutusData::integer(2),
            PlutusData::Bytes(b"x".to_vec()),
        ]);
        assert_eq!(show_plutus_data(&list), "List [I 1,I 2,B \"x\"]");
    }

    #[test]
    fn dumptofile_plutus_data_renders_map_and_constr() {
        let inner = PlutusData::Constr(
            0,
            vec![
                PlutusData::integer(7),
                PlutusData::List(vec![PlutusData::Bytes(b"a".to_vec())]),
            ],
        );
        let outer = PlutusData::Map(vec![(PlutusData::integer(0), inner)]);
        assert_eq!(
            show_plutus_data(&outer),
            "Map [(I 0,Constr 0 [I 7,List [B \"a\"]])]"
        );
    }

    #[test]
    fn dumptofile_collateral_list_renders_sorted() {
        // The collateral set reuses `show_tx_in_list`, which
        // sorts to match upstream `Set TxIn` Show ordering.
        let collateral = [
            ShelleyTxIn {
                transaction_id: [0xCC_u8; 32],
                index: 0,
            },
            ShelleyTxIn {
                transaction_id: [0x11_u8; 32],
                index: 4,
            },
        ];
        let rendered = show_tx_in_list(&collateral);
        assert_eq!(
            rendered,
            format!(
                "TxIn (TxId {{unTxId = SafeHash \"{}\"}}) (TxIx {{unTxIx = 4}}),\
TxIn (TxId {{unTxId = SafeHash \"{}\"}}) (TxIx {{unTxIx = 0}})",
                "11".repeat(32),
                "cc".repeat(32),
            )
        );
    }

    #[test]
    fn dumptofile_script_integrity_hash_render() {
        assert_eq!(show_strict_maybe_script_integrity_hash(None), "SNothing");
        assert_eq!(
            show_strict_maybe_script_integrity_hash(Some([0x3F_u8; 32])),
            format!("SJust (SafeHash \"{}\")", "3f".repeat(32))
        );
    }

    #[test]
    fn dumptofile_aux_data_hash_render() {
        assert_eq!(show_strict_maybe_aux_data_hash(None), "SNothing");
        let rendered = show_strict_maybe_aux_data_hash(Some([0x9E_u8; 32]));
        assert_eq!(
            rendered,
            format!(
                "SJust (TxAuxDataHash {{unTxAuxDataHash = SafeHash \"{}\"}})",
                "9e".repeat(32)
            )
        );
    }

    #[test]
    fn dumptofile_req_signer_hashes_render() {
        // Empty / absent renders as an empty fromList.
        assert_eq!(show_req_signer_hashes(None), "fromList []");
        // Multiple hashes are sorted to match upstream Set ordering.
        let hashes = [[0xCC_u8; 28], [0x11_u8; 28]];
        let rendered = show_req_signer_hashes(Some(&hashes));
        assert_eq!(
            rendered,
            format!(
                "fromList [KeyHash {{unKeyHash = \"{}\"}},KeyHash {{unKeyHash = \"{}\"}}]",
                "11".repeat(28),
                "cc".repeat(28),
            )
        );
    }

    #[test]
    fn dumptofile_withdrawals_render() {
        // Empty / absent withdrawals render as an empty fromList.
        assert_eq!(
            show_withdrawals(None).expect("none"),
            "Withdrawals {unWithdrawals = fromList []}"
        );
        // A one-entry map renders the AccountAddress key + Coin value.
        let mut map: BTreeMap<yggdrasil_ledger::RewardAccount, u64> = BTreeMap::new();
        map.insert(
            yggdrasil_ledger::RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x4B_u8; 28]),
            },
            2_500_000,
        );
        let rendered = show_withdrawals(Some(&map)).expect("one entry");
        assert!(
            rendered.starts_with(
                "Withdrawals {unWithdrawals = fromList [(AccountAddress {aaNetworkId = Testnet,"
            ),
            "got: {rendered}"
        );
        assert!(rendered.ends_with(",Coin 2500000)]}"), "got: {rendered}");
    }

    #[test]
    fn dumptofile_strict_maybe_network_renders() {
        // StrictMaybe Network — SNothing / SJust Testnet / SJust
        // Mainnet, with Network's stock-derived nullary Show.
        assert_eq!(show_strict_maybe_network(None).expect("none"), "SNothing");
        assert_eq!(
            show_strict_maybe_network(Some(0)).expect("testnet"),
            "SJust Testnet"
        );
        assert_eq!(
            show_strict_maybe_network(Some(1)).expect("mainnet"),
            "SJust Mainnet"
        );
        assert!(
            show_strict_maybe_network(Some(7))
                .expect_err("invalid network id rejects")
                .to_string()
                .contains("unsupported Shelley network id 7"),
        );
    }

    #[test]
    fn dumptofile_alonzo_redeemers_render_single_spending_entry() {
        let redeemer = Redeemer {
            tag: 0,
            index: 3,
            data: PlutusData::integer(42),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        };
        let rendered = show_alonzo_redeemers(&[redeemer]).expect("single spending redeemer");
        assert!(
            rendered.contains("(AlonzoSpending (AsIx {unAsIx = 3}),(I 42,ExUnits {exUnitsMem = 1000, exUnitsSteps = 2000}))"),
            "unexpected redeemer rendering: {rendered}"
        );
        assert!(
            rendered.starts_with("MkRedeemers (RedeemersRaw {unRedeemersRaw = fromList ["),
            "unexpected redeemers envelope: {rendered}"
        );
    }

    #[test]
    fn dumptofile_alonzo_redeemers_sort_by_tag_then_index() {
        // Insert out-of-order; expect sorted (tag, index) on output.
        let rs = vec![
            Redeemer {
                tag: 1,
                index: 0,
                data: PlutusData::integer(0),
                ex_units: ExUnits { mem: 0, steps: 0 },
            },
            Redeemer {
                tag: 0,
                index: 5,
                data: PlutusData::integer(0),
                ex_units: ExUnits { mem: 0, steps: 0 },
            },
            Redeemer {
                tag: 0,
                index: 2,
                data: PlutusData::integer(0),
                ex_units: ExUnits { mem: 0, steps: 0 },
            },
        ];
        let rendered = show_alonzo_redeemers(&rs).expect("multi-redeemer rendering");
        let spend2 = rendered
            .find("AlonzoSpending (AsIx {unAsIx = 2})")
            .expect("spend2");
        let spend5 = rendered
            .find("AlonzoSpending (AsIx {unAsIx = 5})")
            .expect("spend5");
        let mint0 = rendered
            .find("AlonzoMinting (AsIx {unAsIx = 0})")
            .expect("mint0");
        assert!(
            spend2 < spend5 && spend5 < mint0,
            "redeemer ordering wrong: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_info_and_no_confidence() {
        let info = yggdrasil_ledger::GovAction::InfoAction;
        assert_eq!(show_conway_gov_action(&info).expect("info"), "InfoAction");

        let nc_none = yggdrasil_ledger::GovAction::NoConfidence {
            prev_action_id: None,
        };
        assert_eq!(
            show_conway_gov_action(&nc_none).expect("nc-none"),
            "NoConfidence SNothing"
        );

        let nc_some = yggdrasil_ledger::GovAction::NoConfidence {
            prev_action_id: Some(yggdrasil_ledger::GovActionId {
                transaction_id: [0xAB; 32],
                gov_action_index: 4,
            }),
        };
        let rendered = show_conway_gov_action(&nc_some).expect("nc-some");
        assert!(rendered.starts_with("NoConfidence (SJust GovActionId {"));
        assert!(rendered.contains("gaidGovActionIx = GovActionIx {unGovActionIx = 4}"));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_hard_fork_initiation() {
        let action = yggdrasil_ledger::GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (11, 0),
        };
        let rendered = show_conway_gov_action(&action).expect("hard fork");
        assert!(rendered.starts_with("HardForkInitiation SNothing (ProtVer {pvMajor = Version 11"));
        assert!(rendered.contains("pvMinor = 0}"));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_new_constitution() {
        let constitution = yggdrasil_ledger::Constitution {
            anchor: yggdrasil_ledger::Anchor {
                url: "https://x.test/c".to_string(),
                data_hash: [0x33; 32],
            },
            guardrails_script_hash: Some([0x44; 28]),
        };
        let action = yggdrasil_ledger::GovAction::NewConstitution {
            prev_action_id: None,
            constitution,
        };
        let rendered = show_conway_gov_action(&action).expect("new constitution");
        assert!(
            rendered
                .contains("NewConstitution SNothing (Constitution {constitutionAnchor = Anchor")
        );
        assert!(rendered.contains("constitutionGuardrailsScriptHash = SJust (ScriptHash \"4444"));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_treasury_withdrawals() {
        // Empty withdrawals + no guardrails: minimal form.
        let action_empty = yggdrasil_ledger::GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        };
        assert_eq!(
            show_conway_gov_action(&action_empty).expect("treasury empty"),
            "TreasuryWithdrawals (fromList []) SNothing"
        );

        // Single key-hash withdrawal with non-zero amount + script-hash
        // guardrails: full form.
        let mut withdrawals = BTreeMap::new();
        withdrawals.insert(
            yggdrasil_ledger::RewardAccount {
                network: 1,
                credential: yggdrasil_ledger::StakeCredential::AddrKeyHash([0x77; 28]),
            },
            123_456_789_u64,
        );
        let action_full = yggdrasil_ledger::GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: Some([0x88; 28]),
        };
        let rendered = show_conway_gov_action(&action_full).expect("treasury full");
        assert!(
            rendered.starts_with(
                "TreasuryWithdrawals (fromList [(AccountAddress {aaNetworkId = Mainnet, aaId = KeyHashObj (KeyHash {unKeyHash = \""
            ),
            "unexpected treasury prefix: {rendered}"
        );
        assert!(
            rendered.contains("Coin 123456789)"),
            "expected Coin entry in: {rendered}"
        );
        assert!(
            rendered.contains("(SJust (ScriptHash \""),
            "expected SJust guardrails in: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_unit_interval_wraps_with_parens() {
        let ui = yggdrasil_ledger::UnitInterval {
            numerator: 1,
            denominator: 2,
        };
        assert_eq!(show_unit_interval(ui), "(1 % 2)");
    }

    #[test]
    fn dumptofile_show_stake_credential_variants() {
        assert_eq!(
            show_stake_credential(&yggdrasil_ledger::StakeCredential::AddrKeyHash([0x11; 28])),
            "KeyHashObj (KeyHash {unKeyHash = \"11111111111111111111111111111111111111111111111111111111\"})"
        );
        assert_eq!(
            show_stake_credential(&yggdrasil_ledger::StakeCredential::ScriptHash([0x22; 28])),
            "ScriptHashObj (ScriptHash \"22222222222222222222222222222222222222222222222222222222\")"
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_update_committee() {
        // Minimal empty form: no prev, no remove, no add, quorum 1/3.
        let empty = yggdrasil_ledger::GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: Vec::new(),
            members_to_add: BTreeMap::new(),
            quorum: yggdrasil_ledger::UnitInterval {
                numerator: 1,
                denominator: 3,
            },
        };
        let rendered = show_conway_gov_action(&empty).expect("empty update committee");
        assert_eq!(
            rendered,
            "UpdateCommittee SNothing (fromList []) (fromList []) (1 % 3)"
        );

        // Full form with one removal, one addition, and SJust prev.
        let mut members_to_add = BTreeMap::new();
        members_to_add.insert(
            yggdrasil_ledger::StakeCredential::AddrKeyHash([0xCC; 28]),
            500_u64,
        );
        let full = yggdrasil_ledger::GovAction::UpdateCommittee {
            prev_action_id: Some(yggdrasil_ledger::GovActionId {
                transaction_id: [0xAA; 32],
                gov_action_index: 9,
            }),
            members_to_remove: vec![yggdrasil_ledger::StakeCredential::AddrKeyHash([0xBB; 28])],
            members_to_add,
            quorum: yggdrasil_ledger::UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        };
        let rendered = show_conway_gov_action(&full).expect("full update committee");
        assert!(rendered.starts_with("UpdateCommittee (SJust GovActionId {"));
        assert!(
            rendered.contains("(fromList [KeyHashObj (KeyHash {unKeyHash = \"bbbb"),
            "expected sorted removal set: {rendered}"
        );
        assert!(
            rendered.contains(",EpochNo 500)"),
            "expected EpochNo entry: {rendered}"
        );
        assert!(
            rendered.ends_with(" (2 % 3)"),
            "expected quorum: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_empty() {
        // Empty PParamsUpdate renders the full Conway 30-field ConwayPParams
        // record with all SNothing values (and cppProtocolVersion = NoUpdate).
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: Default::default(),
            guardrails_script_hash: None,
        };
        let rendered = show_conway_gov_action(&parameter_change).expect("empty parameter change");
        assert!(
            rendered
                .starts_with("ParameterChange SNothing (ConwayPParams {cppTxFeePerByte = SNothing")
        );
        assert!(rendered.contains("cppMinFeeRefScriptCostPerByte = SNothing}) SNothing"));
        assert!(rendered.contains("cppProtocolVersion = NoUpdate"));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_with_coin_fields() {
        // 8 Coin-family fields set: each renders as SJust (CompactCoin
        // {unCompactCoin = N}) at p=0 inside the record. min_committee_size
        // and the Word{16,32} fields render as SJust <n>.
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            min_fee_a: Some(44),
            min_fee_b: Some(155381),
            max_block_body_size: Some(90112),
            max_tx_size: Some(16384),
            max_block_header_size: Some(1100),
            key_deposit: Some(2_000_000),
            pool_deposit: Some(500_000_000),
            n_opt: Some(500),
            min_pool_cost: Some(170_000_000),
            coins_per_utxo_byte: Some(4310),
            max_val_size: Some(5000),
            collateral_percentage: Some(150),
            max_collateral_inputs: Some(3),
            min_committee_size: Some(7),
            gov_action_deposit: Some(100_000_000_000),
            drep_deposit: Some(500_000_000),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let rendered =
            show_conway_gov_action(&parameter_change).expect("parameter change with coin fields");
        assert!(rendered.contains("cppTxFeePerByte = SJust (CompactCoin {unCompactCoin = 44})"));
        assert!(rendered.contains("cppKeyDeposit = SJust (CompactCoin {unCompactCoin = 2000000})"));
        assert!(rendered.contains("cppMaxBBSize = SJust 90112"));
        assert!(rendered.contains("cppNOpt = SJust 500"));
        assert!(rendered.contains("cppCommitteeMinSize = SJust 7"));
        assert!(
            rendered.contains(
                "cppGovActionDeposit = SJust (CompactCoin {unCompactCoin = 100000000000})"
            )
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_with_interval_fields() {
        // 8 interval fields: 4 EpochInterval as `SJust (EpochInterval N)`
        // and 4 NonNegativeInterval/UnitInterval as `SJust (num % den)`.
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            e_max: Some(18),
            committee_term_limit: Some(146),
            gov_action_lifetime: Some(6),
            drep_activity: Some(20),
            a0: Some(yggdrasil_ledger::UnitInterval {
                numerator: 3,
                denominator: 10,
            }),
            rho: Some(yggdrasil_ledger::UnitInterval {
                numerator: 3,
                denominator: 1000,
            }),
            tau: Some(yggdrasil_ledger::UnitInterval {
                numerator: 2,
                denominator: 10,
            }),
            min_fee_ref_script_cost_per_byte: Some(yggdrasil_ledger::UnitInterval {
                numerator: 44,
                denominator: 1,
            }),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let rendered = show_conway_gov_action(&parameter_change)
            .expect("parameter change with interval fields");
        assert!(rendered.contains("cppEMax = SJust (EpochInterval 18)"));
        assert!(rendered.contains("cppCommitteeMaxTermLength = SJust (EpochInterval 146)"));
        assert!(rendered.contains("cppGovActionLifetime = SJust (EpochInterval 6)"));
        assert!(rendered.contains("cppDRepActivity = SJust (EpochInterval 20)"));
        assert!(rendered.contains("cppA0 = SJust (3 % 10)"));
        assert!(rendered.contains("cppRho = SJust (3 % 1000)"));
        assert!(rendered.contains("cppTau = SJust (2 % 10)"));
        assert!(rendered.contains("cppMinFeeRefScriptCostPerByte = SJust (44 % 1)"));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_with_prices_and_exunits() {
        // cppPrices: requires both price_mem and price_step set.
        // cppMaxTxExUnits / cppMaxBlockExUnits: ExUnits record.
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            price_mem: Some(yggdrasil_ledger::UnitInterval {
                numerator: 577,
                denominator: 10000,
            }),
            price_step: Some(yggdrasil_ledger::UnitInterval {
                numerator: 721,
                denominator: 10_000_000,
            }),
            max_tx_ex_units: Some(yggdrasil_ledger::eras::alonzo::ExUnits {
                mem: 14_000_000,
                steps: 10_000_000_000,
            }),
            max_block_ex_units: Some(yggdrasil_ledger::eras::alonzo::ExUnits {
                mem: 62_000_000,
                steps: 20_000_000_000,
            }),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let rendered = show_conway_gov_action(&parameter_change)
            .expect("parameter change with prices+exunits");
        assert!(rendered.contains(
            "cppPrices = SJust (Prices {prMem = 577 % 10000, prSteps = 721 % 10000000})"
        ));
        assert!(rendered.contains(
            "cppMaxTxExUnits = SJust (ExUnits {exUnitsMem = 14000000, exUnitsSteps = 10000000000})"
        ));
        assert!(rendered.contains(
            "cppMaxBlockExUnits = SJust (ExUnits {exUnitsMem = 62000000, exUnitsSteps = 20000000000})"
        ));
    }

    #[test]
    fn dumptofile_show_conway_pparam_prices_rejects_unpaired() {
        // Setting only price_mem without price_step must be reported.
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            price_mem: Some(yggdrasil_ledger::UnitInterval {
                numerator: 1,
                denominator: 2,
            }),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let err =
            show_conway_gov_action(&parameter_change).expect_err("unpaired prices should reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("cppPrices") && msg.contains("price_mem and price_step"),
            "expected pairing message: {msg}"
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_with_voting_thresholds() {
        let half = yggdrasil_ledger::UnitInterval {
            numerator: 1,
            denominator: 2,
        };
        let two_thirds = yggdrasil_ledger::UnitInterval {
            numerator: 2,
            denominator: 3,
        };
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            pool_voting_thresholds: Some(yggdrasil_ledger::PoolVotingThresholds {
                motion_no_confidence: half,
                committee_normal: half,
                committee_no_confidence: two_thirds,
                hard_fork_initiation: half,
                pp_security_group: half,
            }),
            drep_voting_thresholds: Some(yggdrasil_ledger::DRepVotingThresholds {
                motion_no_confidence: two_thirds,
                committee_normal: half,
                committee_no_confidence: two_thirds,
                update_to_constitution: two_thirds,
                hard_fork_initiation: two_thirds,
                pp_network_group: half,
                pp_economic_group: half,
                pp_technical_group: half,
                pp_gov_group: two_thirds,
                treasury_withdrawal: two_thirds,
            }),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let rendered = show_conway_gov_action(&parameter_change)
            .expect("parameter change with voting thresholds");
        assert!(rendered.contains(
            "cppPoolVotingThresholds = SJust (PoolVotingThresholds {pvtMotionNoConfidence = 1 % 2, pvtCommitteeNormal = 1 % 2, pvtCommitteeNoConfidence = 2 % 3, pvtHardForkInitiation = 1 % 2, pvtPPSecurityGroup = 1 % 2})"
        ));
        assert!(rendered.contains(
            "cppDRepVotingThresholds = SJust (DRepVotingThresholds {dvtMotionNoConfidence = 2 % 3, dvtCommitteeNormal = 1 % 2, dvtCommitteeNoConfidence = 2 % 3, dvtUpdateToConstitution = 2 % 3, dvtHardForkInitiation = 2 % 3, dvtPPNetworkGroup = 1 % 2, dvtPPEconomicGroup = 1 % 2, dvtPPTechnicalGroup = 1 % 2, dvtPPGovGroup = 2 % 3, dvtTreasuryWithdrawal = 2 % 3})"
        ));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_with_cost_models() {
        let mut models: BTreeMap<u8, Vec<i64>> = BTreeMap::new();
        models.insert(0, vec![100, 200]);
        models.insert(2, vec![300]);
        models.insert(7, vec![999]); // unknown tag → _costModelsUnknown
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            cost_models: Some(models),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let rendered =
            show_conway_gov_action(&parameter_change).expect("parameter change with cost models");
        assert!(rendered.contains(
            "cppCostModels = SJust (CostModels {_costModelsValid = fromList [(PlutusV1,CostModel PlutusV1 [100,200]),(PlutusV3,CostModel PlutusV3 [300])], _costModelsUnknown = fromList [(7,[999])]})"
        ));
    }

    #[test]
    fn dumptofile_show_conway_gov_action_parameter_change_rejects_shelley_only_field() {
        // Setting min_utxo_value (a Shelley-era-only PParamsUpdate field
        // that Conway dropped) reports an explicit boundary error.
        let update = yggdrasil_ledger::ProtocolParameterUpdate {
            min_utxo_value: Some(1_000_000),
            ..Default::default()
        };
        let parameter_change = yggdrasil_ledger::GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: update,
            guardrails_script_hash: None,
        };
        let err = show_conway_gov_action(&parameter_change)
            .expect_err("Shelley-only PParamsUpdate field should reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("min_utxo_value"),
            "expected field-name in error: {msg}"
        );
    }

    #[test]
    fn dumptofile_show_account_address_keyhash_and_script() {
        // Mainnet key-hash reward account: header 0xE1.
        let mut bytes = vec![0xE1_u8];
        bytes.extend_from_slice(&[0x11; 28]);
        let rendered = show_account_address(&bytes).expect("keyhash account");
        assert!(
            rendered.starts_with("AccountAddress {aaNetworkId = Mainnet, aaId = KeyHashObj"),
            "unexpected keyhash rendering: {rendered}"
        );

        // Testnet script-hash reward account: header 0xF0.
        let mut bytes = vec![0xF0_u8];
        bytes.extend_from_slice(&[0x22; 28]);
        let rendered = show_account_address(&bytes).expect("script account");
        assert!(
            rendered.starts_with("AccountAddress {aaNetworkId = Testnet, aaId = ScriptHashObj"),
            "unexpected script rendering: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_conway_proposal_procedures_empty_and_full() {
        // Empty / None returns the prior `OSet {...empty...}` shape.
        let empty = show_conway_proposal_procedures(None).expect("empty");
        assert!(empty.contains("osSSeq = StrictSeq {fromStrict = fromList []}"));

        let mut bytes = vec![0xE1_u8];
        bytes.extend_from_slice(&[0x55; 28]);
        let proc = yggdrasil_ledger::ProposalProcedure {
            deposit: 500_000_000,
            reward_account: bytes,
            gov_action: yggdrasil_ledger::GovAction::InfoAction,
            anchor: yggdrasil_ledger::Anchor {
                url: "https://x.test".to_string(),
                data_hash: [0x66; 32],
            },
        };
        let rendered = show_conway_proposal_procedures(Some(&[proc])).expect("full");
        assert!(rendered.contains("ProposalProcedure {pProcDeposit = Coin 500000000"));
        assert!(rendered.contains("pProcGovAction = InfoAction"));
        assert!(rendered.contains("pProcAnchor = Anchor {anchorUrl = Url"));
    }

    #[test]
    fn dumptofile_show_conway_vote() {
        assert_eq!(show_conway_vote(yggdrasil_ledger::Vote::No), "VoteNo");
        assert_eq!(show_conway_vote(yggdrasil_ledger::Vote::Yes), "VoteYes");
        assert_eq!(show_conway_vote(yggdrasil_ledger::Vote::Abstain), "Abstain");
    }

    #[test]
    fn dumptofile_show_conway_voter_variants() {
        use yggdrasil_ledger::Voter;
        let h = [0x11_u8; 28];
        let hex_h = hex::encode(h);
        assert_eq!(
            show_conway_voter(&Voter::CommitteeKeyHash(h)),
            format!("CommitteeVoter (KeyHashObj (KeyHash {{unKeyHash = \"{hex_h}\"}}))")
        );
        assert_eq!(
            show_conway_voter(&Voter::CommitteeScript(h)),
            format!("CommitteeVoter (ScriptHashObj (ScriptHash \"{hex_h}\"))")
        );
        assert_eq!(
            show_conway_voter(&Voter::DRepKeyHash(h)),
            format!("DRepVoter (KeyHashObj (KeyHash {{unKeyHash = \"{hex_h}\"}}))")
        );
        assert_eq!(
            show_conway_voter(&Voter::DRepScript(h)),
            format!("DRepVoter (ScriptHashObj (ScriptHash \"{hex_h}\"))")
        );
        assert_eq!(
            show_conway_voter(&Voter::StakePool(h)),
            format!("StakePoolVoter (KeyHash {{unKeyHash = \"{hex_h}\"}})")
        );
    }

    #[test]
    fn dumptofile_show_conway_gov_action_id_renders_record_form() {
        let id = yggdrasil_ledger::GovActionId {
            transaction_id: [0xAB; 32],
            gov_action_index: 7,
        };
        let rendered = show_conway_gov_action_id(&id);
        assert!(rendered.starts_with("GovActionId {gaidTxId = TxId {unTxId = SafeHash \""));
        assert!(rendered.contains("gaidGovActionIx = GovActionIx {unGovActionIx = 7}"));
    }

    #[test]
    fn dumptofile_show_conway_voting_procedure_with_and_without_anchor() {
        let vp_no = yggdrasil_ledger::VotingProcedure {
            vote: yggdrasil_ledger::Vote::Yes,
            anchor: None,
        };
        assert_eq!(
            show_conway_voting_procedure(&vp_no),
            "VotingProcedure {vProcVote = VoteYes, vProcAnchor = SNothing}"
        );

        let vp_yes = yggdrasil_ledger::VotingProcedure {
            vote: yggdrasil_ledger::Vote::No,
            anchor: Some(yggdrasil_ledger::Anchor {
                url: "https://example.test/m.json".to_string(),
                data_hash: [0xCD; 32],
            }),
        };
        let rendered = show_conway_voting_procedure(&vp_yes);
        assert!(rendered.starts_with("VotingProcedure {vProcVote = VoteNo, vProcAnchor = SJust ("));
        assert!(
            rendered
                .contains("Anchor {anchorUrl = Url {urlToText = \"https://example.test/m.json\"}")
        );
    }

    #[test]
    fn dumptofile_show_conway_voting_procedures_empty_and_full() {
        // Empty (None) renders as an empty `unVotingProcedures = fromList []`.
        let none_rendered = show_conway_voting_procedures(None);
        assert_eq!(
            none_rendered,
            "VotingProcedures {unVotingProcedures = fromList []}"
        );

        // Single outer entry with a single inner entry.
        let mut inner = BTreeMap::new();
        inner.insert(
            yggdrasil_ledger::GovActionId {
                transaction_id: [0x11; 32],
                gov_action_index: 0,
            },
            yggdrasil_ledger::VotingProcedure {
                vote: yggdrasil_ledger::Vote::Yes,
                anchor: None,
            },
        );
        let mut outer = BTreeMap::new();
        outer.insert(yggdrasil_ledger::Voter::DRepKeyHash([0x22; 28]), inner);
        let vp = yggdrasil_ledger::VotingProcedures { procedures: outer };
        let rendered = show_conway_voting_procedures(Some(&vp));
        assert!(rendered.starts_with("VotingProcedures {unVotingProcedures = fromList [("));
        assert!(rendered.contains("DRepVoter (KeyHashObj (KeyHash {unKeyHash = \""));
        assert!(rendered.contains("vProcVote = VoteYes"));
    }

    #[test]
    fn dumptofile_show_coin_helpers() {
        assert_eq!(show_coin(0), "Coin 0");
        assert_eq!(show_coin(123_456_789), "Coin 123456789");
        assert_eq!(show_strict_maybe_coin(None), "SNothing");
        assert_eq!(show_strict_maybe_coin(Some(0)), "SJust (Coin 0)");
        assert_eq!(show_strict_maybe_coin(Some(42)), "SJust (Coin 42)");
    }

    #[test]
    fn dumptofile_alonzo_witness_set_renders_empty_script_map() {
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let rendered = show_alonzo_witness_set(&ws).expect("empty witness set");
        assert!(
            rendered.contains("atwrScriptTxWits = fromList []"),
            "empty witness set must keep `atwrScriptTxWits = fromList []`: {rendered}"
        );
    }

    #[test]
    fn dumptofile_alonzo_witness_set_renders_plutus_v2_script() {
        let bytes = vec![0xCA, 0xFE, 0xBA, 0xBE];
        let expected_hash = hex::encode(plutus_script_hash(2, &bytes));
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![bytes],
            plutus_v3_scripts: vec![],
        };
        let rendered = show_alonzo_witness_set(&ws).expect("v2 witness set");
        let expected_entry = format!(
            "atwrScriptTxWits = fromList [(ScriptHash \"{expected_hash}\",PlutusScript PlutusV2 ScriptHash \"{expected_hash}\")]"
        );
        assert!(
            rendered.contains(&expected_entry),
            "expected witness-set script entry {expected_entry} not found in {rendered}"
        );
    }

    #[test]
    fn dumptofile_alonzo_witness_set_sorts_scripts_by_hash_bytelex() {
        // Two scripts whose hashes will be different. We don't know which
        // sort-order will result from arbitrary bytes, so we sort the
        // expected hashes manually and check the rendered order.
        let bytes_a = vec![0x01];
        let bytes_b = vec![0x02];
        let hash_a = plutus_script_hash(1, &bytes_a);
        let hash_b = plutus_script_hash(2, &bytes_b);
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![bytes_a],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![bytes_b],
            plutus_v3_scripts: vec![],
        };
        let rendered = show_alonzo_witness_set(&ws).expect("multi-version witness set");
        let pos_a = rendered.find(&hex::encode(hash_a)).expect("hash_a missing");
        let pos_b = rendered.find(&hex::encode(hash_b)).expect("hash_b missing");
        // Both hashes must appear; they should appear in byte-lex order.
        if hash_a <= hash_b {
            assert!(
                pos_a < pos_b,
                "expected byte-lex order: hash_a={} appears at {pos_a}, hash_b={} at {pos_b}: {rendered}",
                hex::encode(hash_a),
                hex::encode(hash_b)
            );
        } else {
            assert!(
                pos_b < pos_a,
                "expected byte-lex order: hash_b={} appears at {pos_b}, hash_a={} at {pos_a}: {rendered}",
                hex::encode(hash_b),
                hex::encode(hash_a)
            );
        }
    }

    #[test]
    fn dumptofile_alonzo_witness_set_renders_native_script_entry() {
        let ns = yggdrasil_ledger::NativeScript::ScriptPubkey([0x11; 28]);
        let expected_hash = hex::encode(yggdrasil_ledger::native_script_hash(&ns));
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![ns.clone()],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let rendered = show_alonzo_witness_set(&ws).expect("native witness set");
        assert!(
            rendered.contains(&format!(
                "(ScriptHash \"{expected_hash}\",NativeScript MkTimelock"
            )),
            "expected native-script witness entry in {rendered}"
        );
        assert!(
            rendered.contains("TimelockSignature (KeyHash {unKeyHash = \""),
            "expected TimelockSignature inner in {rendered}"
        );
    }

    #[test]
    fn dumptofile_babbage_script_ref_renders_snothing_and_plutus_versions() {
        use yggdrasil_ledger::Script;

        assert_eq!(
            show_babbage_script_ref(None).expect("no script ref"),
            "SNothing"
        );

        // PlutusV1: hash domain is Blake2b-224 over [0x01, <script bytes>].
        let v1 = ScriptRef(Script::PlutusV1(vec![0xAA, 0xBB]));
        let v1_rendered = show_babbage_script_ref(Some(&v1)).expect("v1 script ref");
        assert!(
            v1_rendered.starts_with("SJust PlutusScript PlutusV1 ScriptHash \""),
            "v1 rendering unexpected: {v1_rendered}"
        );
        assert!(
            v1_rendered.ends_with("\""),
            "v1 trailing quote: {v1_rendered}"
        );

        let v2 = ScriptRef(Script::PlutusV2(vec![0xCA, 0xFE, 0xBA, 0xBE]));
        let v2_rendered = show_babbage_script_ref(Some(&v2)).expect("v2 script ref");
        assert!(
            v2_rendered.starts_with("SJust PlutusScript PlutusV2 ScriptHash \""),
            "v2 rendering unexpected: {v2_rendered}"
        );

        let v3 = ScriptRef(Script::PlutusV3(vec![0x12, 0x34]));
        let v3_rendered = show_babbage_script_ref(Some(&v3)).expect("v3 script ref");
        assert!(
            v3_rendered.starts_with("SJust PlutusScript PlutusV3 ScriptHash \""),
            "v3 rendering unexpected: {v3_rendered}"
        );

        // Same script bytes under different language tags must hash to
        // different values (because the prefix differs).
        let same_v1 = ScriptRef(Script::PlutusV1(vec![0x12, 0x34]));
        let same_v3 = ScriptRef(Script::PlutusV3(vec![0x12, 0x34]));
        let r1 = show_babbage_script_ref(Some(&same_v1)).expect("same v1");
        let r3 = show_babbage_script_ref(Some(&same_v3)).expect("same v3");
        let h1 = r1
            .split("ScriptHash \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("v1 hash");
        let h3 = r3
            .split("ScriptHash \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("v3 hash");
        assert_ne!(
            h1, h3,
            "PlutusV1 and PlutusV3 with identical bytes must hash differently"
        );
    }

    #[test]
    fn dumptofile_babbage_script_ref_renders_native_script() {
        use yggdrasil_ledger::{NativeScript, Script};

        let ns = NativeScript::ScriptPubkey([0xAB; 28]);
        let sr = ScriptRef(Script::Native(ns.clone()));
        let rendered = show_babbage_script_ref(Some(&sr)).expect("native reference script");
        assert!(
            rendered.starts_with("SJust NativeScript MkTimelock TimelockSignature (KeyHash {"),
            "unexpected native ref script rendering: {rendered}"
        );
        assert!(
            rendered.contains("(blake2b_256: SafeHash \""),
            "expected blake2b_256 SafeHash inside MemoBytes show: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_bootstrap_witness_record_form() {
        let bw = yggdrasil_ledger::BootstrapWitness {
            public_key: [0x11; 32],
            signature: [0x22; 64],
            chain_code: [0x33; 32],
            attributes: vec![0xAA, 0xBB],
        };
        let rendered = show_bootstrap_witness(&bw);
        assert!(rendered.starts_with("BootstrapWitness {bwKey = VKey (VerKeyEd25519DSIGN \""));
        assert!(rendered.contains("bwSignature = SignedDSIGN (SigEd25519DSIGN \""));
        assert!(rendered.contains("bwChainCode = ChainCode \""));
        assert!(rendered.contains("bwAttributes = \"\\170\\187\""));
    }

    #[test]
    fn dumptofile_show_alonzo_bootstrap_witnesses_empty_and_full() {
        // Empty path keeps the prior `fromList []` shape.
        assert_eq!(show_alonzo_bootstrap_witnesses(&[]), "fromList []");

        let bw_a = yggdrasil_ledger::BootstrapWitness {
            public_key: [0x00; 32],
            signature: [0x00; 64],
            chain_code: [0x00; 32],
            attributes: vec![],
        };
        let bw_b = yggdrasil_ledger::BootstrapWitness {
            public_key: [0xFF; 32],
            signature: [0x00; 64],
            chain_code: [0x00; 32],
            attributes: vec![],
        };
        // Pass in reverse order to confirm sort. Upstream `Ord
        // BootstrapWitness = comparing bootstrapWitKeyHash`, so the
        // post-sort order is whichever witness has the smaller
        // Blake2b-224 over SHA3-256 over (prefix ++ key ++ chain_code ++
        // attributes).
        let rendered = show_alonzo_bootstrap_witnesses(&[bw_b.clone(), bw_a.clone()]);
        assert!(rendered.starts_with("fromList [BootstrapWitness {bwKey = VKey"));
        let hash_a =
            bootstrap_witness_key_hash(&bw_a.public_key, &bw_a.chain_code, &bw_a.attributes);
        let hash_b =
            bootstrap_witness_key_hash(&bw_b.public_key, &bw_b.chain_code, &bw_b.attributes);
        let zero_pos = rendered
            .find("VerKeyEd25519DSIGN \"00000000")
            .expect("zero public_key");
        let ff_pos = rendered
            .find("VerKeyEd25519DSIGN \"ffffffff")
            .expect("ff public_key");
        if hash_a <= hash_b {
            assert!(zero_pos < ff_pos, "expected hash sort: a < b in {rendered}");
        } else {
            assert!(ff_pos < zero_pos, "expected hash sort: b < a in {rendered}");
        }
    }

    #[test]
    fn dumptofile_bootstrap_witness_key_hash_matches_upstream_domain() {
        // Verify the hash domain directly:
        // Blake2b-224 (SHA3-256 ([0x83,0x00,0x82,0x00,0x58,0x40] ++ key ++ cc ++ attrs))
        let key = [0x42_u8; 32];
        let cc = [0x99_u8; 32];
        let attrs: Vec<u8> = vec![0xAA, 0xBB];
        let mut expected_input = vec![0x83_u8, 0x00, 0x82, 0x00, 0x58, 0x40];
        expected_input.extend_from_slice(&key);
        expected_input.extend_from_slice(&cc);
        expected_input.extend_from_slice(&attrs);
        let expected_sha3 = yggdrasil_crypto::sha3_256(&expected_input).0;
        let expected = yggdrasil_crypto::hash_bytes_224(&expected_sha3).0;
        assert_eq!(bootstrap_witness_key_hash(&key, &cc, &attrs), expected);
    }

    #[test]
    fn dumptofile_alonzo_witness_set_renders_bootstrap_witness_entry() {
        let bw = yggdrasil_ledger::BootstrapWitness {
            public_key: [0x44; 32],
            signature: [0x55; 64],
            chain_code: [0x66; 32],
            attributes: vec![],
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![bw],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let rendered = show_alonzo_witness_set(&ws).expect("bootstrap witness set");
        assert!(
            rendered.contains(
                "atwrBootAddrTxWits = fromList [BootstrapWitness {bwKey = VKey (VerKeyEd25519DSIGN \""
            ),
            "expected non-empty bootstrap witness in witness-set rendering: {rendered}"
        );
    }

    #[test]
    fn dumptofile_show_native_script_variants() {
        use yggdrasil_ledger::NativeScript;

        let sig = NativeScript::ScriptPubkey([0xCC; 28]);
        let sig_rendered = show_native_script(&sig);
        assert!(sig_rendered.starts_with("MkTimelock TimelockSignature (KeyHash {unKeyHash = \""));
        assert!(sig_rendered.ends_with("\")"));

        let all = NativeScript::ScriptAll(vec![sig.clone(), sig.clone()]);
        let all_rendered = show_native_script(&all);
        assert!(all_rendered.starts_with(
            "MkTimelock TimelockAllOf (StrictSeq {fromStrict = fromList [MkTimelock TimelockSignature"
        ));

        let any = NativeScript::ScriptAny(vec![sig.clone()]);
        let any_rendered = show_native_script(&any);
        assert!(
            any_rendered.contains("TimelockAnyOf (StrictSeq {fromStrict = fromList [MkTimelock")
        );

        let nofk = NativeScript::ScriptNOfK(2, vec![sig.clone(), sig.clone(), sig]);
        let nofk_rendered = show_native_script(&nofk);
        assert!(
            nofk_rendered.contains("TimelockMOf 2 (StrictSeq {fromStrict = fromList [MkTimelock")
        );

        let before = NativeScript::InvalidBefore(123);
        assert!(show_native_script(&before).contains("TimelockTimeStart (SlotNo 123)"));

        let after = NativeScript::InvalidHereafter(456);
        assert!(show_native_script(&after).contains("TimelockTimeExpire (SlotNo 456)"));
    }

    #[test]
    fn dumptofile_plutus_script_hash_matches_language_prefix_domain() {
        let bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let h1 = plutus_script_hash(1, &bytes);
        // Verify hash domain: prepend tag, hash 28-byte Blake2b.
        let mut expected_input = vec![0x01_u8];
        expected_input.extend_from_slice(&bytes);
        assert_eq!(
            h1,
            yggdrasil_crypto::hash_bytes_224(&expected_input).0,
            "plutus_script_hash must Blake2b-224 over [lang_tag, ...script_bytes]"
        );
    }

    #[test]
    fn dumptofile_babbage_datum_renders_no_datum_and_hash() {
        assert_eq!(show_babbage_datum(None).expect("no datum"), "NoDatum");
        let hash = [0x11_u8; 32];
        let rendered = show_babbage_datum(Some(&DatumOption::Hash(hash))).expect("datum hash");
        assert_eq!(
            rendered,
            "DatumHash (SafeHash \"1111111111111111111111111111111111111111111111111111111111111111\")"
        );
    }

    #[test]
    fn dumptofile_babbage_datum_renders_inline_simple_integer() {
        // PlutusData::Integer(42) encodes to a single CBOR unsigned-int byte
        // 0x18 0x2a → bytestring shows as `\24*` (0x18 = decimal 24 → `\24`;
        // 0x2a = ASCII `*`). The `\&` separator is not needed because `*`
        // is not a digit.
        let pd = PlutusData::integer(42);
        let rendered =
            show_babbage_datum(Some(&DatumOption::Inline(pd.clone()))).expect("inline datum");
        let expected_inner = show_haskell_bytestring(&pd.to_cbor_bytes());
        assert_eq!(rendered, format!("Datum (BinaryData {expected_inner})"));
        // Sanity: must start with `Datum (BinaryData "` and end with `")`.
        assert!(rendered.starts_with("Datum (BinaryData \""));
        assert!(rendered.ends_with("\")"));
    }

    #[test]
    fn dumptofile_babbage_datum_renders_inline_constr_with_bytes() {
        // Nested PlutusData inside an inline datum: Constr 0 [B "abc"]
        let inner = PlutusData::Bytes(b"abc".to_vec());
        let pd = PlutusData::Constr(0, vec![inner]);
        let rendered = show_babbage_datum(Some(&DatumOption::Inline(pd.clone())))
            .expect("inline constr datum");
        let expected_inner = show_haskell_bytestring(&pd.to_cbor_bytes());
        assert_eq!(rendered, format!("Datum (BinaryData {expected_inner})"));
    }

    #[test]
    fn dumptofile_alonzo_tx_dats_render_single_datum() {
        let datum = PlutusData::integer(42);
        let rendered = show_alonzo_tx_dats(std::slice::from_ref(&datum));
        assert!(
            rendered.starts_with("MkTxDats (TxDatsRaw {unTxDatsRaw = fromList [("),
            "unexpected TxDats envelope: {rendered}"
        );
        assert!(
            rendered.contains(",MkData I 42 (blake2b_256: SafeHash \""),
            "expected inner MkData with PlutusData show: {rendered}"
        );
    }

    #[test]
    fn dumptofile_mary_value_sorts_assets_within_policy_by_byte_lex_order() {
        let mut inner = BTreeMap::new();
        inner.insert(vec![0xFF, 0xFF], 2);
        inner.insert(vec![0x00, 0x00], 1);
        let mut ma = BTreeMap::new();
        ma.insert([0x55; 28], inner);

        let rendered = show_mary_value(&yggdrasil_ledger::Value::CoinAndAssets(0, ma))
            .expect("multi-asset-per-policy value");

        let lo = rendered.find("(\"0000\",1)").expect("low asset rendered");
        let hi = rendered.find("(\"ffff\",2)").expect("high asset rendered");
        assert!(
            lo < hi,
            "assets within a policy must be rendered in ascending byte-lex order: rendered={rendered}"
        );
    }

    #[test]
    fn dumptofile_submit_generates_alonzo_haskell_show_transaction() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Alonzo,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-alonzo-dump-{}.out",
            std::process::id()
        ));
        let _ = fs::remove_file(&output_path);
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Alonzo,
            &SubmitMode::DumpToFile(output_path.clone()),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("alonzo dump submit");

        let rendered = fs::read_to_string(&output_path).expect("alonzo dump output");
        let _ = fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraAlonzo (AlonzoTx {atBody = MkAlonzoTxBody AlonzoTxBodyRaw"
        ));
        assert!(rendered.contains("atbrCollateral = fromList []"));
        assert!(rendered.contains("atbrScriptIntegrityHash = SNothing"));
        assert!(rendered.contains("atWits = AlonzoTxWitsRaw"));
        assert!(rendered.contains("atwrDatsTxWits = MkTxDats"));
        assert!(rendered.contains("atwrRdmrsTxWits = MkRedeemers"));
        assert!(rendered.contains("atIsValid = IsValid True"));
        assert_eq!(rendered.lines().filter(|line| !line.is_empty()).count(), 1);
    }

    #[test]
    fn dumptofile_submit_generates_babbage_haskell_show_transaction() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Babbage,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-babbage-dump-{}.out",
            std::process::id()
        ));
        let _ = fs::remove_file(&output_path);
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Babbage,
            &SubmitMode::DumpToFile(output_path.clone()),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("babbage dump submit");

        let rendered = fs::read_to_string(&output_path).expect("babbage dump output");
        let _ = fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraBabbage (AlonzoTx {atBody = MkBabbageTxBody BabbageTxBodyRaw"
        ));
        assert!(rendered.contains("btbrCollateralInputs = fromList []"));
        assert!(rendered.contains("btbrReferenceInputs = fromList []"));
        assert!(rendered.contains("btbrCollateralReturn = SNothing"));
        assert!(rendered.contains("btbrTotalCollateral = SNothing"));
        assert!(rendered.contains("btbrScriptIntegrityHash = SNothing"));
        assert!(rendered.contains("btbrNetworkId = SNothing"));
        assert!(rendered.contains("btbrMint = MultiAsset (fromList [])"));
        assert!(rendered.contains("Sized {sizedValue = ("));
        assert!(rendered.contains(",NoDatum,SNothing), sizedSize = "));
        assert!(rendered.contains("atWits = AlonzoTxWitsRaw"));
        assert!(rendered.contains("atwrDatsTxWits = MkTxDats"));
        assert!(rendered.contains("atwrRdmrsTxWits = MkRedeemers"));
        assert!(rendered.contains("atIsValid = IsValid True"));
        assert_eq!(rendered.lines().filter(|line| !line.is_empty()).count(), 1);
    }

    #[test]
    fn dumptofile_submit_generates_conway_haskell_show_transaction() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-conway-dump-{}.out",
            std::process::id()
        ));
        let _ = fs::remove_file(&output_path);
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DumpToFile(output_path.clone()),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("conway dump submit");

        let rendered = fs::read_to_string(&output_path).expect("conway dump output");
        let _ = fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraConway (AlonzoTx {atBody = MkConwayTxBody ConwayTxBodyRaw"
        ));
        assert!(rendered.contains("ctbrSpendInputs = fromList ["));
        assert!(rendered.contains("ctbrCollateralInputs = fromList []"));
        assert!(rendered.contains("ctbrReferenceInputs = fromList []"));
        assert!(rendered.contains("ctbrCollateralReturn = SNothing"));
        assert!(rendered.contains("ctbrTotalCollateral = SNothing"));
        assert!(rendered.contains(
            "ctbrCerts = OSet {osSSeq = StrictSeq {fromStrict = fromList []}, osSet = fromList []}"
        ));
        assert!(rendered.contains("ctbrVldt = ValidityInterval"));
        assert!(rendered.contains(
            "ctbrVotingProcedures = VotingProcedures {unVotingProcedures = fromList []}"
        ));
        assert!(rendered.contains("ctbrProposalProcedures = OSet"));
        assert!(rendered.contains("ctbrCurrentTreasuryValue = SNothing"));
        assert!(rendered.contains("ctbrTreasuryDonation = Coin 0"));
        assert!(rendered.contains("Sized {sizedValue = ("));
        assert!(rendered.contains(",NoDatum,SNothing), sizedSize = "));
        assert!(rendered.contains("atWits = AlonzoTxWitsRaw"));
        assert!(rendered.contains("atIsValid = IsValid True"));
        assert_eq!(rendered.lines().filter(|line| !line.is_empty()).count(), 1);
    }

    #[test]
    fn dumptofile_submit_generates_shelley_haskell_show_transaction() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Shelley,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-shelley-dump-{}.out",
            std::process::id()
        ));
        let _ = fs::remove_file(&output_path);
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Shelley,
            &SubmitMode::DumpToFile(output_path.clone()),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("shelley dump submit");

        let rendered = fs::read_to_string(&output_path).expect("shelley dump output");
        let _ = fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraShelley (ShelleyTx {stBody = MkShelleyTxBody ShelleyTxBodyRaw"
        ));
        assert!(rendered.contains("stbrFee = Coin 10"));
        assert!(rendered.contains("stbrTtl = SlotNo 18446744073709551615"));
        assert!(rendered.contains("stWits = ShelleyTxWitsRaw"));
        assert_eq!(rendered.lines().filter(|line| !line.is_empty()).count(), 1);
    }

    #[test]
    fn discard_submit_spends_script_fund_and_updates_destination_wallet() {
        let _cleanup = BudgetSummaryFileCleanup;
        let _ = fs::remove_file(PLUTUS_BUDGET_SUMMARY_FILE);
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        init_wallet(&mut env, "collateral").expect("collateral wallet");
        get_env_wallets_mut(&mut env, "source")
            .expect("source wallet")
            .insert_fund(Fund::script_fund(
                AnyCardanoEra::Conway,
                format!("{INPUT_TX_ID}#0"),
                100,
                FundWitness::ScriptWitness(ScriptWitnessForSpending {
                    language: "PlutusV2".to_string(),
                    script_bytes: vec![1, 2, 3],
                    datum: PlutusData::integer(0),
                    redeemer: PlutusData::integer(1),
                    execution_units: ExecutionUnits {
                        execution_steps: 1,
                        execution_memory: 1,
                    },
                }),
            ));
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "collateral",
            "100102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f#0",
            1_000,
            "key",
        )
        .expect("collateral fund");
        let generator = Generator::NtoM(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
            1,
            None,
            Some("collateral".to_string()),
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("script spend submit");

        assert!(
            get_env_wallets(&env, "source")
                .expect("source")
                .funds()
                .is_empty()
        );
        let dest_funds = get_env_wallets(&env, "dest").expect("dest").funds();
        assert_eq!(dest_funds.len(), 1);
        assert_eq!(dest_funds[0].lovelace, 90);
        assert_eq!(
            get_env_wallets(&env, "collateral")
                .expect("collateral")
                .funds()
                .len(),
            1
        );
    }

    #[test]
    fn secure_genesis_submit_generates_initial_fund_and_updates_wallet() {
        let mut env = Env::empty_env();
        let network_id = NetworkId::Testnet(42);
        let genesis_key = genesis_signing_key(7);
        let pay_key = signing_key(9);
        set_env_network_id(&mut env, network_id.clone());
        set_env_genesis(
            &mut env,
            GenesisHandle {
                config_file: PathBuf::from("node-config.json"),
                shelley_genesis_file: Some(PathBuf::from("shelley-genesis.json")),
                shelley_genesis_hash: Some("00".repeat(32)),
                network_magic: 42,
                initial_funds: vec![genesis_initial_fund(&network_id, &genesis_key, 2_000_000)],
            },
        );
        init_wallet(&mut env, "dest").expect("dest wallet");
        define_signing_key(&mut env, "GenesisInputFund", genesis_key);
        define_signing_key(&mut env, "TxGenFunds", pay_key);
        seed_static_plutus_protocol_parameters(&mut env);

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &Generator::SecureGenesis(
                "dest".to_string(),
                "GenesisInputFund".to_string(),
                "TxGenFunds".to_string(),
            ),
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 77,
            },
        )
        .expect("secure genesis submit");

        let dest_funds = get_env_wallets(&env, "dest").expect("dest").funds();
        assert_eq!(dest_funds.len(), 1);
        assert_eq!(dest_funds[0].lovelace, 1_999_990);
        assert_eq!(dest_funds[0].key_name, "TxGenFunds");
    }

    #[test]
    fn split_submit_stores_change_and_payments_in_their_target_wallets() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            1_000,
            "key",
        )
        .expect("source fund");
        let generator = Generator::Split(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            PayMode::PayToAddr("key".to_string(), "source".to_string()),
            vec![100, 200],
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("split submit");

        let source_funds = get_env_wallets(&env, "source").expect("source").funds();
        let dest_funds = get_env_wallets(&env, "dest").expect("dest").funds();
        assert_eq!(source_funds.len(), 1);
        assert_eq!(source_funds[0].lovelace, 690);
        assert_eq!(
            dest_funds
                .iter()
                .map(|fund| fund.lovelace)
                .collect::<Vec<_>>(),
            vec![100, 200]
        );
    }

    #[test]
    fn take_cycle_generates_the_requested_number_of_transactions() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            1_000,
            "key",
        )
        .expect("source fund");
        let generator = Generator::Take(
            3,
            Box::new(Generator::Cycle(Box::new(Generator::SplitN(
                "source".to_string(),
                PayMode::PayToAddr("key".to_string(), "source".to_string()),
                1,
            )))),
        );

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("cycle submit");

        let source_funds = get_env_wallets(&env, "source").expect("source").funds();
        assert_eq!(source_funds.len(), 1);
        assert_eq!(source_funds[0].lovelace, 970);
    }

    #[test]
    fn benchmark_submit_stores_async_control_and_waits_for_summary() {
        let network_magic = 42;
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        std_listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = std_listener.local_addr().expect("local addr");
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .expect("server runtime");
            runtime.block_on(async move {
                let listener = TcpListener::from_std(std_listener).expect("tokio listener");
                let (stream, _) = listener.accept().await.expect("accept");
                let mut connection = peer_accept(stream, network_magic, &[HandshakeVersion::V14])
                    .await
                    .expect("peer accept");
                let handle = connection
                    .protocols
                    .remove(&MiniProtocolNum::TX_SUBMISSION)
                    .expect("tx submission handle");
                let mut server = TxSubmissionServer::new(handle);
                server.recv_init().await.expect("init");
                let txids = match server
                    .request_tx_ids(true, 0, 1)
                    .await
                    .expect("request ids")
                {
                    ServerTxIdsReply::TxIds(txids) => txids,
                    ServerTxIdsReply::Done => panic!("first request should advertise txids"),
                };
                assert_eq!(txids.len(), 1);
                let submitted = server
                    .request_txs(txids.iter().map(|tx| tx.txid).collect())
                    .await
                    .expect("request txs");
                match server
                    .request_tx_ids(true, 1, 1)
                    .await
                    .expect("request done")
                {
                    ServerTxIdsReply::Done => {}
                    ServerTxIdsReply::TxIds(txids) => {
                        panic!("final blocking request should end protocol, got {txids:?}")
                    }
                }
                submitted
            })
        });

        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        add_fund(
            &mut env,
            AnyCardanoEra::Conway,
            "source",
            &format!("{INPUT_TX_ID}#0"),
            100,
            "key",
        )
        .expect("source fund");
        let generator = Generator::SplitN(
            "source".to_string(),
            PayMode::PayToAddr("key".to_string(), "dest".to_string()),
            1,
        );
        let target = NodeDescription {
            addr: "127.0.0.1".to_string(),
            port: addr.port(),
            name: "loopback".to_string(),
        };

        submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::Benchmark(vec![target], 100_000.0, 1),
            &generator,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect("benchmark submit");
        assert!(crate::script::env::get_env_threads(&env).is_some());

        wait_benchmark(&mut env).expect("wait benchmark");

        let submitted = server.join().expect("server thread");
        assert_eq!(submitted.len(), 1);
        let control = crate::script::env::get_env_threads(&env).expect("control");
        let summary = control.summary().expect("summary");
        assert_eq!(summary.ss_tx_sent.get(), 1);
        assert_eq!(summary.ss_tx_unavailable.get(), 0);
        assert!(summary.ss_failures.is_empty());
        assert!(
            env.bench_tracers
                .as_ref()
                .expect("tracers")
                .messages()
                .iter()
                .any(|message| message.starts_with("TraceBenchTxSubSummary "))
        );
    }

    #[test]
    fn round_robin_matches_upstream_unimplemented_error() {
        let mut env = Env::empty_env();
        seed_static_plutus_protocol_parameters(&mut env);

        let err = submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &Generator::RoundRobin(Vec::new()),
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect_err("upstream TODO error");

        assert_eq!(
            err,
            Error::TxGenError("return $ foldr1 Streaming.interleaves gList".to_string())
        );
    }

    #[test]
    fn one_of_matches_upstream_unimplemented_error() {
        let mut env = Env::empty_env();
        seed_static_plutus_protocol_parameters(&mut env);

        let err = submit_in_era(
            &mut env,
            AnyCardanoEra::Conway,
            &SubmitMode::DiscardTx,
            &Generator::OneOf(Vec::new()),
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 1,
            },
        )
        .expect_err("upstream TODO error");

        assert_eq!(
            err,
            Error::TxGenError("todo: implement Quickcheck style oneOf generator".to_string())
        );
    }

    #[test]
    fn interpret_pay_mode_builds_key_output_builder_and_address_trace_value() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        let pay_mode = PayMode::PayToAddr("key".to_string(), "dest".to_string());

        let interpreted =
            interpret_pay_mode(&mut env, AnyCardanoEra::Conway, &pay_mode).expect("pay mode");

        assert_eq!(interpreted.to_utxo.era(), AnyCardanoEra::Conway);
        assert_eq!(interpreted.to_utxo.key_name(), Some("key"));
        assert_eq!(interpreted.destination_wallet, "dest");
        assert_eq!(interpreted.address_hex.len(), 58);
        assert!(interpreted.address_hex.starts_with("60"));
    }

    #[test]
    fn interpret_pay_mode_builds_static_script_output_builder_and_witness() {
        let mut env = Env::empty_env();
        seed_pay_to_addr_env(&mut env);
        seed_static_plutus_protocol_parameters(&mut env);
        let pay_mode = PayMode::PayToScript(
            ScriptSpec {
                script_spec_file: PlutusScriptRef::Named("Loop".to_string()),
                script_spec_budget: ScriptBudget::StaticScriptBudget(
                    PathBuf::new(),
                    PathBuf::new(),
                    ExecutionUnits {
                        execution_steps: 10,
                        execution_memory: 20,
                    },
                    false,
                ),
                script_spec_plutus_type: TxGenPlutusType::CustomScript,
            },
            "dest".to_string(),
        );

        let interpreted =
            interpret_pay_mode(&mut env, AnyCardanoEra::Alonzo, &pay_mode).expect("pay mode");

        assert_eq!(interpreted.destination_wallet, "dest");
        assert_eq!(interpreted.to_utxo.key_name(), None);
        assert_eq!(interpreted.address_hex.len(), 58);
        assert!(interpreted.address_hex.starts_with("70"));

        let (output, pending) = interpreted.to_utxo.build(2_000_000).expect("output");
        let datum = PlutusData::integer(0);
        assert_eq!(output.datum_hash(), Some(script_data_hash(&datum)));
        let fund = pending.fund_for_tx_id(0, "00");
        match get_fund_witness(AnyCardanoEra::Alonzo, &fund).expect("witness") {
            FundWitness::ScriptWitness(witness) => {
                assert_eq!(witness.language, "PlutusV1");
                assert_eq!(witness.datum, datum);
                assert_eq!(witness.redeemer, PlutusData::integer(0));
                assert_eq!(
                    witness.execution_units,
                    ExecutionUnits {
                        execution_steps: 10,
                        execution_memory: 20,
                    }
                );
            }
            FundWitness::KeyWitnessForSpending => panic!("expected script witness"),
        }
    }

    #[test]
    fn make_plutus_context_with_check_rejects_mismatched_pre_execution_units() {
        let mut env = Env::empty_env();
        seed_real_plutus_protocol_parameters(&mut env);
        let script_path = write_temp_v1_plutus_script("pre-execute-mismatch", THREE_ARG_UNIT_FLAT);
        let script_spec = ScriptSpec {
            script_spec_file: PlutusScriptRef::File(script_path.clone()),
            script_spec_budget: ScriptBudget::StaticScriptBudget(
                PathBuf::new(),
                PathBuf::new(),
                ExecutionUnits {
                    execution_steps: 1,
                    execution_memory: 1,
                },
                true,
            ),
            script_spec_plutus_type: TxGenPlutusType::CustomScript,
        };

        let err = make_plutus_context(&mut env, AnyCardanoEra::Alonzo, &script_spec)
            .expect_err("withCheck mismatch");

        let _ = fs::remove_file(script_path);
        match err {
            Error::WalletError(message) => {
                assert!(
                    message.contains("Stated execution Units do not match result of pre execution")
                );
                assert!(message.contains("PreExecution result"));
            }
            other => panic!("expected WalletError, got {other:?}"),
        }
    }

    #[test]
    fn interpret_pay_mode_requires_network_before_key_output_builder() {
        let mut env = Env::empty_env();
        init_wallet(&mut env, "dest").expect("dest wallet");
        define_signing_key(&mut env, "key", signing_key(7));
        let pay_mode = PayMode::PayToAddr("key".to_string(), "dest".to_string());

        assert_eq!(
            interpret_pay_mode(&mut env, AnyCardanoEra::Conway, &pay_mode),
            Err(Error::UserError("Unset Genesis".to_string()))
        );
    }

    #[test]
    fn select_collateral_funds_matches_empty_and_era_boundaries() {
        let mut env = Env::empty_env();
        init_wallet(&mut env, "collateral").expect("collateral wallet");

        assert_eq!(
            select_collateral_funds(&env, AnyCardanoEra::Conway, Some("collateral")),
            Err(Error::WalletError(
                "selectCollateralFunds: emptylist".to_string()
            ))
        );

        define_signing_key(&mut env, "key", signing_key(3));
        add_fund(
            &mut env,
            AnyCardanoEra::Shelley,
            "collateral",
            "abc#0",
            5,
            "key",
        )
        .expect("collateral fund");

        assert_eq!(
            select_collateral_funds(&env, AnyCardanoEra::Shelley, Some("collateral")),
            Err(Error::WalletError(
                "selectCollateralFunds: collateral: era not supported :Shelley".to_string()
            ))
        );

        let selected = select_collateral_funds(&env, AnyCardanoEra::Alonzo, Some("collateral"))
            .expect("collateral");
        assert_eq!(selected.tx_ins, vec!["abc#0".to_string()]);
        assert_eq!(selected.funds.len(), 1);
        assert_eq!(
            select_collateral_funds(&env, AnyCardanoEra::Conway, None).expect("none"),
            SelectedCollateral {
                tx_ins: Vec::new(),
                funds: Vec::new()
            }
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
            wait_benchmark(&mut env),
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
