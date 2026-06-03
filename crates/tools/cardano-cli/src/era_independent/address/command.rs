//! EraIndependent command.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Address/Command.hs`.
//! R293 landed the file with the API skeleton. R519 ports the
//! concrete `AddressCmds` group for `address key-gen`, `address
//! key-hash`, and `address build`; `address info` remains scheduled
//! until the address decoder surface is implemented.

use std::path::PathBuf;

use clap::Subcommand;

/// Era-independent address commands.
///
/// Mirrors upstream `AddressCmds` from
/// `Cardano.CLI.EraIndependent.Address.Command`. The Rust surface
/// covers the concrete offline commands already implemented in
/// `Address.Run`: key generation, key hashing, and Shelley address
/// construction. Upstream `AddressInfo` is intentionally not exposed
/// yet because the matching address decoder is not implemented.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum AddressCmds {
    /// Create an address key pair.
    #[command(name = "key-gen")]
    AddressKeyGen {
        /// Path to write the verification-key TextEnvelope.
        #[arg(long)]
        verification_key_file: PathBuf,
        /// Path to write the signing-key TextEnvelope.
        #[arg(long)]
        signing_key_file: PathBuf,
    },
    /// Print the hash of an address key.
    #[command(name = "key-hash")]
    AddressKeyHash {
        /// Path to a payment verification-key TextEnvelope.
        #[arg(long)]
        payment_verification_key_file: PathBuf,
        /// Optional output file; when omitted the hash prints to stdout.
        #[arg(long)]
        out_file: Option<PathBuf>,
    },
    /// Build a Shelley payment address.
    #[command(name = "build")]
    AddressBuild {
        /// Path to the payment verification-key TextEnvelope.
        #[arg(long)]
        payment_verification_key_file: PathBuf,
        /// Optional stake verification-key TextEnvelope.
        #[arg(long)]
        stake_verification_key_file: Option<PathBuf>,
        /// Use the mainnet network ID.
        #[arg(long, conflicts_with = "testnet_magic")]
        mainnet: bool,
        /// Use a testnet network ID. The magic value is accepted for
        /// CLI parity but Shelley addresses carry only the network ID.
        #[arg(long, conflicts_with = "mainnet")]
        testnet_magic: Option<u32>,
        /// Optional output file; when omitted the address prints to stdout.
        #[arg(long)]
        out_file: Option<PathBuf>,
    },
}

/// Render the upstream command path for an [`AddressCmds`] value.
///
/// Mirrors `renderAddressCmds` from
/// `Cardano.CLI.EraIndependent.Address.Command`.
pub fn render_address_cmds(command: &AddressCmds) -> &'static str {
    match command {
        AddressCmds::AddressKeyGen { .. } => "address key-gen",
        AddressCmds::AddressKeyHash { .. } => "address key-hash",
        AddressCmds::AddressBuild { .. } => "address build",
    }
}
