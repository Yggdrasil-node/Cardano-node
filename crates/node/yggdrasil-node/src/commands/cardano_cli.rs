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

use std::path::PathBuf;

use eyre::Result;

use yggdrasil_cardano_cli::environment;
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
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::Tip,
        ),
        CardanoCliCommand::QueryUtxo {
            socket_path: _socket_path,
            network_magic,
            address,
            tx_in,
        } => {
            let _magic = network_magic.unwrap_or(reference_network_magic);
            let query = match (address, tx_in) {
                (Some(addr), None) => {
                    crate::commands::query::QueryCommand::UtxoByAddress { address: addr }
                }
                (None, Some(tx)) => {
                    // Upstream `cardano-cli` accepts `--tx-in TX#INDEX`
                    // as a single token. Split here so the downstream
                    // `UtxoByTxIn` query gets the structured pair.
                    let (tx_id, index_str) = tx.split_once('#').ok_or_else(|| {
                        eyre::eyre!(
                            "--tx-in expects TX_HASH#INDEX (e.g. 0123ab…#0); got {tx:?}"
                        )
                    })?;
                    let index: u16 = index_str.parse().map_err(|e| {
                        eyre::eyre!(
                            "--tx-in index {index_str:?} is not a valid u16: {e}"
                        )
                    })?;
                    crate::commands::query::QueryCommand::UtxoByTxIn {
                        tx_id: tx_id.to_string(),
                        index,
                    }
                }
                (None, None) => eyre::bail!(
                    "query-utxo requires either --address or --tx-in; pass one of them"
                ),
                (Some(_), Some(_)) => unreachable!(
                    "clap's conflicts_with = ... pair prevents both flags being set"
                ),
            };
            run_query_via_binary_runtime(_socket_path, _magic, query)
        }
        CardanoCliCommand::QueryProtocolParameters {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::ProtocolParams,
        ),
        CardanoCliCommand::QueryStakePools {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::StakePools,
        ),
        CardanoCliCommand::QueryStakeDistribution {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::StakeDistribution,
        ),
        CardanoCliCommand::TransactionSubmit {
            socket_path: _socket_path,
            network_magic,
            tx_file,
            tx_hex,
        } => {
            let _magic = network_magic.unwrap_or(reference_network_magic);
            let tx_bytes = read_tx_input(tx_file, tx_hex, "transaction-submit")?;
            run_submit_via_binary_runtime(_socket_path, _magic, tx_bytes)
        }
        CardanoCliCommand::TransactionTxid { tx_file, tx_hex } => {
            let tx_bytes = read_tx_input(tx_file, tx_hex, "transaction-txid")?;
            let txid = compute_txid_from_tx_cbor(&tx_bytes)?;
            // Print as 64-char lowercase hex without any prefix —
            // upstream `cardano-cli transaction txid` output shape.
            println!("{}", hex::encode(txid));
            Ok(())
        }
        CardanoCliCommand::AddressKeyHash {
            payment_verification_key_file,
        } => {
            let envelope_bytes = std::fs::read(&payment_verification_key_file).map_err(|e| {
                eyre::eyre!(
                    "failed to read --payment-verification-key-file {}: {e}",
                    payment_verification_key_file.display()
                )
            })?;
            let key_bytes = read_verification_key_text_envelope(&envelope_bytes)?;
            let hash = yggdrasil_crypto::hash_bytes_224(&key_bytes);
            // Upstream prints lowercase hex without prefix (56 chars).
            println!("{}", hex::encode(hash.0));
            Ok(())
        }
    }
}

/// Shared `--tx-file` / `--tx-hex` flag resolver. Both arms route
/// through here so the conflict-already-rejected-by-clap +
/// missing-flag error message stays uniform across the `transaction-*`
/// subcommand family.
fn read_tx_input(
    tx_file: Option<PathBuf>,
    tx_hex: Option<String>,
    subcommand: &str,
) -> Result<Vec<u8>> {
    match (tx_file, tx_hex) {
        (Some(path), None) => std::fs::read(&path)
            .map_err(|e| eyre::eyre!("failed to read --tx-file {}: {e}", path.display())),
        (None, Some(hex_str)) => crate::commands::submit_tx::decode_tx_hex_arg(&hex_str),
        (None, None) => {
            eyre::bail!("{subcommand} requires either --tx-file or --tx-hex")
        }
        (Some(_), Some(_)) => unreachable!(
            "clap's conflicts_with prevents both flags being set"
        ),
    }
}

