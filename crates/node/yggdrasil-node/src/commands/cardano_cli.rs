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
        CardanoCliCommand::QueryCurrentEra {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::CurrentEra,
        ),
        CardanoCliCommand::QueryChainBlockNo {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::ChainBlockNo,
        ),
        CardanoCliCommand::QuerySystemStart {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::SystemStart,
        ),
        CardanoCliCommand::QueryCurrentEpoch {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::CurrentEpoch,
        ),
        CardanoCliCommand::QueryExpectedNetworkId {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::ExpectedNetworkId,
        ),
        CardanoCliCommand::QueryEraHistory {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::EraHistory,
        ),
        CardanoCliCommand::QueryTreasuryAndReserves {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::TreasuryAndReserves,
        ),
        CardanoCliCommand::QueryDrepStakeDistr {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::DrepStakeDistr,
        ),
        CardanoCliCommand::QueryConstitution {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::Constitution,
        ),
        CardanoCliCommand::QueryGovState {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::GovState,
        ),
        CardanoCliCommand::QueryDrepState {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::DrepState,
        ),
        CardanoCliCommand::QueryAccountState {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::AccountState,
        ),
        CardanoCliCommand::QueryGenesisDelegations {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::GenesisDelegations,
        ),
        CardanoCliCommand::QueryStabilityWindow {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::StabilityWindow,
        ),
        CardanoCliCommand::QueryNumDormantEpochs {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::NumDormantEpochs,
        ),
        CardanoCliCommand::QueryDepositPot {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::DepositPot,
        ),
        CardanoCliCommand::QueryLedgerCounts {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::LedgerCounts,
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
        CardanoCliCommand::AddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => generate_keypair_to_envelopes(
            &verification_key_file,
            &signing_key_file,
            KeyKind::Payment,
        ),
        CardanoCliCommand::StakeAddressKeyGen {
            verification_key_file,
            signing_key_file,
        } => generate_keypair_to_envelopes(
            &verification_key_file,
            &signing_key_file,
            KeyKind::Stake,
        ),
        CardanoCliCommand::TransactionSign {
            tx_file,
            tx_hex,
            signing_key_file,
            out_file,
        } => {
            let tx_bytes = read_tx_input(tx_file, tx_hex, "transaction-sign")?;
            let sk_envelope = std::fs::read(&signing_key_file).map_err(|e| {
                eyre::eyre!(
                    "failed to read --signing-key-file {}: {e}",
                    signing_key_file.display()
                )
            })?;
            let sk_bytes = read_verification_key_text_envelope(&sk_envelope)?;
            let signed_tx = sign_tx_with_fresh_witness_set(&tx_bytes, &sk_bytes)?;
            std::fs::write(&out_file, &signed_tx).map_err(|e| {
                eyre::eyre!("failed to write --out-file {}: {e}", out_file.display())
            })?;
            Ok(())
        }
        CardanoCliCommand::StakeAddressBuild {
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => {
            let network_id: u8 = if mainnet {
                1
            } else if testnet_magic.is_some() {
                0
            } else {
                eyre::bail!(
                    "stake-address-build requires either --mainnet or --testnet-magic"
                );
            };
            let env_bytes = std::fs::read(&stake_verification_key_file).map_err(|e| {
                eyre::eyre!(
                    "failed to read --stake-verification-key-file {}: {e}",
                    stake_verification_key_file.display()
                )
            })?;
            let stake_vk = read_verification_key_text_envelope(&env_bytes)?;
            let stake_hash = yggdrasil_crypto::hash_bytes_224(&stake_vk).0;
            let bech32_addr = build_shelley_reward_address_bech32(network_id, &stake_hash)?;
            match out_file {
                Some(path) => std::fs::write(&path, format!("{bech32_addr}\n")).map_err(|e| {
                    eyre::eyre!("failed to write --out-file {}: {e}", path.display())
                })?,
                None => println!("{bech32_addr}"),
            };
            Ok(())
        }
        CardanoCliCommand::AddressBuild {
            payment_verification_key_file,
            stake_verification_key_file,
            mainnet,
            testnet_magic,
            out_file,
        } => {
            // Network selection: --mainnet OR --testnet-magic, default to
            // testnet when neither is given (matches upstream's "must
            // specify one" stance but with a safer default for
            // mistype-ridden operator paste flows).
            let network_id: u8 = if mainnet {
                1
            } else if testnet_magic.is_some() {
                0
            } else {
                eyre::bail!(
                    "address-build requires either --mainnet or --testnet-magic"
                );
            };

            // Read and hash the payment verification key.
            let pay_env = std::fs::read(&payment_verification_key_file).map_err(|e| {
                eyre::eyre!(
                    "failed to read --payment-verification-key-file {}: {e}",
                    payment_verification_key_file.display()
                )
            })?;
            let pay_vk = read_verification_key_text_envelope(&pay_env)?;
            let pay_hash = yggdrasil_crypto::hash_bytes_224(&pay_vk).0;

            // Optionally read and hash the stake verification key.
            let stake_hash: Option<[u8; 28]> = match stake_verification_key_file {
                Some(p) => {
                    let env = std::fs::read(&p).map_err(|e| {
                        eyre::eyre!(
                            "failed to read --stake-verification-key-file {}: {e}",
                            p.display()
                        )
                    })?;
                    let vk = read_verification_key_text_envelope(&env)?;
                    Some(yggdrasil_crypto::hash_bytes_224(&vk).0)
                }
                None => None,
            };

            let bech32_addr = build_shelley_address_bech32(network_id, &pay_hash, stake_hash.as_ref())?;
            match out_file {
                Some(path) => std::fs::write(&path, format!("{bech32_addr}\n")).map_err(|e| {
                    eyre::eyre!("failed to write --out-file {}: {e}", path.display())
                })?,
                None => println!("{bech32_addr}"),
            };
            Ok(())
        }
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
pub(crate) fn build_shelley_address_bech32(
    network_id: u8,
    payment_hash: &[u8; 28],
    stake_hash: Option<&[u8; 28]>,
) -> Result<String> {
    if network_id > 0x0F {
        eyre::bail!(
            "network_id {network_id} must fit in 4 bits (0..=15); got {network_id}"
        );
    }
    let header = match stake_hash {
        // Base address (type 0); upper nibble 0x0 + network id in low nibble.
        Some(_) => network_id,
        // Enterprise address (type 6); upper nibble 0x6 + network id.
        None => 0x60 | network_id,
    };
    let mut addr_bytes: Vec<u8> = Vec::with_capacity(57);
    addr_bytes.push(header);
    addr_bytes.extend_from_slice(payment_hash);
    if let Some(sh) = stake_hash {
        addr_bytes.extend_from_slice(sh);
    }

    let hrp_str = if network_id == 1 { "addr" } else { "addr_test" };
    let hrp = bech32::Hrp::parse(hrp_str)
        .map_err(|e| eyre::eyre!("bech32 HRP parse failed for {hrp_str:?}: {e}"))?;
    bech32::encode::<bech32::Bech32>(hrp, &addr_bytes)
        .map_err(|e| eyre::eyre!("bech32 encode failed: {e}"))
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
pub(crate) fn build_shelley_reward_address_bech32(
    network_id: u8,
    stake_hash: &[u8; 28],
) -> Result<String> {
    if network_id > 0x0F {
        eyre::bail!(
            "network_id {network_id} must fit in 4 bits (0..=15); got {network_id}"
        );
    }
    let mut addr_bytes: Vec<u8> = Vec::with_capacity(29);
    addr_bytes.push(0xE0 | network_id);
    addr_bytes.extend_from_slice(stake_hash);

    let hrp_str = if network_id == 1 { "stake" } else { "stake_test" };
    let hrp = bech32::Hrp::parse(hrp_str)
        .map_err(|e| eyre::eyre!("bech32 HRP parse failed for {hrp_str:?}: {e}"))?;
    bech32::encode::<bech32::Bech32>(hrp, &addr_bytes)
        .map_err(|e| eyre::eyre!("bech32 encode failed: {e}"))
}

/// Kind of key being generated/loaded — selects the TextEnvelope
/// `type` + `description` fields. The on-wire bytes are identical
/// for both kinds (32-byte Ed25519 SK / VK); only the metadata
/// changes so upstream `cardano-cli` can tell payment from stake at
/// file-load time.
#[derive(Clone, Copy)]
pub(crate) enum KeyKind {
    Payment,
    Stake,
}

impl KeyKind {
    fn signing_envelope_type(self) -> &'static str {
        match self {
            KeyKind::Payment => "PaymentSigningKeyShelley_ed25519",
            KeyKind::Stake => "StakeSigningKeyShelley_ed25519",
        }
    }
    fn signing_description(self) -> &'static str {
        match self {
            KeyKind::Payment => "Payment Signing Key",
            KeyKind::Stake => "Stake Signing Key",
        }
    }
    fn verification_envelope_type(self) -> &'static str {
        match self {
            KeyKind::Payment => "PaymentVerificationKeyShelley_ed25519",
            KeyKind::Stake => "StakeVerificationKeyShelley_ed25519",
        }
    }
    fn verification_description(self) -> &'static str {
        match self {
            KeyKind::Payment => "Payment Verification Key",
            KeyKind::Stake => "Stake Verification Key",
        }
    }
}

