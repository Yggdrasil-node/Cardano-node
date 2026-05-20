//! `validate-config` subcommand: deep operator-side preflight check
//! for a node config + topology + peer-snapshot bundle.
//!
//! Mirrors the upstream `Cardano.Node.Configuration.POM` validation
//! pipeline (cardano-node performs its own preflight as part of
//! `nodeProtocolModeP` on startup; the dedicated `cardano-cli`
//! preflight surface is a Yggdrasil convenience).
//!
//! Walks the loaded `NodeConfigFile`, runs each cross-field invariant
//! (security parameter, KES, governor targets, RequiresNetworkMagic,
//! checkpoints integrity, peer snapshot, on-disk storage recovery)
//! and produces a structured JSON report. Warning-only fields surface
//! as entries in `warnings`; hard misconfigurations bail.
//!
//! Pure `NodeConfigFile` checks, the operator-side **role** report
//! (`node_role_report`), and the **block-producer credential** policy
//! are owned by `yggdrasil-node-config` so `run`, `validate-config`,
//! and sibling tools share one interpretation of upstream config fields.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/POM.hs>
//!  +         <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/Logging.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `validate-config` runner. Wires the binary's `validate-config` subcommand entry point to the actual validation logic in sibling `commands/configuration.rs`. No upstream parallel.

use std::path::PathBuf;

use eyre::{Result, WrapErr};
use serde::Serialize;

use crate::commands::configuration::{
    apply_block_producer_credential_overrides, apply_inbound_listen_overrides,
    apply_topology_override, load_effective_config,
};

use yggdrasil_ledger::Point;
use yggdrasil_network::{LedgerPeerSnapshot, LedgerStateJudgement};
use yggdrasil_node::recover_ledger_state_chaindb_epoch_boundary;
use yggdrasil_node_config::{
    BlockProducerCredentialStatus, NetworkPreset, NodeConfigFile, NodeRoleValidationReport,
    ensure_block_producer_credential_policy, load_peer_snapshot_file, node_config_preflight_report,
};
use yggdrasil_node_tracer::NodeTracer;
use yggdrasil_storage::{ChainDb, FileImmutable, FileLedgerStore, FileVolatile};

