//! `db-analyser` run metadata — fed to the BenchmarkLedgerOps JSON
//! output stream so the analysis CSV/JSON can be correlated with the
//! environment that produced it.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/Metadata.hs.
//!
//! Direct port of the metadata-record + JSON-instance + getter
//! surface used by upstream's BenchmarkLedgerOps analysis.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                                          |
//! |---------------------------------------------------------|----------------------------------------------------|
//! | `data Metadata = Metadata { rtsGCMaxStkSize, ... }`     | [`Metadata`] (10-field struct)                     |
//! | `instance ToJSON Metadata`                              | `serde::Serialize` derive on [`Metadata`]          |
//! | `getMetadata :: LedgerApplicationMode -> IO Metadata`   | [`Metadata::collect`]                              |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`GHC.RTS.Flags`** (rts_gc_max_stk_size / rts_gc_max_heap_size /
//!   rts_concurrent_ctxt_switch_time / rts_par_n_capabilities): the
//!   four RTS-flag fields capture upstream Haskell's runtime-system
//!   stack/heap/context-switch/capability settings. Yggdrasil runs
//!   on the Rust runtime — there is no equivalent flag surface, and
//!   the values would be meaningless if synthesized from Rust's
//!   thread/heap configuration. The four fields are kept for JSON
//!   key-shape parity but populated with zeros, which is the same
//!   shape upstream would produce on a default-flags run with no
//!   GC-stats enabled. A future round can wire to crates such as
//!   `tikv-jemalloc-ctl` if RSS-pressure observability becomes
//!   relevant.
//! - **`Cardano.Tools.GitRev.gitRev`**: upstream uses a
//!   TemplateHaskell-driven git-info splice (`tGitInfoCwdTry`) to
//!   embed the commit hash at build time. Yggdrasil reads the
//!   `YGGDRASIL_GIT_REV` env var at compile time via `env!()`,
//!   falling back to "unavailable (git info missing at build time)"
//!   to mirror upstream's exact fallback string.
//! - **`Data.Version.showVersion System.Info.compilerVersion`**:
//!   upstream emits the Haskell compiler version. Yggdrasil emits
//!   the rustc version it was built with, captured at compile time
//!   via the `RUSTC_VERSION` env var (set by the build harness) with
//!   a fallback to the `rust-toolchain.toml` pinned version.

use serde::Serialize;

use crate::types::LedgerApplicationMode;

/// Run-environment metadata accompanying a BenchmarkLedgerOps output.
/// Mirror of upstream `data Metadata = Metadata { ... }`.
///
/// Field-order preserves the upstream record-syntax declaration so
/// JSON output emits keys in the same order. All field types match
/// upstream's `Word32` / `Word64` / `String` widths; the four RTS
/// fields are zero-populated per the carve-out documented in the
/// module docstring.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct Metadata {
    /// Upstream RTS GC max stack size — zero in Yggdrasil per
    /// carve-out (no Rust analog).
    #[serde(rename = "rtsGCMaxStkSize")]
    pub rts_gc_max_stk_size: u32,
    /// Upstream RTS GC max heap size — zero in Yggdrasil per
    /// carve-out (no Rust analog).
    #[serde(rename = "rtsGCMaxHeapSize")]
    pub rts_gc_max_heap_size: u32,
    /// Upstream RTS concurrent context-switch time — zero in
    /// Yggdrasil per carve-out (no Rust analog).
    #[serde(rename = "rtsConcurrentCtxtSwitchTime")]
    pub rts_concurrent_ctxt_switch_time: u64,
    /// Upstream RTS `-N` (parallel-runtime capability count) — zero
    /// in Yggdrasil per carve-out (no Rust analog).
    #[serde(rename = "rtsParNCapabilities")]
    pub rts_par_n_capabilities: u32,
    /// Compiler version string. Yggdrasil emits the rustc version
    /// it was built with, captured at compile time.
    #[serde(rename = "compilerVersion")]
    pub compiler_version: String,
    /// Compiler name. Yggdrasil hardcodes "rustc".
    #[serde(rename = "compilerName")]
    pub compiler_name: String,
    /// Host operating system (`std::env::consts::OS`).
    #[serde(rename = "operatingSystem")]
    pub operating_system: String,
    /// Host machine architecture (`std::env::consts::ARCH`).
    #[serde(rename = "machineArchitecture")]
    pub machine_architecture: String,
    /// Git revision of the source tree at build time. Note: the JSON
    /// key matches upstream's `gitRevison` typo exactly for
    /// byte-equivalent output.
    #[serde(rename = "gitRevison")]
    pub git_revison: String,
    /// Ledger application mode, rendered as `"full-application"` for
    /// LedgerApply or `"reapplication"` for LedgerReapply (matching
    /// upstream's exact strings).
    #[serde(rename = "ledgerApplicationMode")]
    pub ledger_application_mode: String,
}

