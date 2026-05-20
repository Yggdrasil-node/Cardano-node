//! Core transaction-generator script operations.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`.
//! Ports the state/query/runtime helper boundary consumed by
//! `Cardano.Benchmarking.Script.Action.action`. This slice owns the
//! deterministic state-only operations, Plutus context construction,
//! finite transaction-stream evaluation, LocalSocket submission,
//! Benchmark submission control, Shelley/Mary-family `DumpToFile`
//! rendering, and budget-summary projection. The remaining Alonzo-family
//! `DumpToFile` era/witness shapes still return explicit `TxGenError`
//! boundaries until their downstream mirrors land.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use num_bigint::BigInt;
use serde_json::Value;
use yggdrasil_crypto::{hash_bytes_224, hash_bytes_256};
use yggdrasil_ledger::{
    Address, AllegraTxBody, CborDecode, CborEncode, Decoder, Encoder, MaryTxBody, MaryTxOut,
    PlutusData, ProtocolParameters, ShelleyCompatibleSubmittedTx, ShelleyTxBody, ShelleyTxIn,
    ShelleyTxOut, ShelleyVkeyWitness, ShelleyWitnessSet, StakeCredential, eras::alonzo::ExUnits,
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
        tx => Err(lift_tx_gen_error(format!(
            "DumpToFile: upstream Show(Tx) renderer is implemented for Shelley/Mary-family key-witnessed transactions only; got {:?}",
            tx.era()
        ))),
    }
}

fn show_shelley_tx_for_dump(
    tx: &ShelleyCompatibleSubmittedTx<ShelleyTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Shelley", "stbrCerts")?;
    ensure_empty_or_absent_btree(tx.body.withdrawals.as_ref(), "Shelley", "stbrWithdrawals")?;
    ensure_absent(tx.body.update.as_ref(), "Shelley", "stbrUpdate")?;
    ensure_absent(
        tx.body.auxiliary_data_hash.as_ref(),
        "Shelley",
        "stbrAuxDataHash",
    )?;
    ensure_absent(tx.auxiliary_data.as_ref(), "Shelley", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_shelley_tx_out_list(&tx.body.outputs, "Shelley")?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Shelley")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraShelley (ShelleyTx {{stBody = MkShelleyTxBody ShelleyTxBodyRaw {{stbrInputs = fromList [{inputs}], stbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, stbrCerts = StrictSeq {{fromStrict = fromList []}}, stbrWithdrawals = Withdrawals {{unWithdrawals = fromList []}}, stbrFee = Coin {}, stbrTtl = SlotNo {}, stbrUpdate = SNothing, stbrAuxDataHash = SNothing}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee, tx.body.ttl,
    ))
}

fn show_allegra_tx_for_dump(
    tx: &ShelleyCompatibleSubmittedTx<AllegraTxBody>,
) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Allegra", "atbrCerts")?;
    ensure_empty_or_absent_btree(tx.body.withdrawals.as_ref(), "Allegra", "atbrWithdrawals")?;
    ensure_absent(tx.body.update.as_ref(), "Allegra", "atbrUpdate")?;
    ensure_absent(
        tx.body.auxiliary_data_hash.as_ref(),
        "Allegra",
        "atbrAuxDataHash",
    )?;
    ensure_absent(tx.auxiliary_data.as_ref(), "Allegra", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_shelley_tx_out_list(&tx.body.outputs, "Allegra")?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Allegra")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraAllegra (ShelleyTx {{stBody = MkAllegraTxBody AllegraTxBodyRaw {{atbrInputs = fromList [{inputs}], atbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, atbrCerts = StrictSeq {{fromStrict = fromList []}}, atbrWithdrawals = Withdrawals {{unWithdrawals = fromList []}}, atbrFee = Coin {}, atbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, atbrUpdate = SNothing, atbrAuxDataHash = SNothing, atbrMint = ()}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
}

fn show_mary_tx_for_dump(tx: &ShelleyCompatibleSubmittedTx<MaryTxBody>) -> Result<String, Error> {
    ensure_empty_or_absent(tx.body.certificates.as_deref(), "Mary", "atbrCerts")?;
    ensure_empty_or_absent_btree(tx.body.withdrawals.as_ref(), "Mary", "atbrWithdrawals")?;
    ensure_absent(tx.body.update.as_ref(), "Mary", "atbrUpdate")?;
    ensure_absent(
        tx.body.auxiliary_data_hash.as_ref(),
        "Mary",
        "atbrAuxDataHash",
    )?;
    ensure_empty_mint(tx.body.mint.as_ref(), "Mary", "atbrMint")?;
    ensure_absent(tx.auxiliary_data.as_ref(), "Mary", "stAuxData")?;

    let inputs = show_tx_in_list(&tx.body.inputs);
    let outputs = show_mary_tx_out_list(&tx.body.outputs)?;
    let body_hash = hex::encode(hash_bytes_256(tx.raw_body()).0);
    let witnesses = show_shelley_witness_set(&tx.witness_set, "Mary")?;

    Ok(format!(
        "\nShelleyTx ShelleyBasedEraMary (ShelleyTx {{stBody = MkMaryTxBody AllegraTxBodyRaw {{atbrInputs = fromList [{inputs}], atbrOutputs = StrictSeq {{fromStrict = fromList [{outputs}]}}, atbrCerts = StrictSeq {{fromStrict = fromList []}}, atbrWithdrawals = Withdrawals {{unWithdrawals = fromList []}}, atbrFee = Coin {}, atbrValidityInterval = ValidityInterval {{invalidBefore = {}, invalidHereafter = {}}}, atbrUpdate = SNothing, atbrAuxDataHash = SNothing, atbrMint = MultiAsset (fromList [])}} (blake2b_256: SafeHash \"{body_hash}\"), stWits = {witnesses}, stAuxData = SNothing}})",
        tx.body.fee,
        show_strict_maybe_slot(tx.body.validity_interval_start),
        show_strict_maybe_slot(tx.body.ttl),
    ))
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

fn ensure_empty_or_absent_btree<K, V>(
    value: Option<&BTreeMap<K, V>>,
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

fn show_mary_value(value: &yggdrasil_ledger::Value) -> Result<String, Error> {
    match value {
        yggdrasil_ledger::Value::Coin(coin) => Ok(format!(
            "MaryValue (Coin {coin}) (MultiAsset (fromList []))"
        )),
        yggdrasil_ledger::Value::CoinAndAssets(coin, assets) if assets.is_empty() => Ok(format!(
            "MaryValue (Coin {coin}) (MultiAsset (fromList []))"
        )),
        yggdrasil_ledger::Value::CoinAndAssets(_, _) => Err(lift_tx_gen_error(
            "DumpToFile: Mary Show(Tx) renderer does not yet support multi-asset values",
        )),
    }
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
