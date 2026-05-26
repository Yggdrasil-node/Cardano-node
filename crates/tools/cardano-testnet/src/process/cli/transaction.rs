//! `cardano-cli` transaction command builders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/Cli/Transaction.hs.

use crate::runtime_types::{KeyPair, PaymentKeyInfo};
use crate::types::{CardanoEra, ShelleyBasedEra};

use std::path::{Path, PathBuf};

/// Marker for upstream `data VoteFile`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct VoteFile;

/// Marker for upstream `data TxBody`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TxBody;

/// Marker for upstream `data SignedTx`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SignedTx;

/// Marker for upstream `data ScriptJSON`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ScriptJson;

/// Destination address selector for upstream `TxOutAddress`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxOutAddress {
    /// The output is addressed to a payment key address.
    PubKeyAddress(PaymentKeyInfo),
    /// The output will be created at the address of this script file.
    ScriptAddress(PathBuf),
}

/// A transaction output address after runtime-only script-address
/// resolution has happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedTxOutAddress {
    /// The output is addressed to a payment key address.
    PubKeyAddress(PaymentKeyInfo),
    /// The output will be created at this already-built script address.
    ScriptAddress {
        /// Script file passed to upstream `cardano-cli address build`.
        payment_script_file: PathBuf,
        /// Address returned by upstream `cardano-cli address build`.
        resolved_address: String,
    },
}

/// One output passed to upstream `mkSpendOutputsOnlyTx`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendOutput {
    /// Destination address selector.
    pub address: ResolvedTxOutAddress,
    /// Lovelace amount for this output.
    pub amount: u64,
    /// Optional reference script attached by upstream for script outputs.
    pub reference_script: Option<PathBuf>,
}

impl SpendOutput {
    /// Build a pubkey-address output.
    pub fn pub_key(wallet: PaymentKeyInfo, amount: u64) -> Self {
        SpendOutput {
            address: ResolvedTxOutAddress::PubKeyAddress(wallet),
            amount,
            reference_script: None,
        }
    }

    /// Build a pubkey-address output carrying the same ignored reference-script
    /// input that upstream accepts but does not emit for pubkey outputs.
    pub fn pub_key_with_reference_script(
        wallet: PaymentKeyInfo,
        amount: u64,
        reference_script: impl Into<PathBuf>,
    ) -> Self {
        SpendOutput {
            address: ResolvedTxOutAddress::PubKeyAddress(wallet),
            amount,
            reference_script: Some(reference_script.into()),
        }
    }

    /// Build a script-address output from a pre-resolved script address.
    pub fn script<R>(
        payment_script_file: impl Into<PathBuf>,
        resolved_address: impl Into<String>,
        amount: u64,
        reference_script: Option<R>,
    ) -> Self
    where
        R: Into<PathBuf>,
    {
        SpendOutput {
            address: ResolvedTxOutAddress::ScriptAddress {
                payment_script_file: payment_script_file.into(),
                resolved_address: resolved_address.into(),
            },
            amount,
            reference_script: reference_script.map(Into::into),
        }
    }
}

/// Planned upstream script-address build invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptAddressPlan {
    /// Script file passed to `cardano-cli address build`.
    pub payment_script_file: PathBuf,
    /// Address returned by that runtime command.
    pub resolved_script_address: String,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned upstream `mkSpendOutputsOnlyTx` invocation and output path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendOutputsOnlyTxPlan {
    /// The unsigned transaction body returned by upstream.
    pub output_tx_body_path: PathBuf,
    /// Runtime address-build commands required before the txbody command.
    pub script_address_plans: Vec<ScriptAddressPlan>,
    /// Arguments passed to `cardano-cli transaction build`.
    pub args: Vec<String>,
}

/// A signing-key path erased from a typed [`KeyPair`].
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AnySigningKey {
    signing_key_file: PathBuf,
}

impl AnySigningKey {
    /// Erase any typed key pair to the signing-key path used by
    /// upstream `signTx`.
    pub fn from_key_pair<K>(key_pair: &KeyPair<K>) -> Self {
        AnySigningKey {
            signing_key_file: key_pair.signing_key_fp().to_path_buf(),
        }
    }

