// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::{
    BatchErrorDisposition, BatchTraceExtras, CheckpointPersistenceOutcome, LedgerJudgementSettings,
    NodeConfig, ReconnectingRunState, ReconnectingVerifiedSyncRequest,
    ResumeReconnectingVerifiedSyncRequest, RuntimeBlockProducerConfig, VerifiedSyncServiceConfig,
    block_producer_ledger_state_judgement, checkpoint_trace_fields, derive_judgement_at,
    direct_sync_bootstrap_pending, handle_reconnect_batch_error, kes_expiry_warning_from_periods,
    local_root_targets_from_config, mempool_entries_for_forging, ordered_reconnect_fallback_peers,
    peer_share_request_amount, preferred_hot_peer_from_registry, preferred_hot_peer_handoff_target,
    prepare_reconnect_attempt_state, reconnect_preferred_peer,
    reconnect_preferred_peer_with_source, reconnect_storage_tip, record_verified_batch_progress,
    recover_ledger_state_for_runtime, refresh_ledger_peer_sources_from_chain_db,
    reserve_bootstrap_sync_peers, retire_failed_outbound_peer, seed_peer_registry,
    self_validate_forged_block, session_established_trace_fields,
    stake_snapshots_for_recovered_point, suppress_outbound_promotions_while_bootstrap_pending,
    sync_error_trace_fields, tip_context_from_chain_db, verified_sync_batch_trace_fields,
    wall_clock_unix_secs,
};
use crate::sync::LedgerCheckpointPolicy;
use crate::sync::{MultiEraSyncProgress, SyncError, VerificationConfig};
use crate::tracer::NodeTracer;
use serde_json::json;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, RwLock};
use yggdrasil_consensus::mempool::SharedMempool;
use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState};
use yggdrasil_consensus::{HeaderBody as ConsensusHeaderBody, OpCert};
use yggdrasil_crypto::blake2b::{hash_bytes_224, hash_bytes_256};
use yggdrasil_crypto::ed25519::{Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_ledger::{
    BlockNo, Encoder, Era, HeaderHash, LedgerState, Nonce, Point, PoolParams, PraosHeader,
    PraosHeaderBody, Relay, RewardAccount, ShelleyOpCert, ShelleyVrfCert, SlotNo, StakeCredential,
    UnitInterval,
};
use yggdrasil_network::{
    AbstractState, AfterSlot, BlockFetchClientError, ChainSyncClientError, ConnectionManagerState,
    DataFlow, GovernorAction, GovernorState, GovernorTargets, HandshakeVersion,
    LedgerStateJudgement, LocalRootConfig, PeerAccessPoint, PeerRegistry, PeerSource, PeerStatus,
    TimeoutExpired, TopologyConfig, UseBootstrapPeers, UseLedgerPeers,
};
use yggdrasil_storage::{ChainDb, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile};

fn local_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn sample_mempool_entry(
    seed: u8,
    fee: u64,
    ttl: u64,
) -> yggdrasil_consensus::mempool::MempoolEntry {
    yggdrasil_consensus::mempool::MempoolEntry {
        era: yggdrasil_ledger::Era::Shelley,
        tx_id: yggdrasil_ledger::TxId([seed; 32]),
        fee,
        body: vec![seed],
        raw_tx: vec![seed, seed.wrapping_add(1)],
        size_bytes: 2,
        ttl: SlotNo(ttl),
        inputs: vec![],
    }
}

fn sample_node_config() -> NodeConfig {
    NodeConfig {
        peer_addr: local_addr(3001),
        network_magic: 42,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    }
}

fn sample_sync_config() -> VerifiedSyncServiceConfig {
    VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
            max_major_protocol_version: Some(10),
            future_check: None,
            ocert_counters: None,
            pp_major_protocol_version: None,
            network_magic: None,
        },
        nonce_config: None,
        security_param: None,
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
        verify_vrf: false,
        active_slot_coeff: None,
        slot_length_secs: None,
        system_start_unix_secs: None,
        epoch_schedule: None,
        block_fetch_pool: None,
        max_concurrent_block_fetch_peers: 1,
        density_registry: None,
        shared_fetch_worker_pool: None,
        shared_chainsync_worker_pool: None,
    }
}

fn sample_nonce_config() -> NonceEvolutionConfig {
    NonceEvolutionConfig {
        epoch_size: EpochSize(100),
        stability_window: 10,
        extra_entropy: Nonce::Neutral,
        byron_shelley_transition: None,
    }
}

#[test]
fn recovered_non_origin_requires_stake_snapshot_sidecar() {
    let dir = tempfile::tempdir().expect("temp recovery dir");
    let mut config = sample_sync_config();
    config.nonce_config = Some(sample_nonce_config());
    let point = Point::BlockPoint(SlotNo(10), HeaderHash([1; 32]));

    let err = stake_snapshots_for_recovered_point(&config, Some(dir.path()), &point)
        .expect_err("persistent non-origin recovery requires stake snapshots");

    assert!(
        err.to_string()
            .contains("missing exact StakeSnapshots sidecar history")
    );
}

#[test]
fn recovered_origin_uses_empty_stake_snapshots_without_sidecar() {
    let dir = tempfile::tempdir().expect("temp recovery dir");
    let mut config = sample_sync_config();
    config.nonce_config = Some(sample_nonce_config());

    let snapshots = stake_snapshots_for_recovered_point(&config, Some(dir.path()), &Point::Origin)
        .expect("origin recovery tolerates empty sidecar")
        .expect("nonce tracking enables stake snapshots");

    assert_eq!(snapshots.fee_pot, 0);
    assert_eq!(
        snapshots.set.pool_stake_distribution().total_active_stake(),
        0
    );
}

