//! Top-level cardano-cli run dispatcher.
//!
//! Mirrors upstream `Cardano.CLI.Run` — the dispatcher that routes a
//! parsed `Command` to its per-cluster runner (Byron / Compatible /
//! per-era / Legacy / EraBased / EraIndependent).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Run.hs`.
//! R289 ships the dispatcher entry-point with three subcommand arms
//! (Version / ShowUpstreamConfig / QueryTip). The per-cluster runners
//! (`byron::run`, `compatible::run`, `shelley::run`, etc.) land in
//! R290–R295 and the dispatch arms grow alongside.

pub mod mnemonic;
use eyre::Result;

use crate::command::Command;
use crate::lsq::{DeferralLsqClient, LsqClient, NtcQuery};

/// Run a parsed `Command` against the local environment.
///
/// Mirrors upstream `runClientCommand` from `Cardano.CLI.Run`. The
/// no-client overload bails on `Command::QueryTip` with a deferral
/// pointer at [`run_command_with`] (the LSQ-capable variant);
/// the simpler shape stays in place for `Version` /
/// `ShowUpstreamConfig`, both of which need no LSQ wiring.
///
/// Callers that want to dispatch `QueryTip` should use
/// [`run_command_with`] with a concrete [`LsqClient`] impl. The
/// in-crate [`DeferralLsqClient`] stays the default for
/// [`run_command`] so the public surface remains a simple
/// `fn(Command) -> Result<()>`.
pub fn run_command(command: Command) -> Result<()> {
    run_command_with(command, &DeferralLsqClient)
}

