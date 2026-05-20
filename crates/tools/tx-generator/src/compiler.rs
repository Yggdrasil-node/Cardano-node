//! High-level option compiler for `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Compiler.hs`.
//! Ports `compileOptions`, `compileToScript`, split planning, fee
//! magic, fixed signing-key names, and high-level generator assembly.
//! The produced `Action` script is executable once the later
//! `Script` / `GeneratorTx` runtime slices land.

use std::fmt;
use std::path::PathBuf;

use crate::script::types::{
    Action, Generator, PayMode, Script, ScriptBudget, ScriptSpec, SigningKeyEnvelope, SubmitMode,
};
use crate::setup::nix_service::{NixServiceOptions, get_node_config_file, tx_gen_tx_params};
use crate::types::{
    Lovelace, TxGenPlutusParams, TxGenPlutusType, has_loop_calibration, has_static_budget,
};

/// Mirror of upstream `maxOutputsPerTx`.
pub const MAX_OUTPUTS_PER_TX: usize = 30;

const SCRIPT_FEES: Lovelace = 5_000_000;
const COLLATERAL_PERCENTAGE: Lovelace = 200;

const KEY_NAME_GENESIS_INPUT_FUND: &str = "GenesisInputFund";
const KEY_NAME_TX_GEN_FUNDS: &str = "TxGenFunds";
const KEY_NAME_COLLATERALS: &str = "Collaterals";
const KEY_NAME_SPLIT_PHASE: &str = "SplitPhase";
const KEY_NAME_BENCHMARK_INPUTS: &str = "BenchmarkInputs";
const KEY_NAME_BENCHMARK_DONE: &str = "BenchmarkingDone";

const KEY_TX_GEN_FUNDS: &str =
    "5820617f846fc8b0e753bd51790de5f5a916de500175c6f5a0e27dde9da7879e1d35";
const KEY_COLLATERALS: &str =
    "58204babdb63537ccdac393ea23d042af3b7c3587d7dc88ed3b66c959f198ad358fa";
const KEY_SPLIT_PHASE: &str =
    "5820cf0083c2a5d4c90ab255bc8e68f407d52eebd9408de60a0b9e4c468f9714f076";
const KEY_BENCHMARK_INPUTS: &str =
    "58205b7f272602661d4ad3d9a4081f25fdcdcdf64fdc4892107de50e50937b77ea42";
const KEY_BENCHMARK_DONE: &str =
    "582016ca4f13fa17557e56a7d0dd3397d747db8e1e22fdb5b9df638abdb680650d50";

/// Errors emitted while compiling high-level options.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// Mirrors upstream `SomeCompilerError`.
    #[error("{0}")]
    SomeCompilerError(String),
}

/// Mirror of upstream `compileOptions`.
pub fn compile_options(opts: &NixServiceOptions) -> Result<Script, CompileError> {
    let mut compiler = Compiler::new(opts);
    compiler.compile_to_script()?;
    Ok(compiler.actions)
}

/// Mirror of upstream `Split`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Split {
    /// `SplitWithChange`.
    SplitWithChange(Lovelace, usize),
    /// `FullSplits`.
    FullSplits(usize),
}

impl fmt::Display for Split {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SplitWithChange(lovelace, count) => {
                write!(f, "SplitWithChange (Coin {lovelace}) {count}")
            }
            Self::FullSplits(count) => write!(f, "FullSplits {count}"),
        }
    }
}

/// Mirror of upstream `Fees`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fees {
    /// Upstream `_safeCollateral`.
    pub safe_collateral: Lovelace,
    /// Upstream `_minValuePerInput`.
    pub min_value_per_input: Lovelace,
}

struct Compiler<'a> {
    opts: &'a NixServiceOptions,
    actions: Script,
    next_identifier: usize,
}

impl<'a> Compiler<'a> {
    fn new(opts: &'a NixServiceOptions) -> Self {
        Self {
            opts,
            actions: Vec::new(),
            next_identifier: 0,
        }
    }

    fn compile_to_script(&mut self) -> Result<(), CompileError> {
        self.init_constants();
        let node_config = get_node_config_file(self.opts)
            .ok_or_else(|| {
                CompileError::SomeCompilerError("nodeConfigFile not set in Nix options".to_string())
            })?
            .to_path_buf();
        self.emit(Action::StartProtocol(
            node_config,
            self.opts.nix_cardano_tracer_socket.clone(),
        ));
        let genesis_wallet = self.import_genesis_funds();
        let collateral_wallet = self.add_collaterals(&genesis_wallet)?;
        let split_wallet = self.splitting_phase(&genesis_wallet)?;
        self.benchmarking_phase(&split_wallet, collateral_wallet);
        Ok(())
    }