#[derive(Debug, Serialize)]
pub struct ConfigValidationReport {
    pub primary_peer: String,
    pub network_magic: u32,
    pub protocol_versions: Vec<u32>,
    pub storage_dir: String,
    pub node_role: NodeRoleValidationReport,
    pub configured_fallback_peer_count: usize,
    pub resolved_startup_peer_count: usize,
    pub use_ledger_peers: String,
    pub checkpoint_interval_slots: u64,
    pub max_ledger_snapshots: usize,
    pub peer_snapshot: PeerSnapshotValidationReport,
    pub storage: StorageValidationReport,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PeerSnapshotValidationReport {
    pub status: &'static str,
    pub path: Option<String>,
    pub slot: Option<u64>,
    pub ledger_peer_count: usize,
    pub big_ledger_peer_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StorageValidationReport {
    pub status: &'static str,
    pub tip: String,
    pub recovered_point: Option<String>,
    pub checkpoint_slot: Option<u64>,
    pub replayed_volatile_blocks: Option<usize>,
    pub ledger_peer_count: usize,
}

#[cfg(feature = "forge")]
pub fn load_configured_block_producer_credentials(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
    non_producing_node: bool,
) -> Result<Option<yggdrasil_node_block_producer::BlockProducerCredentials>> {
    let status = ensure_block_producer_credential_policy(file_cfg, non_producing_node)?;
    if non_producing_node || status == BlockProducerCredentialStatus::Absent {
        return Ok(None);
    }

    let creds = yggdrasil_node_block_producer::load_block_producer_credentials(
        &crate::resolve_config_path(
            std::path::Path::new(
                file_cfg
                    .shelley_kes_key
                    .as_ref()
                    .expect("complete block producer credentials include ShelleyKesKey"),
            ),
            config_base_dir,
        ),
        &crate::resolve_config_path(
            std::path::Path::new(
                file_cfg
                    .shelley_vrf_key
                    .as_ref()
                    .expect("complete block producer credentials include ShelleyVrfKey"),
            ),
            config_base_dir,
        ),
        &crate::resolve_config_path(
            std::path::Path::new(file_cfg.shelley_operational_certificate.as_ref().expect(
                "complete block producer credentials include ShelleyOperationalCertificate",
            )),
            config_base_dir,
        ),
        file_cfg.slots_per_kes_period,
        file_cfg.max_kes_evolutions,
    )
    .wrap_err("failed to load block producer credentials")?;

    Ok(Some(creds))
}

/// Drive the `validate-config` subcommand: load effective config,
/// apply CLI overrides, generate the JSON report, and print it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_validate_config_subcommand(
    config: Option<PathBuf>,
    network: Option<NetworkPreset>,
    topology: Option<PathBuf>,
    database_path: Option<PathBuf>,
    port: Option<u16>,
    host_addr: Option<String>,
    non_producing_node: bool,
    shelley_kes_key: Option<PathBuf>,
    shelley_vrf_key: Option<PathBuf>,
    shelley_operational_certificate: Option<PathBuf>,
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
    apply_inbound_listen_overrides(&mut file_cfg, port, host_addr)?;
    apply_block_producer_credential_overrides(
        &mut file_cfg,
        shelley_kes_key.as_ref(),
        shelley_vrf_key.as_ref(),
        shelley_operational_certificate.as_ref(),
    );
    let report = if non_producing_node {
        validate_config_report_with_role(&file_cfg, config_base_dir.as_deref(), non_producing_node)?
    } else {
        validate_config_report(&file_cfg, config_base_dir.as_deref())?
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

pub fn validate_config_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<ConfigValidationReport> {
    validate_config_report_with_role(file_cfg, config_base_dir, false)
}

pub fn validate_config_report_with_role(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
    non_producing_node: bool,
) -> Result<ConfigValidationReport> {
    let storage_dir = crate::resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    let preflight = node_config_preflight_report(file_cfg, config_base_dir, non_producing_node)?;
    let node_role = preflight.node_role;
    let mut warnings = preflight.warnings;
    #[cfg(feature = "forge")]
    if node_role.block_producer_credentials == "complete" {
        load_configured_block_producer_credentials(file_cfg, config_base_dir, non_producing_node)?;
    }
    #[cfg(not(feature = "forge"))]
    if node_role.block_producer_credentials == "complete" {
        warnings.push(
            "block producer credential paths are configured but this binary was built without \
             the `forge` feature; credentials will be ignored and the forge loop is unavailable"
                .to_owned(),
        );
    }

    let peer_snapshot = if let Some(peer_snapshot_file) = file_cfg.peer_snapshot_file.as_deref() {
        let peer_snapshot_path =
            crate::resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
        match load_peer_snapshot_file(&peer_snapshot_path) {
            Ok(loaded) => PeerSnapshotValidationReport {
                status: "loaded",
                path: Some(peer_snapshot_path.display().to_string()),
                slot: loaded.slot,
                ledger_peer_count: loaded.snapshot.ledger_peers.len(),
                big_ledger_peer_count: loaded.snapshot.big_ledger_peers.len(),
                error: None,
            },
            Err(err) => {
                warnings.push(format!(
                    "configured peer snapshot file could not be loaded: {}",
                    err
                ));
                PeerSnapshotValidationReport {
                    status: "unavailable",
                    path: Some(peer_snapshot_path.display().to_string()),
                    slot: None,
                    ledger_peer_count: 0,
                    big_ledger_peer_count: 0,
                    error: Some(err.to_string()),
                }
            }
        }
    } else {
        PeerSnapshotValidationReport {
            status: "disabled",
            path: None,
            slot: None,
            ledger_peer_count: 0,
            big_ledger_peer_count: 0,
            error: None,
        }
    };

    let (storage, latest_slot, ledger_state_judgement, ledger_snapshot) = if immutable_dir.exists()
        || volatile_dir.exists()
        || ledger_dir.exists()
    {
        let base_ledger_state = crate::best_effort_base_ledger_state(file_cfg, config_base_dir);
        let chain_db = ChainDb::new(
            FileImmutable::open_read_only(&immutable_dir).wrap_err_with(|| {
                format!("failed to open immutable store {}", immutable_dir.display())
            })?,
            FileVolatile::open_read_only(&volatile_dir).wrap_err_with(|| {
                format!("failed to open volatile store {}", volatile_dir.display())
            })?,
            FileLedgerStore::open_read_only(&ledger_dir).wrap_err_with(|| {
                format!("failed to open ledger store {}", ledger_dir.display())
            })?,
        );
        let tip = chain_db.recovery().tip;
        let recovery = recover_ledger_state_chaindb_epoch_boundary(
            &chain_db,
            base_ledger_state,
            file_cfg.epoch_schedule(),
            None,
        )
        .wrap_err_with(|| {
            format!(
                "failed to recover ledger state from storage directory {}",
                storage_dir.display()
            )
        })?;
        let latest_slot = crate::point_slot(&recovery.point).or_else(|| crate::point_slot(&tip));
        let ledger_snapshot = crate::ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state);
        (
            StorageValidationReport {
                status: "initialized",
                tip: format!("{:?}", tip),
                recovered_point: Some(format!("{:?}", recovery.point)),
                checkpoint_slot: recovery.checkpoint_slot.map(|slot| slot.0),
                replayed_volatile_blocks: Some(recovery.replayed_volatile_blocks),
                ledger_peer_count: ledger_snapshot.ledger_peers.len(),
            },
            latest_slot,
            LedgerStateJudgement::YoungEnough,
            ledger_snapshot,
        )
    } else {
        warnings.push(
            "storage directories are not initialized; a deployment preflight cannot validate restart recovery yet"
                .to_owned(),
        );
        (
            StorageValidationReport {
                status: "not-initialized",
                tip: format!("{:?}", Point::Origin),
                recovered_point: None,
                checkpoint_slot: None,
                replayed_volatile_blocks: None,
                ledger_peer_count: 0,
            },
            None,
            LedgerStateJudgement::Unavailable,
            LedgerPeerSnapshot::default(),
        )
    };

    let fallback_peers = crate::configured_fallback_peers(
        file_cfg,
        config_base_dir,
        &ledger_snapshot,
        latest_slot,
        ledger_state_judgement,
        &NodeTracer::disabled(),
    );

    Ok(ConfigValidationReport {
        primary_peer: file_cfg.peer_addr.to_string(),
        network_magic: file_cfg.network_magic,
        protocol_versions: file_cfg.protocol_versions.clone(),
        storage_dir: storage_dir.display().to_string(),
        node_role,
        configured_fallback_peer_count: file_cfg.ordered_fallback_peers().len(),
        resolved_startup_peer_count: 1 + fallback_peers.len(),
        use_ledger_peers: format!("{:?}", file_cfg.use_ledger_peers_policy()),
        checkpoint_interval_slots: file_cfg.checkpoint_interval_slots,
        max_ledger_snapshots: file_cfg.max_ledger_snapshots,
        peer_snapshot,
        storage,
        warnings,
    })
}
