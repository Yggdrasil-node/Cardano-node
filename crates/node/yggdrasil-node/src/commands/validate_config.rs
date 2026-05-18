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
//! The same module also implements the operator-side **role** report
//! (`node_role_report`) and the **block-producer credential** policy
//! enforcement consumed by the `run` subcommand at startup so a
//! mis-configured forge configuration cannot silently boot as a
//! relay-only node.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/POM.hs>
//!  +         <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/Logging.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `validate-config` runner. Wires the binary's `validate-config` subcommand entry point to the actual validation logic in sibling `commands/configuration.rs`. No upstream parallel.

use std::path::PathBuf;

use eyre::{Result, WrapErr, bail};
use serde::Serialize;

use yggdrasil_node_config::NetworkPreset;

use crate::commands::configuration::{
    apply_block_producer_credential_overrides, apply_inbound_listen_overrides,
    apply_topology_override, load_effective_config,
};

use yggdrasil_ledger::Point;
use yggdrasil_network::{GovernorTargets, LedgerPeerSnapshot, LedgerStateJudgement};
use yggdrasil_node::recover_ledger_state_chaindb_epoch_boundary;
use yggdrasil_node_config::{NodeConfigFile, load_peer_snapshot_file};
use yggdrasil_node_genesis as genesis;
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

