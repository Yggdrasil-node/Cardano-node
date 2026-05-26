//! `cardano-cli` DRep command builders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/Cli/DRep.hs.

use crate::runtime_types::{KeyPair, PaymentKey, PaymentKeyInfo};

use std::path::{Path, PathBuf};

/// Marker for upstream `data Certificate`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Certificate;

/// Planned upstream `generateDRepKeyPair` invocation and output keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DRepKeyPairPlan {
    /// The key pair returned by upstream `generateDRepKeyPair`.
    pub key_pair: KeyPair<PaymentKey>,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned upstream `generateRegistrationCertificate` invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DRepRegistrationCertificatePlan {
    /// The registration certificate returned by upstream.
    pub output_certificate_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned upstream `generateVoteFiles` invocation for one vote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DRepVoteFilePlan {
    /// The vote file returned by upstream.
    pub output_vote_file_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned upstream DRep transaction-body builder invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DRepTxBodyPlan {
    /// The txbody file returned by upstream.
    pub output_tx_body_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Build upstream `generateDRepKeyPair` output paths and argv.
pub fn generate_drep_key_pair_plan(work: impl AsRef<Path>, prefix: &str) -> DRepKeyPairPlan {
    let base_dir = posix_join(work.as_ref(), Path::new(prefix));
    let verification_key = posix_join(&base_dir, Path::new("verification.vkey"));
    let signing_key = posix_join(&base_dir, Path::new("signature.skey"));
    let key_pair = KeyPair::<PaymentKey>::new(verification_key, signing_key);
    let args = vec![
        "conway".to_string(),
        "governance".to_string(),
        "drep".to_string(),
        "key-gen".to_string(),
        "--verification-key-file".to_string(),
        path_to_cli_arg(key_pair.verification_key_fp()),
        "--signing-key-file".to_string(),
        path_to_cli_arg(key_pair.signing_key_fp()),
    ];
    DRepKeyPairPlan { key_pair, args }
}

/// Build upstream `generateRegistrationCertificate` path and argv.
pub fn generate_registration_certificate_plan(
    work: impl AsRef<Path>,
    prefix: &str,
    drep_key_pair: &KeyPair<PaymentKey>,
    deposit_amount: i128,
) -> DRepRegistrationCertificatePlan {
    let certificate_name = format!("{prefix}.regcert");
    let output_certificate_path = posix_join(work.as_ref(), Path::new(&certificate_name));
    DRepRegistrationCertificatePlan {
        output_certificate_path: output_certificate_path.clone(),
        args: vec![
            "conway".to_string(),
            "governance".to_string(),
            "drep".to_string(),
            "registration-certificate".to_string(),
            "--drep-verification-key-file".to_string(),
            path_to_cli_arg(drep_key_pair.verification_key_fp()),
            "--key-reg-deposit-amt".to_string(),
            deposit_amount.to_string(),
            "--out-file".to_string(),
            path_to_cli_arg(&output_certificate_path),
        ],
    }
}

/// Build upstream `generateVoteFiles` paths and argv.
pub fn generate_vote_file_plans<'a, I, V>(
    work: impl AsRef<Path>,
    prefix: &str,
    governance_action_tx_id: &str,
    governance_action_index: u16,
    all_votes: I,
) -> Vec<DRepVoteFilePlan>
where
    I: IntoIterator<Item = (&'a KeyPair<PaymentKey>, V)>,
    V: AsRef<str>,
{
    let base_dir = posix_join(work.as_ref(), Path::new(prefix));
    all_votes
        .into_iter()
        .enumerate()
        .map(|(idx, (drep_key_pair, vote))| {
            let output_vote_file_path =
                posix_join(&base_dir, Path::new(&format!("vote-drep-{}", idx + 1)));
            DRepVoteFilePlan {
                output_vote_file_path: output_vote_file_path.clone(),
                args: vec![
                    "conway".to_string(),
                    "governance".to_string(),
                    "vote".to_string(),
                    "create".to_string(),
                    format!("--{}", vote.as_ref()),
                    "--governance-action-tx-id".to_string(),
                    governance_action_tx_id.to_string(),
                    "--governance-action-index".to_string(),
                    governance_action_index.to_string(),
                    "--drep-verification-key-file".to_string(),
                    path_to_cli_arg(drep_key_pair.verification_key_fp()),
                    "--out-file".to_string(),
                    path_to_cli_arg(&output_vote_file_path),
                ],
            }
        })
        .collect()
}

/// Build upstream `createCertificatePublicationTxBody` path and argv from a
/// preselected tx input.
pub fn create_certificate_publication_tx_body_plan(
    work: impl AsRef<Path>,
    prefix: &str,
    certificate: impl AsRef<Path>,
    wallet: &PaymentKeyInfo,
    tx_in: &str,
) -> DRepTxBodyPlan {
    let tx_body_name = format!("{prefix}.txbody");
    let output_tx_body_path = posix_join(work.as_ref(), Path::new(&tx_body_name));
    DRepTxBodyPlan {
        output_tx_body_path: output_tx_body_path.clone(),
        args: vec![
            "conway".to_string(),
            "transaction".to_string(),
            "build".to_string(),
            "--change-address".to_string(),
            wallet.payment_key_info_addr.clone(),
            "--tx-in".to_string(),
            tx_in.to_string(),
            "--certificate-file".to_string(),
            path_to_cli_arg(certificate.as_ref()),
            "--witness-override".to_string(),
            "2".to_string(),
            "--out-file".to_string(),
            path_to_cli_arg(&output_tx_body_path),
        ],
    }
}

/// Build upstream `createVotingTxBody` path and argv from preselected tx input.
pub fn create_voting_tx_body_plan<I, P>(
    work: impl AsRef<Path>,
    prefix: &str,
    vote_files: I,
    wallet: &PaymentKeyInfo,
    tx_in: &str,
) -> DRepTxBodyPlan
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let tx_body_name = format!("{prefix}.txbody");
    let output_tx_body_path = posix_join(work.as_ref(), Path::new(&tx_body_name));
    let vote_files: Vec<String> = vote_files
        .into_iter()
        .map(|vote_file| path_to_cli_arg(vote_file.as_ref()))
        .collect();
    let mut args = vec![
        "conway".to_string(),
        "transaction".to_string(),
        "build".to_string(),
        "--change-address".to_string(),
        wallet.payment_key_info_addr.clone(),
        "--tx-in".to_string(),
        tx_in.to_string(),
    ];
    for vote_file in &vote_files {
        args.push("--vote-file".to_string());
        args.push(vote_file.clone());
    }
    args.push("--witness-override".to_string());
    args.push(vote_files.len().to_string());
    args.push("--out-file".to_string());
    args.push(path_to_cli_arg(&output_tx_body_path));
    DRepTxBodyPlan {
        output_tx_body_path,
        args,
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
    use crate::runtime_types::{KeyPair, PaymentKey, PaymentKeyInfo};

    use super::*;

    #[test]
    fn drep_keygen_and_registration_certificate_match_upstream_args() {
        let keygen = generate_drep_key_pair_plan("work", "drep-1");

        assert_eq!(
            keygen.key_pair,
            KeyPair::<PaymentKey>::new(
                "work/drep-1/verification.vkey",
                "work/drep-1/signature.skey"
            )
        );
        assert_eq!(
            keygen.args,
            vec![
                "conway",
                "governance",
                "drep",
                "key-gen",
                "--verification-key-file",
                "work/drep-1/verification.vkey",
                "--signing-key-file",
                "work/drep-1/signature.skey",
            ]
        );

        let registration =
            generate_registration_certificate_plan("work", "drep-1", &keygen.key_pair, 500);

        assert_eq!(
            registration.output_certificate_path,
            std::path::PathBuf::from("work/drep-1.regcert")
        );
        assert_eq!(
            registration.args,
            vec![
                "conway",
                "governance",
                "drep",
                "registration-certificate",
                "--drep-verification-key-file",
                "work/drep-1/verification.vkey",
                "--key-reg-deposit-amt",
                "500",
                "--out-file",
                "work/drep-1.regcert",
            ]
        );
    }

    #[test]
    fn drep_vote_file_plans_index_votes_and_render_vote_flags() {
        let yes = KeyPair::<PaymentKey>::new("drep-a.vkey", "drep-a.skey");
        let abstain = KeyPair::<PaymentKey>::new("drep-b.vkey", "drep-b.skey");

        let plans = generate_vote_file_plans(
            "work",
            "votes",
            "abc123",
            7,
            [(&yes, "yes"), (&abstain, "abstain")],
        );

        assert_eq!(plans.len(), 2);
        assert_eq!(
            plans[0].output_vote_file_path,
            std::path::PathBuf::from("work/votes/vote-drep-1")
        );
        assert_eq!(
            plans[0].args,
            vec![
                "conway",
                "governance",
                "vote",
                "create",
                "--yes",
                "--governance-action-tx-id",
                "abc123",
                "--governance-action-index",
                "7",
                "--drep-verification-key-file",
                "drep-a.vkey",
                "--out-file",
                "work/votes/vote-drep-1",
            ]
        );
        assert_eq!(
            plans[1].args,
            vec![
                "conway",
                "governance",
                "vote",
                "create",
                "--abstain",
                "--governance-action-tx-id",
                "abc123",
                "--governance-action-index",
                "7",
                "--drep-verification-key-file",
                "drep-b.vkey",
                "--out-file",
                "work/votes/vote-drep-2",
            ]
        );
    }

    #[test]
    fn drep_certificate_and_voting_txbody_plans_match_upstream_witness_overrides() {
        let wallet = PaymentKeyInfo {
            payment_key_info_pair: KeyPair::new("wallet.vkey", "wallet.skey"),
            payment_key_info_addr: "addr_test1wallet".to_string(),
        };

        let certificate = create_certificate_publication_tx_body_plan(
            "work",
            "reg-cert-txbody",
            "work/drep.regcert",
            &wallet,
            "abcd#0",
        );

        assert_eq!(
            certificate.output_tx_body_path,
            std::path::PathBuf::from("work/reg-cert-txbody.txbody")
        );
        assert_eq!(
            certificate.args,
            vec![
                "conway",
                "transaction",
                "build",
                "--change-address",
                "addr_test1wallet",
                "--tx-in",
                "abcd#0",
                "--certificate-file",
                "work/drep.regcert",
                "--witness-override",
                "2",
                "--out-file",
                "work/reg-cert-txbody.txbody",
            ]
        );

        let voting = create_voting_tx_body_plan(
            "work",
            "vote-txbody",
            ["work/votes/vote-drep-1", "work/votes/vote-drep-2"],
            &wallet,
            "ef01#2",
        );

        assert_eq!(
            voting.output_tx_body_path,
            std::path::PathBuf::from("work/vote-txbody.txbody")
        );
        assert_eq!(
            voting.args,
            vec![
                "conway",
                "transaction",
                "build",
                "--change-address",
                "addr_test1wallet",
                "--tx-in",
                "ef01#2",
                "--vote-file",
                "work/votes/vote-drep-1",
                "--vote-file",
                "work/votes/vote-drep-2",
                "--witness-override",
                "2",
                "--out-file",
                "work/vote-txbody.txbody",
            ]
        );
    }
}