#[test]
fn runtime_recovery_preserves_current_epoch_block_counts() {
    let mut config = sample_sync_config();
    config.nonce_config = Some(sample_nonce_config());

    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    let block = yggdrasil_ledger::Block {
        era: Era::Shelley,
        header: yggdrasil_ledger::BlockHeader {
            hash: HeaderHash([0x5a; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x42; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: None,
    };
    let point = Point::BlockPoint(block.header.slot_no, block.header.hash);

    let mut checkpoint_state = LedgerState::new(Era::Byron);
    checkpoint_state
        .apply_block_validated(&block, None)
        .expect("empty Shelley block applies in recovery fixture");
    let pool_hash = hash_bytes_224(&block.header.issuer_vkey).0;
    assert_eq!(checkpoint_state.blocks_made().get(&pool_hash), Some(&1));

    chain_db
        .add_volatile_block(block)
        .expect("insert volatile tip block");
    chain_db
        .persist_ledger_checkpoint(&point, &checkpoint_state.checkpoint(), 8)
        .expect("persist checkpoint");

    let recovery =
        recover_ledger_state_for_runtime(&chain_db, LedgerState::new(Era::Byron), &config, None)
            .expect("recover runtime ledger state");

    assert_eq!(recovery.outcome.point, point);
    assert_eq!(recovery.pool_block_counts.get(&pool_hash), Some(&1));
    assert_eq!(
        recovery.outcome.ledger_state.blocks_made().get(&pool_hash),
        Some(&1)
    );
    assert!(recovery.stake_snapshots.is_some());
}

fn sample_pool_params(relay: Relay, operator: u8) -> PoolParams {
    PoolParams {
        operator: [operator; 28],
        vrf_keyhash: [operator; 32],
        pledge: 1,
        cost: 1,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([operator; 28]),
        },
        pool_owners: vec![[operator; 28]],
        relays: vec![relay],
        pool_metadata: None,
    }
}

fn sample_forged_block_for_self_validation() -> crate::block_producer::ForgedBlock {
    let mut body_enc = Encoder::new();
    body_enc.array(0);
    body_enc.array(0);
    body_enc.map(0);
    body_enc.array(0);
    let body_payload = body_enc.into_bytes();
    // Match upstream `bbHash` / `hashTxSeq`: H( H(seg_1) || ... || H(seg_n) )
    // over the four post-header CBOR segments emitted above.
    let body_hash = {
        use yggdrasil_ledger::cbor::Decoder;
        let mut dec = Decoder::new(&body_payload);
        let mut combined = Vec::with_capacity(32 * 4);
        for _ in 0..4 {
            let s = dec.position();
            dec.skip().expect("skip body segment");
            let e = dec.position();
            combined.extend_from_slice(&hash_bytes_256(&body_payload[s..e]).0);
        }
        hash_bytes_256(&combined).0
    };
    let body_size = u32::try_from(body_payload.len()).expect("body size must fit in u32");

    let header_body = ConsensusHeaderBody {
        block_number: BlockNo(1),
        slot: SlotNo(1),
        prev_hash: None,
        issuer_vkey: VerificationKey::from_bytes([0x11; 32]),
        vrf_vkey: VrfVerificationKey::from_bytes([0x22; 32]),
        leader_vrf_output: vec![0x33; 32],
        leader_vrf_proof: [0x44; 80],
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: body_size,
        block_body_hash: body_hash,
        operational_cert: OpCert {
            hot_vkey: SumKesVerificationKey::from_bytes([0x55; 32]),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature([0x66; 64]),
        },
        protocol_version: (9, 0),
    };

    let kes_signature =
        SumKesSignature::from_bytes(0, &[0u8; 64]).expect("construct sum-kes signature");
    let praos_header = PraosHeader {
        body: PraosHeaderBody {
            block_number: header_body.block_number.0,
            slot: header_body.slot.0,
            prev_hash: header_body.prev_hash.map(|h| h.0),
            issuer_vkey: header_body.issuer_vkey.to_bytes(),
            vrf_vkey: header_body.vrf_vkey.to_bytes(),
            vrf_result: ShelleyVrfCert {
                output: header_body.leader_vrf_output.clone(),
                proof: header_body.leader_vrf_proof,
            },
            block_body_size: header_body.block_body_size,
            block_body_hash: header_body.block_body_hash,
            operational_cert: ShelleyOpCert {
                hot_vkey: header_body.operational_cert.hot_vkey.to_bytes(),
                sequence_number: header_body.operational_cert.sequence_number,
                kes_period: header_body.operational_cert.kes_period,
                sigma: header_body.operational_cert.sigma.0,
            },
            protocol_version: header_body.protocol_version,
        },
        signature: kes_signature.to_bytes().to_vec(),
    };
    let header_hash = praos_header.header_hash();

    crate::block_producer::ForgedBlock {
        header: crate::block_producer::ForgedBlockHeader {
            header_body,
            kes_signature,
        },
        transactions: Vec::new(),
        header_hash,
        slot: SlotNo(1),
        block_number: BlockNo(1),
        body_size,
        total_fees: 0,
    }
}

#[test]
fn mempool_entries_for_forging_is_fee_ordered() {
    let mempool = SharedMempool::with_capacity(1024);
    mempool
        .insert(sample_mempool_entry(1, 10, 1000))
        .expect("insert low-fee tx");
    mempool
        .insert(sample_mempool_entry(2, 50, 1000))
        .expect("insert mid-fee tx");
    mempool
        .insert(sample_mempool_entry(3, 100, 1000))
        .expect("insert high-fee tx");

    let entries = mempool_entries_for_forging(&mempool);
    let fees = entries.iter().map(|entry| entry.fee).collect::<Vec<_>>();
    assert_eq!(fees, vec![100, 50, 10]);
}

#[test]
fn self_validate_forged_block_accepts_structurally_valid_block() {
    let forged = sample_forged_block_for_self_validation();
    self_validate_forged_block(&forged)
        .expect("structurally valid forged block should self-validate");
}

#[test]
fn self_validate_forged_block_rejects_body_hash_mismatch() {
    let mut forged = sample_forged_block_for_self_validation();
    forged.header.header_body.block_body_hash = [0xAB; 32];

    let err = self_validate_forged_block(&forged)
        .expect_err("tampered body hash must fail self-validation");

    assert!(
        err.to_string().contains("block body hash mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn self_validate_forged_block_rejects_header_hash_mismatch() {
    let mut forged = sample_forged_block_for_self_validation();
    forged.header_hash = HeaderHash([0xCD; 32]);

    let err = self_validate_forged_block(&forged)
        .expect_err("tampered header hash must fail self-validation");

    assert!(
        err.to_string().contains("forged header hash mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn kes_expiry_warning_triggers_near_window_end() {
    // cert validity window: [100, 162)
    let warning = kes_expiry_warning_from_periods(158, 100, 62, 129_600)
        .expect("warning should be emitted near KES expiry");

    assert_eq!(warning.current_period, 158);
    assert_eq!(warning.cert_start_period, 100);
    assert_eq!(warning.cert_end_period, 162);
    assert_eq!(warning.remaining_periods, 4);
    assert_eq!(warning.remaining_slots, 4 * 129_600);
}

#[test]
fn kes_expiry_warning_suppressed_when_far_from_expiry() {
    // cert validity window: [10, 72), current=40 => remaining=32 (> threshold)
    let warning = kes_expiry_warning_from_periods(40, 10, 62, 129_600);
    assert!(warning.is_none());
}

#[test]
fn tip_context_from_chain_db_reads_tip_block_number() {
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    let block = yggdrasil_ledger::Block {
        era: Era::Conway,
        header: yggdrasil_ledger::BlockHeader {
            hash: HeaderHash([9; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: yggdrasil_ledger::BlockNo(7),
            issuer_vkey: [1; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: None,
    };
    chain_db
        .add_volatile_block(block)
        .expect("insert volatile tip block");

    let (tip_slot, tip_block_no, tip_hash) = tip_context_from_chain_db(&chain_db);
    assert_eq!(tip_slot, Some(SlotNo(42)));
    assert_eq!(tip_block_no, Some(yggdrasil_ledger::BlockNo(7)));
    assert_eq!(tip_hash, Some(HeaderHash([9; 32])));
}

#[test]
fn peer_share_request_amount_is_clamped_to_u16() {
    let targets = GovernorTargets {
        target_known: usize::MAX,
        target_established: 5,
        target_active: 2,
        ..Default::default()
    };

    assert_eq!(peer_share_request_amount(&targets), u16::MAX);

    let targets = GovernorTargets {
        target_known: 0,
        target_established: 0,
        target_active: 0,
        ..Default::default()
    };
    assert_eq!(peer_share_request_amount(&targets), 1);
}

fn ledger_state_with_pool_relay(peer: SocketAddr) -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    state.pool_state_mut().register(sample_pool_params(
        Relay::SingleHostAddr(
            Some(peer.port()),
            Some(match peer.ip() {
                IpAddr::V4(addr) => addr.octets(),
                IpAddr::V6(_) => panic!("test peer should be IPv4"),
            }),
            None,
        ),
        7,
    ));
    state
}

#[test]
fn reconnect_request_builder_sets_optional_fields() {
    let node = sample_node_config();
    let cfg = sample_sync_config();
    let path = std::path::PathBuf::from("snapshot.json");

    let req = ReconnectingVerifiedSyncRequest::new(
        &node,
        &[],
        Point::Origin,
        LedgerState::new(Era::Byron),
        &cfg,
    )
    .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
    .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
        yggdrasil_network::AfterSlot::Always,
    )))
    .with_peer_snapshot_path(Some(path.clone()));

    assert!(req.nonce_state.is_some());
    assert_eq!(
        req.use_ledger_peers,
        Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always
        ))
    );
    assert_eq!(req.peer_snapshot_path, Some(path));
}

#[test]
fn reconnect_request_builder_last_call_wins_for_overrides() {
    let node = sample_node_config();
    let cfg = sample_sync_config();
    let first = std::path::PathBuf::from("first.json");
    let second = std::path::PathBuf::from("second.json");

    let req = ReconnectingVerifiedSyncRequest::new(
        &node,
        &[],
        Point::Origin,
        LedgerState::new(Era::Byron),
        &cfg,
    )
    .with_peer_snapshot_path(Some(first))
    .with_peer_snapshot_path(Some(second.clone()))
    .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
        yggdrasil_network::AfterSlot::Always,
    )))
    .with_use_ledger_peers(None)
    .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
    .with_nonce_state(None);

    assert_eq!(req.peer_snapshot_path, Some(second));
    assert_eq!(req.use_ledger_peers, None);
    assert_eq!(req.nonce_state, None);
}

#[test]
fn resume_request_builder_sets_optional_fields() {
    let node = sample_node_config();
    let cfg = sample_sync_config();
    let path = std::path::PathBuf::from("snapshot.json");
    let metrics = crate::tracer::NodeMetrics::new();

    let req =
        ResumeReconnectingVerifiedSyncRequest::new(&node, &[], LedgerState::new(Era::Byron), &cfg)
            .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
            .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
                yggdrasil_network::AfterSlot::Always,
            )))
            .with_peer_snapshot_path(Some(path.clone()))
            .with_metrics(Some(&metrics));

    assert!(req.nonce_state.is_some());
    assert_eq!(
        req.use_ledger_peers,
        Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always
        ))
    );
    assert_eq!(req.peer_snapshot_path, Some(path));
    assert!(req.metrics.is_some());
}

