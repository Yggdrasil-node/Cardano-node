//! `cardano-cli` stake-pool-operator command builders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/Cli/SPO.hs.

use crate::runtime_types::SpoNodeKeys;
use crate::types::{CardanoEra, ShelleyBasedEra};

use std::path::{Path, PathBuf};

/// Planned upstream SPO certificate builder invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpoCertificatePlan {
    /// The certificate path written by upstream.
    pub output_certificate_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned upstream `generateVoteFiles` invocation for one SPO vote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpoVoteFilePlan {
    /// The vote file returned by upstream.
    pub output_vote_file_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Build upstream `createStakeDelegationCertificate` path and argv.
pub fn create_stake_delegation_certificate_plan(
    era: ShelleyBasedEra,
    temp_abs_path: impl AsRef<Path>,
    delegator_stake_verification_key: impl AsRef<Path>,
    pool_id: &str,
    output_fp: impl AsRef<Path>,
) -> SpoCertificatePlan {
    let output_certificate_path = posix_join(temp_abs_path.as_ref(), output_fp.as_ref());
    SpoCertificatePlan {
        output_certificate_path: output_certificate_path.clone(),
        args: vec![
            era.era_to_string().to_string(),
            "stake-address".to_string(),
            "stake-delegation-certificate".to_string(),
            "--stake-verification-key-file".to_string(),
            path_to_cli_arg(delegator_stake_verification_key.as_ref()),
            "--stake-pool-id".to_string(),
            pool_id.to_string(),
            "--out-file".to_string(),
            path_to_cli_arg(&output_certificate_path),
        ],
    }
}

/// Build upstream `createStakeKeyRegistrationCertificate` path and argv.
pub fn create_stake_key_registration_certificate_plan(
    era: ShelleyBasedEra,
    temp_abs_path: impl AsRef<Path>,
    stake_verification_key: impl AsRef<Path>,
    deposit: u64,
    output_fp: impl AsRef<Path>,
) -> SpoCertificatePlan {
    let output_certificate_path = posix_join(temp_abs_path.as_ref(), output_fp.as_ref());
    let mut args = vec![
        era.era_to_string().to_string(),
        "stake-address".to_string(),
        "registration-certificate".to_string(),
        "--stake-verification-key-file".to_string(),
        path_to_cli_arg(stake_verification_key.as_ref()),
        "--out-file".to_string(),
        path_to_cli_arg(&output_certificate_path),
    ];
    args.extend(maybe_conway_deposit_args(CardanoEra::from(era), deposit));
    SpoCertificatePlan {
        output_certificate_path,
        args,
    }
}

/// Build upstream `createScriptStakeRegistrationCertificate` path and argv.
pub fn create_script_stake_registration_certificate_plan(
    era: CardanoEra,
    temp_abs_path: impl AsRef<Path>,
    script_file: impl AsRef<Path>,
    deposit: u64,
    output_fp: impl AsRef<Path>,
) -> SpoCertificatePlan {
    let output_certificate_path = posix_join(temp_abs_path.as_ref(), output_fp.as_ref());
    let mut args = vec![
        era.era_to_string().to_string(),
        "stake-address".to_string(),
        "registration-certificate".to_string(),
        "--stake-script-file".to_string(),
        path_to_cli_arg(script_file.as_ref()),
        "--out-file".to_string(),
        path_to_cli_arg(&output_certificate_path),
    ];
    args.extend(maybe_conway_deposit_args(era, deposit));
    SpoCertificatePlan {
        output_certificate_path,
        args,
    }
}

/// Build upstream `createScriptStakeDelegationCertificate` path and argv.
pub fn create_script_stake_delegation_certificate_plan(
    era: CardanoEra,
    temp_abs_path: impl AsRef<Path>,
    script_file: impl AsRef<Path>,
    cold_verification_key: impl AsRef<Path>,
    output_fp: impl AsRef<Path>,
) -> SpoCertificatePlan {
    let output_certificate_path = posix_join(temp_abs_path.as_ref(), output_fp.as_ref());
    SpoCertificatePlan {
        output_certificate_path: output_certificate_path.clone(),
        args: vec![
            era.era_to_string().to_string(),
            "stake-address".to_string(),
            "stake-delegation-certificate".to_string(),
            "--stake-script-file".to_string(),
            path_to_cli_arg(script_file.as_ref()),
            "--cold-verification-key-file".to_string(),
            path_to_cli_arg(cold_verification_key.as_ref()),
            "--out-file".to_string(),
            path_to_cli_arg(&output_certificate_path),
        ],
    }
}

/// Build upstream `createStakeKeyDeregistrationCertificate` path and argv.
pub fn create_stake_key_deregistration_certificate_plan(
    era: ShelleyBasedEra,
    temp_abs_path: impl AsRef<Path>,
    stake_verification_key: impl AsRef<Path>,
    deposit: u64,
    output_fp: impl AsRef<Path>,
) -> SpoCertificatePlan {
    let output_certificate_path = posix_join(temp_abs_path.as_ref(), output_fp.as_ref());
    let mut args = vec![
        era.era_to_string().to_string(),
        "stake-address".to_string(),
        "deregistration-certificate".to_string(),
        "--stake-verification-key-file".to_string(),
        path_to_cli_arg(stake_verification_key.as_ref()),
        "--out-file".to_string(),
        path_to_cli_arg(&output_certificate_path),
    ];
    args.extend(maybe_conway_deposit_args(CardanoEra::from(era), deposit));
    SpoCertificatePlan {
        output_certificate_path,
        args,
    }
}

/// Build upstream `generateVoteFiles` paths and argv.
pub fn generate_vote_file_plans<'a, I, V>(
    era: CardanoEra,
    work: impl AsRef<Path>,
    prefix: &str,
    governance_action_tx_id: &str,
    governance_action_index: u16,
    all_votes: I,
) -> Vec<SpoVoteFilePlan>
where
    I: IntoIterator<Item = (&'a SpoNodeKeys, V)>,
    V: AsRef<str>,
{
    let base_dir = posix_join(work.as_ref(), Path::new(prefix));
    all_votes
        .into_iter()
        .enumerate()
        .map(|(idx, (spo_keys, vote))| {
            let output_vote_file_path =
                posix_join(&base_dir, Path::new(&format!("vote-spo-{}", idx + 1)));
            SpoVoteFilePlan {
                output_vote_file_path: output_vote_file_path.clone(),
                args: vec![
                    era.era_to_string().to_string(),
                    "governance".to_string(),
                    "vote".to_string(),
                    "create".to_string(),
                    format!("--{}", vote.as_ref()),
                    "--governance-action-tx-id".to_string(),
                    governance_action_tx_id.to_string(),
                    "--governance-action-index".to_string(),
                    governance_action_index.to_string(),
                    "--cold-verification-key-file".to_string(),
                    path_to_cli_arg(spo_keys.pool_node_keys_cold.verification_key_fp()),
                    "--out-file".to_string(),
                    path_to_cli_arg(&output_vote_file_path),
                ],
            }
        })
        .collect()
}

fn maybe_conway_deposit_args(era: CardanoEra, deposit: u64) -> Vec<String> {
    match era {
        CardanoEra::Conway | CardanoEra::Dijkstra => {
            vec!["--key-reg-deposit-amt".to_string(), deposit.to_string()]
        }
        _ => Vec::new(),
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
    use crate::runtime_types::{KeyPair, SpoNodeKeys, StakeKey, StakePoolKey, VrfKey};
    use crate::types::{CardanoEra, ShelleyBasedEra};

    use super::*;

    #[test]
    fn stake_key_certificate_builders_match_upstream_args() {
        let delegation = create_stake_delegation_certificate_plan(
            ShelleyBasedEra::Babbage,
            "tmp",
            "keys/stake.vkey",
            "pool1expected",
            "deleg.cert",
        );

        assert_eq!(
            delegation.output_certificate_path,
            std::path::PathBuf::from("tmp/deleg.cert")
        );
        assert_eq!(
            delegation.args,
            vec![
                "babbage",
                "stake-address",
                "stake-delegation-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--stake-pool-id",
                "pool1expected",
                "--out-file",
                "tmp/deleg.cert",
            ]
        );

        let conway_registration = create_stake_key_registration_certificate_plan(
            ShelleyBasedEra::Conway,
            "tmp",
            "keys/stake.vkey",
            2_000_000,
            "stake.regcert",
        );
        assert_eq!(
            conway_registration.output_certificate_path,
            std::path::PathBuf::from("tmp/stake.regcert")
        );
        assert_eq!(
            conway_registration.args,
            vec![
                "conway",
                "stake-address",
                "registration-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--out-file",
                "tmp/stake.regcert",
                "--key-reg-deposit-amt",
                "2000000",
            ]
        );

        let babbage_registration = create_stake_key_registration_certificate_plan(
            ShelleyBasedEra::Babbage,
            "tmp",
            "keys/stake.vkey",
            2_000_000,
            "stake.regcert",
        );
        assert_eq!(
            babbage_registration.args,
            vec![
                "babbage",
                "stake-address",
                "registration-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--out-file",
                "tmp/stake.regcert",
            ]
        );
    }

    #[test]
    fn script_stake_certificate_builders_match_upstream_args() {
        let registration = create_script_stake_registration_certificate_plan(
            CardanoEra::Conway,
            "tmp",
            "scripts/stake.plutus",
            2_000_000,
            "script.regcert",
        );

        assert_eq!(
            registration.output_certificate_path,
            std::path::PathBuf::from("tmp/script.regcert")
        );
        assert_eq!(
            registration.args,
            vec![
                "conway",
                "stake-address",
                "registration-certificate",
                "--stake-script-file",
                "scripts/stake.plutus",
                "--out-file",
                "tmp/script.regcert",
                "--key-reg-deposit-amt",
                "2000000",
            ]
        );

        let delegation = create_script_stake_delegation_certificate_plan(
            CardanoEra::Alonzo,
            "tmp",
            "scripts/stake.plutus",
            "keys/cold.vkey",
            "script.delegcert",
        );
        assert_eq!(
            delegation.args,
            vec![
                "alonzo",
                "stake-address",
                "stake-delegation-certificate",
                "--stake-script-file",
                "scripts/stake.plutus",
                "--cold-verification-key-file",
                "keys/cold.vkey",
                "--out-file",
                "tmp/script.delegcert",
            ]
        );
    }

    #[test]
    fn stake_key_deregistration_conway_adds_deposit_and_older_eras_do_not() {
        let conway = create_stake_key_deregistration_certificate_plan(
            ShelleyBasedEra::Conway,
            "tmp",
            "keys/stake.vkey",
            2_000_000,
            "stake.deregcert",
        );
        assert_eq!(
            conway.args,
            vec![
                "conway",
                "stake-address",
                "deregistration-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--out-file",
                "tmp/stake.deregcert",
                "--key-reg-deposit-amt",
                "2000000",
            ]
        );

        let mary = create_stake_key_deregistration_certificate_plan(
            ShelleyBasedEra::Mary,
            "tmp",
            "keys/stake.vkey",
            2_000_000,
            "stake.deregcert",
        );
        assert_eq!(
            mary.args,
            vec![
                "mary",
                "stake-address",
                "deregistration-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--out-file",
                "tmp/stake.deregcert",
            ]
        );

        let dijkstra = create_stake_key_deregistration_certificate_plan(
            ShelleyBasedEra::Dijkstra,
            "tmp",
            "keys/stake.vkey",
            2_000_000,
            "stake.deregcert",
        );
        assert_eq!(
            dijkstra.args,
            vec![
                "dijkstra",
                "stake-address",
                "deregistration-certificate",
                "--stake-verification-key-file",
                "keys/stake.vkey",
                "--out-file",
                "tmp/stake.deregcert",
                "--key-reg-deposit-amt",
                "2000000",
            ]
        );
    }

    #[test]
    fn spo_vote_file_plans_index_votes_and_use_cold_verification_key() {
        let first = SpoNodeKeys {
            pool_node_keys_cold: KeyPair::<StakePoolKey>::new(
                "pool-a/cold.vkey",
                "pool-a/cold.skey",
            ),
            pool_node_keys_vrf: KeyPair::<VrfKey>::new("pool-a/vrf.vkey", "pool-a/vrf.skey"),
            pool_node_keys_staking: KeyPair::<StakeKey>::new(
                "pool-a/stake.vkey",
                "pool-a/stake.skey",
            ),
        };
        let second = SpoNodeKeys {
            pool_node_keys_cold: KeyPair::<StakePoolKey>::new(
                "pool-b/cold.vkey",
                "pool-b/cold.skey",
            ),
            pool_node_keys_vrf: KeyPair::<VrfKey>::new("pool-b/vrf.vkey", "pool-b/vrf.skey"),
            pool_node_keys_staking: KeyPair::<StakeKey>::new(
                "pool-b/stake.vkey",
                "pool-b/stake.skey",
            ),
        };

        let plans = generate_vote_file_plans(
            CardanoEra::Conway,
            "work",
            "votes",
            "txid123",
            3,
            [(&first, "yes"), (&second, "no")],
        );

        assert_eq!(plans.len(), 2);
        assert_eq!(
            plans[0].output_vote_file_path,
            std::path::PathBuf::from("work/votes/vote-spo-1")
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
                "txid123",
                "--governance-action-index",
                "3",
                "--cold-verification-key-file",
                "pool-a/cold.vkey",
                "--out-file",
                "work/votes/vote-spo-1",
            ]
        );
        assert_eq!(
            plans[1].args,
            vec![
                "conway",
                "governance",
                "vote",
                "create",
                "--no",
                "--governance-action-tx-id",
                "txid123",
                "--governance-action-index",
                "3",
                "--cold-verification-key-file",
                "pool-b/cold.vkey",
                "--out-file",
                "work/votes/vote-spo-2",
            ]
        );
    }
}
