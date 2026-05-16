//! cardano-cli argument parser.
//!
//! Mirrors upstream `Cardano.CLI.Parser` — the optparse-applicative
//! parser that produces a `ClientCommand` from `argv`. Yggdrasil uses
//! `clap` (`derive` style) instead of optparse-applicative; the
//! upstream parser layout is the conceptual mirror.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Parser.hs`.
//! R289 ships only the top-level `parse_command` shell. The full
//! parser tree (per-cluster sub-parsers for Byron / Compatible /
//! per-era / Legacy) lands in R290–R295 alongside the runners.

use clap::Parser;

use crate::command::Command;

/// Top-level clap `Parser` shell. The `command: Command` field
/// expands via the `Subcommand` derive on `Command` into the
/// three subcommand arms `version` / `show-upstream-config` /
/// `query-tip`. Mirrors upstream's `ClientCommand` optparse-
/// applicative aggregate parser.
#[derive(Parser)]
#[command(name = "yggdrasil-cardano-cli", version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

/// Parse `argv` into a [`Command`].
///
/// Mirrors upstream `parseClientCommand` from `Cardano.CLI.Parser`.
/// R503 (May 2026): wired to clap's `try_parse_from` via the new
/// `Args { command: Command }` aggregate — was an
/// `Err(NotYetMigrated)` stub before. Callers (the standalone
/// `[[bin]]` target when it lands, plus the existing
/// `parser::tests`) can now actually produce `Command::Version` /
/// `ShowUpstreamConfig { upstream_config_root }` /
/// `QueryTip { socket_path, network_magic }` from argv.
pub fn parse_command<I, T>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let parsed = Args::try_parse_from(args)?;
    Ok(parsed.command)
}

/// Parse error returned by [`parse_command`].
///
/// Mirrors upstream `ClientCommandErrors` from `Cardano.CLI.Run`.
/// R503 retired the prior `NotYetMigrated` variant after `parse_command`
/// became operational via `Args::try_parse_from`; today the only
/// failure mode is the clap parser itself (which also raises
/// `--help` / `--version` short-circuit "errors" with
/// `kind() == DisplayHelp / DisplayVersion`).
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// A clap parser failure surfaced through. Also wraps clap's
    /// `--help` / `--version` short-circuit "errors" — the binary
    /// `main` discriminates via `err.kind()` and prints those on
    /// stdout (exit 0) vs other variants on stderr (exit 2).
    #[error("{0}")]
    Clap(#[from] clap::Error),
}

/// Re-export of `clap::Parser` so consumers can wire their own
/// derive-based parsers against the same Command type. Useful for the
/// node binary's transitional integration in R289.
pub trait ClapBackend: Parser {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `version` subcommand parses cleanly to `Command::Version`.
    #[test]
    fn parses_version_subcommand() {
        let cmd = parse_command(["yggdrasil-cardano-cli", "version"]).expect("parse");
        assert_eq!(cmd, Command::Version);
    }

