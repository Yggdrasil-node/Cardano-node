//! Programmatic-introspection helpers for the kes-agent-control
//! deferred surfaces.
//!
//! R440 surfaces the upstream `Cardano.KESAgent.Processes.ControlClient`
//! socket I/O carve-out as a `*_status()` helper returning a
//! structured descriptor, mirroring the precedent set by
//! `crates/snapshot-converter/src/status.rs` (R439) +
//! cardano-tracer's R424-R429 carve-out inventory.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the kes-agent-control deferred carve-outs.

/// Status descriptor for the deferred ControlClient socket I/O
/// surface — the upstream
/// `Cardano.KESAgent.Processes.ControlClient` module that connects
/// to a running kes-agent daemon over its Unix-domain socket and
/// drives one of the 6 control subcommands (gen-staged-key /
/// export-staged-vkey / drop-staged-key / install-key / drop-key /
/// info).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControlClientStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// What this deferral depends on — the missing yggdrasil-side
    /// surface that needs to land first.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry point this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the deferral-status descriptor for the upstream
/// `ControlClient` socket I/O surface.
pub fn control_client_status() -> ControlClientStatus {
    ControlClientStatus {
        status: "deferred",
        depends_on: "the kes-agent server mini-arc (per the playful-tickling-plum.md plan, R344-R354 \
             — covers the daemon-side socket protocol that ControlClient connects to). The \
             socket protocol must be byte-equivalent or live SPO setups break, so this is \
             the highest-stakes parity surface in the sister-tools arc; the control client \
             is intentionally gated on the server arc landing first.",
        deferred_round: "R362+",
        upstream_reference: ".reference-haskell-cardano-node (post-R326b kes-agent vendor) — Cardano.KESAgent.Processes.ControlClient + the per-subcommand runners (runGenKey / runQueryKey / runDropStagedKey / runInstallKey / runDropKey / runGetInfo) in cli/ControlMain.hs",
    }
}

/// Stable identifier for one of the 6 kes-agent-control
/// subcommands. Used by [`crate::RunError::SubcommandSocketIoDeferred`]
/// to surface the operator's selected subcommand without coupling
/// to the full [`crate::types::ProgramOptions`] payload (which
/// carries paths + flags that the deferral message doesn't need).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Subcommand {
    /// `gen-staged-key` — generate a staged KES key.
    GenStagedKey,
    /// `export-staged-vkey` — export the staged verification key.
    ExportStagedVkey,
    /// `drop-staged-key` — discard the staged key.
    DropStagedKey,
    /// `install-key` — install a previously-staged key as the
    /// operational key.
    InstallKey,
    /// `drop-key` — drop the currently-installed operational key.
    DropKey,
    /// `info` — query the agent for its current state.
    Info,
}

impl Subcommand {
    /// The canonical CLI verb for this subcommand. Mirror of the
    /// upstream optparse-applicative `command` keyword.
    pub const fn cli_verb(self) -> &'static str {
        match self {
            Self::GenStagedKey => "gen-staged-key",
            Self::ExportStagedVkey => "export-staged-vkey",
            Self::DropStagedKey => "drop-staged-key",
            Self::InstallKey => "install-key",
            Self::DropKey => "drop-key",
            Self::Info => "info",
        }
    }
}

impl std::fmt::Display for Subcommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.cli_verb())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_client_status_describes_deferral() {
        let s = control_client_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("kes-agent server mini-arc"));
        assert!(s.upstream_reference.contains("ControlClient"));
    }

    #[test]
    fn subcommand_cli_verbs_match_upstream() {
        // Lock down the upstream-canonical CLI verbs.
        assert_eq!(Subcommand::GenStagedKey.cli_verb(), "gen-staged-key");
        assert_eq!(
            Subcommand::ExportStagedVkey.cli_verb(),
            "export-staged-vkey"
        );
        assert_eq!(Subcommand::DropStagedKey.cli_verb(), "drop-staged-key");
        assert_eq!(Subcommand::InstallKey.cli_verb(), "install-key");
        assert_eq!(Subcommand::DropKey.cli_verb(), "drop-key");
        assert_eq!(Subcommand::Info.cli_verb(), "info");
    }

    #[test]
    fn subcommand_display_matches_cli_verb() {
        assert_eq!(format!("{}", Subcommand::GenStagedKey), "gen-staged-key");
        assert_eq!(format!("{}", Subcommand::Info), "info");
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = control_client_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
