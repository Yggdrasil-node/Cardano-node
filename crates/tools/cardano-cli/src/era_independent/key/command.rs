//! EraIndependent command.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Key/Command.hs`.
//! R293 landed the file with the API skeleton. R520 ports the
//! concrete `key verification-key` subset; the remaining mnemonic
//! and key-conversion commands stay scheduled until their supporting
//! codecs are implemented.

use std::path::PathBuf;

use clap::Subcommand;

/// Key utility commands.
///
/// Mirrors upstream `KeyCmds` from
/// `Cardano.CLI.EraIndependent.Key.Command`. This bounded Rust
/// subset exposes the pure `key verification-key` command first.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum KeyCmds {
    /// Get a verification key from a signing key.
    #[command(name = "verification-key")]
    KeyVerificationKeyCmd(KeyVerificationKeyCmdArgs),
}

/// Arguments for `key verification-key`.
///
/// Mirrors upstream `KeyVerificationKeyCmdArgs`.
#[derive(Clone, Debug, Eq, PartialEq, clap::Args)]
pub struct KeyVerificationKeyCmdArgs {
    /// Input filepath of the signing key.
    #[arg(long)]
    pub signing_key_file: PathBuf,
    /// Output filepath of the verification key.
    #[arg(long)]
    pub verification_key_file: PathBuf,
}

/// Render the upstream command path for a [`KeyCmds`] value.
///
/// Mirrors `renderKeyCmds` from
/// `Cardano.CLI.EraIndependent.Key.Command`.
pub fn render_key_cmds(command: &KeyCmds) -> &'static str {
    match command {
        KeyCmds::KeyVerificationKeyCmd(_) => "key verification-key",
    }
}