#[test]
fn resume_request_builder_last_call_wins_for_overrides() {
    let node = sample_node_config();
    let cfg = sample_sync_config();
    let first = std::path::PathBuf::from("first.json");
    let second = std::path::PathBuf::from("second.json");

    let req =
        ResumeReconnectingVerifiedSyncRequest::new(&node, &[], LedgerState::new(Era::Byron), &cfg)
            .with_peer_snapshot_path(Some(first))
            .with_peer_snapshot_path(Some(second.clone()))
            .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
                yggdrasil_network::AfterSlot::Always,
            )))
            .with_use_ledger_peers(None)
            .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
            .with_nonce_state(None);

    assert_eq!(req.peer_snapshot_path, Some(second));
    assert_eq!(req.use_ledger_peers, None);
    assert_eq!(req.nonce_state, None);
}

#[test]
fn resume_request_builder_sets_mempool() {
    let node = sample_node_config();
    let cfg = sample_sync_config();
    let mempool = SharedMempool::default();

    let req =
        ResumeReconnectingVerifiedSyncRequest::new(&node, &[], LedgerState::new(Era::Byron), &cfg)
            .with_mempool(Some(mempool.clone()));

    assert!(req.mempool.is_some());

    // Default constructor has none.
    let req2 =
        ResumeReconnectingVerifiedSyncRequest::new(&node, &[], LedgerState::new(Era::Byron), &cfg);
    assert!(req2.mempool.is_none());
}

#[test]
fn re_admit_rolled_back_tx_ids_reinserts_cached_entries() {
    let mempool = SharedMempool::with_capacity(1024);
    let entry = sample_mempool_entry(42, 100, 1000);
    let tx_id = entry.tx_id;
    mempool.insert(entry.clone()).expect("insert entry");

    let mut recently_confirmed = BTreeMap::new();
    let cached = super::cache_confirmed_entries(&mempool, &[tx_id], &mut recently_confirmed);
    assert_eq!(cached, 1);

    let removed = mempool.remove_confirmed(&[tx_id]);
    assert_eq!(removed, 1);
    assert!(!mempool.contains(&tx_id));

    let stats =
        super::re_admit_rolled_back_tx_ids(&mempool, &[tx_id], SlotNo(10), &mut recently_confirmed);

    assert_eq!(stats.re_admitted, 1);
    assert_eq!(stats.missing_cache_entry, 0);
    assert!(mempool.contains(&tx_id));
    assert!(!recently_confirmed.contains_key(&tx_id));
}

#[test]
fn re_admit_rolled_back_tx_ids_counts_missing_cache_entries() {
    let mempool = SharedMempool::with_capacity(1024);
    let tx_id = yggdrasil_ledger::TxId([7; 32]);
    let mut recently_confirmed = BTreeMap::new();

    let stats =
        super::re_admit_rolled_back_tx_ids(&mempool, &[tx_id], SlotNo(10), &mut recently_confirmed);

    assert_eq!(stats.re_admitted, 0);
    assert_eq!(stats.missing_cache_entry, 1);
    assert!(!mempool.contains(&tx_id));
}

#[test]
fn checkpoint_trace_fields_include_persisted_prune_counts() {
    let policy = LedgerCheckpointPolicy {
        min_slot_delta: 2160,
        max_snapshots: 8,
    };
    let fields = checkpoint_trace_fields(
        &CheckpointPersistenceOutcome::Persisted {
            slot: SlotNo(4320),
            retained_snapshots: 8,
            pruned_snapshots: 2,
            rollback_count: 1,
        },
        &policy,
    );

    assert_eq!(fields.get("action"), Some(&json!("persisted")));
    assert_eq!(fields.get("slot"), Some(&json!(4320)));
    assert_eq!(fields.get("retainedSnapshots"), Some(&json!(8)));
    assert_eq!(fields.get("prunedSnapshots"), Some(&json!(2)));
    assert_eq!(fields.get("rollbackCount"), Some(&json!(1)));
    assert_eq!(fields.get("checkpointIntervalSlots"), Some(&json!(2160)));
    assert_eq!(fields.get("maxLedgerSnapshots"), Some(&json!(8)));
}

#[test]
fn checkpoint_trace_fields_include_skip_delta() {
    let policy = LedgerCheckpointPolicy {
        min_slot_delta: 2160,
        max_snapshots: 8,
    };
    let fields = checkpoint_trace_fields(
        &CheckpointPersistenceOutcome::Skipped {
            slot: SlotNo(1200),
            rollback_count: 0,
            since_last_slot_delta: 1200,
        },
        &policy,
    );

    assert_eq!(fields.get("action"), Some(&json!("skipped")));
    assert_eq!(fields.get("slot"), Some(&json!(1200)));
    assert_eq!(fields.get("sinceLastSlotDelta"), Some(&json!(1200)));
    assert_eq!(fields.get("rollbackCount"), Some(&json!(0)));
}

#[test]
fn session_established_trace_fields_include_peer_reconnects_and_point() {
    let fields = session_established_trace_fields(
        local_addr(3001),
        2,
        Point::BlockPoint(SlotNo(42), HeaderHash([7; 32])),
    );

    assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3001")));
    assert_eq!(fields.get("reconnectCount"), Some(&json!(2)));
    let from_point = fields
        .get("fromPoint")
        .and_then(|value| value.as_str())
        .expect("fromPoint should be a string");
    assert!(from_point.starts_with("BlockPoint(SlotNo(42), HeaderHash(0707070707070707"));
}

#[test]
fn verified_sync_batch_trace_fields_include_optional_runtime_context() {
    let progress = MultiEraSyncProgress {
        current_point: Point::BlockPoint(SlotNo(21), HeaderHash([5; 32])),
        steps: vec![],
        fetched_blocks: 3,
        rollback_count: 1,
    };
    let mut run_state = ReconnectingRunState::new();
    run_state.record_progress(&progress);
    run_state.stable_block_count = 9;

    let fields = verified_sync_batch_trace_fields(
        local_addr(3002),
        progress.current_point,
        &progress,
        &run_state,
        BatchTraceExtras {
            stable_block_count: Some(run_state.stable_block_count),
            checkpoint_tracked: Some(true),
        },
    );

    assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3002")));
    assert_eq!(fields.get("batchFetchedBlocks"), Some(&json!(3)));
    assert_eq!(fields.get("batchRollbacks"), Some(&json!(1)));
    assert_eq!(fields.get("totalBlocks"), Some(&json!(3)));
    assert_eq!(fields.get("batchesCompleted"), Some(&json!(1)));
    assert_eq!(fields.get("stableBlocks"), Some(&json!(9)));
    assert_eq!(fields.get("checkpointTracked"), Some(&json!(true)));
}

#[test]
fn sync_error_trace_fields_include_error_and_point() {
    let fields = sync_error_trace_fields(
        local_addr(3003),
        &SyncError::Recovery("checkpoint gap".to_owned()),
        Point::Origin,
    );

    assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3003")));
    assert_eq!(fields.get("currentPoint"), Some(&json!("Origin")));
    assert_eq!(
        fields.get("error"),
        Some(&json!("recovery error: checkpoint gap"))
    );
}

#[test]
fn handle_reconnect_batch_error_reconnects_for_connectivity_errors() {
    let tracer = NodeTracer::disabled();

    let chainsync = handle_reconnect_batch_error(
        &tracer,
        local_addr(3004),
        Point::Origin,
        &SyncError::ChainSync(ChainSyncClientError::ConnectionClosed),
    );
    let blockfetch = handle_reconnect_batch_error(
        &tracer,
        local_addr(3005),
        Point::Origin,
        &SyncError::BlockFetch(BlockFetchClientError::ConnectionClosed),
    );

    assert!(matches!(chainsync, BatchErrorDisposition::Reconnect));
    assert!(matches!(blockfetch, BatchErrorDisposition::Reconnect));
}