    fn init_constants(&mut self) {
        self.emit(Action::SetSocketPath(
            self.opts.nix_local_node_socket_path.clone(),
        ));
        self.emit(Action::DefineSigningKey(
            KEY_NAME_TX_GEN_FUNDS.to_string(),
            signing_key(KEY_TX_GEN_FUNDS),
        ));
        self.emit(Action::DefineSigningKey(
            KEY_NAME_COLLATERALS.to_string(),
            signing_key(KEY_COLLATERALS),
        ));
        self.emit(Action::DefineSigningKey(
            KEY_NAME_SPLIT_PHASE.to_string(),
            signing_key(KEY_SPLIT_PHASE),
        ));
        self.emit(Action::DefineSigningKey(
            KEY_NAME_BENCHMARK_INPUTS.to_string(),
            signing_key(KEY_BENCHMARK_INPUTS),
        ));
        self.emit(Action::DefineSigningKey(
            KEY_NAME_BENCHMARK_DONE.to_string(),
            signing_key(KEY_BENCHMARK_DONE),
        ));
    }

    fn import_genesis_funds(&mut self) -> String {
        self.log_msg("Importing Genesis Fund.");
        let wallet = self.new_wallet("genesis_wallet");
        let tx_params = tx_gen_tx_params(self.opts);
        self.emit(Action::ReadSigningKey(
            KEY_NAME_GENESIS_INPUT_FUND.to_string(),
            self.opts.nix_sig_key.clone(),
        ));
        self.emit(Action::Submit(
            self.opts.nix_era,
            SubmitMode::LocalSocket,
            tx_params,
            Generator::SecureGenesis(
                wallet.clone(),
                KEY_NAME_GENESIS_INPUT_FUND.to_string(),
                KEY_NAME_TX_GEN_FUNDS.to_string(),
            ),
        ));
        self.delay();
        self.log_msg("Importing Genesis Fund. Done.");
        wallet
    }

    fn add_collaterals(&mut self, src: &str) -> Result<Option<String>, CompileError> {
        if !self.is_any_plutus_mode() {
            return Ok(None);
        }

        self.log_msg("Create collaterals.");
        let safe_collateral = self.evil_fee_magic()?.safe_collateral;
        let collateral_wallet = self.new_wallet("collateral_wallet");
        let generator = Generator::Split(
            src.to_string(),
            PayMode::PayToAddr(KEY_NAME_COLLATERALS.to_string(), collateral_wallet.clone()),
            PayMode::PayToAddr(KEY_NAME_TX_GEN_FUNDS.to_string(), src.to_string()),
            vec![safe_collateral],
        );
        self.emit(Action::Submit(
            self.opts.nix_era,
            SubmitMode::LocalSocket,
            tx_gen_tx_params(self.opts),
            generator,
        ));
        self.log_msg("Create collaterals. Done.");
        Ok(Some(collateral_wallet))
    }

    fn splitting_phase(&mut self, src_wallet: &str) -> Result<String, CompileError> {
        let tx_params = tx_gen_tx_params(self.opts);
        let min_value_per_input = self.evil_fee_magic()?.min_value_per_input;
        let final_dest = self.new_wallet("final_split_wallet");
        let split_steps = self.split_sequence_wallet_names(
            src_wallet.to_string(),
            final_dest.clone(),
            &unfold_split_sequence(
                tx_params.tx_param_fee,
                min_value_per_input,
                self.opts.nix_tx_count * self.opts.nix_inputs_per_tx,
            ),
        );

        let last_index = split_steps
            .len()
            .checked_sub(1)
            .ok_or_else(|| CompileError::SomeCompilerError("empty split sequence".to_string()))?;
        let is_plutus = self.is_any_plutus_mode();
        for (idx, (src, dst, split)) in split_steps.into_iter().enumerate() {
            self.create_change(
                tx_params.clone(),
                idx == last_index,
                is_plutus && idx == last_index,
                src,
                dst,
                split,
            )?;
        }
        Ok(final_dest)
    }