/// Shared keypair generator used by `address-key-gen` and
/// `stake-address-key-gen`. Reads 32 bytes of OS entropy, derives
/// the VK, and writes both TextEnvelope files with the metadata
/// for `kind`.
fn generate_keypair_to_envelopes(
    verification_key_file: &std::path::Path,
    signing_key_file: &std::path::Path,
    kind: KeyKind,
) -> Result<()> {
    let seed = read_os_entropy_32_bytes()?;
    let sk = yggdrasil_crypto::SigningKey::from_bytes(seed);
    let vk = sk
        .verification_key()
        .map_err(|e| eyre::eyre!("failed to derive VK from generated SK: {e}"))?;
    write_text_envelope(
        signing_key_file,
        kind.signing_envelope_type(),
        kind.signing_description(),
        &sk.to_bytes(),
        /* private = */ true,
    )?;
    write_text_envelope(
        verification_key_file,
        kind.verification_envelope_type(),
        kind.verification_description(),
        &vk.to_bytes(),
        /* private = */ false,
    )?;
    Ok(())
}

/// Read 32 cryptographically-secure random bytes from the OS.
///
/// Yggdrasil's `cardano-cli address key-gen` parity surface uses
/// `/dev/urandom` directly rather than pulling in a new workspace
/// dep on `getrandom` / `rand` — every supported Yggdrasil platform
/// (Linux, macOS, …) provides the kernel-backed entropy device.
/// On non-Unix this errors out cleanly rather than silently downgrading.
fn read_os_entropy_32_bytes() -> Result<[u8; 32]> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut buf = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .map_err(|e| eyre::eyre!("open /dev/urandom failed: {e}"))?
            .read_exact(&mut buf)
            .map_err(|e| eyre::eyre!("read 32 bytes from /dev/urandom failed: {e}"))?;
        Ok(buf)
    }
    #[cfg(not(unix))]
    {
        eyre::bail!(
            "address-key-gen needs /dev/urandom for entropy; not supported on this platform"
        )
    }
}