#[test]
fn handle_reconnect_batch_error_fails_for_non_connectivity_errors() {
    let tracer = NodeTracer::disabled();
    let disposition = handle_reconnect_batch_error(
        &tracer,
        local_addr(3006),
        Point::Origin,
        &SyncError::Recovery("inconsistent checkpoint".to_owned()),
    );

    assert!(matches!(disposition, BatchErrorDisposition::Fail));
}

#[test]
fn handle_reconnect_batch_error_punishes_for_peer_attributable_errors() {
    let tracer = NodeTracer::disabled();

    // Exhaustive — every variant that `SyncError::is_peer_attributable`
    // returns `true` for MUST route to `ReconnectAndPunish`. Keeping
    // this list in lockstep with the `matches!` arms in
    // `is_peer_attributable` (+ the slice-52 exhaustiveness test)
    // gives two independent sources of truth: the classification
    // function AND the downstream disposition.
    let variants: Vec<SyncError> = vec![
        SyncError::BlockBodyHashMismatch,
        SyncError::Consensus(yggdrasil_consensus::ConsensusError::InvalidKesSignature),
        SyncError::LedgerDecode(yggdrasil_ledger::LedgerError::CborTrailingBytes(1)),
        SyncError::BlockFromFuture {
            slot: 999,
            excess_slots: 100,
        },
        SyncError::WrongBlockBodySize {
            declared: 1,
            actual: 2,
        },
        SyncError::ProtocolVersionMismatch {
            era: yggdrasil_ledger::Era::Conway,
            major: 1,
            minor: 0,
            expected_range: "9+".to_owned(),
        },
        SyncError::ProtocolVersionTooHigh { major: 99, max: 10 },
        SyncError::HeaderProtVerTooHigh {
            header_major: 15,
            pp_major: 10,
        },
    ];

    for err in &variants {
        assert!(
            err.is_peer_attributable(),
            "test precondition: {err:?} must be peer-attributable",
        );
        let disposition =
            handle_reconnect_batch_error(&tracer, local_addr(3006), Point::Origin, err);
        assert!(
            matches!(disposition, BatchErrorDisposition::ReconnectAndPunish),
            "expected ReconnectAndPunish for peer-attributable {err:?}, \
                 got {disposition:?}",
        );
    }
}

#[test]
fn reconnecting_run_state_accumulates_progress_and_session_metadata() {
    let mut run_state = ReconnectingRunState::new();
    let mut had_session = false;
    let first_peer = local_addr(3007);
    let second_peer = local_addr(3008);

    run_state.record_session(first_peer, &mut had_session);
    run_state.record_session(second_peer, &mut had_session);
    run_state.record_progress(&MultiEraSyncProgress {
        current_point: Point::Origin,
        steps: vec![],
        fetched_blocks: 2,
        rollback_count: 1,
    });
    run_state.record_progress(&MultiEraSyncProgress {
        current_point: Point::Origin,
        steps: vec![],
        fetched_blocks: 4,
        rollback_count: 0,
    });
    run_state.stable_block_count = 5;

    let outcome = run_state.finish(Point::Origin, None, None);

    assert_eq!(outcome.total_blocks, 6);
    assert_eq!(outcome.total_rollbacks, 1);
    assert_eq!(outcome.batches_completed, 2);
    assert_eq!(outcome.stable_block_count, 5);
    assert_eq!(outcome.reconnect_count, 1);
    assert_eq!(outcome.last_connected_peer_addr, Some(second_peer));
}

#[test]
fn record_verified_batch_progress_updates_point_totals_and_preserves_empty_nonce_state() {
    let progress = MultiEraSyncProgress {
        current_point: Point::BlockPoint(SlotNo(5), HeaderHash([9; 32])),
        steps: vec![],
        fetched_blocks: 4,
        rollback_count: 2,
    };
    let nonce_cfg = NonceEvolutionConfig {
        epoch_size: EpochSize(10),
        stability_window: 100,
        extra_entropy: Nonce::Neutral,
        byron_shelley_transition: None,
    };
    let mut from_point = Point::Origin;
    let mut run_state = ReconnectingRunState::new();
    let mut nonce_state = NonceEvolutionState::new(Nonce::Neutral);

    record_verified_batch_progress(
        &mut from_point,
        &mut run_state,
        &progress,
        Some(&mut nonce_state),
        Some(&nonce_cfg),
        None,
    );

    assert_eq!(from_point, progress.current_point);
    assert_eq!(run_state.total_blocks, 4);
    assert_eq!(run_state.total_rollbacks, 2);
    assert_eq!(run_state.batches_completed, 1);
    assert_eq!(nonce_state.evolving_nonce, Nonce::Neutral);
}

#[test]
fn seed_peer_registry_preserves_bootstrap_and_local_root_sources() {
    let primary = local_addr(3001);
    let local_root = LocalRootConfig {
        access_points: vec![PeerAccessPoint {
            address: "127.0.0.1".to_owned(),
            port: 3002,
        }],
        advertise: false,
        trustable: true,
        hot_valency: 1,
        warm_valency: Some(1),
        diffusion_mode: Default::default(),
    };
    let topology = TopologyConfig {
        bootstrap_peers: UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
            address: "127.0.0.1".to_owned(),
            port: 3003,
        }]),
        local_roots: vec![local_root],
        public_roots: Vec::new(),
        use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
        peer_snapshot_file: None,
    };

    let registry = seed_peer_registry(primary, &topology);

    let primary_entry = registry.get(&primary).expect("primary peer present");
    let local_root_entry = registry
        .get(&local_addr(3002))
        .expect("local root peer present");
    let bootstrap_entry = registry
        .get(&local_addr(3003))
        .expect("bootstrap peer present");

    assert!(
        primary_entry
            .sources
            .contains(&PeerSource::PeerSourceBootstrap)
    );
    assert!(
        local_root_entry
            .sources
            .contains(&PeerSource::PeerSourceLocalRoot)
    );
    assert!(
        bootstrap_entry
            .sources
            .contains(&PeerSource::PeerSourceBootstrap)
    );
    assert_eq!(primary_entry.status, PeerStatus::PeerCooling);
    assert_eq!(bootstrap_entry.status, PeerStatus::PeerCooling);
    assert_eq!(local_root_entry.status, PeerStatus::PeerCold);
}

#[test]
fn reserve_bootstrap_sync_peers_does_not_downgrade_active_peer() {
    let primary = local_addr(3001);
    let fallback = local_addr(3002);
    let hot = local_addr(3003);
    let mut registry = PeerRegistry::default();
    registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot, PeerStatus::PeerHot);

    assert!(reserve_bootstrap_sync_peers(
        &mut registry,
        [primary, fallback, hot]
    ));

    assert_eq!(
        registry.get(&primary).expect("primary").status,
        PeerStatus::PeerCooling
    );
    assert_eq!(
        registry.get(&fallback).expect("fallback").status,
        PeerStatus::PeerCooling
    );
    assert_eq!(registry.get(&hot).expect("hot").status, PeerStatus::PeerHot);
}

#[test]
fn direct_sync_bootstrap_pending_requires_reserved_bootstrap_without_hot_peer() {
    let primary = local_addr(3001);
    let hot = local_addr(3002);
    let mut registry = PeerRegistry::default();

    registry.insert_source(primary, PeerSource::PeerSourceBootstrap);
    registry.set_status(primary, PeerStatus::PeerCooling);
    assert!(direct_sync_bootstrap_pending(&registry));

    registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot, PeerStatus::PeerHot);
    assert!(!direct_sync_bootstrap_pending(&registry));
}

