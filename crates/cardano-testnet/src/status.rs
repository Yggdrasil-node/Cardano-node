//! Programmatic-introspection helpers for the cardano-testnet
//! deferred surfaces.
//!
//! R445 surfaces the era-aware-dispatch + Process/Property carve-outs as a `*_status()` helper.
//!
//! Mirrors the precedent set by R424-R429 cardano-tracer +
//! R439-R444 sister-tool deferral sweeps.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side documentation
//! infrastructure for the cardano-testnet deferred carve-outs.

/// Stable identifier for one of the 3 cardano-testnet
/// top-level subcommands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Subcommand {
    /// `cardano` — multi-node testnet spinup.
    Cardano,
    /// `create-env` — generate genesis + topology files for a new env.
    CreateEnv,
    /// `version` — print version string (already byte-equivalent to upstream).
    Version,
}

impl Subcommand {
    /// The canonical CLI verb for this subcommand.
    pub const fn cli_verb(self) -> &'static str {
        match self {
            Self::Cardano => "cardano",
            Self::CreateEnv => "create-env",
            Self::Version => "version",
        }
    }
}

impl std::fmt::Display for Subcommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.cli_verb())
    }
}

/// Status descriptor for the deferred cardano-testnet era-aware
/// dispatch surface.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EraDispatchStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// What this deferral depends on.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
    /// Pointer to the upstream Haskell entry points this surface
    /// would mirror.
    pub upstream_reference: &'static str,
}

/// Get the deferral-status descriptor for the upstream
/// per-subcommand era-aware dispatch.
pub fn era_dispatch_status() -> EraDispatchStatus {
    EraDispatchStatus {
        status: "deferred",
        depends_on: "the cardano-testnet mini-arc per the playful-tickling-plum.md plan (R416-R433 — LARGE; 32 upstream .hs files; Hedgehog Process/Property modules approved as Rust-idiomatic carve-out using tokio::process + proptest). Gated on yggdrasil-ledger's era surface being exposed at crate boundaries.",
        deferred_round: "R367+",
        upstream_reference: ".reference-haskell-cardano-node/cardano-testnet/src/Testnet/{Defaults, Filepath, Orphans, Runtime, Types}.hs + Testnet/Start/{Types, Byron, Cardano}.hs + Testnet/Components/{Query, Configuration}.hs + Testnet/Process/Cli/*.hs (era-aware subcommand option records)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommand_cli_verbs_match_upstream() {
        assert_eq!(Subcommand::Cardano.cli_verb(), "cardano");
        assert_eq!(Subcommand::CreateEnv.cli_verb(), "create-env");
        assert_eq!(Subcommand::Version.cli_verb(), "version");
    }

    #[test]
    fn subcommand_display_matches_cli_verb() {
        assert_eq!(format!("{}", Subcommand::Cardano), "cardano");
        assert_eq!(format!("{}", Subcommand::CreateEnv), "create-env");
    }

    #[test]
    fn era_dispatch_status_describes_deferral() {
        let s = era_dispatch_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("cardano-testnet mini-arc"));
        assert!(s.depends_on.contains("Hedgehog"));
        assert!(s.depends_on.contains("yggdrasil-ledger"));
        assert!(s.upstream_reference.contains("Testnet"));
    }

    #[test]
    fn status_is_clone_eq_hash_round_trip() {
        let s1 = era_dispatch_status();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        let mut set = std::collections::HashSet::new();
        set.insert(s1);
        assert!(set.contains(&s2));
    }
}
