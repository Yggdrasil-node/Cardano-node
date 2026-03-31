
#![cfg_attr(test, allow(clippy::unwrap_used))]
/// Yggdrasil node — integration layer wiring consensus, ledger, network,
/// storage, and mempool crates into a running Cardano node.
pub mod block_producer;
pub mod trace_forwarder;

pub mod config;
pub mod genesis;
pub mod local_server;
pub mod plutus_eval;
pub mod runtime;
pub mod server;
pub mod sync;
pub mod tracer;

pub use runtime::{
	ChainTipNotify, MempoolAddTxError, MempoolAddTxResult, NodeConfig, PeerSession,
	ReconnectingSyncServiceOutcome, ReconnectingVerifiedSyncRequest,
	ResumeReconnectingVerifiedSyncRequest, ResumedSyncServiceOutcome,
	RuntimeBlockProducerConfig, RuntimeGovernorConfig, SharedBlockProducerState,
	TxSubmissionServiceError, TxSubmissionServiceOutcome, add_tx_to_mempool,
	add_tx_to_shared_mempool, add_txs_to_mempool, add_txs_to_shared_mempool,
	bootstrap, bootstrap_with_fallbacks, run_txsubmission_service,
	local_root_targets_from_config, run_block_producer_loop,
	run_governor_loop, seed_peer_registry,
	resume_reconnecting_verified_sync_service_chaindb,
	resume_reconnecting_verified_sync_service_shared_chaindb,
	run_reconnecting_verified_sync_service_chaindb,
	run_reconnecting_verified_sync_service,
	resume_reconnecting_verified_sync_service_chaindb_with_tracer,
	resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer,
	run_reconnecting_verified_sync_service_chaindb_with_tracer,
	run_reconnecting_verified_sync_service_with_tracer,
	run_txsubmission_service_shared, serve_txsubmission_request_from_mempool,
	serve_txsubmission_request_from_reader,
};
pub use sync::{
	DecodedSyncStep, FutureBlockCheckConfig, MultiEraBlock, MultiEraSyncProgress, MultiEraSyncStep, SyncError,
	SyncProgress, SyncServiceConfig, SyncServiceOutcome, SyncStep, TypedIntersectResult,
	LedgerCheckpointPolicy, LedgerRecoveryOutcome,
	TypedSyncProgress, TypedSyncStep, VerificationConfig, VerifiedSyncServiceConfig,
	VerifiedSyncServiceOutcome, VrfVerificationParams,
	apply_multi_era_step_to_volatile, apply_nonce_evolution,
	apply_typed_progress_to_volatile, apply_typed_step_to_volatile, decode_multi_era_block,
	decode_multi_era_blocks, decode_point, decode_shelley_blocks, decode_shelley_header,
	evict_confirmed_from_mempool, extract_tx_ids, collect_rolled_back_tx_ids, keepalive_heartbeat,
	multi_era_block_to_block, multi_era_block_to_chain_entry, promote_stable_blocks,
	recover_ledger_state_chaindb,
	run_verified_sync_service_chaindb, track_chain_state, track_chain_state_entries,
	praos_header_body_to_consensus, praos_header_to_consensus, run_sync_service,
	run_verified_sync_service,
	shelley_block_to_block, alonzo_block_to_block, shelley_header_body_to_consensus, shelley_header_to_consensus,
	shelley_opcert_to_consensus, sync_batch_apply, sync_batch_apply_verified, sync_step,
	sync_step_decoded, sync_step_multi_era, sync_step_typed, sync_steps, sync_steps_typed,
	sync_until_typed, typed_find_intersect, verify_block_body_hash, verify_block_vrf,
	verify_block_vrf_with_stake, block_issuer_vkey, block_vrf_vkey,
	block_opcert_sequence_number, validate_block_opcert_counter,
	validate_block_body_size, validate_block_protocol_version,
	verify_multi_era_block,
	verify_praos_header, verify_shelley_header, SHELLEY_KES_DEPTH,
};
pub use tracer::{MetricsSnapshot, NodeMetrics, NodeTracer, trace_fields};

pub use server::{
	BlockProvider, ChainProvider, InboundPeerSession, InboundServiceError,
	PeerSharingProvider, SharedPeerSharingProvider,
	SharedTxSubmissionConsumer, TxSubmissionConsumer,
	SharedChainDb,
	run_blockfetch_server, run_chainsync_server, run_inbound_accept_loop,
	run_keepalive_server, run_peersharing_server,
	run_txsubmission_server,
};

pub use local_server::{
	BasicLocalQueryDispatcher, LocalQueryDispatcher,
	LocalStateQuerySessionError, LocalTxMonitorSessionError,
	LocalTxSubmissionSessionError, LocalServerError,
	run_local_tx_submission_session, run_local_state_query_session,
	run_local_tx_monitor_session,
};
#[cfg(unix)]
pub use local_server::{run_local_client_session, run_local_accept_loop};