    /// `show-upstream-config --network mainnet` parses to the
    /// expected variant with `upstream_config_root: None`.
    #[test]
    fn parses_show_upstream_config_default() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "show-upstream-config",
            "--network",
            "mainnet",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::ShowUpstreamConfig {
                network: "mainnet".to_string(),
                upstream_config_root: None,
            }
        );
    }

    /// `show-upstream-config --network preview --upstream-config-root /opt/...`
    /// parses the operator-supplied override.
    #[test]
    fn parses_show_upstream_config_with_root() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "show-upstream-config",
            "--network",
            "preview",
            "--upstream-config-root",
            "/opt/cardano",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::ShowUpstreamConfig {
                network: "preview".to_string(),
                upstream_config_root: Some(PathBuf::from("/opt/cardano")),
            }
        );
    }

    /// `query-tip --socket-path /tmp/node.socket` parses with the
    /// canonical socket-path argument.
    #[test]
    fn parses_query_tip_with_socket_path() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-tip",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryTip {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// `query-tip --socket-path … --network-magic N` parses the
    /// magic override.
    #[test]
    fn parses_query_tip_with_network_magic() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-tip",
            "--socket-path",
            "/tmp/node.socket",
            "--network-magic",
            "764824073",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryTip {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: Some(764_824_073),
            }
        );
    }

    /// `query-chain-block-no --socket-path …` parses with the
    /// canonical socket-path argument.
    #[test]
    fn parses_query_chain_block_no() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-chain-block-no",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryChainBlockNo {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// `query-current-era --socket-path …` parses to the expected
    /// variant.
    #[test]
    fn parses_query_current_era() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-current-era",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QueryCurrentEra {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// `query-system-start --socket-path …` parses to the expected
    /// variant.
    #[test]
    fn parses_query_system_start() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "query-system-start",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::QuerySystemStart {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// The socket-only `query-*` subcommands all parse to their
    /// expected variant with just `--socket-path`.
    #[test]
    fn parses_socket_only_query_subcommands() {
        let stake_distribution = parse_command([
            "yggdrasil-cardano-cli",
            "query-stake-distribution",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            stake_distribution,
            Command::QueryStakeDistribution {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
        let stake_pools = parse_command([
            "yggdrasil-cardano-cli",
            "query-stake-pools",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            stake_pools,
            Command::QueryStakePools {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
        let protocol_parameters = parse_command([
            "yggdrasil-cardano-cli",
            "query-protocol-parameters",
            "--socket-path",
            "/tmp/node.socket",
        ])
        .expect("parse");
        assert_eq!(
            protocol_parameters,
            Command::QueryProtocolParameters {
                socket_path: PathBuf::from("/tmp/node.socket"),
                network_magic: None,
            }
        );
    }

    /// The 5 Conway-governance `query-*` subcommands all parse to
    /// their expected variant with just `--socket-path`.
    #[test]
    fn parses_governance_query_subcommands() {
        let socket = "/tmp/node.socket";
        let drep_distr = parse_command([
            "yggdrasil-cardano-cli",
            "query-drep-stake-distribution",
            "--socket-path",
            socket,
        ])
        .expect("parse");
        assert_eq!(
            drep_distr,
            Command::QueryDrepStakeDistribution {
                socket_path: PathBuf::from(socket),
                network_magic: None,
            }
        );
        let constitution = parse_command([
            "yggdrasil-cardano-cli",
            "query-constitution",
            "--socket-path",
            socket,
        ])
        .expect("parse");
        assert_eq!(
            constitution,
            Command::QueryConstitution {
                socket_path: PathBuf::from(socket),
                network_magic: None,
            }
        );
        let gov_state = parse_command([
            "yggdrasil-cardano-cli",
            "query-gov-state",
            "--socket-path",
            socket,
        ])
        .expect("parse");
        assert_eq!(
            gov_state,
            Command::QueryGovState {
                socket_path: PathBuf::from(socket),
                network_magic: None,
            }
        );
        let drep_state = parse_command([
            "yggdrasil-cardano-cli",
            "query-drep-state",
            "--socket-path",
            socket,
        ])
        .expect("parse");
        assert_eq!(
            drep_state,
            Command::QueryDrepState {
                socket_path: PathBuf::from(socket),
                network_magic: None,
            }
        );
        let committee = parse_command([
            "yggdrasil-cardano-cli",
            "query-committee-state",
            "--socket-path",
            socket,
        ])
        .expect("parse");
        assert_eq!(
            committee,
            Command::QueryCommitteeState {
                socket_path: PathBuf::from(socket),
                network_magic: None,
            }
        );
    }

    /// `address-key-gen --verification-key-file … --signing-key-file …`
    /// parses to the expected variant.
    #[test]
    fn parses_address_key_gen() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "address-key-gen",
            "--verification-key-file",
            "/tmp/p.vkey",
            "--signing-key-file",
            "/tmp/p.skey",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::AddressKeyGen {
                verification_key_file: PathBuf::from("/tmp/p.vkey"),
                signing_key_file: PathBuf::from("/tmp/p.skey"),
            }
        );
    }

    /// `address-key-hash --payment-verification-key-file …` parses
    /// to the expected variant.
    #[test]
    fn parses_address_key_hash() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "address-key-hash",
            "--payment-verification-key-file",
            "/tmp/p.vkey",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::AddressKeyHash {
                payment_verification_key_file: PathBuf::from("/tmp/p.vkey"),
            }
        );
    }

    /// `stake-address-key-gen --verification-key-file …
    /// --signing-key-file …` parses to the expected variant.
    #[test]
    fn parses_stake_address_key_gen() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "stake-address-key-gen",
            "--verification-key-file",
            "/tmp/s.vkey",
            "--signing-key-file",
            "/tmp/s.skey",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::StakeAddressKeyGen {
                verification_key_file: PathBuf::from("/tmp/s.vkey"),
                signing_key_file: PathBuf::from("/tmp/s.skey"),
            }
        );
    }

    /// `transaction-txid --tx-hex …` parses to the expected variant.
    #[test]
    fn parses_transaction_txid_with_hex() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "transaction-txid",
            "--tx-hex",
            "82a0a0",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::TransactionTxid {
                tx_file: None,
                tx_hex: Some("82a0a0".to_string()),
            }
        );
    }

    /// `transaction-txid` rejects `--tx-file` + `--tx-hex` together
    /// (clap `conflicts_with`).
    #[test]
    fn transaction_txid_rejects_both_tx_flags() {
        let result = parse_command([
            "yggdrasil-cardano-cli",
            "transaction-txid",
            "--tx-file",
            "/tmp/tx.cbor",
            "--tx-hex",
            "82a0a0",
        ]);
        assert!(
            matches!(result, Err(ParseError::Clap(_))),
            "conflicting --tx-file + --tx-hex must be a clap error; got {result:?}"
        );
    }

    /// `address-build --payment-verification-key-file … --mainnet`
    /// parses to the expected variant.
    #[test]
    fn parses_address_build_mainnet() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "address-build",
            "--payment-verification-key-file",
            "/tmp/p.vkey",
            "--mainnet",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::AddressBuild {
                payment_verification_key_file: PathBuf::from("/tmp/p.vkey"),
                stake_verification_key_file: None,
                mainnet: true,
                testnet_magic: None,
                out_file: None,
            }
        );
    }

    /// `address-build` rejects `--mainnet` + `--testnet-magic`
    /// together (clap `conflicts_with`).
    #[test]
    fn address_build_rejects_both_network_flags() {
        let result = parse_command([
            "yggdrasil-cardano-cli",
            "address-build",
            "--payment-verification-key-file",
            "/tmp/p.vkey",
            "--mainnet",
            "--testnet-magic",
            "2",
        ]);
        assert!(
            matches!(result, Err(ParseError::Clap(_))),
            "conflicting --mainnet + --testnet-magic must be a clap error; got {result:?}"
        );
    }

    /// `stake-address-build --stake-verification-key-file …
    /// --testnet-magic …` parses to the expected variant.
    #[test]
    fn parses_stake_address_build_testnet() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "stake-address-build",
            "--stake-verification-key-file",
            "/tmp/s.vkey",
            "--testnet-magic",
            "2",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::StakeAddressBuild {
                stake_verification_key_file: PathBuf::from("/tmp/s.vkey"),
                mainnet: false,
                testnet_magic: Some(2),
                out_file: None,
            }
        );
    }

    /// `transaction-sign --tx-hex … --signing-key-file …
    /// --out-file …` parses to the expected variant.
    #[test]
    fn parses_transaction_sign() {
        let cmd = parse_command([
            "yggdrasil-cardano-cli",
            "transaction-sign",
            "--tx-hex",
            "82a0a0",
            "--signing-key-file",
            "/tmp/p.skey",
            "--out-file",
            "/tmp/signed.tx",
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::TransactionSign {
                tx_file: None,
                tx_hex: Some("82a0a0".to_string()),
                signing_key_file: PathBuf::from("/tmp/p.skey"),
                out_file: PathBuf::from("/tmp/signed.tx"),
            }
        );
    }

    /// Unknown subcommand surfaces through `ParseError::Clap`.
    #[test]
    fn rejects_unknown_subcommand() {
        let result = parse_command(["yggdrasil-cardano-cli", "bogus-subcommand"]);
        assert!(
            matches!(result, Err(ParseError::Clap(_))),
            "expected ParseError::Clap; got {result:?}"
        );
    }
}