    /// Construct from a raw signing-key path.
    pub fn from_signing_key_file(path: impl Into<PathBuf>) -> Self {
        AnySigningKey {
            signing_key_file: path.into(),
        }
    }

    /// Borrow the signing-key path.
    pub fn signing_key_file(&self) -> &Path {
        &self.signing_key_file
    }
}

/// Planned upstream `signTx` invocation and output path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignTxPlan {
    /// The signed transaction file returned by upstream `signTx`.
    pub output_signed_tx_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Result classification for upstream `failToSubmitTx`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FailedSubmissionResult {
    /// Submission failed and stderr contained the expected reason.
    ExpectedFailure,
    /// Submission unexpectedly succeeded.
    UnexpectedSuccess,
    /// Submission failed for a different reason.
    WrongFailureReason,
}

/// Build upstream `mkSpendOutputsOnlyTx` path and argv from a preselected tx
/// input and pre-resolved script addresses.
pub fn mk_spend_outputs_only_tx_plan(
    era: ShelleyBasedEra,
    work: impl AsRef<Path>,
    prefix: &str,
    src_wallet: &PaymentKeyInfo,
    tx_in: &str,
    tx_outputs: impl IntoIterator<Item = SpendOutput>,
) -> SpendOutputsOnlyTxPlan {
    let cardano_era = CardanoEra::from(era);
    let output_tx_body_path = posix_join(work.as_ref(), Path::new(&format!("{prefix}.txbody")));
    let mut script_address_plans = Vec::new();
    let mut args = vec![
        cardano_era.era_to_string().to_string(),
        "transaction".to_string(),
        "build".to_string(),
        "--change-address".to_string(),
        src_wallet.payment_key_info_addr.clone(),
        "--tx-in".to_string(),
        tx_in.to_string(),
    ];

    for output in tx_outputs {
        match output.address {
            ResolvedTxOutAddress::PubKeyAddress(wallet) => {
                args.push("--tx-out".to_string());
                args.push(format!(
                    "{}+{}",
                    wallet.payment_key_info_addr, output.amount
                ));
            }
            ResolvedTxOutAddress::ScriptAddress {
                payment_script_file,
                resolved_address,
            } => {
                script_address_plans.push(ScriptAddressPlan {
                    payment_script_file: payment_script_file.clone(),
                    resolved_script_address: resolved_address.clone(),
                    args: vec![
                        cardano_era.era_to_string().to_string(),
                        "address".to_string(),
                        "build".to_string(),
                        "--payment-script-file".to_string(),
                        path_to_cli_arg(&payment_script_file),
                    ],
                });
                args.push("--tx-out".to_string());
                args.push(format!("{resolved_address}+{}", output.amount));
                if let Some(reference_script) = output.reference_script {
                    args.push("--tx-out-reference-script-file".to_string());
                    args.push(path_to_cli_arg(&reference_script));
                }
            }
        }
    }

    args.push("--out-file".to_string());
    args.push(path_to_cli_arg(&output_tx_body_path));
    SpendOutputsOnlyTxPlan {
        output_tx_body_path,
        script_address_plans,
        args,
    }
}

/// Build upstream `mkSimpleSpendOutputsOnlyTx` path and argv.
pub fn mk_simple_spend_outputs_only_tx_plan(
    era: ShelleyBasedEra,
    work: impl AsRef<Path>,
    prefix: &str,
    src_wallet: &PaymentKeyInfo,
    tx_in: &str,
    dst_wallet: PaymentKeyInfo,
    amount: u64,
) -> SpendOutputsOnlyTxPlan {
    mk_spend_outputs_only_tx_plan(
        era,
        work,
        prefix,
        src_wallet,
        tx_in,
        [SpendOutput::pub_key(dst_wallet, amount)],
    )
}

