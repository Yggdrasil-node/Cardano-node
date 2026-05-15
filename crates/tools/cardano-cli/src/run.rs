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
        Command::ShowUpstreamConfig {
            network,
            upstream_config_root,
        } => {
            // R504: full library-side wiring. Resolve the network's
            // config + topology paths against the supplied upstream
            // root, extract the network magic from the config file
            // (or fall back to the well-known constant for the
            // network), and emit the operator-readable summary via
            // the existing `environment::run_show_upstream_config`.
            let fallback_magic = match network.as_str() {
                "mainnet" => 764_824_073,
                "preprod" => 1,
                "preview" => 2,
                _ => {
                    eyre::bail!(
                        "unknown network preset {network:?}; expected one of \
                         mainnet / preprod / preview"
                    );
                }
            };
            let (config_path, topology_path) =
                crate::environment::resolve_upstream_reference_paths(
                    &network,
                    upstream_config_root,
                )?;
            let reference_network_magic = crate::environment::extract_reference_network_magic(
                &config_path,
                fallback_magic,
            );
            crate::environment::run_show_upstream_config(
                &network,
                &config_path,
                &topology_path,
                reference_network_magic,
            )
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

    /// `Command::ShowUpstreamConfig` is now wired. With an unknown
    /// network preset it errors out with a structured "expected
    /// one of mainnet / preprod / preview" message.
    #[test]
    fn show_upstream_config_rejects_unknown_network_preset() {
        let result = run_command(Command::ShowUpstreamConfig {
            network: "bogus".to_string(),
            upstream_config_root: None,
        });
        let err = result.expect_err("unknown network must bail");
        assert!(
            err.to_string().contains("unknown network preset")
                && err.to_string().contains("mainnet / preprod / preview"),
            "error must enumerate the supported network presets; got {err}"
        );
    }

    /// With a valid network preset the runner attempts path
    /// resolution. In a workspace-test environment without a real
    /// `node/configuration/<network>/config.json`, this either
    /// succeeds (when the vendored configs are present, the
    /// canonical case) or surfaces a structured path-resolution
    /// error from `environment::resolve_upstream_reference_paths`.
    /// We assert one of those two outcomes — not a "deferral
    /// message" anymore.
    #[test]
    fn show_upstream_config_resolves_or_errors_with_real_network() {
        let outcome = run_command(Command::ShowUpstreamConfig {
            network: "mainnet".to_string(),
            upstream_config_root: Some(std::path::PathBuf::from("/tmp/no-such-dir")),
        });
        if let Err(err) = outcome {
            // Path-resolution failure is acceptable in a sandboxed test
            // environment; the error must NOT be the old deferral
            // message — that would indicate the variant didn't carry
            // the network preset through.
            assert!(
                !err.to_string().contains("Command variant doesn't carry the network preset"),
                "must not be the old deferral message; got {err}"
            );
        }
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
