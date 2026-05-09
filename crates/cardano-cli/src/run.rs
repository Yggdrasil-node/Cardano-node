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
            // Stub: per-cluster `helper::version_info` will land in R295
            // (sweeper). Until then, the node binary's existing
            // version-output path covers this case.
            println!("yggdrasil-cardano-cli skeleton — Phase F bootstrap (R289)");
            Ok(())
        }
        Command::ShowUpstreamConfig { .. } => {
            // Stub: forwards to the existing
            // `node/src/commands/cardano_cli.rs` implementation in
            // R289. Migration to in-crate `environment::resolve_*`
            // helpers happens in R290 (Byron cluster) since the
            // existing implementation lives at the same upstream
            // namespace level.
            eyre::bail!(
                "ShowUpstreamConfig is implemented in node/src/commands/cardano_cli.rs; \
                 migration to yggdrasil-cardano-cli is scheduled for R290+"
            );
        }
        Command::QueryTip { .. } => {
            // Stub: same forwarding as above; QueryTip migrates with
            // R291 (Compatible cluster) since upstream lives in
            // `Cardano.CLI.Compatible.Run::runQueryTip`.
            eyre::bail!(
                "QueryTip is implemented in node/src/commands/cardano_cli.rs; \
                 migration to yggdrasil-cardano-cli is scheduled for R291"
            );
        }
    }
}
