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

/// Top-level dispatch enum for `yggdrasil-cardano-cli`.
///
/// Mirrors the entry-point shape of upstream `ClientCommand` from
/// `Cardano.CLI.Command`. R289 ships the three subcommands the node
/// binary's `cardano-cli` subcommand already implements; per-cluster
/// rounds R290–R295 expand the variant set to mirror upstream's full
/// surface (Byron / Compatible / Shelley / Alonzo / Babbage / Conway).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Print pure-Rust cardano-cli compatibility version info.
    /// Mirrors upstream `DisplayVersion` arm.
    Version,
    /// Show resolved reference config paths and network magic.
    /// Mirrors upstream's `Cardano.CLI.Helper`-style operator
    /// introspection helpers; Yggdrasil-specific utility.
    ShowUpstreamConfig {
        /// Override path for the upstream Haskell-share root
        /// (typically `/tmp/cardano-tooling/share`); falls back to
        /// the vendored `node/configuration/<network>/` directory.
        upstream_config_root: Option<PathBuf>,
    },
    /// Query the running node for tip / chain-point / block-no.
    /// Mirrors upstream `QueryTip` from `Cardano.CLI.Compatible.Run`.
    QueryTip {
        /// Path to the node socket.
        socket_path: PathBuf,
        /// Override network magic instead of using the upstream
        /// reference config.
        network_magic: Option<u32>,
    },
}