#[test]
fn bootstrap_pending_suppresses_only_outbound_promotions() {
    let primary = local_addr(3001);
    let warm_candidate = local_addr(3002);
    let hot_candidate = local_addr(3003);
    let demote_candidate = local_addr(3004);
    let mut registry = PeerRegistry::default();

    registry.insert_source(primary, PeerSource::PeerSourceBootstrap);
    registry.set_status(primary, PeerStatus::PeerCooling);

    let mut actions = vec![
        GovernorAction::PromoteToWarm(warm_candidate),
        GovernorAction::PromoteToHot(hot_candidate),
        GovernorAction::DemoteToCold(demote_candidate),
        GovernorAction::RequestPublicRoots,
    ];

    assert_eq!(
        suppress_outbound_promotions_while_bootstrap_pending(&registry, &mut actions),
        2
    );
    assert_eq!(
        actions,
        vec![
            GovernorAction::DemoteToCold(demote_candidate),
            GovernorAction::RequestPublicRoots,
        ]
    );
}

#[test]
fn reconnect_storage_tip_uses_immutable_tip_when_volatile_is_empty() {
    let immutable_tip = Point::BlockPoint(SlotNo(855_632), HeaderHash([0x42; 32]));

    assert_eq!(
        reconnect_storage_tip(Point::Origin, immutable_tip),
        immutable_tip
    );
}

#[test]
fn reconnect_storage_tip_prefers_non_origin_volatile_tip() {
    let volatile_tip = Point::BlockPoint(SlotNo(868_687), HeaderHash([0x24; 32]));
    let immutable_tip = Point::BlockPoint(SlotNo(855_632), HeaderHash([0x42; 32]));

    assert_eq!(
        reconnect_storage_tip(volatile_tip, immutable_tip),
        volatile_tip
    );
}

#[test]
fn refresh_ledger_peer_sources_uses_supplied_base_ledger_state() {
    let relay_peer = local_addr(3010);
    let base_ledger_state = ledger_state_with_pool_relay(relay_peer);
    let chain_db = Arc::new(RwLock::new(ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    )));
    let topology = TopologyConfig {
        use_ledger_peers: UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
        ..TopologyConfig::default()
    };
    let tracer = NodeTracer::disabled();
    let mut registry = yggdrasil_network::PeerRegistry::default();

    let observation = refresh_ledger_peer_sources_from_chain_db(
        &mut registry,
        &chain_db,
        &base_ledger_state,
        &topology,
        &tracer,
        LedgerJudgementSettings::default(),
        None,
    );

    assert!(observation.update.changed);
    assert_eq!(observation.judgement, LedgerStateJudgement::YoungEnough);
    let entry = registry
        .get(&relay_peer)
        .expect("ledger-derived relay peer should be present");
    assert!(entry.sources.contains(&PeerSource::PeerSourceLedger));
}

/// Pins the production-shaped legacy fallback: when the genesis
/// timing inputs aren't configured, the runtime helper must return
/// `YoungEnough` (matching pre-slice behaviour) so test paths and
/// no-genesis configurations don't suddenly start reporting `TooOld`
/// against an arbitrary wall-clock value.
#[test]
fn derive_judgement_at_falls_back_to_young_enough_without_genesis() {
    // Missing system_start.
    assert_eq!(
        derive_judgement_at(Some(100), None, Some(1.0), 60.0, 1000.0),
        yggdrasil_network::LedgerStateJudgement::YoungEnough
    );
    // Missing slot_length.
    assert_eq!(
        derive_judgement_at(Some(100), Some(0.0), None, 60.0, 1000.0),
        yggdrasil_network::LedgerStateJudgement::YoungEnough
    );
}

/// When BOTH genesis timing inputs are configured, the runtime helper
/// delegates to `judge_ledger_state_age` and produces a real wall-
/// clock-derived judgement: a 100-slot tip at 1 s slot length is 100 s
/// old at `now=200`, which exceeds a 60 s `max_age` → `TooOld`.
/// Without the wiring this slice introduced, this would still report
/// `YoungEnough` from the historical hardcoded constant — the assertion
/// `TooOld` here is the regression guard.
#[test]
fn derive_judgement_at_returns_too_old_when_genesis_present_and_tip_stale() {
    let judgement = derive_judgement_at(Some(100), Some(0.0), Some(1.0), 60.0, 200.0);
    assert_eq!(judgement, yggdrasil_network::LedgerStateJudgement::TooOld);
}

/// Sibling of the previous test: same setup but `now=150` keeps the
/// tip's age under the threshold → `YoungEnough`. Pins both branches
/// of the runtime helper at the production-relevant boundary.
#[test]
fn derive_judgement_at_returns_young_enough_when_genesis_present_and_tip_fresh() {
    let judgement = derive_judgement_at(Some(100), Some(0.0), Some(1.0), 60.0, 150.0);
    assert_eq!(
        judgement,
        yggdrasil_network::LedgerStateJudgement::YoungEnough
    );
}

#[test]
fn block_producer_ledger_judgement_blocks_stale_tips() {
    let now = wall_clock_unix_secs();
    let mut cfg = RuntimeBlockProducerConfig {
        slot_length: std::time::Duration::from_secs(1),
        system_start_unix_secs: Some(now - 100.0),
        max_ledger_state_age_secs: Some(10.0),
        active_slot_coeff: yggdrasil_consensus::ActiveSlotCoeff::new(0.05)
            .expect("valid active slot coefficient"),
        sigma_num: 1,
        sigma_den: 1,
        epoch_nonce: Nonce::Neutral,
        max_block_body_size: 65_536,
        protocol_version: (10, 0),
    };

    assert_eq!(
        block_producer_ledger_state_judgement(Some(SlotNo(50)), &cfg),
        LedgerStateJudgement::TooOld
    );

    cfg.max_ledger_state_age_secs = Some(60.0);
    assert_eq!(
        block_producer_ledger_state_judgement(Some(SlotNo(50)), &cfg),
        LedgerStateJudgement::YoungEnough
    );
}

#[test]
fn local_root_targets_use_effective_warm_valency() {
    let local_roots = vec![LocalRootConfig {
        access_points: vec![PeerAccessPoint {
            address: "127.0.0.1".to_owned(),
            port: 4001,
        }],
        advertise: false,
        trustable: false,
        hot_valency: 2,
        warm_valency: None,
        diffusion_mode: Default::default(),
    }];

    let targets = local_root_targets_from_config(&local_roots);

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].hot_valency, 2);
    assert_eq!(targets[0].warm_valency, 2);
    assert_eq!(targets[0].peers, vec![local_addr(4001)]);
}

#[test]
fn promote_to_hot_marks_warm_peer() {
    use super::OutboundPeerManager;
    use yggdrasil_network::ControlMessage;

    let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();

    // Cannot promote unknown peer.
    assert!(!mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new()));

    // Simulate adding a warm peer directly.
    let session = fake_peer_session(addr);
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    // First promotion succeeds.
    assert!(mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new()));
    assert!(mgr.warm_peers[&addr].is_hot);
    assert_eq!(mgr.warm_peers[&addr].control.hot, ControlMessage::Continue);
    assert_eq!(mgr.warm_peers[&addr].control.warm, ControlMessage::Quiesce);

    // Second promotion is idempotent.
    assert!(!mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new()));
}

#[test]
fn promote_to_hot_applies_upstream_canonical_weights() {
    // Slice D-Scheduler — `apply_hot_weights` must consult the
    // `HotPeerScheduling` table rather than the previously-hardcoded
    // constants.  The default `HotPeerScheduling::new()` carries the
    // upstream `defaultMiniProtocolParameters` values
    // (BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1).
    use super::OutboundPeerManager;
    use yggdrasil_network::multiplexer::MiniProtocolNum;

    let addr: std::net::SocketAddr = "1.2.3.4:3050".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session(addr);
    let weight_lookup: std::collections::BTreeMap<
        MiniProtocolNum,
        yggdrasil_network::WeightHandle,
    > = session.protocol_weights.iter().cloned().collect();
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    let scheduling = yggdrasil_network::HotPeerScheduling::new();
    assert!(mgr.promote_to_hot(addr, &scheduling));

    // BlockFetch must now carry the upstream-canonical 10 (was 2 in
    // the pre-Slice-D-Scheduler hardcoded path).
    assert_eq!(
        weight_lookup[&MiniProtocolNum::BLOCK_FETCH].get(),
        10,
        "BlockFetch weight must match HotPeerScheduling upstream default",
    );
    assert_eq!(weight_lookup[&MiniProtocolNum::CHAIN_SYNC].get(), 3);
    assert_eq!(weight_lookup[&MiniProtocolNum::TX_SUBMISSION].get(), 2);
    assert_eq!(weight_lookup[&MiniProtocolNum::KEEP_ALIVE].get(), 1);
}

