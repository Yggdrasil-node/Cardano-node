//! Shared option parsers.
//!
//! Mirrors upstream `Cardano.CLI.Option` — option parsers that are
//! reusable across multiple sub-parsers (file paths, network magic,
//! socket path, JSON file readers, etc.).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Option.hs`.
//! R289 lands the file as a skeleton; concrete parsers come with
//! the per-cluster rounds since they are consumed in tandem with the
//! runners.

use std::path::PathBuf;

/// Parse a `--socket-path /path/to/sock` argument. Reusable across
/// query / submit-tx / tx-mempool subcommands.
///
/// Mirrors upstream `pSocketPath` from `Cardano.CLI.Option`.
pub fn parse_socket_path(s: &str) -> Result<PathBuf, std::io::Error> {
    let path = PathBuf::from(s);
    if path.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "socket path is empty",
        ));
    }
    Ok(path)
}

/// Parse a `--network-magic <u32>` argument.
///
/// Mirrors upstream `pNetworkMagic` from `Cardano.CLI.Option`.
pub fn parse_network_magic(s: &str) -> Result<u32, std::num::ParseIntError> {
    s.parse::<u32>()
}
