//! `cardano-cli` key command builders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/Cli/Keys.hs.

use crate::runtime_types::{KesKey, KeyPair, PaymentKey, StakeKey, StakePoolKey, VrfKey};

use std::path::{Path, PathBuf};

/// Marker for upstream `OperatorCounter`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OperatorCounter;

/// Marker for a Byron delegation key file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ByronDelegationKey;

/// Marker for a Byron delegation certificate file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ByronDelegationCert;

/// Marker for a legacy Byron signing key file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ByronKeyLegacy;

/// Marker for a Byron address file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ByronAddr;

/// Planned legacy Byron `keygen` invocation and output path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliKeyGenPlan {
    /// The key path returned by upstream `cliKeyGen`.
    pub output_key_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Planned legacy Byron `signing-key-address` invocation and output path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliByronSigningKeyAddressPlan {
    /// The address file written by upstream `cliByronSigningKeyAddress`.
    pub output_address_path: PathBuf,
    /// Arguments passed to `cardano-cli`.
    pub args: Vec<String>,
}

/// Build upstream `cliAddressKeyGen` argv.
pub fn cli_address_key_gen_args(key_pair: &KeyPair<PaymentKey>) -> Vec<String> {
    shelley_key_gen_args("address", "key-gen", key_pair)
}

/// Build upstream `cliStakeAddressKeyGen` argv.
pub fn cli_stake_address_key_gen_args(key_pair: &KeyPair<StakeKey>) -> Vec<String> {
    shelley_key_gen_args("stake-address", "key-gen", key_pair)
}

/// Build upstream `cliNodeKeyGenVrf` argv.
pub fn cli_node_key_gen_vrf_args(key_pair: &KeyPair<VrfKey>) -> Vec<String> {
    shelley_key_gen_args("node", "key-gen-VRF", key_pair)
}

/// Build upstream `cliNodeKeyGenKes` argv.
pub fn cli_node_key_gen_kes_args(key_pair: &KeyPair<KesKey>) -> Vec<String> {
    shelley_key_gen_args("node", "key-gen-KES", key_pair)
}

/// Build upstream `cliNodeKeyGen` argv.
pub fn cli_node_key_gen_args(
    key_pair: &KeyPair<StakePoolKey>,
    counter_path: impl AsRef<Path>,
) -> Vec<String> {
    vec![
        "latest".to_string(),
        "node".to_string(),
        "key-gen".to_string(),
        "--cold-verification-key-file".to_string(),
        path_to_cli_arg(key_pair.verification_key_fp()),
        "--cold-signing-key-file".to_string(),
        path_to_cli_arg(key_pair.signing_key_fp()),
        "--operational-certificate-issue-counter-file".to_string(),
        path_to_cli_arg(counter_path.as_ref()),
    ]
}

/// Build upstream `cliKeyGen` path and argv.
pub fn cli_key_gen_plan(tmp_dir: impl AsRef<Path>, key: impl AsRef<Path>) -> CliKeyGenPlan {
    let output_key_path = posix_join(tmp_dir.as_ref(), key.as_ref());
    CliKeyGenPlan {
        args: vec![
            "keygen".to_string(),
            "--secret".to_string(),
            path_to_cli_arg(&output_key_path),
        ],
        output_key_path,
    }
}

/// Build upstream `cliByronSigningKeyAddress` path and argv.
pub fn cli_byron_signing_key_address_plan(
    tmp_dir: impl AsRef<Path>,
    testnet_magic: i64,
    key: impl AsRef<Path>,
    dest_path: impl AsRef<Path>,
) -> CliByronSigningKeyAddressPlan {
    let tmp_dir = tmp_dir.as_ref();
    let output_address_path = posix_join(tmp_dir, dest_path.as_ref());
    let secret_path = posix_join(tmp_dir, key.as_ref());
    CliByronSigningKeyAddressPlan {
        args: vec![
            "signing-key-address".to_string(),
            "--testnet-magic".to_string(),
            testnet_magic.to_string(),
            "--secret".to_string(),
            path_to_cli_arg(&secret_path),
        ],
        output_address_path,
    }
}

fn shelley_key_gen_args<K>(
    command: &'static str,
    subcommand: &'static str,
    key_pair: &KeyPair<K>,
) -> Vec<String> {
    vec![
        "latest".to_string(),
        command.to_string(),
        subcommand.to_string(),
        "--verification-key-file".to_string(),
        path_to_cli_arg(key_pair.verification_key_fp()),
        "--signing-key-file".to_string(),
        path_to_cli_arg(key_pair.signing_key_fp()),
    ]
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
    use crate::runtime_types::KeyPair;

    #[test]
    fn shelley_keygen_builders_match_upstream_cli_argv() {
        let payment: KeyPair<PaymentKey> = KeyPair::new("keys/payment.vkey", "keys/payment.skey");
        assert_eq!(
            cli_address_key_gen_args(&payment),
            vec![
                "latest",
                "address",
                "key-gen",
                "--verification-key-file",
                "keys/payment.vkey",
                "--signing-key-file",
                "keys/payment.skey",
            ]
        );

        let stake: KeyPair<StakeKey> = KeyPair::new("keys/stake.vkey", "keys/stake.skey");
        assert_eq!(
            cli_stake_address_key_gen_args(&stake),
            vec![
                "latest",
                "stake-address",
                "key-gen",
                "--verification-key-file",
                "keys/stake.vkey",
                "--signing-key-file",
                "keys/stake.skey",
            ]
        );

        let vrf: KeyPair<VrfKey> = KeyPair::new("keys/vrf.vkey", "keys/vrf.skey");
        assert_eq!(
            cli_node_key_gen_vrf_args(&vrf),
            vec![
                "latest",
                "node",
                "key-gen-VRF",
                "--verification-key-file",
                "keys/vrf.vkey",
                "--signing-key-file",
                "keys/vrf.skey",
            ]
        );

        let kes: KeyPair<KesKey> = KeyPair::new("keys/kes.vkey", "keys/kes.skey");
        assert_eq!(
            cli_node_key_gen_kes_args(&kes),
            vec![
                "latest",
                "node",
                "key-gen-KES",
                "--verification-key-file",
                "keys/kes.vkey",
                "--signing-key-file",
                "keys/kes.skey",
            ]
        );
    }

    #[test]
    fn node_keygen_uses_cold_key_and_operator_counter_flags() {
        let pool: KeyPair<StakePoolKey> = KeyPair::new("pool/cold.vkey", "pool/cold.skey");
        assert_eq!(
            cli_node_key_gen_args(&pool, "pool/operator.counter"),
            vec![
                "latest",
                "node",
                "key-gen",
                "--cold-verification-key-file",
                "pool/cold.vkey",
                "--cold-signing-key-file",
                "pool/cold.skey",
                "--operational-certificate-issue-counter-file",
                "pool/operator.counter",
            ]
        );
    }

    #[test]
    fn byron_legacy_builders_return_output_paths_and_argv() {
        let key = cli_key_gen_plan("work", "delegate.key");
        assert_eq!(
            key.output_key_path,
            std::path::PathBuf::from("work/delegate.key")
        );
        assert_eq!(key.args, vec!["keygen", "--secret", "work/delegate.key"]);

        let address =
            cli_byron_signing_key_address_plan("work", 42, "work/delegate.key", "delegate.addr");
        assert_eq!(
            address.output_address_path,
            std::path::PathBuf::from("work/delegate.addr")
        );
        assert_eq!(
            address.args,
            vec![
                "signing-key-address",
                "--testnet-magic",
                "42",
                "--secret",
                "work/work/delegate.key",
            ]
        );
    }
}