#[test]
fn promote_to_hot_honours_runtime_weight_overrides() {
    // Operators can override per-protocol weights via
    // `set_hot_protocol_weight` — `apply_hot_weights` must read the
    // overridden value, not fall back to the upstream default.
    use super::OutboundPeerManager;
    use yggdrasil_network::multiplexer::MiniProtocolNum;

    let addr: std::net::SocketAddr = "1.2.3.4:3051".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session(addr);
    let weight_lookup: std::collections::BTreeMap<
        MiniProtocolNum,
        yggdrasil_network::WeightHandle,
    > = session.protocol_weights.iter().cloned().collect();
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    let mut scheduling = yggdrasil_network::HotPeerScheduling::new();
    scheduling.set_hot_protocol_weight(MiniProtocolNum::BLOCK_FETCH, 20);
    // Set BlockFetch to a non-default 20 and verify it lands.
    assert!(mgr.promote_to_hot(addr, &scheduling));
    assert_eq!(weight_lookup[&MiniProtocolNum::BLOCK_FETCH].get(), 20);
}

#[test]
fn hot_peer_addrs_returns_only_hot_peers_in_sorted_order() {
    // Phase 6 seam: the sync loop calls `hot_peer_addrs()` to size
    // the dispatcher's effective concurrency without holding a
    // `&mut` borrow on the manager.  Returns a deterministic
    // BTreeMap-ordered slice so the dispatcher sees peers in
    // stable order across ticks.
    use super::OutboundPeerManager;

    let a1: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let a2: std::net::SocketAddr = "1.2.3.4:3002".parse().unwrap();
    let a3: std::net::SocketAddr = "1.2.3.4:3003".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    for a in [a1, a2, a3] {
        let session = fake_peer_session(a);
        mgr.warm_peers.insert(
            a,
            super::ManagedWarmPeer::new(session, std::time::Instant::now()),
        );
    }
    // Promote a1 and a3 only; a2 stays warm.
    mgr.promote_to_hot(a1, &yggdrasil_network::HotPeerScheduling::new());
    mgr.promote_to_hot(a3, &yggdrasil_network::HotPeerScheduling::new());

    let hot = mgr.hot_peer_addrs();
    assert_eq!(hot, vec![a1, a3]);
}

#[test]
fn hot_peer_addrs_empty_when_no_hot_peers() {
    use super::OutboundPeerManager;
    let a1: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session(a1);
    mgr.warm_peers.insert(
        a1,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    // Warm but not hot — must not appear.
    assert!(mgr.hot_peer_addrs().is_empty());
}

#[test]
fn with_hot_block_fetch_clients_yields_handles_for_hot_peers_only() {
    // Phase 6 seam: the closure receives a sliced view of every
    // hot peer's BlockFetchClient.  Warm-only peers are excluded.
    use super::OutboundPeerManager;

    let a1: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let a2: std::net::SocketAddr = "1.2.3.4:3002".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    for a in [a1, a2] {
        let session = fake_peer_session(a);
        mgr.warm_peers.insert(
            a,
            super::ManagedWarmPeer::new(session, std::time::Instant::now()),
        );
    }
    mgr.promote_to_hot(a1, &yggdrasil_network::HotPeerScheduling::new());

    let count = mgr.with_hot_block_fetch_clients(|handles| {
        // Pin the slice contents: only a1 (hot), in BTreeMap order.
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].0, a1);
        handles.len()
    });
    assert_eq!(count, 1);
}

