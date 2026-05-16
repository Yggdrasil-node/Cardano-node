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
//!
//! ## Carve-outs (NOT ported this round — Phase 4 R2/R3 slice boundary)
//!
//! - **`initialize`** — upstream reads `config.json`, parses the
//!   `NodeConfigStub`, loads + validates `ShelleyGenesis`, and builds
//!   a `CardanoProtocolParams` via `mkConsensusProtocolCardano`. That
//!   is the genesis-loading round (db-synthesizer R2). This slice
//!   instead stubs the epoch length to [`STUB_EPOCH_SIZE`] and the
//!   synthesis era to [`SYNTH_ERA`]; `orphans::parse_node_config_stub`
//!   already exists and will be wired here in R2.
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
use yggdrasil_storage::{FileImmutable, ImmutableStore, StorageError};

use crate::forging::{ForgeRunOutcome, run_forge};
use crate::types::{DBSynthesizerOpenMode, DBSynthesizerOptions, ForgeLimit};

/// Stubbed epoch length used until genesis loading lands (R2).
///
/// Mirror of upstream's `sgEpochLength confShelleyGenesis`. The
/// mainnet Shelley genesis ships `epochLength = 432000`; this slice
/// uses that value verbatim as a placeholder so the
/// [`ForgeLimit::Epoch`] arithmetic is exercised against a realistic
/// constant. It is replaced by the parsed genesis value in R2.
pub const STUB_EPOCH_SIZE: u64 = 432_000;

/// Era tag stamped onto synthesized structural blocks until the
/// genesis-derived hard-fork era plan is wired (R2).
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
/// `epoch_size` is the Shelley-genesis epoch length — stubbed to
/// [`STUB_EPOCH_SIZE`] by [`synthesize_default`] until genesis loading
/// lands (R2).
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

/// [`synthesize`] with the stubbed epoch size + era.
///
/// This is the entry point the run-loop dispatch ([`crate::run`])
/// uses until genesis loading lands. It is split out so R2 can wire
/// the genesis-derived `epoch_size`/`era` through [`synthesize`]
/// without re-touching the dispatch.
pub fn synthesize_default(
    options: DBSynthesizerOptions,
    db_dir: &Path,
) -> Result<SynthesizeOutcome, RunError> {
    synthesize(options, db_dir, STUB_EPOCH_SIZE, SYNTH_ERA)
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
}