#[derive(Clone, Debug, Serialize)]
pub struct NodeRoleValidationReport {
    pub role: &'static str,
    pub non_producing_node: bool,
    pub inbound_listen_addr: Option<String>,
    pub block_producer_credentials: &'static str,
    pub credential_fields_present: Vec<&'static str>,
    pub credential_fields_missing: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockProducerCredentialStatus {
    Absent,
    Complete,
    Partial,
}

fn block_producer_credential_fields(
    file_cfg: &NodeConfigFile,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let fields = [
        ("ShelleyKesKey", file_cfg.shelley_kes_key.is_some()),
        ("ShelleyVrfKey", file_cfg.shelley_vrf_key.is_some()),
        (
            "ShelleyOperationalCertificate",
            file_cfg.shelley_operational_certificate.is_some(),
        ),
    ];

    let mut present = Vec::new();
    let mut missing = Vec::new();
    for (field, is_present) in fields {
        if is_present {
            present.push(field);
        } else {
            missing.push(field);
        }
    }
    (present, missing)
}

fn block_producer_credential_status(file_cfg: &NodeConfigFile) -> BlockProducerCredentialStatus {
    let (present, missing) = block_producer_credential_fields(file_cfg);
    match (present.is_empty(), missing.is_empty()) {
        (true, false) => BlockProducerCredentialStatus::Absent,
        (false, true) => BlockProducerCredentialStatus::Complete,
        _ => BlockProducerCredentialStatus::Partial,
    }
}

fn ensure_block_producer_credential_policy(
    file_cfg: &NodeConfigFile,
    non_producing_node: bool,
) -> Result<BlockProducerCredentialStatus> {
    let status = block_producer_credential_status(file_cfg);
    if status == BlockProducerCredentialStatus::Partial && !non_producing_node {
        let (present, missing) = block_producer_credential_fields(file_cfg);
        bail!(
            "block producer credentials are partially configured; present: {}; missing: {}. \
             Provide all three ShelleyKesKey, ShelleyVrfKey, ShelleyOperationalCertificate, \
             or pass --non-producing-node to run explicitly as a relay/non-producing node",
            present.join(", "),
            missing.join(", "),
        );
    }
    Ok(status)
}

pub fn node_role_report(
    file_cfg: &NodeConfigFile,
    non_producing_node: bool,
) -> Result<NodeRoleValidationReport> {
    let status = ensure_block_producer_credential_policy(file_cfg, non_producing_node)?;
    let (present, missing) = block_producer_credential_fields(file_cfg);
    let inbound_listen_addr = file_cfg.inbound_listen_addr.map(|addr| addr.to_string());
    let role = if status == BlockProducerCredentialStatus::Complete && !non_producing_node {
        "block-producer"
    } else if inbound_listen_addr.is_some() {
        "relay"
    } else if non_producing_node {
        "non-producing"
    } else {
        "sync-only"
    };
    let block_producer_credentials = match (non_producing_node, status) {
        (true, BlockProducerCredentialStatus::Absent) => "absent",
        (true, _) => "ignored-by-non-producing-node",
        (false, BlockProducerCredentialStatus::Absent) => "absent",
        (false, BlockProducerCredentialStatus::Complete) => "complete",
        (false, BlockProducerCredentialStatus::Partial) => "partial",
    };

    Ok(NodeRoleValidationReport {
        role,
        non_producing_node,
        inbound_listen_addr,
        block_producer_credentials,
        credential_fields_present: present,
        credential_fields_missing: missing,
    })
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
    if file_cfg.protocol_versions.is_empty() {
        bail!("node config must include at least one protocol version");
    }

    if file_cfg.security_param_k == 0 {
        bail!(
            "security_param_k (Ouroboros k) must be > 0; a zero value \
             collapses the stability window and makes Praos non-functional"
        );
    }

    if file_cfg.epoch_length == 0 {
        bail!(
            "epoch_length must be > 0; a zero value causes a divide-by-zero \
             in slot-to-epoch conversion"
        );
    }

    if file_cfg.byron_to_shelley_slot.is_some() && file_cfg.byron_epoch_length == 0 {
        bail!(
            "byron_epoch_length must be > 0 when byron_to_shelley_slot is set; \
             the Byron prefix is otherwise ill-formed"
        );
    }

    if file_cfg.slots_per_kes_period == 0 {
        bail!(
            "slots_per_kes_period must be > 0; a zero period makes KES \
             evolution math ill-defined and blocks header verification"
        );
    }

    if file_cfg.max_kes_evolutions == 0 {
        bail!(
            "max_kes_evolutions must be > 0; a zero cap means every KES \
             period is immediately expired and all operational certificates \
             are rejected"
        );
    }

    if !(file_cfg.active_slot_coeff.is_finite()
        && file_cfg.active_slot_coeff > 0.0
        && file_cfg.active_slot_coeff <= 1.0)
    {
        bail!(
            "active_slot_coeff must be finite and within (0, 1], got {}",
            file_cfg.active_slot_coeff
        );
    }

    let storage_dir = crate::resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    let mut warnings = Vec::new();
    let node_role = node_role_report(file_cfg, non_producing_node)?;
    if node_role.block_producer_credentials == "ignored-by-non-producing-node" {
        warnings.push(
            "block producer credential paths are configured but --non-producing-node is set; \
             credentials will be ignored and the forge loop will stay disabled"
                .to_owned(),
        );
    }
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

    // Surface genesis-hash mismatches in the preflight report (without
    // bailing) so an operator running `validate-config` sees the
    // corruption flag alongside any other warnings rather than seeing
    // only the first error. The actual `run` path bails on mismatch via
    // `strict_base_ledger_state` so a misconfigured node cannot start.
    if let Err(err) = file_cfg.verify_known_genesis_hashes(config_base_dir) {
        warnings.push(format!("genesis hash verification: {err}"));
    }

    // `verify_known_genesis_hashes` validates Byron content when both the
    // path and hash are present. If an operator supplied only
    // `ByronGenesisHash`, add a more specific format warning for malformed
    // hex alongside the paired-file warning above.
    if file_cfg.byron_genesis_file.is_none()
        && let Some(byron_hex) = file_cfg.byron_genesis_hash.as_deref()
    {
        if let Err(err) = genesis::parse_blake2b_256_hex(byron_hex, "ByronGenesisHash") {
            warnings.push(format!("ByronGenesisHash format: {err}"));
        }
    }

    // Upstream `Protocol` selects which block-producer family the node
    // runs (`Cardano`, `Shelley`, `Byron`, `RealPBFT`). Yggdrasil only
    // implements `Cardano` — the field docstring makes the value
    // documentation-only — so any other declared value is a
    // misconfiguration that would silently be ignored. Warn with the
    // exact offending value so a typo ("Cadrano") or a copy-pasted
    // legacy config ("RealPBFT") surfaces at preflight time.
    //
    // Reference: `Protocol` in `Cardano.Node.Configuration.POM.nodeProtocolModeP`.
    if let Some(proto) = file_cfg.protocol.as_deref() {
        if proto != "Cardano" {
            warnings.push(format!(
                "Protocol = {proto:?} is not supported; Yggdrasil only implements \
                 \"Cardano\". The value would be silently ignored at runtime — fix the \
                 config to \"Cardano\" or upgrade to a node that implements the declared \
                 protocol family"
            ));
        }
    }

    // Upstream NtN handshake `peerSharing` field is a `Word8` with exactly
    // two defined wire values: 0 (disabled) and 1 (enabled). Values >= 2
    // are undefined and will be silently normalized to "enabled" by the
    // receiver (`NodePeerSharing::from_wire` uses `value >= 1`), but
    // transmitting an undefined value is a misconfiguration on our side
    // that other peers implementing strict wire-value checks may reject.
    // Warn so an operator who meant `0` or `1` spots a typo like `2`.
    //
    // Reference: `Ouroboros.Network.PeerSharing` — `peerSharing` codec in
    // `NodeToNodeVersionData`.
    if file_cfg.peer_sharing > 1 {
        warnings.push(format!(
            "peer_sharing = {} is outside the upstream-defined wire range {{0, 1}}; \
             peers implementing strict codecs may reject this handshake. \
             Use 0 (disabled) or 1 (enabled)",
            file_cfg.peer_sharing,
        ));
    }

    // Upstream `MinNodeVersion` (e.g. `"10.6.2"`) is a dotted-numeric
    // cardano-node version string. We do NOT cross-compare it against our
    // own `CARGO_PKG_VERSION` because the two namespaces are independent
    // (yggdrasil's version is not a cardano-node version even under
    // 100%-parity goals). But we can still format-sanity the declared
    // string: a typo like `"10,6.2"` or `"ten.six.two"` is always a bug,
    // and surfacing it at preflight time avoids the silent "carried but
    // ignored" pitfall the field doc already warns about.
    if let Some(mnv) = file_cfg.min_node_version.as_deref() {
        let trimmed = mnv.trim();
        let valid = !trimmed.is_empty()
            && trimmed
                .split('.')
                .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()));
        if !valid {
            warnings.push(format!(
                "MinNodeVersion = {mnv:?} is not a dotted-numeric version string \
                 (expected shape like \"10.6.2\"). The value is otherwise carried \
                 through verbatim for upstream-config compatibility."
            ));
        }
    }

    // Upstream `LastKnownBlockVersion-{Major, Minor, Alt}` is the Byron-era
    // block-version triplet; it is declared atomically in cardano-node
    // configs (all three appear together or none of them do). A partial
    // set is almost always a copy-paste bug.
    let lkbv_present = [
        file_cfg.last_known_block_version_major.is_some(),
        file_cfg.last_known_block_version_minor.is_some(),
        file_cfg.last_known_block_version_alt.is_some(),
    ];
    let set_count = lkbv_present.iter().filter(|b| **b).count();
    if set_count != 0 && set_count != 3 {
        warnings.push(format!(
            "LastKnownBlockVersion triplet is partially set (Major: {}, Minor: {}, Alt: {}); \
             upstream expects all three fields together or none. This is almost always a \
             copy-paste bug",
            if lkbv_present[0] { "set" } else { "missing" },
            if lkbv_present[1] { "set" } else { "missing" },
            if lkbv_present[2] { "set" } else { "missing" },
        ));
    }

    // Protocol-version floor: Shelley-era hard fork introduced major 2.
    if file_cfg.max_major_protocol_version < 2 {
        warnings.push(format!(
            "max_major_protocol_version = {} is pre-Shelley; Shelley-era \
             and later blocks will be rejected as unsupported. \
             Recommended: {} (Conway-era default)",
            file_cfg.max_major_protocol_version,
            yggdrasil_node_config::CONWAY_MAJOR_PROTOCOL_VERSION,
        ));
    }

    if file_cfg.governor_tick_interval_secs == 0 {
        warnings.push(
            "governor_tick_interval_secs is 0; the governor loop will busy-\
             spin at runtime-scheduler resolution and pin a CPU core. \
             Recommended: 1-30"
                .to_owned(),
        );
    }

    let targets = GovernorTargets {
        target_known: file_cfg.governor_target_known,
        target_established: file_cfg.governor_target_established,
        target_active: file_cfg.governor_target_active,
        target_known_big_ledger: file_cfg.governor_target_known_big_ledger,
        target_established_big_ledger: file_cfg.governor_target_established_big_ledger,
        target_active_big_ledger: file_cfg.governor_target_active_big_ledger,
        ..Default::default()
    };
    if !targets.is_sane() {
        warnings.push(format!(
            "governor targets violate upstream `sanePeerSelectionTargets` \
             invariants (0 <= active <= established <= known; active <= 100, \
             established <= 1000, known <= 10000; same for big-ledger). \
             Got: target_known={}, target_established={}, target_active={}; \
             target_known_big_ledger={}, target_established_big_ledger={}, \
             target_active_big_ledger={}",
            file_cfg.governor_target_known,
            file_cfg.governor_target_established,
            file_cfg.governor_target_active,
            file_cfg.governor_target_known_big_ledger,
            file_cfg.governor_target_established_big_ledger,
            file_cfg.governor_target_active_big_ledger,
        ));
    }

    if let Some(secs) = file_cfg.keepalive_interval_secs {
        if secs >= 97 {
            warnings.push(format!(
                "keepalive_interval_secs = {secs} is >= the 97s upstream \
                 KeepAlive client timeout; peers will disconnect before the \
                 next heartbeat. Recommended: 10-60",
            ));
        } else if secs == 0 {
            warnings.push(
                "keepalive_interval_secs is 0; heartbeats will fire as \
                 fast as the runtime can schedule them (wasteful). \
                 Recommended: 10-60"
                    .to_owned(),
            );
        }
    }

    const CHECKPOINT_INTERVAL_LOWER_SOFT_FLOOR: u64 = 32;

    if file_cfg.checkpoint_interval_slots == 0 {
        warnings.push(
            "checkpoint_interval_slots is 0; checkpoint persistence cadence is effectively unbounded"
                .to_owned(),
        );
    } else if file_cfg.checkpoint_interval_slots < CHECKPOINT_INTERVAL_LOWER_SOFT_FLOOR {
        warnings.push(format!(
            "checkpoint_interval_slots = {} is below the {}-slot soft floor; \
             small cadences steal fsync bandwidth from the hot sync path \
             and can noticeably slow catch-up. Recommended: 100-10_000",
            file_cfg.checkpoint_interval_slots, CHECKPOINT_INTERVAL_LOWER_SOFT_FLOOR,
        ));
    } else if file_cfg.checkpoint_interval_slots > file_cfg.epoch_length {
        warnings.push(format!(
            "checkpoint_interval_slots = {} exceeds epoch_length = {}; \
             a crash after an epoch boundary will force replay of the \
             entire prior epoch on restart. Recommended: at most one \
             checkpoint per epoch (i.e. interval <= epoch_length)",
            file_cfg.checkpoint_interval_slots, file_cfg.epoch_length,
        ));
    }
    if file_cfg.max_ledger_snapshots == 0 {
        warnings.push(
            "max_ledger_snapshots is 0; persisted ledger checkpoints will be pruned immediately"
                .to_owned(),
        );
    }

    if let Some(ckpt_file) = file_cfg.checkpoints_file.as_deref() {
        let ckpt_path =
            crate::resolve_config_path(std::path::Path::new(ckpt_file), config_base_dir);
        if !ckpt_path.exists() {
            warnings.push(format!(
                "CheckpointsFile points at {} which does not exist; \
                 checkpoint pinning will be disabled at runtime",
                ckpt_path.display(),
            ));
        } else if let Some(expected_hex) = file_cfg.checkpoints_file_hash.as_deref() {
            if let Err(err) =
                genesis::verify_genesis_file_hash(&ckpt_path, expected_hex, "CheckpointsFileHash")
            {
                warnings.push(format!("CheckpointsFile hash verification: {err}"));
            }
        }
    }

    if let Some(explicit) = file_cfg.requires_network_magic {
        let expected =
            yggdrasil_node_config::RequiresNetworkMagic::default_for_magic(file_cfg.network_magic);
        if explicit != expected {
            warnings.push(format!(
                "RequiresNetworkMagic = {:?} is inconsistent with network_magic = {}; \
                 the canonical default for this magic is {:?}. Byron-era header \
                 decoding expects the canonical shape and peers using the \
                 default will disagree with this node",
                explicit, file_cfg.network_magic, expected,
            ));
        }
    }
    if !(file_cfg.turn_on_logging && file_cfg.use_trace_dispatcher) {
        warnings.push("runtime tracing is disabled for local operator output".to_owned());
    }
    if !file_cfg.turn_on_log_metrics {
        warnings.push("trace metrics production is disabled".to_owned());
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