    fn create_change(
        &mut self,
        tx_params: crate::types::TxGenTxParams,
        is_last_step: bool,
        is_plutus: bool,
        src: String,
        dst: String,
        split: Split,
    ) -> Result<(), CompileError> {
        self.log_msg(format!("Splitting step: {split}"));
        let value_pay_mode = PayMode::PayToAddr(
            if is_last_step {
                KEY_NAME_SPLIT_PHASE
            } else {
                KEY_NAME_BENCHMARK_INPUTS
            }
            .to_string(),
            dst.clone(),
        );
        let pay_mode = if is_plutus {
            self.plutus_pay_mode(&dst)?
        } else {
            value_pay_mode
        };
        let generator = match split {
            Split::SplitWithChange(lovelace, count) => Generator::Split(
                src.clone(),
                pay_mode,
                PayMode::PayToAddr(KEY_NAME_TX_GEN_FUNDS.to_string(), src),
                vec![lovelace; count],
            ),
            Split::FullSplits(tx_count) => Generator::Take(
                tx_count,
                Box::new(Generator::Cycle(Box::new(Generator::SplitN(
                    src,
                    pay_mode,
                    MAX_OUTPUTS_PER_TX,
                )))),
            ),
        };
        self.emit(Action::Submit(
            self.opts.nix_era,
            SubmitMode::LocalSocket,
            tx_params,
            generator,
        ));
        self.delay();
        self.log_msg("Splitting step: Done");
        Ok(())
    }

    fn plutus_pay_mode(&self, dst: &str) -> Result<PayMode, CompileError> {
        let Some(TxGenPlutusParams::PlutusOn {
            plutus_type,
            plutus_script,
            plutus_datum,
            plutus_redeemer,
            ..
        }) = self.opts.nix_plutus.as_ref()
        else {
            return Err(CompileError::SomeCompilerError(
                "Plutus pay mode requested without Plutus config.".to_string(),
            ));
        };

        let script_budget = if has_loop_calibration(*plutus_type) {
            let redeemer = plutus_redeemer.clone().ok_or_else(|| {
                CompileError::SomeCompilerError(
                    "Plutus loop autoscript requires a redeemer.".to_string(),
                )
            })?;
            ScriptBudget::AutoScript(redeemer, self.opts.nix_inputs_per_tx)
        } else {
            let execution_units = has_static_budget(
                self.opts
                    .nix_plutus
                    .as_ref()
                    .expect("checked Plutus config above"),
            )
            .ok_or_else(|| {
                CompileError::SomeCompilerError(
                    "Plutus custom script requires a static budget.".to_string(),
                )
            })?;
            ScriptBudget::StaticScriptBudget(
                plutus_datum.clone().unwrap_or_else(PathBuf::new),
                plutus_redeemer.clone().unwrap_or_else(PathBuf::new),
                execution_units,
                self.opts.nix_debug_mode,
            )
        };

        Ok(PayMode::PayToScript(
            ScriptSpec {
                script_spec_file: plutus_script.clone(),
                script_spec_budget: script_budget,
                script_spec_plutus_type: *plutus_type,
            },
            dst.to_string(),
        ))
    }

    fn split_sequence_wallet_names(
        &mut self,
        src: String,
        dst: String,
        splits: &[Split],
    ) -> Vec<(String, String, Split)> {
        match splits {
            [] => Vec::new(),
            [split] => vec![(src, dst, split.clone())],
            [split, rest @ ..] => {
                let temp_wallet = self.new_wallet("change_wallet");
                let mut next = self.split_sequence_wallet_names(temp_wallet.clone(), dst, rest);
                next.insert(0, (src, temp_wallet, split.clone()));
                next
            }
        }
    }

    fn benchmarking_phase(&mut self, wallet: &str, collateral_wallet: Option<String>) -> String {
        let done_wallet = self.new_wallet("done_wallet");
        let pay_mode = PayMode::PayToAddr(KEY_NAME_BENCHMARK_DONE.to_string(), done_wallet.clone());
        let tx_params = tx_gen_tx_params(self.opts);
        let submit_mode = if self.opts.nix_debug_mode {
            SubmitMode::LocalSocket
        } else {
            SubmitMode::Benchmark(
                self.opts.nix_target_nodes.clone(),
                self.opts.nix_tps,
                self.opts.nix_tx_count,
            )
        };
        let generator = Generator::Take(
            self.opts.nix_tx_count,
            Box::new(Generator::Cycle(Box::new(Generator::NtoM(
                wallet.to_string(),
                pay_mode,
                self.opts.nix_inputs_per_tx,
                self.opts.nix_outputs_per_tx,
                Some(tx_params.tx_param_add_tx_size),
                collateral_wallet,
            )))),
        );
        self.emit(Action::Submit(
            self.opts.nix_era,
            submit_mode,
            tx_params,
            generator,
        ));
        if !self.opts.nix_debug_mode {
            self.emit(Action::WaitBenchmark);
        }
        done_wallet
    }

