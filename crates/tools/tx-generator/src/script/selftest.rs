//! Built-in tx-generator self-test script.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Selftest.hs`.
//! Ports `runSelftest` and `testScript`, including the deterministic
//! Allegra `DumpToFile` self-test path.

use std::path::{Path, PathBuf};

use crate::script::action::action;
use crate::script::env::{BenchTracers, Env, Error, set_bench_tracers};
use crate::script::types::{
    Action, Generator, NetworkId, PayMode, ProtocolParametersSource, Script, SigningKeyEnvelope,
    SubmitMode,
};
use crate::types::{AnyCardanoEra, DEFAULT_TX_GEN_TX_PARAMS, TxGenTxParams};

const SELFTEST_KEY: &str = "58200b6c317eb6c9762898fa41ca9d683003f86899ab0f2f6dbaf244e415b62826a2";
const GENESIS_WALLET: &str = "genesisWallet";
const SPLIT_WALLET_1: &str = "SplitWallet-1";
const SPLIT_WALLET_2: &str = "SplitWallet-2";
const SPLIT_WALLET_3: &str = "SplitWallet-3";
const DONE_WALLET: &str = "doneWallet";
const KEY_NAME: &str = "pass-partout";
const GENESIS_TX_IN: &str = "900fc5da77a0747da53f7675cbb7d149d46779346dea2f879ab811ccc72a2162#0";
const GENESIS_LOVELACE: u64 = 90_000_000_000_000;

/// Mirror of upstream `runSelftest`.
pub fn run_selftest(out_file: Option<&Path>) -> Result<(), Error> {
    let protocol_file = protocol_parameters_file();
    let submit_mode = out_file
        .map(|path| SubmitMode::DumpToFile(path.to_path_buf()))
        .unwrap_or(SubmitMode::DiscardTx);
    let mut env = Env::empty_env();
    set_bench_tracers(&mut env, BenchTracers::default());

    for script_action in test_script(&protocol_file, submit_mode) {
        action(&mut env, &script_action)?;
    }

    if env.env_threads.is_some() {
        Err(Error::TxGenError(
            "Cardano.Benchmarking.Script.Selftest.runSelftest: thread state spuriously initialized"
                .to_string(),
        ))
    } else {
        Ok(())
    }
}

/// Mirror of upstream `testScript`.
pub fn test_script(protocol_file: &Path, submit_mode: SubmitMode) -> Script {
    let era = AnyCardanoEra::Allegra;
    let tx_params = selftest_tx_params();
    vec![
        Action::SetProtocolParameters(ProtocolParametersSource::UseLocalProtocolFile(
            protocol_file.to_path_buf(),
        )),
        Action::SetNetworkId(NetworkId::Testnet(42)),
        Action::InitWallet(GENESIS_WALLET.to_string()),
        Action::InitWallet(SPLIT_WALLET_1.to_string()),
        Action::InitWallet(SPLIT_WALLET_2.to_string()),
        Action::InitWallet(SPLIT_WALLET_3.to_string()),
        Action::InitWallet(DONE_WALLET.to_string()),
        Action::DefineSigningKey(KEY_NAME.to_string(), selftest_signing_key()),
        Action::AddFund(
            era,
            GENESIS_WALLET.to_string(),
            GENESIS_TX_IN.to_string(),
            GENESIS_LOVELACE,
            KEY_NAME.to_string(),
        ),
        create_change(
            era,
            &submit_mode,
            &tx_params,
            GENESIS_WALLET,
            SPLIT_WALLET_1,
            1,
            10,
        ),
        create_change(
            era,
            &submit_mode,
            &tx_params,
            SPLIT_WALLET_1,
            SPLIT_WALLET_2,
            10,
            30,
        ),
        create_change(
            era,
            &submit_mode,
            &tx_params,
            SPLIT_WALLET_2,
            SPLIT_WALLET_3,
            300,
            30,
        ),
        Action::Submit(
            era,
            submit_mode,
            tx_params,
            Generator::Take(
                4_000,
                Box::new(Generator::Cycle(Box::new(Generator::NtoM(
                    SPLIT_WALLET_3.to_string(),
                    PayMode::PayToAddr(KEY_NAME.to_string(), DONE_WALLET.to_string()),
                    2,
                    2,
                    None,
                    None,
                )))),
            ),
        ),
    ]
}