/// Build upstream `signTx` path and argv.
pub fn sign_tx_plan(
    era: CardanoEra,
    work: impl AsRef<Path>,
    prefix: &str,
    tx_body: impl AsRef<Path>,
    signatory_key_pairs: impl IntoIterator<Item = AnySigningKey>,
) -> SignTxPlan {
    let output_signed_tx_path = posix_join(work.as_ref(), Path::new(&format!("{prefix}.tx")));
    let mut args = vec![
        era.era_to_string().to_string(),
        "transaction".to_string(),
        "sign".to_string(),
        "--tx-body-file".to_string(),
        path_to_cli_arg(tx_body.as_ref()),
    ];
    for signing_key in signatory_key_pairs {
        args.push("--signing-key-file".to_string());
        args.push(path_to_cli_arg(signing_key.signing_key_file()));
    }
    args.push("--out-file".to_string());
    args.push(path_to_cli_arg(&output_signed_tx_path));
    SignTxPlan {
        output_signed_tx_path,
        args,
    }
}

/// Build upstream `submitTx` argv.
pub fn submit_tx_args(era: CardanoEra, signed_tx: impl AsRef<Path>) -> Vec<String> {
    vec![
        era.era_to_string().to_string(),
        "transaction".to_string(),
        "submit".to_string(),
        "--tx-file".to_string(),
        path_to_cli_arg(signed_tx.as_ref()),
    ]
}

/// Build upstream `retrieveTransactionId` argv.
pub fn retrieve_transaction_id_args(signed_tx_body: impl AsRef<Path>) -> Vec<String> {
    vec![
        "latest".to_string(),
        "transaction".to_string(),
        "txid".to_string(),
        "--tx-file".to_string(),
        path_to_cli_arg(signed_tx_body.as_ref()),
    ]
}

/// Classify upstream `failToSubmitTx` process output.
pub fn classify_failed_submission(
    exit_code: i32,
    stderr: &str,
    reason_for_failure: &str,
) -> FailedSubmissionResult {
    if exit_code == 0 {
        FailedSubmissionResult::UnexpectedSuccess
    } else if stderr.contains(reason_for_failure) {
        FailedSubmissionResult::ExpectedFailure
    } else {
        FailedSubmissionResult::WrongFailureReason
    }
}

fn posix_join(left: &Path, right: &Path) -> PathBuf {
    let left = path_to_cli_arg(left);
    let right = path_to_cli_arg(right);
    if left.is_empty() {
        PathBuf::from(right)
    } else if right.is_empty() {
        PathBuf::from(left)
    } else {
        PathBuf::from(format!("{left}/{right}"))
    }
}

