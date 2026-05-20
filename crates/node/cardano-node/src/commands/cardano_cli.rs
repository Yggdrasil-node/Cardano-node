//! Node-binary entry point for the `cardano-cli` subcommand surface.
//!
//! Thin dispatcher that routes the parsed `CardanoCliCommand` to the
//! `yggdrasil-cardano-cli` crate's runners. Network-preset resolution
//! (`NetworkPreset` enum -> network_dir string + fallback magic) lives
//! here so the new crate stays independent of the node binary's
//! config types.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Environment.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side dispatcher for the `yggdrasil-node cardano-cli <subcommand>` integration. Wraps `yggdrasil_cardano_cli::*` library calls; the actual subcommand runtime logic lives in `crates/tools/cardano-cli/` (R447 relocated). No upstream parallel — upstream `cardano-cli` is a separate binary, not a node-binary subcommand.

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use eyre::Result;

use yggdrasil_cardano_cli::environment;
use yggdrasil_cardano_cli::lsq::{
    LsqClient, NtcQuery, delegations_and_rewards_query_arg, reward_balance_query_arg,
    stake_pool_params_query_arg, utxo_by_address_query_arg, utxo_by_tx_in_query_arg,
};
use yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient;
use yggdrasil_node_config::NetworkPreset;

use crate::cli::CardanoCliCommand;

/// Map a `NetworkPreset` enum to its on-disk sub-directory name. The
/// `yggdrasil-cardano-cli` crate accepts the directory name as a `&str`
/// to avoid importing `yggdrasil_node_config::NetworkPreset` (which
/// would invert the dependency direction).
fn network_dir(network: NetworkPreset) -> &'static str {
    match network {
        NetworkPreset::Mainnet => "mainnet",
        NetworkPreset::Preprod => "preprod",
        NetworkPreset::Preview => "preview",
    }
}

fn run_tool_ntc_query(
    socket_path: PathBuf,
    network_magic: Option<u32>,
    reference_network_magic: u32,
    query: NtcQuery,
) -> Result<()> {
    TokioLsqClient.run_query(
        &socket_path,
        network_magic.unwrap_or(reference_network_magic),
        query,
    )
}