    fn evil_fee_magic(&self) -> Result<Fees, CompileError> {
        if self.opts.nix_inputs_per_tx == 0 {
            return Err(CompileError::SomeCompilerError(
                "inputs_per_tx must be greater than zero".to_string(),
            ));
        }

        let total_fee = if self.is_plutus_type(TxGenPlutusType::CustomScript) {
            self.opts.nix_tx_fee + SCRIPT_FEES * self.opts.nix_inputs_per_tx as Lovelace
        } else {
            self.opts.nix_tx_fee
        };
        let safe_collateral = ((SCRIPT_FEES + self.opts.nix_tx_fee) * COLLATERAL_PERCENTAGE / 100)
            .max(self.opts.nix_min_utxo_value);
        let min_total_value =
            self.opts.nix_min_utxo_value * self.opts.nix_outputs_per_tx as Lovelace + total_fee;
        let min_value_per_input = min_total_value / self.opts.nix_inputs_per_tx as Lovelace + 1;

        Ok(Fees {
            safe_collateral,
            min_value_per_input,
        })
    }

    fn emit(&mut self, action: Action) {
        self.actions.push(action);
    }

    fn log_msg(&mut self, message: impl Into<String>) {
        self.emit(Action::LogMsg(message.into()));
    }

    fn delay(&mut self) {
        self.emit(Action::Delay(self.opts.nix_init_cooldown));
    }

    fn is_plutus_type(&self, plutus_type: TxGenPlutusType) -> bool {
        matches!(
            self.opts.nix_plutus.as_ref(),
            Some(TxGenPlutusParams::PlutusOn {
                plutus_type: configured,
                ..
            }) if *configured == plutus_type
        )
    }

    fn is_any_plutus_mode(&self) -> bool {
        self.opts.nix_plutus.is_some()
    }

    fn new_identifier(&mut self, prefix: &str) -> String {
        let n = self.next_identifier;
        self.next_identifier += 1;
        format!("{prefix}_{n}")
    }

    fn new_wallet(&mut self, prefix: &str) -> String {
        let name = self.new_identifier(prefix);
        self.emit(Action::InitWallet(name.clone()));
        name
    }
}

/// Mirror of upstream `unfoldSplitSequence`.
pub fn unfold_split_sequence(fee: Lovelace, value: Lovelace, outputs: usize) -> Vec<Split> {
    if outputs < MAX_OUTPUTS_PER_TX {
        vec![Split::SplitWithChange(value, outputs)]
    } else {
        let txs = outputs.div_ceil(MAX_OUTPUTS_PER_TX);
        let mut prefix =
            unfold_split_sequence(fee, value * MAX_OUTPUTS_PER_TX as Lovelace + fee, txs);
        prefix.push(Split::FullSplits(txs));
        prefix
    }
}

/// Mirror of upstream `evilFeeMagic` as a pure helper for tests.
pub fn evil_fee_magic(opts: &NixServiceOptions) -> Result<Fees, CompileError> {
    Compiler::new(opts).evil_fee_magic()
}

