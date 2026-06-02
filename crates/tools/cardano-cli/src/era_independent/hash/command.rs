//! EraIndependent hash command types.
//!
//! Mirrors upstream `Cardano.CLI.EraIndependent.Hash.Command`, the
//! command sum type behind `cardano-cli hash ...`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Hash/Command.hs`.
//! The Rust surface keeps the upstream constructor names for the
//! implemented pure offline slice. Anchor-data and script hashing
//! remain in the same upstream file and will be added here when their
//! ledger/script dependencies are wired end-to-end.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Subcommand;

/// Hash subcommands under `cardano-cli hash`.
///
/// Mirrors upstream `HashCmds`. R518 wires the pure offline
/// `HashGenesisFile` constructor first because it depends only on
/// byte-for-byte file reading plus Blake2b-256 hashing.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum HashCmds {
    /// Print the Blake2b-256 hash of a genesis file.
    ///
    /// Mirrors upstream `HashGenesisFile !GenesisFile` parsed by
    /// `hash genesis-file --genesis FILE`.
    #[command(name = "genesis-file")]
    HashGenesisFile {
        /// The genesis file. Mirrors upstream `pGenesisFile`, whose
        /// option name is `--genesis`.
        #[arg(long = "genesis")]
        genesis_file: GenesisFile,
    },
}

/// File path wrapper for a genesis file.
///
/// Mirrors upstream `Cardano.CLI.Type.Common.GenesisFile`, including
/// the command-line role rather than accepting an unlabelled `PathBuf`
/// throughout the hash runner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisFile(PathBuf);

impl GenesisFile {
    /// Construct a `GenesisFile` from a filesystem path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Return the wrapped path. Mirrors upstream `unGenesisFile`.
    pub fn un_genesis_file(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for GenesisFile {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl FromStr for GenesisFile {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(value))
    }
}

/// Render the canonical upstream command path for diagnostics.
///
/// Mirrors upstream `renderHashCmds`.
pub fn render_hash_cmds(command: &HashCmds) -> &'static str {
    match command {
        HashCmds::HashGenesisFile { .. } => "hash genesis-file",
    }
}