/// Run selected cardano-cli operations from the pure Rust CLI implementation.
pub(crate) fn run_cardano_cli_command(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
    action: CardanoCliCommand,
) -> Result<()> {
    let dir = network_dir(network);
    let (config_path, topology_path) =
        environment::resolve_upstream_reference_paths(dir, upstream_config_root)?;
    let reference_network_magic =
        environment::extract_reference_network_magic(&config_path, network.network_magic());

    match action {
        CardanoCliCommand::Version => {
            // R296: Version output sources its banner from
            // `yggdrasil_cardano_cli::helper::version_info()` so the
            // pure-Rust subset and any future Phase-F-implemented
            // commands print a consistent version string.
            println!("{}", yggdrasil_cardano_cli::helper::version_info());
            println!("network preset default: {}", network);
            Ok(())
        }
        CardanoCliCommand::ShowUpstreamConfig => {
            // R297: ShowUpstreamConfig migrated into
            // yggdrasil-cardano-cli::environment::run_show_upstream_config.
            environment::run_show_upstream_config(
                &network.to_string(),
                &config_path,
                &topology_path,
                reference_network_magic,
            )
        }
        CardanoCliCommand::QueryTip {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::Tip,
        ),
        CardanoCliCommand::QueryUtxo {
            socket_path: _socket_path,
            network_magic,
            address,
            tx_in,
        } => {
            let query = match (address, tx_in) {
                (Some(addr), None) => utxo_by_address_query_arg(&addr),
                (None, Some(tx)) => utxo_by_tx_in_query_arg(&tx)?,
                (None, None) => {
                    eyre::bail!("query-utxo requires either --address or --tx-in; pass one of them")
                }
                (Some(_), Some(_)) => {
                    unreachable!("clap's conflicts_with = ... pair prevents both flags being set")
                }
            };
            run_tool_ntc_query(_socket_path, network_magic, reference_network_magic, query)
        }
        CardanoCliCommand::QueryProtocolParameters {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::ProtocolParameters,
        ),
        CardanoCliCommand::QueryStakePools {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::StakePools,
        ),
        CardanoCliCommand::QueryStakeDistribution {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::StakeDistribution,
        ),
        CardanoCliCommand::QueryCurrentEra {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::CurrentEra,
        ),
        CardanoCliCommand::QueryChainBlockNo {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::ChainBlockNo,
        ),
        CardanoCliCommand::QuerySystemStart {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::SystemStart,
        ),
        CardanoCliCommand::QueryCurrentEpoch {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::CurrentEpoch,
        ),
        CardanoCliCommand::QueryExpectedNetworkId {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::ExpectedNetworkId,
        ),
        CardanoCliCommand::QueryEraHistory {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::EraHistory,
        ),
        CardanoCliCommand::QueryTreasuryAndReserves {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::TreasuryAndReserves,
        ),
        CardanoCliCommand::QueryDrepStakeDistr {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::DrepStakeDistribution,
        ),
        CardanoCliCommand::QueryConstitution {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::Constitution,
        ),
        CardanoCliCommand::QueryGovState {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::GovState,
        ),
        CardanoCliCommand::QueryDrepState {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::DrepState,
        ),
        CardanoCliCommand::QueryAccountState {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::AccountState,
        ),
        CardanoCliCommand::QueryGenesisDelegations {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::GenesisDelegations,
        ),
        CardanoCliCommand::QueryStabilityWindow {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::StabilityWindow,
        ),
        CardanoCliCommand::QueryNumDormantEpochs {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::NumDormantEpochs,
        ),
        CardanoCliCommand::QueryDepositPot {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::DepositPot,
        ),
        CardanoCliCommand::QueryLedgerCounts {
            socket_path: _socket_path,
            network_magic,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            NtcQuery::LedgerCounts,
        ),
        CardanoCliCommand::QueryRewardBalance {
            socket_path: _socket_path,
            network_magic,
            account,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            reward_balance_query_arg(&account),
        ),
        CardanoCliCommand::QueryDelegationsAndRewards {
            socket_path: _socket_path,
            network_magic,
            credential,
            is_key_hash,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            delegations_and_rewards_query_arg(&credential, is_key_hash),
        ),
        CardanoCliCommand::QueryStakePoolParams {
            socket_path: _socket_path,
            network_magic,
            pool_hash,
        } => run_tool_ntc_query(
            _socket_path,
            network_magic,
            reference_network_magic,
            stake_pool_params_query_arg(&pool_hash),
        ),
        CardanoCliCommand::TransactionSubmit {
            socket_path: _socket_path,
            network_magic,
            tx_file,
            tx_hex,
        } => {
            let _magic = network_magic.unwrap_or(reference_network_magic);
            yggdrasil_cardano_cli::era_based::transaction::run::run_transaction_submit_cmd(
                tx_file,
                tx_hex,
                &_socket_path,
                _magic,
                &TokioLsqClient,
            )
        }
        CardanoCliCommand::TransactionTxid { tx_file, tx_hex } => {
            yggdrasil_cardano_cli::era_based::transaction::run::run_transaction_txid_cmd(
                tx_file, tx_hex,
            )
        }
        CardanoCliCommand::AddressKeyHash {
            payment_verification_key_file,
        } => yggdrasil_cardano_cli::era_independent::address::run::run_address_key_hash_cmd(
            &payment_verification_key_file,
        ),
        CardanoCliCommand::AddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => yggdrasil_cardano_cli::era_independent::address::run::run_address_key_gen_cmd(
            &verification_key_file,
            &signing_key_file,
        ),
        CardanoCliCommand::StakeAddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => yggdrasil_cardano_cli::era_based::stake_address::run::run_stake_address_key_gen_cmd(
            &verification_key_file,
            &signing_key_file,
        ),
        CardanoCliCommand::TransactionSign {
            tx_file,
            tx_hex,
            signing_key_file,
            out_file,
        } => yggdrasil_cardano_cli::era_based::transaction::run::run_transaction_sign_cmd(
            tx_file,
            tx_hex,
            &signing_key_file,
            &out_file,
        ),
        CardanoCliCommand::StakeAddressBuild {
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => yggdrasil_cardano_cli::era_based::stake_address::run::run_stake_address_build_cmd(
            &stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file.as_deref(),
        ),
        CardanoCliCommand::AddressBuild {
            payment_verification_key_file,
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => yggdrasil_cardano_cli::era_independent::address::run::run_address_build_cmd(
            &payment_verification_key_file,
            stake_verification_key_file.as_deref(),
            mainnet,
            testnet_magic,
            out_file.as_deref(),
        ),
    }
}

/// Construct a Shelley address byte sequence and Bech32-encode it.
///
/// Two cases per
/// `Cardano.Ledger.Shelley.API.Wallet.computeShelleyAddress`:
///
/// - Enterprise (`stake_hash == None`): header `0b0110_<netid>`
///   (type 6 = key-payment, enterprise) + 28-byte payment hash =
///   29 raw bytes.
/// - Base (`stake_hash == Some(h)`): header `0b0000_<netid>`
///   (type 0 = key-payment, key-stake) + 28-byte payment hash +
///   28-byte stake hash = 57 raw bytes.
///
/// `network_id` is 1 for mainnet, 0 for any testnet. The Bech32
/// HRP is `addr` (mainnet) or `addr_test` (testnet) per
/// `Cardano.Ledger.Address.serialiseAddrBech32`.
#[cfg(test)]
pub(crate) fn build_shelley_address_bech32(
    network_id: u8,
    payment_hash: &[u8; 28],
    stake_hash: Option<&[u8; 28]>,
) -> Result<String> {
    yggdrasil_cardano_cli::era_independent::address::run::build_shelley_address_bech32(
        network_id,
        payment_hash,
        stake_hash,
    )
}

/// Construct a Shelley reward (stake) address byte sequence and
/// Bech32-encode it.
///
/// Per `Cardano.Ledger.Address.RewardAccount`:
///
/// - Header byte: `0b1110_<netid>` for the standard key-based
///   reward address (upstream address type 14). Script-based reward
///   addresses (type 15) are not yet supported.
/// - Payload: 28-byte stake-key hash (Blake2b-224 of the stake VK).
///
/// Bech32 HRP: `stake` (mainnet, `network_id == 1`) or `stake_test`
/// (any non-mainnet `network_id`).
#[cfg(test)]
pub(crate) fn build_shelley_reward_address_bech32(
    network_id: u8,
    stake_hash: &[u8; 28],
) -> Result<String> {
    yggdrasil_cardano_cli::era_based::stake_address::run::build_shelley_reward_address_bech32(
        network_id, stake_hash,
    )
}

/// Write a TextEnvelope JSON file (`{type, description, cborHex}`)
/// matching upstream `cardano-cli`'s output shape for a 32-byte key.
///
/// `cbor_hex` is constructed from the 32-byte payload as `5820 ||
/// payload` (CBOR major-2 bytes-string of length 32). When
/// `private = true` on Unix, the file is created with `0o600`
/// permissions so the signing key isn't world-readable.
#[cfg(test)]
pub(crate) fn write_text_envelope(
    path: &Path,
    envelope_type: &str,
    description: &str,
    payload: &[u8; 32],
    private: bool,
) -> Result<()> {
    yggdrasil_cardano_cli::era_independent::address::run::write_text_envelope(
        path,
        envelope_type,
        description,
        payload,
        private,
    )
}

/// Parse an upstream-shaped TextEnvelope JSON document and extract the
/// 32-byte Ed25519 verification key bytes from its `cborHex` field.
///
/// The canonical implementation lives in `yggdrasil-cardano-cli`;
/// this node helper is only a compatibility wrapper for the
/// binary-local command tests and command dispatcher.
#[cfg(test)]
pub(crate) fn read_verification_key_text_envelope(envelope_bytes: &[u8]) -> Result<[u8; 32]> {
    yggdrasil_cardano_cli::era_independent::address::run::read_verification_key_text_envelope(
        envelope_bytes,
    )
}

/// Sign a transaction with a single Ed25519 signing key, replacing
/// the existing witness set with a fresh one carrying just the
/// produced VKeyWitness.
///
/// Wire surgery:
///
/// - Read the outer CBOR array length L from `tx_bytes`.
/// - Slice out the TxBody bytes (element 0) via `Decoder::raw_value`.
/// - Skip the original witness set (element 1) and remember the
///   tail-bytes range — element 2 onward (IsValid + optional
///   AuxData) gets preserved verbatim.
/// - Construct a fresh witness-set CBOR map `{0: [[vk, sig]]}` —
///   upstream `Cardano.Ledger.Shelley.TxWits` encoding for vkey
///   witnesses with the `0` map key.
/// - Re-emit the tx as `array(L) || TxBody || NewWitnessSet || tail`.
///
/// This is the single-signer flow; additive-witness flows (preserve
/// existing entries 0..=k of the witness set and append a new
/// VKeyWitness) require a full witness-set decoder + re-encoder
/// that doesn't exist yet — gated on a future round when multi-
/// signer is needed.
#[cfg(test)]
pub(crate) fn sign_tx_with_fresh_witness_set(
    tx_bytes: &[u8],
    sk_bytes: &[u8; 32],
) -> Result<Vec<u8>> {
    yggdrasil_cardano_cli::era_based::transaction::run::sign_tx_with_fresh_witness_set(
        tx_bytes, sk_bytes,
    )
}

/// Compute the transaction id from a complete tx CBOR encoding.
///
/// The canonical implementation lives in `yggdrasil-cardano-cli`;
/// this node helper is only a compatibility wrapper for the
/// binary-local command tests.
#[cfg(test)]
pub(crate) fn compute_txid_from_tx_cbor(tx_bytes: &[u8]) -> Result<[u8; 32]> {
    yggdrasil_cardano_cli::era_based::transaction::run::compute_txid_from_tx_cbor(tx_bytes)
}