/// Write a TextEnvelope JSON file (`{type, description, cborHex}`)
/// matching upstream `cardano-cli`'s output shape for a 32-byte key.
///
/// `cbor_hex` is constructed from the 32-byte payload as `5820 ||
/// payload` (CBOR major-2 bytes-string of length 32). When
/// `private = true` on Unix, the file is created with `0o600`
/// permissions so the signing key isn't world-readable.
pub(crate) fn write_text_envelope(
    path: &std::path::Path,
    envelope_type: &str,
    description: &str,
    payload: &[u8; 32],
    private: bool,
) -> Result<()> {
    let mut cbor = Vec::with_capacity(34);
    cbor.push(0x58);
    cbor.push(0x20);
    cbor.extend_from_slice(payload);
    let envelope = serde_json::json!({
        "type": envelope_type,
        "description": description,
        "cborHex": hex::encode(&cbor),
    });
    let json = serde_json::to_string_pretty(&envelope).map_err(|e| {
        eyre::eyre!("failed to serialise TextEnvelope: {e}")
    })?;

    // Restrictive mode for signing-key files on Unix; default mode for
    // verification-key files so they can be checked in / shared freely.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mode = if private { 0o600 } else { 0o644 };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .open(path)
            .map_err(|e| eyre::eyre!("open {} failed: {e}", path.display()))?;
        use std::io::Write;
        f.write_all(json.as_bytes())
            .map_err(|e| eyre::eyre!("write {} failed: {e}", path.display()))?;
        f.write_all(b"\n")
            .map_err(|e| eyre::eyre!("write {} trailing newline failed: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = private;
        std::fs::write(path, json + "\n")
            .map_err(|e| eyre::eyre!("write {} failed: {e}", path.display()))?;
    }
    Ok(())
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
pub(crate) fn sign_tx_with_fresh_witness_set(
    tx_bytes: &[u8],
    sk_bytes: &[u8; 32],
) -> Result<Vec<u8>> {
    use yggdrasil_crypto::SigningKey;
    use yggdrasil_ledger::cbor::{Decoder, Encoder};

    // Step 1: parse the outer array prefix + identify TxBody bytes +
    // identify the tail (everything after the original witness set).
    let mut dec = Decoder::new(tx_bytes);
    let array_len = dec
        .array()
        .map_err(|e| eyre::eyre!("tx CBOR does not start with an array: {e}"))?;
    if array_len < 2 {
        eyre::bail!(
            "tx CBOR outer array must have ≥2 elements (body + witness set); got {array_len}"
        );
    }
    let body_bytes = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract TxBody bytes: {e}"))?
        .to_vec();
    dec.skip()
        .map_err(|e| eyre::eyre!("failed to skip original witness set: {e}"))?;
    let tail_start = dec.position();
    let tail = &tx_bytes[tail_start..];

    // Step 2: compute txid and Ed25519-sign with the supplied SK.
    let sk = SigningKey::from_bytes(*sk_bytes);
    let vk = sk
        .verification_key()
        .map_err(|e| eyre::eyre!("derive VK from SK failed: {e}"))?;
    let txid = yggdrasil_ledger::compute_tx_id(&body_bytes);
    let sig = sk
        .sign(&txid.0)
        .map_err(|e| eyre::eyre!("sign txid failed: {e}"))?;

    // Step 3: construct fresh witness set = {0: [[vk_bytes, sig_bytes]]}.
    let mut wits = Encoder::new();
    wits.map(1);
    wits.unsigned(0);
    wits.array(1);
    wits.array(2);
    wits.bytes(&vk.to_bytes());
    wits.bytes(&sig.to_bytes());
    let wits_bytes = wits.into_bytes();

    // Step 4: assemble the signed tx: outer array(L) || body || wits || tail.
    let mut header = Encoder::new();
    header.array(array_len);
    let mut out = header.into_bytes();
    out.extend_from_slice(&body_bytes);
    out.extend_from_slice(&wits_bytes);
    out.extend_from_slice(tail);
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