fn path_to_cli_arg(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_types::{KeyPair, PaymentKey, StakeKey};
    use crate::types::{CardanoEra, ShelleyBasedEra};

    #[test]
    fn sign_tx_plan_matches_upstream_era_txbody_signers_and_output() {
        let payment: KeyPair<PaymentKey> = KeyPair::new("keys/payment.vkey", "keys/payment.skey");
        let stake: KeyPair<StakeKey> = KeyPair::new("keys/stake.vkey", "keys/stake.skey");

        let plan = sign_tx_plan(
            CardanoEra::Conway,
            "work",
            "signed-reg-tx",
            "work/reg.txbody",
            [
                AnySigningKey::from_key_pair(&payment),
                AnySigningKey::from_key_pair(&stake),
            ],
        );

        assert_eq!(
            plan.output_signed_tx_path,
            std::path::PathBuf::from("work/signed-reg-tx.tx")
        );
        assert_eq!(
            plan.args,
            vec![
                "conway",
                "transaction",
                "sign",
                "--tx-body-file",
                "work/reg.txbody",
                "--signing-key-file",
                "keys/payment.skey",
                "--signing-key-file",
                "keys/stake.skey",
                "--out-file",
                "work/signed-reg-tx.tx",
            ]
        );
    }

    #[test]
    fn submit_and_txid_builders_match_upstream_args() {
        assert_eq!(
            submit_tx_args(CardanoEra::Babbage, "work/signed.tx"),
            vec![
                "babbage",
                "transaction",
                "submit",
                "--tx-file",
                "work/signed.tx",
            ]
        );

        assert_eq!(
            retrieve_transaction_id_args("work/signed.tx"),
            vec![
                "latest",
                "transaction",
                "txid",
                "--tx-file",
                "work/signed.tx"
            ]
        );
    }

    #[test]
    fn fail_to_submit_result_matches_upstream_success_and_stderr_rules() {
        assert_eq!(
            classify_failed_submission(0, "", "ValueNotConserved"),
            FailedSubmissionResult::UnexpectedSuccess
        );
        assert_eq!(
            classify_failed_submission(1, "ValueNotConservedUTxO", "ValueNotConserved"),
            FailedSubmissionResult::ExpectedFailure
        );
        assert_eq!(
            classify_failed_submission(1, "BadInputsUTxO", "ValueNotConserved"),
            FailedSubmissionResult::WrongFailureReason
        );
    }

    #[test]
    fn spend_outputs_only_tx_plan_matches_upstream_pubkey_outputs() {
        let source = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("src.vkey", "src.skey"),
            payment_key_info_addr: "addr_test1source".to_string(),
        };
        let first = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("dst-a.vkey", "dst-a.skey"),
            payment_key_info_addr: "addr_test1first".to_string(),
        };
        let second = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("dst-b.vkey", "dst-b.skey"),
            payment_key_info_addr: "addr_test1second".to_string(),
        };

        let plan = mk_spend_outputs_only_tx_plan(
            ShelleyBasedEra::Conway,
            "work",
            "pubkey-spend",
            &source,
            "abcd#0",
            [
                SpendOutput::pub_key(first, 1_000),
                SpendOutput::pub_key_with_reference_script(second, 2_000, "ignored.plutus"),
            ],
        );

        assert_eq!(plan.script_address_plans, Vec::new());
        assert_eq!(
            plan.output_tx_body_path,
            std::path::PathBuf::from("work/pubkey-spend.txbody")
        );
        assert_eq!(
            plan.args,
            vec![
                "conway",
                "transaction",
                "build",
                "--change-address",
                "addr_test1source",
                "--tx-in",
                "abcd#0",
                "--tx-out",
                "addr_test1first+1000",
                "--tx-out",
                "addr_test1second+2000",
                "--out-file",
                "work/pubkey-spend.txbody",
            ]
        );
    }

    #[test]
    fn spend_outputs_only_tx_plan_matches_upstream_script_outputs() {
        let source = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("src.vkey", "src.skey"),
            payment_key_info_addr: "addr_test1source".to_string(),
        };

        let plan = mk_spend_outputs_only_tx_plan(
            ShelleyBasedEra::Babbage,
            "work",
            "script-spend",
            &source,
            "ef01#2",
            [SpendOutput::script(
                "scripts/payment.plutus",
                "addr_test1script",
                42,
                Some("scripts/reference.plutus"),
            )],
        );

        assert_eq!(
            plan.script_address_plans,
            vec![ScriptAddressPlan {
                payment_script_file: std::path::PathBuf::from("scripts/payment.plutus"),
                resolved_script_address: "addr_test1script".to_string(),
                args: vec![
                    "babbage".to_string(),
                    "address".to_string(),
                    "build".to_string(),
                    "--payment-script-file".to_string(),
                    "scripts/payment.plutus".to_string(),
                ],
            }]
        );
        assert_eq!(
            plan.args,
            vec![
                "babbage",
                "transaction",
                "build",
                "--change-address",
                "addr_test1source",
                "--tx-in",
                "ef01#2",
                "--tx-out",
                "addr_test1script+42",
                "--tx-out-reference-script-file",
                "scripts/reference.plutus",
                "--out-file",
                "work/script-spend.txbody",
            ]
        );
    }

    #[test]
    fn simple_spend_outputs_only_tx_plan_is_single_pubkey_output() {
        let source = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("src.vkey", "src.skey"),
            payment_key_info_addr: "addr_test1source".to_string(),
        };
        let destination = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("dst.vkey", "dst.skey"),
            payment_key_info_addr: "addr_test1dest".to_string(),
        };

        let plan = mk_simple_spend_outputs_only_tx_plan(
            ShelleyBasedEra::Mary,
            "work",
            "simple",
            &source,
            "cafe#1",
            destination,
            5_000_000,
        );

        assert_eq!(
            plan.args,
            vec![
                "mary",
                "transaction",
                "build",
                "--change-address",
                "addr_test1source",
                "--tx-in",
                "cafe#1",
                "--tx-out",
                "addr_test1dest+5000000",
                "--out-file",
                "work/simple.txbody",
            ]
        );
    }
}