/// Compute the transaction id from a complete tx CBOR encoding.
///
/// All Cardano eras encode a transaction as a CBOR array whose first
/// element is the transaction body (`TxBody`). The transaction id is
/// `Blake2b-256(<bytes-of-TxBody-CBOR>)` per
/// `Cardano.Ledger.Core.txIdTxBody`. Reuses the existing
/// `yggdrasil_ledger::tx::compute_tx_id` helper.
///
/// Works for every era from Shelley through Conway — Shelley/Allegra/
/// Mary use a 3-element array, Alonzo/Babbage/Conway a 4-element
/// array, but in both cases the first element is the body, and
/// `Decoder::raw_value()` reads it as opaque bytes without needing to
/// know the surrounding shape.
/// Parse an upstream-shaped TextEnvelope JSON document and extract the
/// 32-byte Ed25519 verification key bytes from its `cborHex` field.
///
/// The TextEnvelope format (per upstream
/// `Cardano.Api.SerialiseTextEnvelope`) is the JSON object
///
/// ```text
/// { "type":        "PaymentVerificationKeyShelley_ed25519",
///   "description": "Payment Verification Key",
///   "cborHex":     "5820<64 hex chars of 32-byte VK>" }
/// ```
///
/// `cborHex` is a CBOR bytes-string envelope (`0x58 0x20 = bytes,
/// length 32`) wrapping the raw 32-byte VK. This helper strips the
/// 2-byte CBOR prefix and returns the inner 32 bytes.
///
/// The same envelope shape is used for `StakeVerificationKeyShelley_ed25519`
/// (the wire bytes are identical; only the `type` field differs), so
/// this helper is reused by `address key-hash` AND `stake-address
/// key-hash` (the latter lands in a follow-on slice).
pub(crate) fn read_verification_key_text_envelope(envelope_bytes: &[u8]) -> Result<[u8; 32]> {
    let envelope: serde_json::Value = serde_json::from_slice(envelope_bytes)
        .map_err(|e| eyre::eyre!("TextEnvelope is not valid JSON: {e}"))?;
    let cbor_hex = envelope
        .get("cborHex")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            eyre::eyre!("TextEnvelope is missing the required `cborHex` string field")
        })?;
    let cbor_bytes = hex::decode(cbor_hex.trim())
        .map_err(|e| eyre::eyre!("TextEnvelope cborHex is not valid hex: {e}"))?;
    if cbor_bytes.len() != 34 {
        eyre::bail!(
            "expected 34 bytes of cborHex (2-byte CBOR prefix + 32-byte key), got {}",
            cbor_bytes.len()
        );
    }
    // CBOR bytes-string of length 32 = major-type-2 (0x40) | length-32 (0x20) = 0x58 0x20.
    if cbor_bytes[0] != 0x58 || cbor_bytes[1] != 0x20 {
        eyre::bail!(
            "expected CBOR prefix 0x5820 (bytes-string of length 32), got 0x{:02x}{:02x}",
            cbor_bytes[0],
            cbor_bytes[1]
        );
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(&cbor_bytes[2..]);
    Ok(out)
}

pub(crate) fn compute_txid_from_tx_cbor(tx_bytes: &[u8]) -> Result<[u8; 32]> {
    use yggdrasil_ledger::cbor::Decoder;
    use yggdrasil_ledger::compute_tx_id;

    let mut dec = Decoder::new(tx_bytes);
    let _array_len = dec
        .array()
        .map_err(|e| eyre::eyre!("transaction CBOR does not start with an array: {e}"))?;
    let body_bytes = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract TxBody bytes: {e}"))?;
    Ok(compute_tx_id(body_bytes).0)
}

/// Shared dispatch helper for the `cardano-cli transaction submit`
/// path — builds the binary's tokio runtime and drives
/// `commands::submit_tx::run_submit_tx`.
fn run_submit_via_binary_runtime(
    socket_path: PathBuf,
    network_magic: u32,
    tx_bytes: Vec<u8>,
) -> Result<()> {
    let _socket_path = socket_path;
    let _magic = network_magic;
    let _tx_bytes = tx_bytes;
    #[cfg(unix)]
    {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(crate::commands::submit_tx::run_submit_tx(
            _socket_path,
            _magic,
            _tx_bytes,
        ))
    }
    #[cfg(not(unix))]
    {
        eyre::bail!(
            "cardano-cli transaction-submit requires a Unix domain socket; \
             not supported on this platform"
        )
    }
}

/// Shared dispatch helper: build the binary's `tokio::runtime::Runtime`
/// and drive `crate::commands::query::run_query` to completion.
///
/// Used by every `cardano-cli query-*` variant. Centralised here so
/// the Unix-only `cfg` gate + runtime construction logic lives in one
/// place instead of being duplicated across every match arm. The
/// `_socket_path` underscored prefix is preserved from the upstream
/// expansion of the QueryTip arm to keep the non-Unix `cfg` branch
/// non-warning.
fn run_query_via_binary_runtime(
    socket_path: PathBuf,
    network_magic: u32,
    query: crate::commands::query::QueryCommand,
) -> Result<()> {
    let _socket_path = socket_path;
    let _magic = network_magic;
    let _query = query;
    #[cfg(unix)]
    {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(crate::commands::query::run_query(
            _socket_path,
            _magic,
            _query,
        ))
    }
    #[cfg(not(unix))]
    {
        eyre::bail!(
            "cardano-cli query subcommands require a Unix domain socket; \
             not supported on this platform"
        )
    }
}
