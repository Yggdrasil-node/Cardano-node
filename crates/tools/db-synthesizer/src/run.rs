//! ChainDB-open + synthesize supervisor for the `db-synthesizer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Run.hs.
//!
//! Direct port of upstream's `Cardano.Tools.DBSynthesizer.Run`:
//!
//! - [`pre_open_chain_db`] — mirror of upstream `preOpenChainDB`: the
//!   `DBSynthesizerOpenMode` (`OpenCreate`/`OpenCreateForce`/`OpenAppend`)
//!   directory-state handling, including the `checkIsDB` heuristic
//!   (a directory "looks like a ChainDB" iff every entry is one of
//!   `immutable`/`ledger`/`volatile`/`gsm`).
//! - [`synthesize`] — mirror of upstream `synthesize`: opens the
//!   ChainDB at `confDbDir`, finds the resume slot from the current
//!   tip, and drives [`crate::forging::run_forge`].
//! - [`resolve_epoch_size_from_config`] — mirror of the genesis-loading
//!   half of upstream `initialize`/`initConf`: `config.json` →
//!   `NodeConfigStub` → Shelley genesis → real `epochLength` (R2).
//! - [`synthesize_from_config`] — the production entry point: resolve
//!   the genesis epoch length, then [`synthesize`].
//!
//! ## Carve-out (Phase 4 R3 slice boundary)
//!
//! - **`initialize` — protocol-building half.** Upstream's
//!   `initProtocol` builds `CardanoProtocolParams` via
//!   `mkConsensusProtocolCardano` (the multi-era hard-fork plan).
//!   Until that lands, synthesized blocks keep the [`SYNTH_ERA`]
//!   structural stamp.
//! - **The Praos forge path** (`BlockForging`, `checkShouldForge`,
//!   `forgeBlock`) — the per-slot VRF/KES/OpCert leader check. See
//!   [`crate::forging`]'s module note. This slice forges deterministic
//!   non-Praos structural blocks.
//!
//! What this slice DOES port faithfully: the `preOpenChainDB` mode
//! semantics (directory creation, the `OpenCreate`-refuses-non-empty
//! rule, `OpenAppend`/`OpenCreateForce` on a ChainDB-shaped directory,
//! and the abort-on-foreign-directory rule) and the tip-driven
//! resume-slot derivation.

use std::collections::BTreeSet;
use std::path::Path;

use yggdrasil_ledger::{Era, Point, SlotNo};
use yggdrasil_node_genesis::load_shelley_genesis;
use yggdrasil_storage::{FileImmutable, ImmutableStore, StorageError};

use crate::forging::{ForgeRunOutcome, run_forge};
use crate::orphans::{AdjustFilePaths, parse_node_config_stub};
use crate::types::{DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit};

/// Fallback epoch length for the config-free [`synthesize_default`]
/// convenience.
///
/// `432000` is the mainnet Shelley `epochLength`. The production entry
/// point [`synthesize_from_config`] parses the real `sgEpochLength`
/// from the node config's Shelley genesis (R2); this constant only
/// backs the config-free test / convenience helper.
pub const DEFAULT_EPOCH_SIZE: u64 = 432_000;

/// Era tag stamped onto synthesized structural blocks until the
/// genesis-derived hard-fork era plan is wired (R3 — `initProtocol`).
pub const SYNTH_ERA: Era = Era::Shelley;

/// Directory names that mark a directory as a ChainDB.
///
/// Mirror of upstream `chainDBDirs = Set.fromList ["immutable",
/// "ledger", "volatile", "gsm"]`. Yggdrasil's `FileImmutable` also
/// writes a `.dirty` marker file plus per-block CBOR files inside
/// `immutable/`, so the directory-level heuristic is applied against
/// sub-directories only (matching upstream's `listSubdirectories`).
const CHAIN_DB_DIRS: &[&str] = &["immutable", "ledger", "volatile", "gsm"];

