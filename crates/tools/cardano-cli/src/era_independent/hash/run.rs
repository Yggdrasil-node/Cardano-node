//! EraIndependent hash command runner.
//!
//! Mirrors upstream `Cardano.CLI.EraIndependent.Hash.Run`, which
//! dispatches parsed `HashCmds` values to concrete hash commands.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraIndependent/Hash/Run.hs`.
//! R518 wires the `HashGenesisFile` branch with the same file-byte
//! Blake2b-256 behavior as upstream `runHashGenesisFile`.

use std::fs;
use std::path::Path;

use eyre::{Result, WrapErr};

use crate::era_independent::hash::command::{GenesisFile, HashCmds};

/// Dispatch an era-independent hash command.
///
/// Mirrors upstream `runHashCmds`.
pub fn run_hash_cmds(command: HashCmds) -> Result<()> {
    match command {
        HashCmds::HashGenesisFile { genesis_file } => run_hash_genesis_file(&genesis_file),
    }
}

/// Print the Blake2b-256 hash of a genesis file as lowercase hex.
///
/// Mirrors upstream `runHashGenesisFile`: read the file bytes exactly
/// as stored, hash them with Blake2b-256, and print the hexadecimal
/// digest followed by a newline.
pub fn run_hash_genesis_file(genesis_file: &GenesisFile) -> Result<()> {
    let bytes = fs::read(genesis_file.un_genesis_file()).wrap_err_with(|| {
        format!(
            "failed to read genesis file {}",
            genesis_file.un_genesis_file().display()
        )
    })?;
    println!("{}", hash_genesis_file_bytes(&bytes));
    Ok(())
}

/// Return the `hash genesis-file` digest for already-read bytes.
///
/// This helper keeps tests deterministic without capturing stdout; it
/// is the same Blake2b-256 operation used by [`run_hash_genesis_file`].
pub fn hash_genesis_file_bytes(bytes: &[u8]) -> String {
    hex::encode(yggdrasil_crypto::hash_bytes_256(bytes).0)
}

/// Read a genesis file path and return the digest string.
///
/// Useful for library callers that need the same behavior as the CLI
/// command without printing to stdout.
pub fn hash_genesis_file_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .wrap_err_with(|| format!("failed to read genesis file {}", path.display()))?;
    Ok(hash_genesis_file_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_genesis_file_bytes_matches_blake2b_256_hex() {
        let bytes = br#"{"systemStart":"2026-06-02T00:00:00Z"}"#;
        let expected = hex::encode(yggdrasil_crypto::hash_bytes_256(bytes).0);
        assert_eq!(hash_genesis_file_bytes(bytes), expected);
    }

    #[test]
    fn hash_genesis_file_path_reads_exact_file_bytes() {
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-cli-genesis-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("hash")
        ));
        let bytes = b"{
  \"networkMagic\": 42
}
";
        std::fs::write(&path, bytes).expect("write genesis fixture");

        let actual = hash_genesis_file_path(&path).expect("hash fixture");
        let expected = hash_genesis_file_bytes(bytes);
        let _ = std::fs::remove_file(&path);

        assert_eq!(actual, expected);
    }

    #[test]
    fn run_hash_cmds_accepts_hash_genesis_file_constructor() {
        let command = HashCmds::HashGenesisFile {
            genesis_file: GenesisFile::new("/tmp/genesis.json"),
        };
        assert_eq!(
            crate::era_independent::hash::command::render_hash_cmds(&command),
            "hash genesis-file"
        );
    }
}