fn create_change(
    era: AnyCardanoEra,
    submit_mode: &SubmitMode,
    tx_params: &TxGenTxParams,
    source_wallet: &str,
    destination_wallet: &str,
    tx_count: usize,
    outputs: usize,
) -> Action {
    Action::Submit(
        era,
        submit_mode.clone(),
        tx_params.clone(),
        Generator::Take(
            tx_count,
            Box::new(Generator::Cycle(Box::new(Generator::SplitN(
                source_wallet.to_string(),
                PayMode::PayToAddr(KEY_NAME.to_string(), destination_wallet.to_string()),
                outputs,
            )))),
        ),
    )
}

fn selftest_signing_key() -> SigningKeyEnvelope {
    SigningKeyEnvelope::genesis_utxo_signing_key(SELFTEST_KEY)
}

fn selftest_tx_params() -> TxGenTxParams {
    TxGenTxParams {
        tx_param_fee: 1_000_000,
        ..DEFAULT_TX_GEN_TX_PARAMS
    }
}

fn protocol_parameters_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/protocol-parameters.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::core::plutus_budget_summary_file_for_tests;
    use crate::script::types::ProtocolParametersSource;

    struct BudgetSummaryFileCleanup;

    impl BudgetSummaryFileCleanup {
        fn new() -> Self {
            let _ = std::fs::remove_file(plutus_budget_summary_file_for_tests());
            Self
        }
    }

    impl Drop for BudgetSummaryFileCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(plutus_budget_summary_file_for_tests());
        }
    }

    #[test]
    fn test_script_matches_upstream_selftest_shape() {
        let script = test_script(
            Path::new("data/protocol-parameters.json"),
            SubmitMode::DiscardTx,
        );

        assert_eq!(script.len(), 13);
        assert!(matches!(
            &script[0],
            Action::SetProtocolParameters(ProtocolParametersSource::UseLocalProtocolFile(path))
                if path == Path::new("data/protocol-parameters.json")
        ));
        assert_eq!(script[1], Action::SetNetworkId(NetworkId::Testnet(42)));
        assert_eq!(
            script[7],
            Action::DefineSigningKey(KEY_NAME.to_string(), selftest_signing_key())
        );
        assert_eq!(
            script[8],
            Action::AddFund(
                AnyCardanoEra::Allegra,
                GENESIS_WALLET.to_string(),
                GENESIS_TX_IN.to_string(),
                GENESIS_LOVELACE,
                KEY_NAME.to_string()
            )
        );
        assert!(matches!(
            &script[12],
            Action::Submit(
                AnyCardanoEra::Allegra,
                SubmitMode::DiscardTx,
                TxGenTxParams {
                    tx_param_fee: 1_000_000,
                    ..
                },
                Generator::Take(4_000, _)
            )
        ));
    }

    #[test]
    fn run_selftest_discard_executes_complete_static_script() {
        let _summary_file_cleanup = BudgetSummaryFileCleanup::new();

        run_selftest(None).expect("selftest discard");
    }

    #[test]
    fn run_selftest_with_output_file_writes_haskell_show_transactions() {
        let output_path = std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-selftest-{}.out",
            std::process::id()
        ));
        let _summary_file_cleanup = BudgetSummaryFileCleanup::new();
        let _ = std::fs::remove_file(&output_path);

        run_selftest(Some(&output_path)).expect("selftest dump");

        let rendered = std::fs::read_to_string(&output_path).expect("dump output");
        let _ = std::fs::remove_file(&output_path);
        assert!(rendered.starts_with(
            "\nShelleyTx ShelleyBasedEraAllegra (ShelleyTx {stBody = MkAllegraTxBody"
        ));
        assert_eq!(
            rendered.lines().filter(|line| !line.is_empty()).count(),
            4_000
        );
        let first_tx = rendered
            .lines()
            .find(|line| !line.is_empty())
            .expect("first transaction");
        assert!(
            first_tx.contains("3986ae75caaf853a53e6963288c680baf8a7be1239eceec7705d7ef6f045700a")
        );
        assert!(
            first_tx.contains("05736377bfed5ad124e25c1f57b9c3e01d08f701b6ebed409bb4b040f467a8e9")
        );
        assert!(rendered.contains("stWits = ShelleyTxWitsRaw"));
        assert!(rendered.ends_with("stAuxData = SNothing})"));
    }
}