/// Errors from the db-synthesizer run supervisor.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Underlying storage failure (ChainDB open / append).
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    /// `OpenCreate` was requested but the target directory already
    /// exists. Mirror of upstream's
    /// `"<loc> already exists. Use -f to overwrite or -a to append."`.
    #[error("preOpenChainDB: '{0}' already exists. Use -f to overwrite or -a to append.")]
    AlreadyExists(String),
    /// The target directory is non-empty and does not look like a
    /// ChainDB. Mirror of upstream's foreign-directory abort.
    #[error(
        "preOpenChainDB: '{0}' is non-empty and does not look like a ChainDB \
         (i.e. it contains directories other than 'immutable'/'ledger'/'volatile'/'gsm'). \
         Aborting."
    )]
    NotAChainDb(String),
    /// An I/O error while inspecting / preparing the target directory.
    #[error("preOpenChainDB: I/O error on '{path}': {source}")]
    Io {
        /// Target directory the error occurred on.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The node `config.json` could not be read. Mirror of upstream
    /// `initConf`'s `handleIOExceptT show (BS.readFile nfpConfig)`.
    #[error("initialize: cannot read config '{path}': {source}")]
    ConfigRead {
        /// Config-file path.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The node `config.json` is not valid JSON.
    #[error("initialize: config '{path}' is not valid JSON: {source}")]
    ConfigParse {
        /// Config-file path.
        path: String,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// The node `config.json` is not a well-formed `NodeConfigStub`.
    /// Mirror of upstream `initConf`'s `readJson` failure path.
    #[error("initialize: {0}")]
    ConfigStub(#[from] crate::orphans::NodeConfigStubParseError),
    /// The Shelley genesis file referenced by the config could not be
    /// loaded. Mirror of upstream `initConf`'s `readFileJson` failure.
    #[error("initialize: cannot load Shelley genesis: {0}")]
    GenesisLoad(#[from] yggdrasil_node_genesis::GenesisLoadError),
}

/// Whether a directory's sub-directory set marks it as a ChainDB.
///
/// Mirror of upstream `checkIsDB ls = Set.fromList ls
/// \`Set.isSubsetOf\` chainDBDirs` — the directory looks like a
/// ChainDB iff every sub-directory is one of the known ChainDB
/// directories. An empty set is trivially a subset (a fresh ChainDB
/// dir before any block is written).
fn looks_like_chain_db(subdirs: &BTreeSet<String>) -> bool {
    let known: BTreeSet<String> = CHAIN_DB_DIRS.iter().map(|s| (*s).to_string()).collect();
    subdirs.is_subset(&known)
}

/// List the immediate sub-directory names of `path`.
///
/// Mirror of upstream `listSubdirectories` (`filterM isDir =<<
/// listDirectory path`).
fn list_subdirectories(path: &Path) -> Result<BTreeSet<String>, std::io::Error> {
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(out)
}

/// Prepare the target ChainDB directory according to `mode`.
///
/// Mirror of upstream `preOpenChainDB`:
///
/// - directory absent → create it (all modes).
/// - directory present + `OpenCreate` → fail [`RunError::AlreadyExists`].
/// - directory present + ChainDB-shaped + `OpenAppend` → leave intact.
/// - directory present + ChainDB-shaped + `OpenCreateForce` → wipe +
///   recreate.
/// - directory present + foreign-shaped → fail [`RunError::NotAChainDb`].
pub fn pre_open_chain_db(mode: DBSynthesizerOpenMode, db: &Path) -> Result<(), RunError> {
    let loc = db.display().to_string();
    let io_err = |source: std::io::Error| RunError::Io {
        path: loc.clone(),
        source,
    };

    if !db.exists() {
        std::fs::create_dir_all(db).map_err(io_err)?;
        return Ok(());
    }

    let subdirs = list_subdirectories(db).map_err(io_err)?;
    let is_chain_db = looks_like_chain_db(&subdirs);

    match mode {
        DBSynthesizerOpenMode::OpenCreate => Err(RunError::AlreadyExists(loc)),
        DBSynthesizerOpenMode::OpenAppend if is_chain_db => Ok(()),
        DBSynthesizerOpenMode::OpenCreateForce if is_chain_db => {
            std::fs::remove_dir_all(db).map_err(io_err)?;
            std::fs::create_dir_all(db).map_err(io_err)?;
            Ok(())
        }
        _ => Err(RunError::NotAChainDb(loc)),
    }
}

/// Outcome of a [`synthesize`] run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SynthesizeOutcome {
    /// The forge-loop outcome (block count + final accumulator).
    pub forge: ForgeRunOutcome,
    /// Slot the forge loop resumed from (0 for a fresh ChainDB,
    /// `tip_slot + 1` when appending).
    pub resumed_from: SlotNo,
}

/// Open the ChainDB and synthesize blocks up to `options.limit`.
///
/// Mirror of upstream `synthesize`: it `preOpenChainDB`s the target,
/// opens the ChainDB, computes the resume slot from the current tip
/// (`Origin → 0`, `At s → succ s`), and runs the forge loop.
///
/// `epoch_size` is the Shelley-genesis `sgEpochLength`. The production
/// path [`synthesize_from_config`] resolves it from the node config;
/// [`synthesize_default`] passes [`DEFAULT_EPOCH_SIZE`].
pub fn synthesize(
    options: DBSynthesizerOptions,
    db_dir: &Path,
    epoch_size: u64,
    era: Era,
) -> Result<SynthesizeOutcome, RunError> {
    pre_open_chain_db(options.open_mode, db_dir)?;

    let mut store = FileImmutable::open(db_dir)?;

    // Mirror of upstream's tip-driven resume:
    //   slotNo = case pointSlot tip of { Origin -> 0; At s -> succ s }
    let resumed_from = match store.get_tip() {
        Point::Origin => SlotNo(0),
        Point::BlockPoint(slot, _) => SlotNo(slot.0 + 1),
    };

    let forge = run_forge(era, epoch_size, resumed_from, options.limit, &mut store)?;
    Ok(SynthesizeOutcome {
        forge,
        resumed_from,
    })
}

/// [`synthesize`] with [`DEFAULT_EPOCH_SIZE`] + [`SYNTH_ERA`].
///
/// A config-free convenience: forges without reading a node
/// `config.json`. The production run-loop dispatch ([`crate::run`])
/// goes through [`synthesize_from_config`], which resolves the real
/// genesis epoch length; `synthesize_default` backs unit tests and
/// callers without a config file.
pub fn synthesize_default(
    options: DBSynthesizerOptions,
    db_dir: &Path,
) -> Result<SynthesizeOutcome, RunError> {
    synthesize(options, db_dir, DEFAULT_EPOCH_SIZE, SYNTH_ERA)
}

/// Resolve the real Shelley-genesis epoch length from the node
/// `config.json`.
///
/// Mirror of the genesis-loading half of upstream
/// `Cardano.Tools.DBSynthesizer.Run.initialize` (`initConf`): read the
/// config file, parse it into a
/// [`NodeConfigStub`](crate::types::NodeConfigStub), resolve the
/// embedded genesis paths relative to the config file's own directory
/// (upstream `relativeToConfig`), load the Shelley genesis, and return
/// its `epochLength`. Upstream `synthesize` then uses
/// `epochSize = sgEpochLength confShelleyGenesis`.
///
/// The protocol-building half (`initProtocol` /
/// `mkConsensusProtocolCardano`) is the db-synthesizer R3 carve-out.
pub fn resolve_epoch_size_from_config(config_path: &Path) -> Result<u64, RunError> {
    let raw = std::fs::read_to_string(config_path).map_err(|source| RunError::ConfigRead {
        path: config_path.display().to_string(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|source| RunError::ConfigParse {
            path: config_path.display().to_string(),
            source,
        })?;
    let stub = parse_node_config_stub(value)?;

    // Upstream `relativeToConfig = (</>) . takeDirectory <$> makeAbsolute
    // nfpConfig`: genesis paths embedded in the config are resolved
    // against the config file's own absolute directory. `PathBuf::join`
    // mirrors Haskell `(</>)` — an absolute genesis path is kept as-is.
    let abs_config = std::path::absolute(config_path).map_err(|source| RunError::ConfigRead {
        path: config_path.display().to_string(),
        source,
    })?;
    let config_dir = abs_config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let stub = stub.adjust_file_paths(|p| config_dir.join(p));

    let genesis = load_shelley_genesis(&stub.shelley_genesis_file)?;
    Ok(genesis.epoch_length)
}

/// [`synthesize`] driven by the operator's node `config.json`.
///
/// The production entry point [`crate::run`] uses: it resolves the
/// real epoch length via [`resolve_epoch_size_from_config`] (the
/// genesis-loading half of upstream `Run.initialize`) and forges with
/// it — mirror of upstream `app/db-synthesizer.hs`'s
/// `initialize … >>= synthesize …` for the epoch-size path.
///
/// The era stays [`SYNTH_ERA`]; the genesis-derived hard-fork era plan
/// needs `initProtocol` and is the db-synthesizer R3 carve-out.
pub fn synthesize_from_config(
    options: DBSynthesizerOptions,
    config_path: &Path,
    db_dir: &Path,
) -> Result<SynthesizeOutcome, RunError> {
    let epoch_size = resolve_epoch_size_from_config(config_path)?;
    synthesize(options, db_dir, epoch_size, SYNTH_ERA)
}

/// Whether the supplied [`ForgeLimit`] would forge zero blocks.
///
/// A `--blocks 0` / `--slots 0` / `--epochs 0` invocation is a no-op
/// — the forge loop's `forging_done` predicate is satisfied
/// immediately. Surfaced for the dispatch layer's reporting.
pub fn is_noop_limit(limit: ForgeLimit) -> bool {
    matches!(
        limit,
        ForgeLimit::Block(0) | ForgeLimit::Slot(SlotNo(0)) | ForgeLimit::Epoch(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit};

    fn opts(limit: ForgeLimit, mode: DBSynthesizerOpenMode) -> DBSynthesizerOptions {
        DBSynthesizerOptions {
            limit,
            open_mode: mode,
        }
    }

    #[test]
    fn looks_like_chain_db_accepts_empty() {
        assert!(looks_like_chain_db(&BTreeSet::new()));
    }

    #[test]
    fn looks_like_chain_db_accepts_known_subset() {
        let mut s = BTreeSet::new();
        s.insert("immutable".to_string());
        s.insert("ledger".to_string());
        assert!(looks_like_chain_db(&s));
    }

    #[test]
    fn looks_like_chain_db_rejects_foreign_dir() {
        let mut s = BTreeSet::new();
        s.insert("immutable".to_string());
        s.insert("my-photos".to_string());
        assert!(!looks_like_chain_db(&s));
    }

    #[test]
    fn pre_open_creates_absent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("fresh-db");
        assert!(!target.exists());
        pre_open_chain_db(DBSynthesizerOpenMode::OpenCreate, &target).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn pre_open_create_rejects_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err =
            pre_open_chain_db(DBSynthesizerOpenMode::OpenCreate, tmp.path()).expect_err("rejects");
        assert!(matches!(err, RunError::AlreadyExists(_)));
    }

    #[test]
    fn pre_open_append_accepts_chain_db_shaped_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("immutable")).unwrap();
        std::fs::create_dir(tmp.path().join("ledger")).unwrap();
        pre_open_chain_db(DBSynthesizerOpenMode::OpenAppend, tmp.path()).unwrap();
        // Untouched.
        assert!(tmp.path().join("immutable").is_dir());
    }

    #[test]
    fn pre_open_append_rejects_foreign_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("not-a-chaindb")).unwrap();
        let err =
            pre_open_chain_db(DBSynthesizerOpenMode::OpenAppend, tmp.path()).expect_err("rejects");
        assert!(matches!(err, RunError::NotAChainDb(_)));
    }

    #[test]
    fn pre_open_force_wipes_chain_db_shaped_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("immutable")).unwrap();
        std::fs::write(tmp.path().join("immutable").join("marker"), b"x").unwrap();
        pre_open_chain_db(DBSynthesizerOpenMode::OpenCreateForce, tmp.path()).unwrap();
        // Recreated empty.
        assert!(tmp.path().is_dir());
        assert!(!tmp.path().join("immutable").join("marker").exists());
    }

    #[test]
    fn pre_open_force_rejects_foreign_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("photos")).unwrap();
        let err = pre_open_chain_db(DBSynthesizerOpenMode::OpenCreateForce, tmp.path())
            .expect_err("rejects");
        assert!(matches!(err, RunError::NotAChainDb(_)));
    }

    #[test]
    fn synthesize_default_creates_chain_db_with_n_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("synth-db");
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(6), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 6);
        assert_eq!(outcome.resumed_from, SlotNo(0));

        // Verification: reopen the ChainDB from disk and count.
        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 6);
    }

    #[test]
    fn synthesize_append_resumes_from_tip() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("append-db");
        // First pass: create with 3 blocks.
        synthesize_default(
            opts(ForgeLimit::Block(3), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        // Second pass: append 4 more.
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(4), DBSynthesizerOpenMode::OpenAppend),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 4);
        // Resume slot is tip_slot + 1; first pass forged slots 0..2.
        assert_eq!(outcome.resumed_from, SlotNo(3));

        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 7);
    }

    #[test]
    fn synthesize_force_overwrites_existing_chain_db() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("force-db");
        synthesize_default(
            opts(ForgeLimit::Block(10), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        // Force-recreate with a smaller chain.
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(2), DBSynthesizerOpenMode::OpenCreateForce),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 2);
        assert_eq!(outcome.resumed_from, SlotNo(0));

        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn is_noop_limit_detects_zero_limits() {
        assert!(is_noop_limit(ForgeLimit::Block(0)));
        assert!(is_noop_limit(ForgeLimit::Slot(SlotNo(0))));
        assert!(is_noop_limit(ForgeLimit::Epoch(0)));
        assert!(!is_noop_limit(ForgeLimit::Block(1)));
    }

    #[test]
    fn synthesize_zero_blocks_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("empty-db");
        let outcome = synthesize_default(
            opts(ForgeLimit::Block(0), DBSynthesizerOpenMode::OpenCreate),
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 0);
        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 0);
    }

    /// Write a `config.json` + Shelley genesis pair into `dir`.
    /// `shelley_rel` is the (possibly nested) `ShelleyGenesisFile` path
    /// recorded in the config — relative paths exercise the
    /// config-directory resolution. Returns the config path.
    fn write_config(dir: &Path, shelley_rel: &str, epoch_length: u64) -> std::path::PathBuf {
        let genesis = dir.join(shelley_rel);
        if let Some(parent) = genesis.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&genesis, format!(r#"{{"epochLength":{epoch_length}}}"#)).unwrap();
        let config = dir.join("config.json");
        let config_json = format!(
            r#"{{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"{shelley_rel}","AlonzoGenesisFile":"alonzo.json","ConwayGenesisFile":"conway.json"}}"#
        );
        std::fs::write(&config, config_json).unwrap();
        config
    }

    #[test]
    fn resolve_epoch_size_reads_non_default_epoch_length() {
        let tmp = tempfile::tempdir().unwrap();
        // 86_400 (the preview epoch length) is deliberately != the
        // 432_000 DEFAULT_EPOCH_SIZE, so a stubbed read would be caught.
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);
        let epoch = resolve_epoch_size_from_config(&config).unwrap();
        assert_eq!(epoch, 86_400);
        assert_ne!(epoch, DEFAULT_EPOCH_SIZE);
    }

    #[test]
    fn resolve_epoch_size_resolves_genesis_path_relative_to_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // ShelleyGenesisFile sits in a sub-directory of the config dir;
        // upstream `relativeToConfig` resolves it there.
        let config = write_config(tmp.path(), "genesis/shelley.json", 21_600);
        assert_eq!(resolve_epoch_size_from_config(&config).unwrap(), 21_600);
    }

    #[test]
    fn resolve_epoch_size_errors_on_missing_config() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.json");
        let err = resolve_epoch_size_from_config(&missing).expect_err("rejects");
        assert!(matches!(err, RunError::ConfigRead { .. }));
    }

    #[test]
    fn resolve_epoch_size_errors_on_missing_genesis() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(
            &config,
            r#"{"Protocol":"Cardano","ByronGenesisFile":"byron.json","ShelleyGenesisFile":"absent-shelley.json","AlonzoGenesisFile":"alonzo.json","ConwayGenesisFile":"conway.json"}"#,
        )
        .unwrap();
        let err = resolve_epoch_size_from_config(&config).expect_err("rejects");
        assert!(matches!(err, RunError::GenesisLoad(_)));
    }

    #[test]
    fn resolve_epoch_size_errors_on_non_cardano_protocol() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(&config, r#"{"Protocol":"Byron"}"#).unwrap();
        let err = resolve_epoch_size_from_config(&config).expect_err("rejects");
        assert!(matches!(err, RunError::ConfigStub(_)));
    }

    #[test]
    fn synthesize_from_config_creates_chain_db() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), "shelley-genesis.json", 86_400);
        let target = tmp.path().join("synth-db");
        let outcome = synthesize_from_config(
            opts(ForgeLimit::Block(4), DBSynthesizerOpenMode::OpenCreate),
            &config,
            &target,
        )
        .unwrap();
        assert_eq!(outcome.forge.result.forged, 4);
        let reopened = FileImmutable::open(&target).unwrap();
        assert_eq!(reopened.len(), 4);
    }
}
