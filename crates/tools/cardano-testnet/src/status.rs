//! Programmatic-introspection helpers for the cardano-testnet
//! deferred surfaces.
//!
//! R445 surfaced the initial era-aware-dispatch + Process/Property carve-out
//! as a `*_status()` helper. R772-R823 filled in the era-free type surface and
//! the Parsers/Cardano.hs option-composition layer; R825 wired the typed
//! `Command` payloads and version subcommand; R826 added the
//! `Testnet/Types.hs` process-handle runtime record carriers; R827 ports the
//! pure `Testnet/Process/Cli/Keys.hs` command builders; R828 ports the
//! `Testnet/Process/Cli/Transaction.hs` sign/submit/txid builders; R829 ports
//! the pure `Testnet/Process/Cli/DRep.hs` key/cert/vote builders; R830 ports
//! the pure `Testnet/Process/Cli/SPO.hs` certificate/vote builders; R831 ports
//! the pure `Testnet/Process/Cli/Transaction.hs` spend-output txbody builders;
//! R832 ports the `Testnet/Process/Run.hs` flexible process wrappers; R833
//! ports the `Testnet/Process/RunIO.hs` plan-json binary-resolution and
//! process-planning helpers; R834 ports the remaining RunIO execution/liftIO
//! helpers; R835 ports the pure Property/Util harness helpers; R836 ports the
//! pure Property/Assert assertion helpers; R837 ports the CLI-backed
//! Property/Assert stake-pool query wrapper; R838 ports the pure
//! Property/Run user-env, ignore-helper, and runtime-message projection; R839
//! ports the pure `testnetProperty` workspace/action plan, keepalive fact, and
//! failed-start branch; recent slices port `Testnet/Components/Configuration.hs`
//! config/hash helpers plus `Testnet/Defaults.hs` pure node-configuration
//! defaults, default key/path helpers, and default P2P topology builders. The
//! remaining explicit deferral is node/KES spawning and supervision,
//! `createSPOGenesisAndFiles`, era genesis beyond pure config defaults, transaction
//! runtime/query orchestration, SPO runtime workflows, DRep runtime workflows,
//! and the remaining Process/Property harness bodies.
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
        depends_on: "the R772-R823 cardano-testnet implementation arc has shipped the era-free Testnet/Start/Types, path/default/component constants, and Parsers/Cardano option-composition surface. R825 typed Command payloads now thread CardanoTestnetCliOptions / CardanoTestnetCreateEnvOptions through parse_args/run and dispatch the version subcommand. R826 ports the Testnet/Types.hs process-handle runtime carriers (TestnetRuntime / TestnetNode / TestnetKesAgent), node socket helpers, and LocalNodeConnectInfo projection. R827 ports the pure Testnet/Process/Cli/Keys.hs cardano-cli key command builders. R828 ports the Testnet/Process/Cli/Transaction.hs sign/submit/txid builders. R829 ports the Testnet/Process/Cli/DRep.hs pure key/cert/vote builders. R830 ports the Testnet/Process/Cli/SPO.hs pure certificate/vote builders. R831 ports the Testnet/Process/Cli/Transaction.hs pure spend-output txbody builders. R832 ports the Testnet/Process/Run.hs flexible process execution wrappers. R833 ports the Testnet/Process/RunIO.hs plan-json binary-resolution and process-planning helpers. R834 ports the remaining RunIO execution/liftIO helpers. R835 ports the pure Testnet/Property/Util.hs retry/workspace naming, DISABLE_RETRIES, Linux predicate, and Aeson object lookup helpers. R836 ports the pure Testnet/Property/Assert.hs JSON-lines, relevant-slot extraction, deadline, stake-pool count, and era-equality assertion helpers. R837 ports the CLI-backed Testnet/Property/Assert.hs assertExpectedSposInLedgerState stake-pool query wrapper with injectable cardano-cli execution. R838 ports the pure Testnet/Property/Run.hs UserProvidedEnv, OS-ignore disposition helpers, and running-testnet operator message rendering. R839 ports the pure Testnet/Property/Run.hs testnetProperty workspace/action plan, keepalive delay, intentional-failure fact, and failed-start rendering. Recent slices port Testnet/Components/Configuration.hs createConfigJson / createConfigJsonNoHash / getByronGenesisHash / getShelleyGenesisHash / eraToString / anyEraToString plus Testnet/Defaults.hs pure node-configuration defaults, default key/path helpers, and default P2P topology builders: defaultGenesisFilepath, defaultYamlConfig, defaultYamlHardforkViaConfig, defaultSpoKeys, DRep / committee key paths, delegator stake keys, defaultUtxoKeys, defaultMainnetTopology, and defaultP2PTopology. Remaining work is node/KES spawning and supervision, createSPOGenesisAndFiles, era-genesis records beyond config defaults, transaction runtime/query orchestration, SPO runtime workflows, DRep runtime workflows, and the remaining Process/Property harness execution for cardano/create-env; Hedgehog Process/Property modules stay a Rust-idiomatic tokio::process + proptest carve-out.",
        deferred_round: "R840+",
        upstream_reference: ".reference-haskell-cardano-node/cardano-testnet/src/Parsers/{Run,Cardano}.hs + Testnet/{Defaults, Filepath, Orphans, Runtime, Types}.hs + Testnet/Start/{Types, Byron, Cardano}.hs + Testnet/Components/{Query, Configuration}.hs + Testnet/Process/Cli/*.hs + Testnet/Property/*.hs",
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
        assert!(s.depends_on.contains("R772-R823"));
        assert!(s.depends_on.contains("R825 typed Command payloads"));
        assert!(s.depends_on.contains("R826 ports"));
        assert!(s.depends_on.contains("R827 ports"));
        assert!(s.depends_on.contains("R828 ports"));
        assert!(s.depends_on.contains("R829 ports"));
        assert!(s.depends_on.contains("R830 ports"));
        assert!(s.depends_on.contains("R831 ports"));
        assert!(s.depends_on.contains("R832 ports"));
        assert!(s.depends_on.contains("R833 ports"));
        assert!(s.depends_on.contains("R834 ports"));
        assert!(s.depends_on.contains("R835 ports"));
        assert!(s.depends_on.contains("R836 ports"));
        assert!(s.depends_on.contains("R837 ports"));
        assert!(s.depends_on.contains("R838 ports"));
        assert!(s.depends_on.contains("R839 ports"));
        assert!(s.depends_on.contains("defaultYamlHardforkViaConfig"));
        assert!(s.depends_on.contains("defaultSpoKeys"));
        assert!(s.depends_on.contains("defaultUtxoKeys"));
        assert!(s.depends_on.contains("defaultMainnetTopology"));
        assert!(s.depends_on.contains("defaultP2PTopology"));
        assert!(s.depends_on.contains("node/KES spawning and supervision"));
        assert_eq!(s.deferred_round, "R840+");
        assert!(s.depends_on.contains("Hedgehog"));
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