fn signing_key(cbor_hex: &'static str) -> SigningKeyEnvelope {
    SigningKeyEnvelope::payment_signing_key_shelley(cbor_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::nix_service::parse_nix_service_options_value;
    use crate::types::{AnyCardanoEra, ExecutionUnits, PlutusScriptRef};
    use serde_json::json;

    fn config(debug_mode: bool) -> NixServiceOptions {
        parse_nix_service_options_value(json!({
            "debugMode": debug_mode,
            "tx_count": 4,
            "tps": 10.0,
            "inputs_per_tx": 2,
            "outputs_per_tx": 3,
            "tx_fee": 212345,
            "min_utxo_value": 1000000,
            "add_tx_size": 39,
            "init_cooldown": 5.0,
            "era": "Conway",
            "keepalive": 45,
            "localNodeSocketPath": "node.socket",
            "nodeConfigFile": "config.json",
            "sigKey": "genesis-utxo.skey",
            "targetNodes": [
                {"addr": "127.0.0.1", "port": 30000, "name": "node0"}
            ],
            "plutus": null
        }))
        .expect("config parses")
    }

    #[test]
    fn unfold_split_sequence_matches_upstream_boundaries() {
        assert_eq!(
            unfold_split_sequence(10, 100, 29),
            vec![Split::SplitWithChange(100, 29)]
        );
        assert_eq!(
            unfold_split_sequence(10, 100, 30),
            vec![Split::SplitWithChange(3010, 1), Split::FullSplits(1)]
        );
        assert_eq!(
            unfold_split_sequence(10, 100, 900),
            vec![
                Split::SplitWithChange(90310, 1),
                Split::FullSplits(1),
                Split::FullSplits(30)
            ]
        );
    }

    #[test]
    fn evil_fee_magic_matches_upstream_arithmetic_for_non_plutus() {
        let opts = config(false);

        assert_eq!(
            evil_fee_magic(&opts).expect("fees"),
            Fees {
                safe_collateral: 10_424_690,
                min_value_per_input: 1_606_173,
            }
        );
    }

    #[test]
    fn compile_options_emits_expected_non_plutus_shape() {
        let opts = config(false);
        let script = compile_options(&opts).expect("compile succeeds");

        assert_eq!(
            &script[..7],
            &[
                Action::SetSocketPath(PathBuf::from("node.socket")),
                Action::DefineSigningKey(
                    KEY_NAME_TX_GEN_FUNDS.to_string(),
                    signing_key(KEY_TX_GEN_FUNDS),
                ),
                Action::DefineSigningKey(
                    KEY_NAME_COLLATERALS.to_string(),
                    signing_key(KEY_COLLATERALS),
                ),
                Action::DefineSigningKey(
                    KEY_NAME_SPLIT_PHASE.to_string(),
                    signing_key(KEY_SPLIT_PHASE),
                ),
                Action::DefineSigningKey(
                    KEY_NAME_BENCHMARK_INPUTS.to_string(),
                    signing_key(KEY_BENCHMARK_INPUTS),
                ),
                Action::DefineSigningKey(
                    KEY_NAME_BENCHMARK_DONE.to_string(),
                    signing_key(KEY_BENCHMARK_DONE),
                ),
                Action::StartProtocol(PathBuf::from("config.json"), None),
            ]
        );
        assert!(script.iter().any(|action| matches!(
            action,
            Action::ReadSigningKey(name, path)
                if name == KEY_NAME_GENESIS_INPUT_FUND && path == &PathBuf::from("genesis-utxo.skey")
        )));
        assert!(matches!(script.last(), Some(Action::WaitBenchmark)));
        assert!(script.iter().any(|action| matches!(
            action,
            Action::Submit(
                AnyCardanoEra::Conway,
                SubmitMode::Benchmark(nodes, 10.0, 4),
                _,
                Generator::Take(4, _)
            ) if nodes.len() == 1
        )));
    }

    #[test]
    fn debug_mode_uses_local_socket_and_skips_wait_benchmark() {
        let opts = config(true);
        let script = compile_options(&opts).expect("compile succeeds");

        assert!(
            !script
                .iter()
                .any(|action| matches!(action, Action::WaitBenchmark))
        );
        assert!(script.iter().any(|action| matches!(
            action,
            Action::Submit(_, SubmitMode::LocalSocket, _, Generator::Take(4, _))
        )));
    }

    #[test]
    fn compile_errors_when_node_config_missing() {
        let mut opts = config(false);
        opts.nix_node_config_file = None;

        let err = compile_options(&opts).expect_err("missing node config fails");
        assert!(err.to_string().contains("nodeConfigFile not set"));
    }

    #[test]
    fn plutus_loop_requires_redeemer() {
        let mut opts = config(false);
        opts.nix_plutus = Some(TxGenPlutusParams::PlutusOn {
            plutus_type: TxGenPlutusType::LimitSaturationLoop,
            plutus_script: PlutusScriptRef::Named("Loop".to_string()),
            plutus_datum: None,
            plutus_redeemer: None,
            plutus_exec_memory: None,
            plutus_exec_steps: None,
        });

        let err = compile_options(&opts).expect_err("missing redeemer fails");
        assert!(err.to_string().contains("requires a redeemer"));
    }

    #[test]
    fn custom_plutus_uses_static_budget() {
        let mut opts = config(true);
        opts.nix_plutus = Some(TxGenPlutusParams::PlutusOn {
            plutus_type: TxGenPlutusType::CustomScript,
            plutus_script: PlutusScriptRef::File(PathBuf::from("custom.plutus")),
            plutus_datum: Some(PathBuf::from("datum.json")),
            plutus_redeemer: Some(PathBuf::from("redeemer.json")),
            plutus_exec_memory: Some(12),
            plutus_exec_steps: Some(34),
        });

        let script = compile_options(&opts).expect("custom plutus compiles");
        assert!(script.iter().any(|action| matches!(
            action,
            Action::Submit(
                _,
                SubmitMode::LocalSocket,
                _,
                Generator::Split(_, PayMode::PayToScript(spec, _), _, _)
            ) if spec.script_spec_budget == ScriptBudget::StaticScriptBudget(
                PathBuf::from("datum.json"),
                PathBuf::from("redeemer.json"),
                ExecutionUnits {
                    execution_steps: 34,
                    execution_memory: 12,
                },
                true,
            )
        )));
    }
}