impl Metadata {
    /// Capture the current run's metadata. Mirror of upstream
    /// `getMetadata :: LedgerApplicationMode -> IO Metadata`.
    ///
    /// Ledger-mode rendering:
    /// - [`LedgerApplicationMode::LedgerApply`] → `"full-application"`
    /// - [`LedgerApplicationMode::LedgerReapply`] → `"reapplication"`
    ///
    /// matching upstream's exact strings for byte-equivalent JSON.
    pub fn collect(ledger_application_mode: LedgerApplicationMode) -> Self {
        Metadata {
            rts_gc_max_stk_size: 0,
            rts_gc_max_heap_size: 0,
            rts_concurrent_ctxt_switch_time: 0,
            rts_par_n_capabilities: 0,
            compiler_version: rustc_version_string(),
            compiler_name: "rustc".to_string(),
            operating_system: std::env::consts::OS.to_string(),
            machine_architecture: std::env::consts::ARCH.to_string(),
            git_revison: git_rev_string(),
            ledger_application_mode: render_ledger_application_mode(ledger_application_mode),
        }
    }
}

/// Render a [`LedgerApplicationMode`] value as the upstream
/// JSON-output string. Mirror of upstream's
/// `case lgrAppMode of LedgerApply -> "full-application"; LedgerReapply -> "reapplication"`.
pub fn render_ledger_application_mode(mode: LedgerApplicationMode) -> String {
    match mode {
        LedgerApplicationMode::LedgerApply => "full-application".to_string(),
        LedgerApplicationMode::LedgerReapply => "reapplication".to_string(),
    }
}

/// Resolve the compile-time rustc version string. Yggdrasil's build
/// harness sets `YGGDRASIL_RUSTC_VERSION` from `rustc --version`; if
/// it's unset (e.g. during plain `cargo build` in a clean checkout)
/// we fall back to the toolchain pin.
fn rustc_version_string() -> String {
    option_env!("YGGDRASIL_RUSTC_VERSION")
        .unwrap_or("rustc 1.95.0 (yggdrasil pinned toolchain)")
        .to_string()
}

