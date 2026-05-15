//! `status` subcommand: inspect on-disk Yggdrasil storage and report
//! sync position, block counts, ledger checkpoint state, and the
//! recovered ledger-state cardinalities for the latest tip.
//!
//! Mirrors upstream `cardano-cli node-status` (and the older
//! `cardano-cli query tip --output json` extended view). The Yggdrasil
//! variant is purely a read-only on-disk inspector — it does not
//! contact the running node — so it works on a stopped node and is
//! safe to run during a sync.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Run.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `status` subcommand —
//! introspection of the on-disk storage and reporting of sync
//! position, block counts, checkpoint state, ledger counts.
//! Yggdrasil-specific operator UX; upstream cardano-cli does
//! not have an equivalent because the `cardano-cli query tip`
//! / `query ledger-state` flow covers the same information
//! via NtC LSQ. Yggdrasil's `status` reads the on-disk DB
//! directly without requiring a running node.

use std::path::PathBuf;

use eyre::{Result, WrapErr};
use serde::Serialize;

use yggdrasil_node_config::NetworkPreset;

use crate::commands::configuration::{apply_topology_override, load_effective_config};

use yggdrasil_ledger::Point;
use yggdrasil_node::recover_ledger_state_chaindb_epoch_boundary;
use yggdrasil_node_config::NodeConfigFile;
use yggdrasil_storage::{
    ChainDb, FileImmutable, FileLedgerStore, FileVolatile, ImmutableStore, LedgerStore,
    VolatileStore,
};

/// Ledger-state cardinality summary mirroring LSQ tag 23
/// `GetLedgerCounts`.  Exposed inside [`StatusReport`] when the node has
/// successfully recovered the latest ledger state from storage.
#[derive(Debug, Serialize)]
pub struct LedgerCountsReport {
    pub stake_credentials: usize,
    pub pools: usize,
    pub dreps: usize,
    pub committee_members: usize,
    pub governance_actions: usize,
    pub gen_delegs: usize,
}

/// On-disk node status report produced by the `status` subcommand.
#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub network_magic: u32,
    pub storage_dir: String,
    pub storage_initialized: bool,
    pub chain_tip: String,
    pub chain_tip_slot: Option<u64>,
    pub chain_tip_hash: Option<String>,
    pub immutable_tip: String,
    pub immutable_block_count: usize,
    pub volatile_tip: String,
    pub volatile_block_count: usize,
    pub ledger_checkpoint_slot: Option<u64>,
    pub ledger_checkpoint_count: usize,
    pub replayed_volatile_blocks: Option<usize>,
    pub recovered_ledger_point: Option<String>,
    /// Era of the recovered ledger state (`Byron`, `Shelley`, …, `Conway`).
    /// `None` when storage is uninitialized or recovery fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_era: Option<String>,
    /// Current epoch number at the recovered ledger tip.
    /// `None` when storage is uninitialized or recovery fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_epoch: Option<u64>,
    /// Aggregate ledger-state cardinalities at the recovered tip.
    /// `None` when storage replay failed or no ledger state was recovered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_counts: Option<LedgerCountsReport>,
}