#[tokio::test]
async fn migrate_session_to_worker_takes_block_fetch_and_registers() {
    // Phase 6 production wire: migrating a session's BlockFetch
    // handle into the worker pool must (a) leave the session's
    // block_fetch field as None, (b) register a worker in the
    // pool keyed on the peer addr.
    use super::OutboundPeerManager;

    let a: std::net::SocketAddr = "1.2.3.4:3300".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(a).await;
    assert!(session.has_block_fetch());
    mgr.warm_peers.insert(
        a,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    assert!(mgr.migrate_session_to_worker(a).await);
    assert!(!mgr.warm_peers[&a].session.has_block_fetch());
    let pool = mgr.shared_fetch_worker_pool();
    assert!(pool.read().await.worker(&a).is_some());
}

#[tokio::test]
async fn migrate_session_to_worker_returns_false_when_already_migrated() {
    use super::OutboundPeerManager;
    let a: std::net::SocketAddr = "1.2.3.4:3301".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(a).await;
    mgr.warm_peers.insert(
        a,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    assert!(mgr.migrate_session_to_worker(a).await);
    // Second call: block_fetch is None, so migration cannot proceed.
    assert!(!mgr.migrate_session_to_worker(a).await);
}

#[tokio::test]
async fn migrate_session_to_worker_returns_false_for_unknown_peer() {
    use super::OutboundPeerManager;
    let mgr_addr: std::net::SocketAddr = "9.9.9.9:9999".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    assert!(!mgr.migrate_session_to_worker(mgr_addr).await);
    let pool = mgr.shared_fetch_worker_pool();
    assert!(pool.read().await.is_empty());
}

#[tokio::test]
async fn unregister_worker_drops_handle_for_clean_shutdown() {
    use super::OutboundPeerManager;
    let a: std::net::SocketAddr = "1.2.3.4:3302".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(a).await;
    mgr.warm_peers.insert(
        a,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    mgr.migrate_session_to_worker(a).await;
    let pool = mgr.shared_fetch_worker_pool();
    assert!(pool.read().await.worker(&a).is_some());
    assert!(mgr.unregister_worker(&a).await);
    assert!(pool.read().await.worker(&a).is_none());
    // Idempotent: a second unregister is a no-op.
    assert!(!mgr.unregister_worker(&a).await);
}

#[tokio::test]
async fn shared_fetch_worker_pool_is_visible_across_arc_clones() {
    // The whole point of the shared pool: governor task
    // populates, sync task reads, both via Arc<RwLock<>>.
    // Validate that a clone of the Arc handle observes
    // registrations made through the original.
    use super::OutboundPeerManager;
    let a: std::net::SocketAddr = "1.2.3.4:3303".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(a).await;
    mgr.warm_peers.insert(
        a,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    // Clone the shared handle BEFORE migrating to model the
    // runtime-startup wiring path: clone into both configs at
    // startup, register later from the governor task.
    let sync_side_view = mgr.shared_fetch_worker_pool();
    assert!(sync_side_view.read().await.worker(&a).is_none());
    mgr.migrate_session_to_worker(a).await;
    // The sync-side clone observes the registration.
    assert!(sync_side_view.read().await.worker(&a).is_some());
}

#[test]
fn with_hot_block_fetch_clients_empty_slice_when_no_hot_peers() {
    // Empty-slice contract: callers should treat this as "fall
    // back to single-peer dispatch via the leader session".
    use super::OutboundPeerManager;

    let a1: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session(a1);
    mgr.warm_peers.insert(
        a1,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    let was_empty = mgr.with_hot_block_fetch_clients(|handles| handles.is_empty());
    assert!(was_empty);
}

#[test]
fn demote_to_warm_clears_hot_flag() {
    use super::OutboundPeerManager;
    use yggdrasil_network::ControlMessage;

    let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session(addr);
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new());
    assert!(mgr.warm_peers[&addr].is_hot);

    assert!(mgr.demote_to_warm(addr));
    assert!(!mgr.warm_peers[&addr].is_hot);
    assert_eq!(mgr.warm_peers[&addr].control.hot, ControlMessage::Quiesce);
    assert_eq!(mgr.warm_peers[&addr].control.warm, ControlMessage::Continue);

    // Demoting an already-warm peer is no-op.
    assert!(!mgr.demote_to_warm(addr));
}

#[tokio::test]
async fn demote_to_cold_terminates_temperature_bundle() {
    use super::OutboundPeerManager;
    use yggdrasil_network::ControlMessage;

    let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(addr).await;
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );

    assert!(mgr.demote_to_cold(addr).await);

    // Internal peer entry is removed after close. This verifies the
    // close path is reachable and does not panic while applying
    // terminate controls before aborting the mux.
    assert!(!mgr.warm_peers.contains_key(&addr));

    // Regression guard for expected control constants used by close.
    let mut bundle = yggdrasil_network::TemperatureBundle {
        hot: ControlMessage::Continue,
        warm: ControlMessage::Continue,
        established: ControlMessage::Continue,
    };
    super::apply_control_close(&mut bundle);
    assert_eq!(bundle.hot, ControlMessage::Terminate);
    assert_eq!(bundle.warm, ControlMessage::Terminate);
    assert_eq!(bundle.established, ControlMessage::Terminate);
}

#[tokio::test]
async fn retire_failed_outbound_peer_marks_registry_and_connection_cold() {
    use super::OutboundPeerManager;

    let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(addr).await;
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    assert!(mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new()));

    let mut registry = PeerRegistry::default();
    registry.insert_source(addr, PeerSource::PeerSourcePublicRoot);
    registry.set_status(addr, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(addr, Some(42));
    let peer_registry = Arc::new(RwLock::new(registry));

    let connection_manager = Arc::new(RwLock::new(ConnectionManagerState::default()));
    {
        let mut cm = connection_manager.write().unwrap();
        let (_result, actions) = cm
            .acquire_outbound_connection(super::outbound_cm_local_addr(), addr)
            .unwrap();
        assert_eq!(actions.len(), 1);
        cm.outbound_handshake_done(super::outbound_cm_local_addr(), addr, DataFlow::Duplex)
            .unwrap();
        assert_eq!(
            cm.abstract_state_of(&addr),
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking)
        );
    }

    let mut governor_state = GovernorState::default();
    assert!(
        retire_failed_outbound_peer(
            &mut mgr,
            &peer_registry,
            &connection_manager,
            &mut governor_state,
            addr,
            "test failure",
            &NodeTracer::disabled(),
        )
        .await
    );

    assert!(!mgr.warm_peers.contains_key(&addr));
    {
        let registry = peer_registry.read().unwrap();
        let entry = registry.get(&addr).unwrap();
        assert_eq!(entry.status, PeerStatus::PeerCold);
        assert_eq!(entry.hot_tip_slot, None);
    }
    let cm = connection_manager.read().unwrap();
    assert_eq!(
        cm.abstract_state_of(&addr),
        AbstractState::UnknownConnectionSt
    );
}

#[tokio::test]
async fn retire_failed_outbound_peer_preserves_bootstrap_hot_marker() {
    use super::OutboundPeerManager;

    let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let mut mgr = OutboundPeerManager::new();
    let session = fake_peer_session_async(addr).await;
    mgr.warm_peers.insert(
        addr,
        super::ManagedWarmPeer::new(session, std::time::Instant::now()),
    );
    assert!(mgr.promote_to_hot(addr, &yggdrasil_network::HotPeerScheduling::new()));

    let mut registry = PeerRegistry::default();
    registry.insert_source(addr, PeerSource::PeerSourceBootstrap);
    registry.set_status(addr, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(addr, Some(42));
    let peer_registry = Arc::new(RwLock::new(registry));

    let connection_manager = Arc::new(RwLock::new(ConnectionManagerState::default()));
    {
        let mut cm = connection_manager.write().unwrap();
        let (_result, actions) = cm
            .acquire_outbound_connection(super::outbound_cm_local_addr(), addr)
            .unwrap();
        assert_eq!(actions.len(), 1);
        cm.outbound_handshake_done(super::outbound_cm_local_addr(), addr, DataFlow::Duplex)
            .unwrap();
    }

    let mut governor_state = GovernorState::default();
    assert!(
        retire_failed_outbound_peer(
            &mut mgr,
            &peer_registry,
            &connection_manager,
            &mut governor_state,
            addr,
            "test failure",
            &NodeTracer::disabled(),
        )
        .await
    );

    assert!(!mgr.warm_peers.contains_key(&addr));
    {
        let registry = peer_registry.read().unwrap();
        let entry = registry.get(&addr).unwrap();
        assert_eq!(entry.status, PeerStatus::PeerHot);
        assert_eq!(entry.hot_tip_slot, None);
        assert!(entry.sources.contains(&PeerSource::PeerSourceBootstrap));
    }
    let cm = connection_manager.read().unwrap();
    assert_eq!(
        cm.abstract_state_of(&addr),
        AbstractState::UnknownConnectionSt
    );
}

#[test]
fn split_timeout_actions_defers_inbound_scoped_actions() {
    let warm_peer: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let inbound_peer: std::net::SocketAddr = "5.6.7.8:3001".parse().unwrap();

    let mut mgr = super::OutboundPeerManager::new();
    mgr.warm_peers.insert(
        warm_peer,
        super::ManagedWarmPeer::new(fake_peer_session(warm_peer), std::time::Instant::now()),
    );

    let warm_conn_id = yggdrasil_network::ConnectionId {
        local: super::outbound_cm_local_addr(),
        remote: warm_peer,
    };
    let inbound_conn_id = yggdrasil_network::ConnectionId {
        local: super::outbound_cm_local_addr(),
        remote: inbound_peer,
    };

    let actions = vec![
        yggdrasil_network::CmAction::PruneConnections(vec![inbound_peer]),
        yggdrasil_network::CmAction::StartResponderTimeout(inbound_conn_id),
        yggdrasil_network::CmAction::TerminateConnection(inbound_conn_id),
        yggdrasil_network::CmAction::TerminateConnection(warm_conn_id),
    ];

    let (applicable, deferred) = super::split_timeout_cm_actions_for_governor(&mgr, actions);

    assert_eq!(deferred, 3);
    assert_eq!(applicable.len(), 1);
    assert!(matches!(
        applicable[0],
        yggdrasil_network::CmAction::TerminateConnection(conn_id) if conn_id.remote == warm_peer
    ));
}

#[test]
fn best_hot_peer_selects_highest_slot() {
    use super::OutboundPeerManager;

    let addr_a: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let addr_b: std::net::SocketAddr = "5.6.7.8:3001".parse().unwrap();

    let mut mgr = OutboundPeerManager::new();

    // Insert two warm peers.
    let sess_a = fake_peer_session(addr_a);
    mgr.warm_peers.insert(
        addr_a,
        super::ManagedWarmPeer::new(sess_a, std::time::Instant::now()),
    );
    let sess_b = fake_peer_session(addr_b);
    mgr.warm_peers.insert(
        addr_b,
        super::ManagedWarmPeer::new(sess_b, std::time::Instant::now()),
    );

    // No hot peers → no best peer.
    assert!(mgr.best_hot_peer().is_none());

    // Promote both to hot.
    mgr.promote_to_hot(addr_a, &yggdrasil_network::HotPeerScheduling::new());
    mgr.promote_to_hot(addr_b, &yggdrasil_network::HotPeerScheduling::new());

    // Still none — no tips cached yet.
    assert!(mgr.best_hot_peer().is_none());

    // Give peer A a higher slot tip.
    mgr.warm_peers.get_mut(&addr_a).unwrap().last_known_tip =
        Some(Point::BlockPoint(SlotNo(200), HeaderHash([0xAA; 32])));
    mgr.warm_peers.get_mut(&addr_b).unwrap().last_known_tip =
        Some(Point::BlockPoint(SlotNo(100), HeaderHash([0xBB; 32])));

    assert_eq!(mgr.best_hot_peer(), Some(addr_a));

    // Switch — peer B gets a higher slot.
    mgr.warm_peers.get_mut(&addr_b).unwrap().last_known_tip =
        Some(Point::BlockPoint(SlotNo(300), HeaderHash([0xCC; 32])));

    assert_eq!(mgr.best_hot_peer(), Some(addr_b));
}

#[test]
fn preferred_hot_peer_from_registry_prefers_highest_tip_slot() {
    let hot_a = local_addr(3101);
    let hot_b = local_addr(3102);
    let mut registry = PeerRegistry::default();

    registry.insert_source(hot_a, PeerSource::PeerSourceBootstrap);
    registry.insert_source(hot_b, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot_a, PeerStatus::PeerHot);
    registry.set_status(hot_b, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(hot_a, Some(100));
    registry.set_hot_tip_slot(hot_b, Some(200));

    let shared = Arc::new(RwLock::new(registry));
    assert_eq!(preferred_hot_peer_from_registry(Some(&shared)), Some(hot_b));
}

#[test]
fn preferred_hot_peer_from_registry_returns_none_without_registry() {
    assert_eq!(preferred_hot_peer_from_registry(None), None);
}

#[test]
fn preferred_hot_peer_handoff_target_prefers_higher_tip_hot_peer() {
    let current = local_addr(3210);
    let better = local_addr(3211);
    let mut registry = PeerRegistry::default();

    registry.insert_source(current, PeerSource::PeerSourceBootstrap);
    registry.insert_source(better, PeerSource::PeerSourceBootstrap);
    registry.set_status(current, PeerStatus::PeerHot);
    registry.set_status(better, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(current, Some(100));
    registry.set_hot_tip_slot(better, Some(200));

    let shared = Arc::new(RwLock::new(registry));
    assert_eq!(
        preferred_hot_peer_handoff_target(Some(&shared), current),
        Some(better)
    );
}

#[test]
fn preferred_hot_peer_handoff_target_ignores_non_improving_peer() {
    let current = local_addr(3212);
    let other = local_addr(3213);
    let mut registry = PeerRegistry::default();

    registry.insert_source(current, PeerSource::PeerSourceBootstrap);
    registry.insert_source(other, PeerSource::PeerSourceBootstrap);
    registry.set_status(current, PeerStatus::PeerHot);
    registry.set_status(other, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(current, Some(300));
    registry.set_hot_tip_slot(other, Some(200));

    let shared = Arc::new(RwLock::new(registry));
    assert_eq!(
        preferred_hot_peer_handoff_target(Some(&shared), current),
        None
    );
}

#[test]
fn reconnect_preferred_peer_prefers_hot_registry_peer_over_previous() {
    let previous = local_addr(3201);
    let hot_peer = local_addr(3202);
    let mut registry = PeerRegistry::default();

    registry.insert_source(hot_peer, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot_peer, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(hot_peer, Some(42));

    let shared = Arc::new(RwLock::new(registry));
    assert_eq!(
        reconnect_preferred_peer(Some(&shared), Some(previous)),
        Some(hot_peer)
    );
}

#[test]
fn reconnect_preferred_peer_falls_back_to_previous_peer() {
    let previous = local_addr(3203);
    assert_eq!(
        reconnect_preferred_peer(None, Some(previous)),
        Some(previous)
    );
}

#[test]
fn reconnect_preferred_peer_returns_none_without_candidates() {
    assert_eq!(reconnect_preferred_peer(None, None), None);
}

#[test]
fn reconnect_preferred_peer_with_source_marks_hot_source() {
    let hot_peer = local_addr(3204);
    let mut registry = PeerRegistry::default();

    registry.insert_source(hot_peer, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot_peer, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(hot_peer, Some(55));

    let shared = Arc::new(RwLock::new(registry));
    assert_eq!(
        reconnect_preferred_peer_with_source(Some(&shared), None),
        Some((hot_peer, "hot"))
    );
}

#[test]
fn reconnect_preferred_peer_with_source_marks_previous_source() {
    let previous = local_addr(3205);
    assert_eq!(
        reconnect_preferred_peer_with_source(None, Some(previous)),
        Some((previous, "previous"))
    );
}

#[test]
fn prepare_reconnect_attempt_state_prefers_hot_peer_over_previous() {
    let primary = local_addr(3301);
    let fallback = local_addr(3302);
    let previous = local_addr(3303);
    let hot = local_addr(3304);

    let mut registry = PeerRegistry::default();
    registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
    registry.insert_source(fallback, PeerSource::PeerSourceBootstrap);
    registry.set_status(hot, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(hot, Some(500));
    let shared = Arc::new(RwLock::new(registry));

    let (attempt_state, preference) =
        prepare_reconnect_attempt_state(primary, &[fallback, hot], Some(&shared), Some(previous));

    assert_eq!(preference, Some((hot, "hot")));
    assert_eq!(attempt_state.preferred_peer(), Some(hot));
}

#[test]
fn prepare_reconnect_attempt_state_uses_previous_without_hot_peer() {
    let primary = local_addr(3305);
    let fallback = local_addr(3306);
    let previous = fallback;

    let (attempt_state, preference) =
        prepare_reconnect_attempt_state(primary, &[fallback], None, Some(previous));

    assert_eq!(preference, Some((previous, "previous")));
    assert_eq!(attempt_state.preferred_peer(), Some(previous));
}

#[test]
fn ordered_reconnect_fallback_peers_prioritizes_ranked_hot_peers() {
    let primary = local_addr(3310);
    let hot_low = local_addr(3311);
    let hot_high = local_addr(3312);
    let cold = local_addr(3313);

    let mut registry = PeerRegistry::default();
    for peer in [hot_low, hot_high, cold] {
        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
    }
    registry.set_status(hot_low, PeerStatus::PeerHot);
    registry.set_status(hot_high, PeerStatus::PeerHot);
    registry.set_hot_tip_slot(hot_low, Some(100));
    registry.set_hot_tip_slot(hot_high, Some(200));

    let shared = Arc::new(RwLock::new(registry));
    let ordered =
        ordered_reconnect_fallback_peers(primary, &[cold, hot_low, hot_high], Some(&shared));

    assert_eq!(ordered, vec![hot_high, hot_low, cold]);
}

/// Build a minimal `PeerSession` for unit tests that don't drive protocols.
fn fake_peer_session(addr: std::net::SocketAddr) -> super::PeerSession {
    // Sync entry point used by `#[test]` callers.  Creates its
    // own current-thread runtime to drive the inner async setup.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(fake_peer_session_async(addr))
}

async fn fake_peer_session_async(addr: std::net::SocketAddr) -> super::PeerSession {
    use yggdrasil_network::multiplexer::MiniProtocolNum;
    use yggdrasil_network::{HandshakeVersion, NodeToNodeVersionData};

    // Async entry point usable from `#[tokio::test]` callers.
    // Reuses the surrounding runtime instead of nesting one.
    // Build a TCP loopback pair and mux it; tests only construct
    // a PeerSession with valid handles, never drive it.
    async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        let client_stream = tokio::net::TcpStream::connect(listen_addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        let protocols = [
            MiniProtocolNum::CHAIN_SYNC,
            MiniProtocolNum::BLOCK_FETCH,
            MiniProtocolNum::KEEP_ALIVE,
            MiniProtocolNum::TX_SUBMISSION,
        ];

        let (mut handles, mux) = yggdrasil_network::mux::start(
            client_stream,
            yggdrasil_network::multiplexer::MiniProtocolDir::Initiator,
            &protocols,
            4096,
        );
        // Also start the server side so the mux doesn't immediately fail.
        let (_server_handles, server_mux) = yggdrasil_network::mux::start(
            server_stream,
            yggdrasil_network::multiplexer::MiniProtocolDir::Responder,
            &protocols,
            4096,
        );

        // Stash server mux so it outlives the construction; it will be
        // cleaned up when tests drop the manager.
        std::mem::forget(server_mux);

        // Extract weight handles before consuming protocol handles.
        let protocol_weights: Vec<(MiniProtocolNum, yggdrasil_network::WeightHandle)> = protocols
            .iter()
            .map(|p| (*p, handles.get(p).unwrap().weight_handle()))
            .collect();

        super::PeerSession {
            connected_peer_addr: addr,
            chain_sync: yggdrasil_network::ChainSyncClient::new(
                handles.remove(&MiniProtocolNum::CHAIN_SYNC).unwrap(),
            ),
            block_fetch: Some(yggdrasil_network::BlockFetchClient::new(
                handles.remove(&MiniProtocolNum::BLOCK_FETCH).unwrap(),
            )),
            keep_alive: yggdrasil_network::KeepAliveClient::new(
                handles.remove(&MiniProtocolNum::KEEP_ALIVE).unwrap(),
            ),
            tx_submission: yggdrasil_network::TxSubmissionClient::new(
                handles.remove(&MiniProtocolNum::TX_SUBMISSION).unwrap(),
            ),
            peer_sharing: None,
            mux,
            version: HandshakeVersion(15),
            version_data: NodeToNodeVersionData {
                network_magic: 764824073,
                initiator_only_diffusion_mode: false,
                peer_sharing: 0,
                query: false,
            },
            protocol_weights,
        }
    }
    .await
}
