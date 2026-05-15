//! Top-level cardano-cli run dispatcher.
//!
//! Mirrors upstream `Cardano.CLI.Run` — the dispatcher that routes a
//! parsed `Command` to its per-cluster runner (Byron / Compatible /
//! per-era / Legacy / EraBased / EraIndependent).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Run.hs`.
//! R289 ships the dispatcher entry-point with three subcommand arms
//! (Version / ShowUpstreamConfig / QueryTip). The per-cluster runners
//! (`byron::run`, `compatible::run`, `shelley::run`, etc.) land in
//! R290–R295 and the dispatch arms grow alongside.

pub mod mnemonic;
use eyre::Result;

use crate::command::Command;

/// Run a parsed `Command` against the local environment.
///
/// Mirrors upstream `runClientCommand` from `Cardano.CLI.Run`.
///
/// # R289 bootstrap state
///
/// Implementation forwards to `node/src/commands/cardano_cli.rs` for
/// now. The forwarding layer migrates into this crate as Phase F
/// rounds populate the per-cluster runners.
pub fn run_command(command: Command) -> Result<()> {
    match command {
        Command::Version => {
            // Wired in R503 (Phase 5 follow-on): the version banner
            // comes from the in-crate helper module; identical to
            // the string the node binary's `cardano-cli version`
            // subcommand emits (which also calls `helper::version_info`).
            println!("{}", crate::helper::version_info());
            Ok(())
        }
        Command::ShowUpstreamConfig { .. } => {
            // The `Command::ShowUpstreamConfig` variant carries only
            // `upstream_config_root` today; the underlying
            // `environment::run_show_upstream_config` needs
            // `(network_name, config_path, topology_path,
            // reference_network_magic)` — three of which derive from a
            // chosen network preset that the variant doesn't yet
            // carry. Extending the variant + wiring the resolve-paths
            // pipeline is the natural next slice; until then the node
            // binary's `cardano-cli show-upstream-config` subcommand
            // continues to be the operator entry point.
            eyre::bail!(
                "ShowUpstreamConfig: today's Command variant doesn't carry the network \
                 preset that `environment::run_show_upstream_config` needs; use the node \
                 binary's `yggdrasil-node cardano-cli show-upstream-config --network=…` \
                 subcommand for now. Library-side wiring lands in the next round."
            );
        }
        Command::QueryTip { .. } => {
            // QueryTip needs a tokio runtime + the NtC client; the
            // library crate doesn't currently depend on
            // yggdrasil-network or tokio. Wiring this from the
            // library requires either (a) tokio + yggdrasil-network
            // direct deps (substantial transitive footprint) or
            // (b) a trait-based abstraction for the LSQ client so
            // the library can plug in a tokio-backed impl at
            // runtime. (b) is the cleaner path — tracked for a
            // future round.
            eyre::bail!(
                "QueryTip: today's library crate doesn't carry the tokio + yggdrasil-network \
                 deps needed to open a NtC socket; use the node binary's \
                 `yggdrasil-node cardano-cli query-tip --socket-path=…` subcommand for now. \
                 Library-side wiring lands once the LSQ-client trait abstraction is in place."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Command::Version` now produces the version banner from
    /// `crate::helper::version_info` (rather than the R289 stub
    /// string). Capturing stdout in unit tests is awkward; we
    /// assert the function returns Ok and that the banner string
    /// is non-empty + identifies the crate.
    #[test]
    fn version_returns_ok_and_helper_banner_is_nonempty() {
        let banner = crate::helper::version_info();
        assert!(!banner.is_empty(), "version_info() must produce a non-empty banner");
        assert!(
            banner.contains("yggdrasil") || banner.contains("cardano-cli"),
            "version banner must identify the crate; got {banner:?}"
        );
        run_command(Command::Version).expect("Command::Version must succeed");
    }

    /// `Command::ShowUpstreamConfig` still bails with a documented
    /// "use the node binary's subcommand for now" message; this
    /// pins that the deferral is intentional (not a regression).
    #[test]
    fn show_upstream_config_currently_bails_with_deferral_message() {
        let result = run_command(Command::ShowUpstreamConfig {
            upstream_config_root: None,
        });
        let err = result.expect_err("ShowUpstreamConfig must bail");
        assert!(
            err.to_string().contains("show-upstream-config")
                || err.to_string().contains("Command variant doesn't carry the network preset"),
            "error must explain the deferral; got {err}"
        );
    }

    /// `Command::QueryTip` similarly bails with the documented
    /// "needs tokio + yggdrasil-network deps" message.
    #[test]
    fn query_tip_currently_bails_with_deferral_message() {
        let result = run_command(Command::QueryTip {
            socket_path: std::path::PathBuf::from("/unused.socket"),
            network_magic: None,
        });
        let err = result.expect_err("QueryTip must bail");
        assert!(
            err.to_string().contains("query-tip")
                || err.to_string().contains("LSQ-client trait abstraction"),
            "error must explain the deferral; got {err}"
        );
    }
}