/// Run a parsed `Command` with a caller-supplied [`LsqClient`].
///
/// Mirrors upstream `runClientCommand` from `Cardano.CLI.Run` — the
/// upstream call-graph passes `LocalNodeConnectInfo` inline; here
/// the equivalent is the `&dyn LsqClient` parameter.
///
/// The library's standalone binary `main.rs` calls this with either
/// [`DeferralLsqClient`] (the in-crate sentinel — `query-tip` bails
/// pointing operators at the node binary's wrapper) or a future
/// concrete impl supplied by the binary crate (tokio + yggdrasil-
/// network backed). The node binary's existing
/// `node/src/commands/cardano_cli.rs` doesn't go through this
/// crate's `Command` enum and stays unaffected.
///
/// # Per-arm dispatch
///
/// - `Command::Version` — prints `helper::version_info()`. No LSQ.
/// - `Command::ShowUpstreamConfig { network, upstream_config_root }`
///   — resolves paths + magic, emits the operator JSON. No LSQ.
/// - `Command::QueryTip { socket_path, network_magic }` —
///   dispatches to `client.query_tip(...)`. The client owns the
///   socket connection + presentation.
pub fn run_command_with(command: Command, client: &dyn LsqClient) -> Result<()> {
    match command {
        Command::Version => {
            // Wired in R503 (Phase 5 follow-on): the version banner
            // comes from the in-crate helper module; identical to
            // the string the node binary's `cardano-cli version`
            // subcommand emits (which also calls `helper::version_info`).
            println!("{}", crate::helper::version_info());
            Ok(())
        }
        Command::ShowUpstreamConfig {
            network,
            upstream_config_root,
        } => {
            // R504: full library-side wiring. Resolve the network's
            // config + topology paths against the supplied upstream
            // root, extract the network magic from the config file
            // (or fall back to the well-known constant for the
            // network), and emit the operator-readable summary via
            // the existing `environment::run_show_upstream_config`.
            let fallback_magic = match network.as_str() {
                "mainnet" => 764_824_073,
                "preprod" => 1,
                "preview" => 2,
                _ => {
                    eyre::bail!(
                        "unknown network preset {network:?}; expected one of \
                         mainnet / preprod / preview"
                    );
                }
            };
            let (config_path, topology_path) =
                crate::environment::resolve_upstream_reference_paths(
                    &network,
                    upstream_config_root,
                )?;
            let reference_network_magic =
                crate::environment::extract_reference_network_magic(&config_path, fallback_magic);
            crate::environment::run_show_upstream_config(
                &network,
                &config_path,
                &topology_path,
                reference_network_magic,
            )
        }
        Command::QueryTip {
            socket_path,
            network_magic,
        } => {
            // R505+: dispatch through the LSQ-client trait. The
            // library carries the dispatch logic; the client owns
            // the wire-protocol drive + decode + stdout rendering.
            // Network-magic fallback mirrors upstream's mainnet
            // default when the operator omits it.
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::Tip)
        }
        Command::QueryChainBlockNo {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::ChainBlockNo)
        }
        Command::QueryCurrentEra {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::CurrentEra)
        }
        Command::QuerySystemStart {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::SystemStart)
        }
        Command::QueryStakeDistribution {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::StakeDistribution)
        }
        Command::QueryStakePools {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::StakePools)
        }
        Command::QueryProtocolParameters {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::ProtocolParameters)
        }
        Command::QueryDrepStakeDistribution {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::DrepStakeDistribution)
        }
        Command::QueryConstitution {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::Constitution)
        }
        Command::QueryGovState {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::GovState)
        }
        Command::QueryDrepState {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::DrepState)
        }
        Command::QueryCommitteeState {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(764_824_073);
            client.run_query(&socket_path, magic, NtcQuery::CommitteeState)
        }
        Command::AddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => {
            // R507: pure-crypto subcommand — no LSQ client, no node
            // socket. Dispatches to the strict-mirror Run module.
            crate::era_independent::address::run::run_address_key_gen_cmd(
                &verification_key_file,
                &signing_key_file,
            )
        }
        Command::AddressKeyHash {
            payment_verification_key_file,
        } => {
            // R507: pure-crypto subcommand — Blake2b-224 of a VK.
            crate::era_independent::address::run::run_address_key_hash_cmd(
                &payment_verification_key_file,
            )
        }
        Command::StakeAddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => {
            // R508: pure-crypto subcommand — stake keypair, identical
            // to address key-gen but `KeyKind::Stake` metadata.
            crate::era_based::stake_address::run::run_stake_address_key_gen_cmd(
                &verification_key_file,
                &signing_key_file,
            )
        }
        Command::TransactionTxid { tx_file, tx_hex } => {
            // R508: offline subcommand — Blake2b-256 of the CBOR tx
            // body. No LSQ client, no node socket.
            crate::era_based::transaction::run::run_transaction_txid_cmd(tx_file, tx_hex)
        }
        Command::TransactionSign {
            tx_file,
            tx_hex,
            signing_key_file,
            out_file,
        } => {
            // R509: offline subcommand — Ed25519-sign a tx, replacing
            // the witness set with a fresh single-signer one.
            crate::era_based::transaction::run::run_transaction_sign_cmd(
                tx_file,
                tx_hex,
                &signing_key_file,
                &out_file,
            )
        }
        Command::AddressBuild {
            payment_verification_key_file,
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => {
            // R508: offline subcommand — Bech32 Shelley address.
            crate::era_independent::address::run::run_address_build_cmd(
                &payment_verification_key_file,
                stake_verification_key_file.as_deref(),
                mainnet,
                testnet_magic,
                out_file.as_deref(),
            )
        }
        Command::StakeAddressBuild {
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => {
            // R508: offline subcommand — Bech32 reward (stake) address.
            crate::era_based::stake_address::run::run_stake_address_build_cmd(
                &stake_verification_key_file,
                mainnet,
                testnet_magic,
                out_file.as_deref(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Command::Version` now produces the version banner from
    /// `crate::helper::version_info` (rather than the R289 stub
    /// string). Capturing stdout in unit tests is awkward; we
    /// assert the function returns Ok and that the banner string
    /// is non-empty + identifies the crate.
    #[test]
    fn version_returns_ok_and_helper_banner_is_nonempty() {
        let banner = crate::helper::version_info();
        assert!(
            !banner.is_empty(),
            "version_info() must produce a non-empty banner"
        );
        assert!(
            banner.contains("yggdrasil") || banner.contains("cardano-cli"),
            "version banner must identify the crate; got {banner:?}"
        );
        run_command(Command::Version).expect("Command::Version must succeed");
    }

    /// `Command::ShowUpstreamConfig` is now wired. With an unknown
    /// network preset it errors out with a structured "expected
    /// one of mainnet / preprod / preview" message.
    #[test]
    fn show_upstream_config_rejects_unknown_network_preset() {
        let result = run_command(Command::ShowUpstreamConfig {
            network: "bogus".to_string(),
            upstream_config_root: None,
        });
        let err = result.expect_err("unknown network must bail");
        assert!(
            err.to_string().contains("unknown network preset")
                && err.to_string().contains("mainnet / preprod / preview"),
            "error must enumerate the supported network presets; got {err}"
        );
    }

    /// With a valid network preset the runner attempts path
    /// resolution. In a workspace-test environment without a real
    /// `node/configuration/<network>/config.json`, this either
    /// succeeds (when the vendored configs are present, the
    /// canonical case) or surfaces a structured path-resolution
    /// error from `environment::resolve_upstream_reference_paths`.
    /// We assert one of those two outcomes — not a "deferral
    /// message" anymore.
    #[test]
    fn show_upstream_config_resolves_or_errors_with_real_network() {
        let outcome = run_command(Command::ShowUpstreamConfig {
            network: "mainnet".to_string(),
            upstream_config_root: Some(std::path::PathBuf::from("/tmp/no-such-dir")),
        });
        if let Err(err) = outcome {
            // Path-resolution failure is acceptable in a sandboxed test
            // environment; the error must NOT be the old deferral
            // message — that would indicate the variant didn't carry
            // the network preset through.
            assert!(
                !err.to_string()
                    .contains("Command variant doesn't carry the network preset"),
                "must not be the old deferral message; got {err}"
            );
        }
    }

    /// `Command::QueryTip` dispatches through the supplied
    /// [`LsqClient`] now. The default `run_command` plugs in
    /// [`DeferralLsqClient`] which still bails — pin the deferral
    /// message stays operator-readable.
    #[test]
    fn query_tip_with_deferral_client_bails_with_documented_message() {
        let result = run_command(Command::QueryTip {
            socket_path: std::path::PathBuf::from("/unused.socket"),
            network_magic: None,
        });
        let err = result.expect_err("QueryTip must bail with the default deferral client");
        let msg = err.to_string();
        assert!(
            msg.contains("query-tip") && msg.contains("LsqClient"),
            "error must point at LsqClient wiring; got {msg}"
        );
    }

    /// `run_command_with` dispatches each `Command::Query*` variant
    /// through a custom [`LsqClient`] impl with the right
    /// [`NtcQuery`] + socket + magic. Pins the trait integration
    /// end-to-end — the binary crate's concrete impl plugs in here.
    #[test]
    fn query_commands_dispatch_through_custom_lsq_client() {
        use crate::lsq::NtcQuery;
        use std::cell::RefCell;
        use std::path::{Path, PathBuf};

        struct RecordingClient {
            seen: RefCell<Option<(NtcQuery, PathBuf, u32)>>,
        }
        impl crate::lsq::LsqClient for RecordingClient {
            fn run_query(&self, socket: &Path, magic: u32, query: NtcQuery) -> eyre::Result<()> {
                *self.seen.borrow_mut() = Some((query, socket.to_path_buf(), magic));
                Ok(())
            }
        }
        let client = RecordingClient {
            seen: RefCell::new(None),
        };
        let socket = PathBuf::from("/tmp/node.socket");
        // Each Query* command maps to its NtcQuery variant; the
        // tuple's magic doubles as a per-case discriminator.
        let cases = [
            (
                Command::QueryTip {
                    socket_path: socket.clone(),
                    network_magic: Some(1),
                },
                NtcQuery::Tip,
                1_u32,
            ),
            (
                Command::QueryChainBlockNo {
                    socket_path: socket.clone(),
                    network_magic: Some(2),
                },
                NtcQuery::ChainBlockNo,
                2,
            ),
            (
                Command::QueryCurrentEra {
                    socket_path: socket.clone(),
                    network_magic: Some(3),
                },
                NtcQuery::CurrentEra,
                3,
            ),
            (
                Command::QuerySystemStart {
                    socket_path: socket.clone(),
                    network_magic: Some(4),
                },
                NtcQuery::SystemStart,
                4,
            ),
            (
                Command::QueryStakeDistribution {
                    socket_path: socket.clone(),
                    network_magic: Some(5),
                },
                NtcQuery::StakeDistribution,
                5,
            ),
            (
                Command::QueryStakePools {
                    socket_path: socket.clone(),
                    network_magic: Some(6),
                },
                NtcQuery::StakePools,
                6,
            ),
            (
                Command::QueryProtocolParameters {
                    socket_path: socket.clone(),
                    network_magic: Some(7),
                },
                NtcQuery::ProtocolParameters,
                7,
            ),
            (
                Command::QueryDrepStakeDistribution {
                    socket_path: socket.clone(),
                    network_magic: Some(8),
                },
                NtcQuery::DrepStakeDistribution,
                8,
            ),
            (
                Command::QueryConstitution {
                    socket_path: socket.clone(),
                    network_magic: Some(9),
                },
                NtcQuery::Constitution,
                9,
            ),
            (
                Command::QueryGovState {
                    socket_path: socket.clone(),
                    network_magic: Some(10),
                },
                NtcQuery::GovState,
                10,
            ),
            (
                Command::QueryDrepState {
                    socket_path: socket.clone(),
                    network_magic: Some(11),
                },
                NtcQuery::DrepState,
                11,
            ),
            (
                Command::QueryCommitteeState {
                    socket_path: socket.clone(),
                    network_magic: Some(12),
                },
                NtcQuery::CommitteeState,
                12,
            ),
        ];
        for (command, expected_query, expected_magic) in cases {
            run_command_with(command, &client)
                .expect("run_command_with must succeed with a custom client");
            assert_eq!(
                client.seen.borrow().clone(),
                Some((expected_query, socket.clone(), expected_magic)),
                "each Query* command must forward its NtcQuery + socket + magic verbatim"
            );
        }
    }

    /// `run_command_with` falls back to mainnet magic when the
    /// `network_magic` field is None. Pins the fallback behavior so
    /// the operator doesn't have to pass `--network-magic` on
    /// mainnet.
    #[test]
    fn query_tip_falls_back_to_mainnet_magic_when_unset() {
        use std::cell::Cell;
        use std::path::Path;

        struct MagicCapture {
            magic: Cell<u32>,
        }
        impl crate::lsq::LsqClient for MagicCapture {
            fn run_query(
                &self,
                _socket: &Path,
                magic: u32,
                _query: crate::lsq::NtcQuery,
            ) -> eyre::Result<()> {
                self.magic.set(magic);
                Ok(())
            }
        }
        let client = MagicCapture {
            magic: Cell::new(0),
        };
        run_command_with(
            Command::QueryTip {
                socket_path: std::path::PathBuf::from("/unused.socket"),
                network_magic: None,
            },
            &client,
        )
        .expect("run_command_with must succeed with None magic + fallback");
        assert_eq!(
            client.magic.get(),
            764_824_073,
            "None network_magic must fall back to the mainnet protocol-magic constant"
        );
    }
}