/// Resolve the compile-time git-revision string mirroring upstream's
/// `gitRev`. The build harness sets `YGGDRASIL_GIT_REV`; if unset, we
/// fall back to upstream's exact "unavailable" sentinel for
/// byte-equivalent output.
fn git_rev_string() -> String {
    option_env!("YGGDRASIL_GIT_REV")
        .unwrap_or("unavailable (git info missing at build time)")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_default_is_all_zeros_and_empty_strings() {
        let md = Metadata::default();
        assert_eq!(md.rts_gc_max_stk_size, 0);
        assert_eq!(md.rts_gc_max_heap_size, 0);
        assert_eq!(md.rts_concurrent_ctxt_switch_time, 0);
        assert_eq!(md.rts_par_n_capabilities, 0);
        assert_eq!(md.compiler_version, "");
        assert_eq!(md.compiler_name, "");
        assert_eq!(md.operating_system, "");
        assert_eq!(md.machine_architecture, "");
        assert_eq!(md.git_revison, "");
        assert_eq!(md.ledger_application_mode, "");
    }

    #[test]
    fn render_ledger_application_mode_full_application() {
        assert_eq!(
            render_ledger_application_mode(LedgerApplicationMode::LedgerApply),
            "full-application",
        );
    }

    #[test]
    fn render_ledger_application_mode_reapplication() {
        assert_eq!(
            render_ledger_application_mode(LedgerApplicationMode::LedgerReapply),
            "reapplication",
        );
    }

    #[test]
    fn collect_full_application_round_trip() {
        let md = Metadata::collect(LedgerApplicationMode::LedgerApply);
        // RTS carve-outs always zero.
        assert_eq!(md.rts_gc_max_stk_size, 0);
        assert_eq!(md.rts_par_n_capabilities, 0);
        // Compiler name is hardcoded.
        assert_eq!(md.compiler_name, "rustc");
        // Compiler version is non-empty (either env-driven or pin).
        assert!(!md.compiler_version.is_empty());
        // OS + ARCH are populated from std::env::consts.
        assert!(!md.operating_system.is_empty());
        assert!(!md.machine_architecture.is_empty());
        // Git rev is non-empty (either env-driven or sentinel).
        assert!(!md.git_revison.is_empty());
        // Ledger mode rendered correctly.
        assert_eq!(md.ledger_application_mode, "full-application");
    }

    #[test]
    fn collect_reapplication_renders_correct_string() {
        let md = Metadata::collect(LedgerApplicationMode::LedgerReapply);
        assert_eq!(md.ledger_application_mode, "reapplication");
    }

    #[test]
    fn metadata_serializes_with_upstream_camel_case_keys() {
        let md = Metadata {
            rts_gc_max_stk_size: 1000,
            rts_gc_max_heap_size: 2000,
            rts_concurrent_ctxt_switch_time: 3000,
            rts_par_n_capabilities: 4,
            compiler_version: "rustc 1.95.0".to_string(),
            compiler_name: "rustc".to_string(),
            operating_system: "linux".to_string(),
            machine_architecture: "x86_64".to_string(),
            git_revison: "abc123def".to_string(),
            ledger_application_mode: "full-application".to_string(),
        };
        let json = serde_json::to_value(&md).expect("serializes");
        assert_eq!(json["rtsGCMaxStkSize"], 1000);
        assert_eq!(json["rtsGCMaxHeapSize"], 2000);
        assert_eq!(json["rtsConcurrentCtxtSwitchTime"], 3000);
        assert_eq!(json["rtsParNCapabilities"], 4);
        assert_eq!(json["compilerVersion"], "rustc 1.95.0");
        assert_eq!(json["compilerName"], "rustc");
        assert_eq!(json["operatingSystem"], "linux");
        assert_eq!(json["machineArchitecture"], "x86_64");
        // Note: upstream typo `gitRevison` (sic) is preserved.
        assert_eq!(json["gitRevison"], "abc123def");
        assert_eq!(json["ledgerApplicationMode"], "full-application");
    }

    #[test]
    fn metadata_field_order_matches_upstream() {
        let md = Metadata {
            rts_gc_max_stk_size: 1,
            rts_gc_max_heap_size: 2,
            rts_concurrent_ctxt_switch_time: 3,
            rts_par_n_capabilities: 4,
            compiler_version: "v".to_string(),
            compiler_name: "n".to_string(),
            operating_system: "o".to_string(),
            machine_architecture: "a".to_string(),
            git_revison: "g".to_string(),
            ledger_application_mode: "m".to_string(),
        };
        let json = serde_json::to_string(&md).expect("serializes");
        // Upstream record-declaration field order is preserved in the
        // JSON output. We check that rtsGCMaxStkSize precedes
        // ledgerApplicationMode.
        let stk_pos = json.find("rtsGCMaxStkSize").expect("has stk size");
        let mode_pos = json.find("ledgerApplicationMode").expect("has mode");
        assert!(
            stk_pos < mode_pos,
            "field order: rtsGCMaxStkSize ({stk_pos}) must precede ledgerApplicationMode ({mode_pos})",
        );
        // And that the order within RTS group + meta group is also
        // preserved: stk → heap → ctxt → par.
        let heap_pos = json.find("rtsGCMaxHeapSize").expect("has heap size");
        let ctxt_pos = json
            .find("rtsConcurrentCtxtSwitchTime")
            .expect("has ctxt time");
        let par_pos = json.find("rtsParNCapabilities").expect("has par");
        assert!(stk_pos < heap_pos);
        assert!(heap_pos < ctxt_pos);
        assert!(ctxt_pos < par_pos);
    }
}