/// Drive the `status` subcommand: load the effective config, apply
/// per-flag overrides, and print the JSON status report.
pub(crate) fn run_status_subcommand(
    config: Option<PathBuf>,
    network: Option<NetworkPreset>,
    topology: Option<PathBuf>,
    database_path: Option<PathBuf>,
) -> Result<()> {
    let (mut file_cfg, config_base_dir) = load_effective_config(config, network)?;
    apply_topology_override(
        &mut file_cfg,
        topology.as_deref(),
        config_base_dir.as_deref(),
    )?;
    if let Some(ref db_path) = database_path {
        file_cfg.storage_dir = db_path.clone();
    }
    let report = status_report(&file_cfg, config_base_dir.as_deref())?;
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

pub fn status_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<StatusReport> {
    let storage_dir = crate::resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    if !(immutable_dir.exists() || volatile_dir.exists() || ledger_dir.exists()) {
        return Ok(StatusReport {
            network_magic: file_cfg.network_magic,
            storage_dir: storage_dir.display().to_string(),
            storage_initialized: false,
            chain_tip: format!("{:?}", Point::Origin),
            chain_tip_slot: None,
            chain_tip_hash: None,
            immutable_tip: format!("{:?}", Point::Origin),
            immutable_block_count: 0,
            volatile_tip: format!("{:?}", Point::Origin),
            volatile_block_count: 0,
            ledger_checkpoint_slot: None,
            ledger_checkpoint_count: 0,
            replayed_volatile_blocks: None,
            recovered_ledger_point: None,
            current_era: None,
            current_epoch: None,
            ledger_counts: None,
        });
    }

    let chain_db = ChainDb::new(
        FileImmutable::open_read_only(immutable_dir).wrap_err("failed to open immutable store")?,
        FileVolatile::open_read_only(volatile_dir).wrap_err("failed to open volatile store")?,
        FileLedgerStore::open_read_only(ledger_dir).wrap_err("failed to open ledger store")?,
    );

    let chain_tip = chain_db.tip();
    let immutable_tip = chain_db.immutable().get_tip();
    let volatile_tip = chain_db.volatile().tip();
    let immutable_block_count = chain_db.immutable().len();

    // Count volatile blocks by walking the prefix up to the volatile tip.
    let volatile_block_count: usize = if volatile_tip != Point::Origin {
        chain_db
            .volatile()
            .prefix_up_to(&volatile_tip)
            .map(|blocks| blocks.len())
            .unwrap_or(0)
    } else {
        0
    };

    let ledger_checkpoint_count = LedgerStore::count(chain_db.ledger());
    let recovery = recover_ledger_state_chaindb_epoch_boundary(
        &chain_db,
        crate::best_effort_base_ledger_state(file_cfg, config_base_dir),
        file_cfg.epoch_schedule(),
        None,
    );

    let (chain_tip_slot, chain_tip_hash) = match &chain_tip {
        Point::Origin => (None, None),
        Point::BlockPoint(slot, hash) => (Some(slot.0), Some(format!("{hash:?}"))),
    };

    // Derive ledger-state cardinalities from the recovered state when
    // available.  Matches the LSQ tag 23 `GetLedgerCounts` breakdown so
    // the two surfaces report the same numbers.
    let ledger_counts = recovery.as_ref().ok().map(|r| {
        let state = &r.ledger_state;
        LedgerCountsReport {
            stake_credentials: state.stake_credentials().len(),
            pools: state.pool_state().len(),
            dreps: state.drep_state().len(),
            committee_members: state.committee_state().len(),
            governance_actions: state.governance_actions().len(),
            gen_delegs: state.gen_delegs().len(),
        }
    });
    let current_era = recovery
        .as_ref()
        .ok()
        .map(|r| format!("{:?}", r.ledger_state.current_era()));
    let current_epoch = recovery
        .as_ref()
        .ok()
        .map(|r| r.ledger_state.current_epoch().0);

    Ok(StatusReport {
        network_magic: file_cfg.network_magic,
        storage_dir: storage_dir.display().to_string(),
        storage_initialized: true,
        chain_tip: format!("{chain_tip:?}"),
        chain_tip_slot,
        chain_tip_hash,
        immutable_tip: format!("{immutable_tip:?}"),
        immutable_block_count,
        volatile_tip: format!("{volatile_tip:?}"),
        volatile_block_count,
        ledger_checkpoint_slot: recovery
            .as_ref()
            .ok()
            .and_then(|r| r.checkpoint_slot.map(|s| s.0)),
        ledger_checkpoint_count,
        replayed_volatile_blocks: recovery.as_ref().ok().map(|r| r.replayed_volatile_blocks),
        recovered_ledger_point: recovery.as_ref().ok().map(|r| format!("{:?}", r.point)),
        current_era,
        current_epoch,
        ledger_counts,
    })
}
