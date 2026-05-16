//! Top-level cardano-cli command type.
//!
//! Mirrors upstream `Cardano.CLI.Command` (the entry-point sum type
//! that aggregates Byron / Compatible / per-era / Legacy / EraBased
//! / EraIndependent command groups).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Command.hs`.
//! Yggdrasil's `Command` enum subset starts with the variants the
//! pure-Rust binary already exposes (Version, ShowUpstreamConfig,
//! QueryTip) and grows with each Phase F round. The full upstream
//! `ClientCommand` carries Byron / Compatible / Legacy / Era branches
//! that R290–R295 will populate.

use std::path::PathBuf;

use clap::Subcommand;

/// Top-level dispatch enum for `yggdrasil-cardano-cli`.
///
/// Mirrors the entry-point shape of upstream `ClientCommand` from
/// `Cardano.CLI.Command`. R289 ships the three subcommands the node
/// binary's `cardano-cli` subcommand already implements; per-cluster
/// rounds R290–R295 expand the variant set to mirror upstream's full
/// surface (Byron / Compatible / Shelley / Alonzo / Babbage / Conway).
///
/// `clap::Subcommand` derive (R503): wires the enum into the library's
/// `parser::parse_command` so a standalone `yggdrasil-cardano-cli`
/// binary (when its `[[bin]]` target lands) can dispatch directly
/// without going through the node binary's wrapper.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum Command {
    /// Print pure-Rust cardano-cli compatibility version info.
    /// Mirrors upstream `DisplayVersion` arm.
    Version,
    /// Show resolved reference config paths and network magic.
    /// Mirrors upstream's `Cardano.CLI.Helper`-style operator
    /// introspection helpers; Yggdrasil-specific utility.
    ShowUpstreamConfig {
        /// Network preset name (`mainnet` / `preprod` / `preview`).
        /// Selects the `node/configuration/<network>/` sub-tree to
        /// resolve config + topology paths against, plus the
        /// well-known network magic for the fallback if
        /// `config.json` lacks one.
        #[arg(long)]
        network: String,
        /// Override path for the upstream Haskell-share root
        /// (typically `/tmp/cardano-tooling/share`); falls back to
        /// the vendored `node/configuration/<network>/` directory.
        #[arg(long)]
        upstream_config_root: Option<PathBuf>,
    },
    /// Query the running node for tip / chain-point / block-no.
    /// Mirrors upstream `QueryTip` from `Cardano.CLI.Compatible.Run`.
    QueryTip {
        /// Path to the node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using the upstream
        /// reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Query the running node for its current chain block number.
    /// Mirrors `GetChainBlockNo` from `Ouroboros.Consensus.Ledger.Query`.
    QueryChainBlockNo {
        /// Path to the node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using the upstream
        /// reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Query the running node for its current ledger era. Mirrors
    /// `GetCurrentEra` from
    /// `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query`.
    QueryCurrentEra {
        /// Path to the node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using the upstream
        /// reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
    /// Generate a fresh Ed25519 payment keypair, writing both keys
    /// as TextEnvelope JSON files. Mirrors upstream `address key-gen`
    /// (`Cardano.CLI.EraIndependent.Address.Command.AddressKeyGen`).
    AddressKeyGen {
        /// Path to write the verification (public) key TextEnvelope.
        #[arg(long)]
        verification_key_file: PathBuf,
        /// Path to write the signing (private) key TextEnvelope.
        #[arg(long)]
        signing_key_file: PathBuf,
    },
    /// Print the Blake2b-224 hash of a verification key. Mirrors
    /// upstream `address key-hash`
    /// (`Cardano.CLI.EraIndependent.Address.Command.AddressKeyHash`).
    AddressKeyHash {
        /// Path to a verification-key TextEnvelope. Both payment and
        /// stake verification-key envelopes are accepted — the wire
        /// shape is identical (32-byte VK in a CBOR bytes envelope).
        #[arg(long)]
        payment_verification_key_file: PathBuf,
    },
    /// Generate a fresh Ed25519 stake keypair (delegation /
    /// reward-account credential), writing both keys as TextEnvelope
    /// JSON files. Mirrors upstream `stake-address key-gen`
    /// (`Cardano.CLI.EraBased.StakeAddress.Command`). Identical
    /// entropy + wire shape to `address key-gen`; only the
    /// TextEnvelope `type` metadata differs.
    StakeAddressKeyGen {
        /// Path to write the stake verification (public) key TextEnvelope.
        #[arg(long)]
        verification_key_file: PathBuf,
        /// Path to write the stake signing (private) key TextEnvelope.
        #[arg(long)]
        signing_key_file: PathBuf,
    },
    /// Print the transaction id (Blake2b-256 of the CBOR tx body)
    /// of a serialized transaction. Mirrors upstream
    /// `transaction txid` (`Cardano.CLI.EraBased.Transaction.Command`).
    TransactionTxid {
        /// Path to a file containing the CBOR-encoded transaction.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR transaction bytes (a leading `0x` and
        /// surrounding whitespace are tolerated).
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
    },
    /// Sign a transaction with a single Ed25519 signing key,
    /// replacing the witness set with a fresh single-signer one.
    /// Mirrors upstream `transaction sign`
    /// (`Cardano.CLI.EraBased.Transaction.Command`).
    TransactionSign {
        /// Path to a file containing the CBOR-encoded unsigned tx.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR unsigned-tx bytes.
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
        /// Path to the Ed25519 signing-key TextEnvelope. Both payment
        /// and stake signing-key envelopes are accepted.
        #[arg(long)]
        signing_key_file: PathBuf,
        /// Path to write the signed transaction CBOR.
        #[arg(long)]
        out_file: PathBuf,
    },
    /// Build a Shelley payment address (Bech32) from a payment
    /// verification key, optionally with a stake credential. Mirrors
    /// upstream `address build`
    /// (`Cardano.CLI.EraIndependent.Address.Command.AddressBuild`).
    AddressBuild {
        /// Path to the payment verification-key TextEnvelope.
        #[arg(long)]
        payment_verification_key_file: PathBuf,
        /// Optional stake verification-key TextEnvelope. When present
        /// the result is a base address (type 0); otherwise an
        /// enterprise address (type 6).
        #[arg(long)]
        stake_verification_key_file: Option<PathBuf>,
        /// Use the mainnet network ID (1) and the `addr` HRP.
        #[arg(long, conflicts_with = "testnet_magic")]
        mainnet: bool,
        /// Use a testnet network ID (0) and the `addr_test` HRP. The
        /// magic value itself is informational — Shelley addresses
        /// do not carry it on-chain.
        #[arg(long, conflicts_with = "mainnet")]
        testnet_magic: Option<u32>,
        /// Optional output file; when omitted the address prints to
        /// stdout.
        #[arg(long)]
        out_file: Option<PathBuf>,
    },
    /// Build a Shelley reward (stake) address (Bech32) from a stake
    /// verification key. Mirrors upstream `stake-address build`
    /// (`Cardano.CLI.EraBased.StakeAddress.Command`).
    StakeAddressBuild {
        /// Path to the stake verification-key TextEnvelope.
        #[arg(long)]
        stake_verification_key_file: PathBuf,
        /// Use the mainnet network ID (1) and the `stake` HRP.
        #[arg(long, conflicts_with = "testnet_magic")]
        mainnet: bool,
        /// Use a testnet network ID (0) and the `stake_test` HRP.
        #[arg(long, conflicts_with = "mainnet")]
        testnet_magic: Option<u32>,
        /// Optional output file; when omitted the address prints to
        /// stdout.
        #[arg(long)]
        out_file: Option<PathBuf>,
    },
}
