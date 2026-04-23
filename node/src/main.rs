#![cfg_attr(test, allow(clippy::unwrap_used))]
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use clap::{Parser, Subcommand};
use eyre::{Result, WrapErr, bail};
use serde::Serialize;
use serde_json::json;

use yggdrasil_consensus::{
    ActiveSlotCoeff, ClockSkew, DiffusionPipeliningSupport, EpochSize, NonceEvolutionConfig,
    NonceEvolutionState, OcertCounters, SecurityParam, TentativeState,
};
use yggdrasil_ledger::{
    Era, GenesisDelegationState, LedgerState, Nonce, Point, PoolRelayAccessPoint,
    StakeCredential,
};
use yggdrasil_mempool::{SharedMempool, SharedTxState};
use yggdrasil_network::{
    ConnectionManagerState, GovernorState, GovernorTargets, HandshakeVersion, InboundGovernorState,
    LedgerPeerSnapshot, LedgerStateJudgement, NodePeerSharing, PeerAccessPoint, PeerListener,
    merge_ledger_peer_snapshots, resolve_peer_access_points,
};
use yggdrasil_node::config::{
    NetworkPreset, NodeConfigFile, TraceNamespaceConfig, apply_topology_to_config, default_config,
    load_peer_snapshot_file, load_topology_file,
};
use yggdrasil_node::genesis;
use yggdrasil_node::tracer::{NodeMetrics, NodeTracer, trace_fields};
use yggdrasil_node::{
    BlockProvider, ChainProvider, FutureBlockCheckConfig, LedgerCheckpointPolicy, NodeConfig,
    ResumeReconnectingVerifiedSyncRequest, ResumedSyncServiceOutcome, RuntimeGovernorConfig,
    SharedChainDb, SharedPeerSharingProvider, SharedTxSubmissionConsumer, VerificationConfig,
    VerifiedSyncServiceConfig, recover_ledger_state_chaindb,
    resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer, run_block_producer_loop,
    run_governor_loop, run_inbound_accept_loop, seed_peer_registry,
};
use yggdrasil_storage::{
    ChainDb, FileImmutable, FileLedgerStore, FileVolatile, ImmutableStore, LedgerStore,
    VolatileStore,
};

const CHECKPOINT_TRACE_NAMESPACE: &str = "Node.Recovery.Checkpoint";

/// Yggdrasil — a pure Rust Cardano node.
#[derive(Parser)]
#[command(name = "yggdrasil", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Connect to a peer and sync the chain.
    Run {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Peer address (host:port). Overrides config file.
        #[arg(long)]
        peer: Option<SocketAddr>,
        /// Network magic. Overrides config file.
        #[arg(long)]
        network_magic: Option<u32>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
        /// Listen port for inbound node-to-node connections.
        #[arg(long)]
        port: Option<u16>,
        /// Listen host address for inbound node-to-node connections.
        #[arg(long)]
        host_addr: Option<String>,
        /// Disable header verification.
        #[arg(long)]
        no_verify: bool,
        /// Batch size for sync iterations.
        #[arg(long, default_value = "10")]
        batch_size: usize,
        /// Minimum slot delta between persisted ledger checkpoints.
        #[arg(long)]
        checkpoint_interval_slots: Option<u64>,
        /// Maximum number of persisted ledger checkpoints to retain.
        #[arg(long)]
        max_ledger_snapshots: Option<usize>,
        /// Maximum checkpoint trace events emitted per second. Use `0` to disable rate limiting.
        #[arg(long)]
        checkpoint_trace_max_frequency: Option<f64>,
        /// Severity override for checkpoint trace events, for example `Info` or `Silence`.
        #[arg(long)]
        checkpoint_trace_severity: Option<String>,
        /// Backend override for checkpoint trace events. Repeat the flag to route to multiple backends.
        #[arg(long, action = clap::ArgAction::Append)]
        checkpoint_trace_backend: Vec<String>,
        /// Port for Prometheus metrics HTTP endpoint. Disabled when not set.
        #[arg(long)]
        metrics_port: Option<u16>,
        /// Path to the NtC Unix domain socket for local client connections.
        #[arg(long)]
        socket_path: Option<PathBuf>,
        /// Path to the KES signing key file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_kes_key: Option<PathBuf>,
        /// Path to the VRF signing key file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_vrf_key: Option<PathBuf>,
        /// Path to the operational certificate file (text-envelope format).
        /// Required for block production.
        #[arg(long)]
        shelley_operational_certificate: Option<PathBuf>,
        /// Path to the issuer cold verification key file (text-envelope format).
        /// Required for strict external validation parity of forged headers.
        #[arg(long)]
        shelley_operational_certificate_issuer_vkey: Option<PathBuf>,
    },
    /// Validate config, snapshot inputs, and any existing on-disk storage state.
    ValidateConfig {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
    },
    /// Inspect on-disk storage and report current sync status.
    Status {
        /// Path to a JSON or YAML configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Path to a P2P topology file (upstream format). Overrides topology
        /// embedded in the config file.
        #[arg(long)]
        topology: Option<PathBuf>,
        /// Database directory path. Overrides config `storage_dir`.
        #[arg(long)]
        database_path: Option<PathBuf>,
    },
    /// Print the default configuration as JSON.
    DefaultConfig,
    /// Execute selected `cardano-cli` operations via a pure Rust implementation.
    CardanoCli {
        /// Network preset used to resolve reference config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset), default_value = "preprod")]
        network: NetworkPreset,
        /// Root directory for reference configs (contains
        /// `mainnet/`, `preprod/`, `preview/`).
        #[arg(long)]
        upstream_config_root: Option<PathBuf>,
        #[command(subcommand)]
        action: CardanoCliCommand,
    },
    /// Query the running node via the NtC LocalStateQuery protocol.
    #[cfg(unix)]
    Query {
        /// Path to the NtC Unix domain socket of the running node.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Network magic used by the running node.
        #[arg(long, env = "CARDANO_NODE_NETWORK_MAGIC", default_value_t = 764824073)]
        network_magic: u32,
        /// Query tag to execute.
        #[command(subcommand)]
        query: QueryCommand,
    },
    /// Submit a transaction to the running node via the NtC LocalTxSubmission protocol.
    #[cfg(unix)]
    SubmitTx {
        /// Path to the NtC Unix domain socket of the running node.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Network magic used by the running node.
        #[arg(long, env = "CARDANO_NODE_NETWORK_MAGIC", default_value_t = 764824073)]
        network_magic: u32,
        /// Path to a file containing the CBOR-encoded transaction.
        #[arg(long, conflicts_with = "tx_hex")]
        tx_file: Option<PathBuf>,
        /// Hex-encoded CBOR transaction bytes.
        #[arg(long, conflicts_with = "tx_file")]
        tx_hex: Option<String>,
    },
}

/// LocalStateQuery query sub-commands.
#[cfg(unix)]
#[derive(Subcommand, Debug)]
enum QueryCommand {
    /// Query the current era.
    CurrentEra,
    /// Query the chain tip.
    Tip,
    /// Query the current epoch number.
    CurrentEpoch,
    /// Query the current protocol parameters.
    ProtocolParams,
    /// Query the UTxO set for a given address (hex-encoded).
    UtxoByAddress {
        /// Hex-encoded address bytes.
        #[arg(long)]
        address: String,
    },
    /// Query the stake distribution.
    StakeDistribution,
    /// Query the reward balance for a reward account (hex-encoded).
    RewardBalance {
        /// Hex-encoded reward account bytes.
        #[arg(long)]
        account: String,
    },
    /// Query the treasury and reserves.
    TreasuryAndReserves,
    /// Query UTxO entries for specific transaction inputs (hex-encoded CBOR array of TxIn).
    UtxoByTxIn {
        /// Hex-encoded transaction ID (32 bytes).
        #[arg(long)]
        tx_id: String,
        /// Output index within the transaction.
        #[arg(long)]
        index: u16,
    },
    /// Query the set of all registered stake pool IDs.
    StakePools,
    /// Query delegations and reward accounts for a stake credential (hex-encoded 28-byte hash).
    DelegationsAndRewards {
        /// Hex-encoded credential hash (28 bytes).
        #[arg(long)]
        credential: String,
        /// Whether the credential is a key hash (true, default) or script hash (false).
        #[arg(long, default_value = "true")]
        is_key_hash: bool,
    },
    /// Query the DRep stake distribution.
    DrepStakeDistr,
    /// Query the enacted Conway constitution (anchor + guardrails script hash).
    Constitution,
    /// Query pending Conway governance-action state
    /// (all currently-submitted proposals + recorded votes).
    GovState,
    /// Query all registered DReps and their registration metadata.
    DrepState,
    /// Query known Conway constitutional committee members
    /// (cold credential, hot-key status, expiration epoch).
    CommitteeMembersState,
    /// Query the registered stake-pool parameters for a specific pool.
    StakePoolParams {
        /// Hex-encoded pool key hash (28 bytes).
        #[arg(long)]
        pool_hash: String,
    },
    /// Query the ledger accounting state
    /// (treasury, reserves, and total deposit obligation).
    AccountState,
    /// Query the Shelley genesis-delegations map
    /// (genesis key hash → (cold delegate, vrf key) pair).
    GenesisDelegations,
    /// Query the derived `3k/f` chain stability window in slots
    /// (null when not configured).
    StabilityWindow,
    /// Query the consecutive dormant-epoch counter
    /// (Conway-only governance bookkeeping; `0` until a dormant epoch fires).
    NumDormantEpochs,
    /// Query the configured expected reward-account network id
    /// (mainnet = 1, test networks = 0; null when not configured).
    ExpectedNetworkId,
    /// Query the four Conway-era deposit-pot buckets
    /// (key / pool / DRep / proposal deposits, in lovelace).
    DepositPot,
    /// Query aggregate cardinality counters for the major ledger-state
    /// buckets (stake credentials / pools / DReps / committee members /
    /// governance actions / genesis delegates). Cheap monitoring query.
    LedgerCounts,
}

/// cardano-cli actions exposed through the pure Rust command implementation.
#[derive(Subcommand)]
enum CardanoCliCommand {
    /// Print pure-Rust cardano-cli compatibility version info.
    Version,
    /// Show resolved reference config paths and network magic.
    ShowUpstreamConfig,
    /// Query tip against a running node socket.
    QueryTip {
        /// Path to node socket.
        #[arg(long, env = "CARDANO_NODE_SOCKET_PATH")]
        socket_path: PathBuf,
        /// Override network magic instead of using upstream reference config.
        #[arg(long)]
        network_magic: Option<u32>,
    },
}

#[derive(Serialize)]
struct ConfigValidationReport {
    primary_peer: String,
    network_magic: u32,
    protocol_versions: Vec<u32>,
    storage_dir: String,
    configured_fallback_peer_count: usize,
    resolved_startup_peer_count: usize,
    use_ledger_peers: String,
    checkpoint_interval_slots: u64,
    max_ledger_snapshots: usize,
    peer_snapshot: PeerSnapshotValidationReport,
    storage: StorageValidationReport,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct PeerSnapshotValidationReport {
    status: &'static str,
    path: Option<String>,
    slot: Option<u64>,
    ledger_peer_count: usize,
    big_ledger_peer_count: usize,
    error: Option<String>,
}

#[derive(Serialize)]
struct StorageValidationReport {
    status: &'static str,
    tip: String,
    recovered_point: Option<String>,
    checkpoint_slot: Option<u64>,
    replayed_volatile_blocks: Option<usize>,
    ledger_peer_count: usize,
}

// ---------------------------------------------------------------------------
// NtC client helpers — query and submit-tx subcommands
// ---------------------------------------------------------------------------

/// Connect to the running node's NtC Unix socket and execute a
/// LocalStateQuery request, printing the result as JSON.
///
/// Reference: `cardano-cli query` commands against
/// `ouroboros-network-protocols` LocalStateQuery.
#[cfg(unix)]
async fn run_query(socket_path: PathBuf, network_magic: u32, query: QueryCommand) -> Result<()> {
    use yggdrasil_network::{
        AcquireTarget, LocalStateQueryClient, MiniProtocolNum, ntc_connect,
    };

    let mut conn = ntc_connect(&socket_path, network_magic, true)
        .await
        .wrap_err_with(|| format!("failed to connect to NtC socket {}", socket_path.display()))?;

    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");
    let mut client = LocalStateQueryClient::new(sq_handle);

    // Acquire at the volatile tip — always available on a running node.
    client
        .acquire(AcquireTarget::VolatileTip)
        .await
        .wrap_err("LocalStateQuery acquire failed")?;

    // Encode the query as CBOR [tag] or [tag, param].
    let query_bytes = encode_ntc_query(&query);

    let result = client
        .query(query_bytes)
        .await
        .wrap_err("LocalStateQuery query failed")?;

    // Decode the result according to the known response format.
    let json_val = decode_ntc_result(&query, &result)?;
    println!("{}", serde_json::to_string_pretty(&json_val)?);

    let _ = client.release().await;
    let _ = client.done().await;
    Ok(())
}

/// Encode a [`QueryCommand`] as a CBOR `[tag, ...]` byte vector matching
/// the format expected by [`BasicLocalQueryDispatcher`].
#[cfg(unix)]
fn encode_ntc_query(query: &QueryCommand) -> Vec<u8> {
    use yggdrasil_ledger::Encoder;
    let mut enc = Encoder::new();
    match query {
        QueryCommand::CurrentEra => {
            enc.array(1).unsigned(0u64);
        }
        QueryCommand::Tip => {
            enc.array(1).unsigned(1u64);
        }
        QueryCommand::CurrentEpoch => {
            enc.array(1).unsigned(2u64);
        }
        QueryCommand::ProtocolParams => {
            enc.array(1).unsigned(3u64);
        }
        QueryCommand::UtxoByAddress { address } => {
            let addr_bytes = hex::decode(address.trim()).unwrap_or_default();
            enc.array(2).unsigned(4u64).bytes(&addr_bytes);
        }
        QueryCommand::StakeDistribution => {
            enc.array(1).unsigned(5u64);
        }
        QueryCommand::RewardBalance { account } => {
            let acct_bytes = hex::decode(account.trim()).unwrap_or_default();
            enc.array(2).unsigned(6u64).bytes(&acct_bytes);
        }
        QueryCommand::TreasuryAndReserves => {
            enc.array(1).unsigned(7u64);
        }
        QueryCommand::UtxoByTxIn { tx_id, index } => {
            let tx_id_bytes = hex::decode(tx_id.trim()).unwrap_or_default();
            enc.array(2).unsigned(14u64);
            // Encode as a single-element array of TxIn: [[tx_id_bytes, index]]
            enc.array(1);
            enc.array(2).bytes(&tx_id_bytes).unsigned(*index as u64);
        }
        QueryCommand::StakePools => {
            enc.array(1).unsigned(15u64);
        }
        QueryCommand::DelegationsAndRewards {
            credential,
            is_key_hash,
        } => {
            let cred_bytes = hex::decode(credential.trim()).unwrap_or_default();
            enc.array(2).unsigned(16u64);
            // Encode as a single-element credential array: [[tag, hash]]
            enc.array(1);
            enc.array(2);
            if *is_key_hash {
                enc.unsigned(0u64);
            } else {
                enc.unsigned(1u64);
            }
            enc.bytes(&cred_bytes);
        }
        QueryCommand::DrepStakeDistr => {
            enc.array(1).unsigned(17u64);
        }
        QueryCommand::Constitution => {
            enc.array(1).unsigned(8u64);
        }
        QueryCommand::GovState => {
            enc.array(1).unsigned(9u64);
        }
        QueryCommand::DrepState => {
            enc.array(1).unsigned(10u64);
        }
        QueryCommand::CommitteeMembersState => {
            enc.array(1).unsigned(11u64);
        }
        QueryCommand::StakePoolParams { pool_hash } => {
            let pool_bytes = hex::decode(pool_hash.trim()).unwrap_or_default();
            enc.array(2).unsigned(12u64).bytes(&pool_bytes);
        }
        QueryCommand::AccountState => {
            enc.array(1).unsigned(13u64);
        }
        QueryCommand::GenesisDelegations => {
            enc.array(1).unsigned(18u64);
        }
        QueryCommand::StabilityWindow => {
            enc.array(1).unsigned(19u64);
        }
        QueryCommand::NumDormantEpochs => {
            enc.array(1).unsigned(20u64);
        }
        QueryCommand::ExpectedNetworkId => {
            enc.array(1).unsigned(21u64);
        }
        QueryCommand::DepositPot => {
            enc.array(1).unsigned(22u64);
        }
        QueryCommand::LedgerCounts => {
            enc.array(1).unsigned(23u64);
        }
    }
    enc.into_bytes()
}

/// Decode a raw CBOR result from the node into a `serde_json::Value` suitable
/// for pretty-printing.
#[cfg(unix)]
fn decode_ntc_result(query: &QueryCommand, result: &[u8]) -> Result<serde_json::Value> {
    use yggdrasil_ledger::Decoder;
    let val = match query {
        QueryCommand::CurrentEra => {
            let mut dec = Decoder::new(result);
            let era = dec.unsigned().unwrap_or(0);
            json!({"era": era})
        }
        QueryCommand::Tip => {
            // Decode chain tip point: Origin = [] or point = [slot, hash].
            let mut dec = Decoder::new(result);
            match dec.array() {
                Ok(0) => json!({"tip": {"origin": true}}),
                Ok(2) => {
                    let slot = dec.unsigned().unwrap_or(0);
                    let hash = dec.bytes().unwrap_or_default();
                    json!({"tip": {"origin": false, "slot": slot, "hash": hex::encode(hash)}})
                }
                _ => json!({"tip_cbor": hex::encode(result)}),
            }
        }
        QueryCommand::CurrentEpoch => {
            let mut dec = Decoder::new(result);
            let epoch = dec.unsigned().unwrap_or(0);
            json!({"epoch": epoch})
        }
        QueryCommand::ProtocolParams => {
            // CBOR null (0xf6) means no parameters available yet.
            if result == [0xf6] {
                json!({"protocol_parameters": null})
            } else {
                json!({"protocol_parameters": hex::encode(result)})
            }
        }
        QueryCommand::UtxoByAddress { .. } => {
            json!({"utxo_cbor": hex::encode(result)})
        }
        QueryCommand::StakeDistribution => {
            json!({"stake_distribution_cbor": hex::encode(result)})
        }
        QueryCommand::RewardBalance { .. } => {
            let mut dec = Decoder::new(result);
            let balance = dec.unsigned().unwrap_or(0);
            json!({"reward_balance_lovelace": balance})
        }
        QueryCommand::TreasuryAndReserves => {
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(2) {
                let treasury = dec.unsigned().unwrap_or(0);
                let reserves = dec.unsigned().unwrap_or(0);
                json!({"treasury_lovelace": treasury, "reserves_lovelace": reserves})
            } else {
                json!({"result_cbor": hex::encode(result)})
            }
        }
        QueryCommand::UtxoByTxIn { .. } => {
            json!({"utxo_cbor": hex::encode(result)})
        }
        QueryCommand::StakePools => {
            // Decode CBOR array of pool hashes, convert to hex strings.
            let mut dec = Decoder::new(result);
            let mut pools = Vec::new();
            if let Ok(n) = dec.array() {
                for _ in 0..n {
                    if let Ok(hash_bytes) = dec.bytes() {
                        pools.push(hex::encode(hash_bytes));
                    }
                }
            }
            json!({"stake_pools": pools, "count": pools.len()})
        }
        QueryCommand::DelegationsAndRewards { .. } => {
            json!({"delegations_and_rewards_cbor": hex::encode(result)})
        }
        QueryCommand::DrepStakeDistr => {
            json!({"drep_stake_distribution_cbor": hex::encode(result)})
        }
        QueryCommand::Constitution => {
            // Complex Conway type — surface raw CBOR so clients can decode.
            json!({"constitution_cbor": hex::encode(result)})
        }
        QueryCommand::GovState => {
            json!({"governance_actions_cbor": hex::encode(result)})
        }
        QueryCommand::DrepState => {
            json!({"drep_state_cbor": hex::encode(result)})
        }
        QueryCommand::CommitteeMembersState => {
            json!({"committee_state_cbor": hex::encode(result)})
        }
        QueryCommand::StakePoolParams { .. } => {
            // Server returns CBOR-encoded RegisteredPool or CBOR null when
            // the pool is not registered.
            if result == [0xf6] {
                json!({"pool": null})
            } else {
                json!({"pool_cbor": hex::encode(result)})
            }
        }
        QueryCommand::AccountState => {
            // Server returns `[treasury, reserves, total_deposits]`.
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(3) {
                let treasury = dec.unsigned().unwrap_or(0);
                let reserves = dec.unsigned().unwrap_or(0);
                let deposits = dec.unsigned().unwrap_or(0);
                json!({
                    "treasury_lovelace": treasury,
                    "reserves_lovelace": reserves,
                    "total_deposits_lovelace": deposits,
                })
            } else {
                json!({"account_state_cbor": hex::encode(result)})
            }
        }
        QueryCommand::GenesisDelegations => {
            // CBOR map keyed by genesis hash; surface raw for now since
            // the value side carries multiple sub-entries per key.
            json!({"genesis_delegations_cbor": hex::encode(result)})
        }
        QueryCommand::StabilityWindow => {
            // CBOR null (0xf6) when unset; otherwise plain unsigned u64.
            if result == [0xf6] {
                json!({"stability_window": null})
            } else {
                let mut dec = Decoder::new(result);
                let w = dec.unsigned().unwrap_or(0);
                json!({"stability_window_slots": w})
            }
        }
        QueryCommand::NumDormantEpochs => {
            let mut dec = Decoder::new(result);
            let n = dec.unsigned().unwrap_or(0);
            json!({"num_dormant_epochs": n})
        }
        QueryCommand::ExpectedNetworkId => {
            // CBOR null (0xf6) means no expectation is configured;
            // otherwise the server returns a plain CBOR unsigned (u8 range).
            if result == [0xf6] {
                json!({"expected_network_id": null})
            } else {
                let mut dec = Decoder::new(result);
                let id = dec.unsigned().unwrap_or(0);
                json!({"expected_network_id": id})
            }
        }
        QueryCommand::DepositPot => {
            // 4-element CBOR array [key, pool, drep, proposal].
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(4) {
                let key_deposits = dec.unsigned().unwrap_or(0);
                let pool_deposits = dec.unsigned().unwrap_or(0);
                let drep_deposits = dec.unsigned().unwrap_or(0);
                let proposal_deposits = dec.unsigned().unwrap_or(0);
                json!({
                    "key_deposits_lovelace": key_deposits,
                    "pool_deposits_lovelace": pool_deposits,
                    "drep_deposits_lovelace": drep_deposits,
                    "proposal_deposits_lovelace": proposal_deposits,
                    "total_lovelace":
                        key_deposits + pool_deposits + drep_deposits + proposal_deposits,
                })
            } else {
                json!({"deposit_pot_cbor": hex::encode(result)})
            }
        }
        QueryCommand::LedgerCounts => {
            // 6-element CBOR array of cardinality counters.
            let mut dec = Decoder::new(result);
            if dec.array().ok() == Some(6) {
                let stake_credentials = dec.unsigned().unwrap_or(0);
                let pools = dec.unsigned().unwrap_or(0);
                let dreps = dec.unsigned().unwrap_or(0);
                let committee_members = dec.unsigned().unwrap_or(0);
                let governance_actions = dec.unsigned().unwrap_or(0);
                let gen_delegs = dec.unsigned().unwrap_or(0);
                json!({
                    "stake_credentials": stake_credentials,
                    "pools": pools,
                    "dreps": dreps,
                    "committee_members": committee_members,
                    "governance_actions": governance_actions,
                    "gen_delegs": gen_delegs,
                })
            } else {
                json!({"ledger_counts_cbor": hex::encode(result)})
            }
        }
    };
    Ok(val)
}

/// Connect to the running node's NtC Unix socket and submit a transaction
/// via the LocalTxSubmission protocol, printing the accept/reject outcome.
///
/// Reference: `cardano-cli transaction submit` against
/// `ouroboros-network-protocols` LocalTxSubmission.
#[cfg(unix)]
async fn run_submit_tx(socket_path: PathBuf, network_magic: u32, tx_bytes: Vec<u8>) -> Result<()> {
    use yggdrasil_network::{LocalTxSubmissionClient, MiniProtocolNum, ntc_connect};

    let mut conn = ntc_connect(&socket_path, network_magic, false)
        .await
        .wrap_err_with(|| format!("failed to connect to NtC socket {}", socket_path.display()))?;

    let tx_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .expect("NTC_LOCAL_TX_SUBMISSION handle missing");
    let mut client = LocalTxSubmissionClient::new(tx_handle);

    match client.submit(tx_bytes).await {
        Ok(()) => {
            let result = json!({"result": "accepted"});
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Err(e) => {
            let result = json!({"result": "rejected", "reason": e.to_string()});
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    let _ = client.done().await;
    Ok(())
}

/// Resolve upstream reference config paths for a given network preset.
///
/// Defaults to `/tmp/cardano-tooling/share` (layout from official release
/// tarballs), and falls back to vendored `node/configuration` when that
/// root does not contain the requested network directory.
fn resolve_upstream_reference_paths(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let network_dir = match network {
        NetworkPreset::Mainnet => "mainnet",
        NetworkPreset::Preprod => "preprod",
        NetworkPreset::Preview => "preview",
    };

    let mut root = upstream_config_root.unwrap_or_else(|| PathBuf::from("/tmp/cardano-tooling/share"));
    if !root.join(network_dir).is_dir() {
        root = PathBuf::from("node/configuration");
    }

    let config_path = root.join(network_dir).join("config.json");
    let topology_path = root.join(network_dir).join("topology.json");

    if !config_path.is_file() {
        bail!(
            "upstream reference config not found: {}",
            config_path.display()
        );
    }

    Ok((config_path, topology_path))
}

fn extract_reference_network_magic(config_path: &std::path::Path, network: NetworkPreset) -> u32 {
    let fallback_magic = network.to_config().network_magic;

    let config_json = std::fs::read(config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());

    if let Some(magic) = config_json
        .as_ref()
        .and_then(|v| v.get("TestnetMagic"))
        .and_then(|v| v.as_u64())
    {
        return magic as u32;
    }

    if let Some(magic) = config_json
        .as_ref()
        .and_then(|v| v.get("NetworkMagic"))
        .and_then(|v| v.as_u64())
    {
        return magic as u32;
    }

    let genesis_path = config_json
        .as_ref()
        .and_then(|v| v.get("ShelleyGenesisFile"))
        .and_then(|v| v.as_str())
        .map(|name| config_path.parent().unwrap_or_else(|| std::path::Path::new(".")).join(name));

    if let Some(path) = genesis_path {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(magic) = v.get("networkMagic").and_then(|n| n.as_u64()) {
                    return magic as u32;
                }
            }
        }
    }

    fallback_magic
}

/// Run selected cardano-cli operations from the pure Rust CLI implementation.
fn run_cardano_cli_command(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
    action: CardanoCliCommand,
) -> Result<()> {
    let (config_path, topology_path) = resolve_upstream_reference_paths(network, upstream_config_root)?;
    let reference_network_magic = extract_reference_network_magic(&config_path, network);

    match action {
        CardanoCliCommand::Version => {
            println!("yggdrasil-cardano-cli (pure-rust) {}", env!("CARGO_PKG_VERSION"));
            println!("network preset default: {}", network);
            Ok(())
        }
        CardanoCliCommand::ShowUpstreamConfig => {
            let out = json!({
                "network": network.to_string(),
                "config": config_path,
                "topology": topology_path,
                "network_magic": reference_network_magic,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        CardanoCliCommand::QueryTip {
            socket_path,
            network_magic,
        } => {
            let magic = network_magic.unwrap_or(reference_network_magic);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_query(socket_path, magic, QueryCommand::Tip))
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::DefaultConfig => {
            let cfg = default_config();
            let json = serde_json::to_string_pretty(&cfg)?;
            println!("{json}");
            Ok(())
        }
        Command::CardanoCli {
            network,
            upstream_config_root,
            action,
        } => run_cardano_cli_command(network, upstream_config_root, action),
        Command::ValidateConfig {
            config,
            network,
            topology,
            database_path,
        } => {
            let (mut file_cfg, config_base_dir) = load_effective_config(config, network)?;
            apply_topology_override(
                &mut file_cfg,
                topology.as_deref(),
                config_base_dir.as_deref(),
            )?;
            if let Some(ref db_path) = database_path {
                file_cfg.storage_dir = db_path.clone();
            }
            let report = validate_config_report(&file_cfg, config_base_dir.as_deref())?;
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
            Ok(())
        }
        Command::Status {
            config,
            network,
            topology,
            database_path,
        } => {
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
        Command::Run {
            config,
            network,
            topology,
            peer,
            network_magic,
            database_path,
            port,
            host_addr,
            no_verify,
            batch_size,
            checkpoint_interval_slots,
            max_ledger_snapshots,
            checkpoint_trace_max_frequency,
            checkpoint_trace_severity,
            checkpoint_trace_backend,
            metrics_port,
            socket_path,
            shelley_kes_key,
            shelley_vrf_key,
            shelley_operational_certificate,
            shelley_operational_certificate_issuer_vkey,
        } => {
            let (mut file_cfg, config_base_dir) = load_effective_config(config, network)?;
            apply_topology_override(
                &mut file_cfg,
                topology.as_deref(),
                config_base_dir.as_deref(),
            )?;

            // CLI --database-path overrides config file storage_dir.
            if let Some(ref db_path) = database_path {
                file_cfg.storage_dir = db_path.clone();
            }

            // CLI --port and --host-addr override inbound listen address.
            if port.is_some() || host_addr.is_some() {
                let listen_ip: std::net::IpAddr = host_addr
                    .as_deref()
                    .unwrap_or("0.0.0.0")
                    .parse()
                    .wrap_err("invalid --host-addr")?;
                let listen_port = port.unwrap_or(3001);
                file_cfg.inbound_listen_addr = Some(SocketAddr::new(listen_ip, listen_port));
            }

            // CLI --socket-path overrides config file SocketPath.
            if let Some(ref sp) = socket_path {
                file_cfg.socket_path = Some(sp.display().to_string());
            }

            // CLI --shelley-kes-key / --shelley-vrf-key /
            // --shelley-operational-certificate /
            // --shelley-operational-certificate-issuer-vkey override config
            // file block producer credential paths.
            if let Some(ref p) = shelley_kes_key {
                file_cfg.shelley_kes_key = Some(p.display().to_string());
            }
            if let Some(ref p) = shelley_vrf_key {
                file_cfg.shelley_vrf_key = Some(p.display().to_string());
            }
            if let Some(ref p) = shelley_operational_certificate {
                file_cfg.shelley_operational_certificate = Some(p.display().to_string());
            }
            if let Some(ref p) = shelley_operational_certificate_issuer_vkey {
                file_cfg.shelley_operational_certificate_issuer_vkey =
                    Some(p.display().to_string());
            }

            if let Some(max_frequency) = checkpoint_trace_max_frequency {
                checkpoint_trace_config_mut(&mut file_cfg).max_frequency = if max_frequency > 0.0 {
                    Some(max_frequency)
                } else {
                    None
                };
            }

            if let Some(severity) = checkpoint_trace_severity {
                checkpoint_trace_config_mut(&mut file_cfg).severity = Some(severity);
            }

            if !checkpoint_trace_backend.is_empty() {
                checkpoint_trace_config_mut(&mut file_cfg).backends = checkpoint_trace_backend;
            }

            let magic = network_magic.unwrap_or(file_cfg.network_magic);
            let protocol_versions: Vec<HandshakeVersion> = file_cfg
                .protocol_versions
                .iter()
                .map(|v| HandshakeVersion(*v as u16))
                .collect();
            let plutus_cost_model = file_cfg
                .load_plutus_cost_model(config_base_dir.as_deref())
                .wrap_err("failed to load genesis Plutus cost model")?;

            // Load the slot length and system start from shelley genesis for the
            // block producer's slot clock and the blocks-from-the-future check.
            // Falls back to 1.0 s slot length when the genesis file is missing.
            let shelley_genesis: Option<genesis::ShelleyGenesis> =
                file_cfg.shelley_genesis_file.as_deref().and_then(|path| {
                    let full_path = if let Some(base) = config_base_dir.as_deref() {
                        base.join(std::path::Path::new(path))
                    } else {
                        std::path::PathBuf::from(path)
                    };
                    genesis::load_shelley_genesis(&full_path).ok()
                });
            let genesis_slot_length: Option<f64> = shelley_genesis.as_ref().map(|g| g.slot_length);
            let genesis_system_start_unix_secs: Option<f64> = shelley_genesis
                .as_ref()
                .and_then(|g| g.system_start.as_deref())
                .and_then(genesis::chrono_parse_system_start);

            // Compute FutureBlockCheckConfig from genesis `system_start` and
            // slot length. The wall slot is derived dynamically per check.
            // Reference: `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`
            let future_check: Option<FutureBlockCheckConfig> = shelley_genesis
                .as_ref()
                .and_then(|g| g.system_start.as_deref())
                .and_then(|start| {
                    let slot_len = genesis_slot_length.unwrap_or(1.0);
                    let system_start_unix_secs = genesis::chrono_parse_system_start(start)?;
                    let clock_skew = ClockSkew::default_for_slot_length(
                        std::time::Duration::from_secs_f64(slot_len),
                    );
                    Some(FutureBlockCheckConfig {
                        system_start_unix_secs,
                        slot_length_secs: slot_len,
                        clock_skew,
                    })
                });

            let verification = if no_verify {
                None
            } else {
                Some(VerificationConfig {
                    slots_per_kes_period: file_cfg.slots_per_kes_period,
                    max_kes_evolutions: file_cfg.max_kes_evolutions,
                    verify_body_hash: true,
                    max_major_protocol_version: Some(file_cfg.max_major_protocol_version),
                    future_check,
                    ocert_counters: Some(OcertCounters::new()),
                    pp_major_protocol_version: None,
                })
            };

            let nonce_config = NonceEvolutionConfig {
                epoch_size: EpochSize(file_cfg.epoch_length),
                // stability_window = 3k/f
                stability_window: (3.0 * file_cfg.security_param_k as f64
                    / file_cfg.active_slot_coeff) as u64,
                extra_entropy: Nonce::Neutral,
            };

            let security_param = SecurityParam(file_cfg.security_param_k);
            let checkpoint_interval_slots =
                checkpoint_interval_slots.unwrap_or(file_cfg.checkpoint_interval_slots);
            let max_ledger_snapshots =
                max_ledger_snapshots.unwrap_or(file_cfg.max_ledger_snapshots);

            let active_slot_coeff = ActiveSlotCoeff::new(file_cfg.active_slot_coeff).ok();

            let sync_config = if let Some(verification) = verification {
                VerifiedSyncServiceConfig {
                    batch_size,
                    verification,
                    nonce_config: Some(nonce_config),
                    security_param: Some(security_param),
                    checkpoint_policy: LedgerCheckpointPolicy {
                        min_slot_delta: checkpoint_interval_slots,
                        max_snapshots: max_ledger_snapshots,
                    },
                    plutus_cost_model: plutus_cost_model.clone(),
                    verify_vrf: active_slot_coeff.is_some(),
                    active_slot_coeff: active_slot_coeff.clone(),
                    slot_length_secs: genesis_slot_length,
                    system_start_unix_secs: genesis_system_start_unix_secs,
                    epoch_schedule: Some(file_cfg.epoch_schedule()),
                    block_fetch_pool: None,
                }
            } else {
                VerifiedSyncServiceConfig {
                    batch_size,
                    verification: VerificationConfig {
                        slots_per_kes_period: file_cfg.slots_per_kes_period,
                        max_kes_evolutions: file_cfg.max_kes_evolutions,
                        verify_body_hash: false,
                        max_major_protocol_version: Some(file_cfg.max_major_protocol_version),
                        future_check,
                        ocert_counters: Some(OcertCounters::new()),
                        pp_major_protocol_version: None,
                    },
                    nonce_config: Some(nonce_config),
                    security_param: Some(security_param),
                    checkpoint_policy: LedgerCheckpointPolicy {
                        min_slot_delta: checkpoint_interval_slots,
                        max_snapshots: max_ledger_snapshots,
                    },
                    plutus_cost_model: plutus_cost_model.clone(),
                    verify_vrf: active_slot_coeff.is_some(),
                    active_slot_coeff,
                    slot_length_secs: genesis_slot_length,
                    system_start_unix_secs: genesis_system_start_unix_secs,
                    epoch_schedule: Some(file_cfg.epoch_schedule()),
                    block_fetch_pool: None,
                }
            };

            let tracer = NodeTracer::from_config(&file_cfg);
            let storage_dir =
                resolve_storage_dir(&file_cfg.storage_dir, config_base_dir.as_deref());
            let base_ledger_state =
                strict_base_ledger_state(&file_cfg, config_base_dir.as_deref())?;

            // Positive audit-trail trace for the genesis-hash integrity
            // check. `strict_base_ledger_state` bails on mismatch before
            // returning `Ok`, so reaching this point means every declared
            // `*GenesisHash` matched the file on disk. Surfacing this in
            // the log gives operators confirmation that the integrity
            // check actually ran, alongside the count of verified pairs.
            trace_genesis_hashes_verified(&tracer, &file_cfg);
            let chain_db = ChainDb::new(
                FileImmutable::open(storage_dir.join("immutable"))?,
                FileVolatile::open(storage_dir.join("volatile"))?,
                FileLedgerStore::open(storage_dir.join("ledger"))?,
            );

            let peer_addr = peer.unwrap_or(file_cfg.peer_addr);
            let recovery = recover_ledger_state_chaindb(&chain_db, base_ledger_state.clone());
            let latest_slot = recovery
                .as_ref()
                .ok()
                .and_then(|recovery| point_slot(&recovery.point))
                .or_else(|| point_slot(&chain_db.recovery().tip));
            let ledger_state_judgement = if recovery.is_ok() {
                LedgerStateJudgement::YoungEnough
            } else {
                LedgerStateJudgement::Unavailable
            };
            let ledger_snapshot = recovery
                .as_ref()
                .map(|recovery| ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state))
                .unwrap_or_default();
            let peer_snapshot_path = file_cfg.peer_snapshot_file.as_deref().map(|path| {
                resolve_config_path(std::path::Path::new(path), config_base_dir.as_deref())
            });

            if let Err(err) = &recovery {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to recover ledger state for startup ledger peers",
                    trace_fields([
                        ("latestSlot", json!(latest_slot)),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }

            let bootstrap_peers = if peer.is_some() {
                Vec::new()
            } else {
                configured_fallback_peers(
                    &file_cfg,
                    config_base_dir.as_deref(),
                    &ledger_snapshot,
                    latest_slot,
                    ledger_state_judgement,
                    &tracer,
                )
            };

            let node_config = NodeConfig {
                peer_addr,
                network_magic: magic,
                protocol_versions,
                peer_sharing: file_cfg.peer_sharing,
            };

            let governor_config = RuntimeGovernorConfig::new(
                std::time::Duration::from_secs(file_cfg.governor_tick_interval_secs),
                file_cfg
                    .keepalive_interval_secs
                    .map(std::time::Duration::from_secs),
                NodePeerSharing::from_wire(file_cfg.peer_sharing),
                file_cfg.consensus_mode.to_network_mode(),
                GovernorTargets {
                    target_known: file_cfg.governor_target_known,
                    target_established: file_cfg.governor_target_established,
                    target_active: file_cfg.governor_target_active,
                    target_known_big_ledger: file_cfg.governor_target_known_big_ledger,
                    target_established_big_ledger: file_cfg.governor_target_established_big_ledger,
                    target_active_big_ledger: file_cfg.governor_target_active_big_ledger,
                    ..Default::default()
                },
            );

            let mut topology_config = file_cfg.topology_config();
            if let Some(peer_snapshot_path) = &peer_snapshot_path {
                topology_config.peer_snapshot_file = Some(peer_snapshot_path.display().to_string());
            }

            // Load block producer credentials when all required paths are present.
            let has_any_block_producer_path = file_cfg.shelley_kes_key.is_some()
                || file_cfg.shelley_vrf_key.is_some()
                || file_cfg.shelley_operational_certificate.is_some()
                || file_cfg
                    .shelley_operational_certificate_issuer_vkey
                    .is_some();

            let has_all_block_producer_paths = file_cfg.shelley_kes_key.is_some()
                && file_cfg.shelley_vrf_key.is_some()
                && file_cfg.shelley_operational_certificate.is_some()
                && file_cfg
                    .shelley_operational_certificate_issuer_vkey
                    .is_some();

            if has_any_block_producer_path && !has_all_block_producer_paths {
                bail!(
                    "block producer credentials are partially configured; \
                     required: ShelleyKesKey, ShelleyVrfKey, \
                     ShelleyOperationalCertificate, \
                     ShelleyOperationalCertificateIssuerVkey"
                );
            }

            let block_producer_credentials = if has_all_block_producer_paths {
                let creds = yggdrasil_node::block_producer::load_block_producer_credentials(
                    &resolve_config_path(
                        std::path::Path::new(
                            file_cfg
                                .shelley_kes_key
                                .as_ref()
                                .expect("shelley_kes_key is checked as present above"),
                        ),
                        config_base_dir.as_deref(),
                    ),
                    &resolve_config_path(
                        std::path::Path::new(
                            file_cfg
                                .shelley_vrf_key
                                .as_ref()
                                .expect("shelley_vrf_key is checked as present above"),
                        ),
                        config_base_dir.as_deref(),
                    ),
                    &resolve_config_path(
                        std::path::Path::new(
                            file_cfg
                                .shelley_operational_certificate
                                .as_ref()
                                .expect("shelley_operational_certificate is checked as present above"),
                        ),
                        config_base_dir.as_deref(),
                    ),
                    &resolve_config_path(
                        std::path::Path::new(
                            file_cfg
                                .shelley_operational_certificate_issuer_vkey
                                .as_ref()
                                .expect("shelley_operational_certificate_issuer_vkey is checked as present above"),
                        ),
                        config_base_dir.as_deref(),
                    ),
                    file_cfg.slots_per_kes_period,
                    file_cfg.max_kes_evolutions,
                ).wrap_err("failed to load block producer credentials")?;
                Some(creds)
            } else {
                None
            };

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_node(RunNodeRequest {
                node_config,
                bootstrap_peers,
                sync_config,
                governor_config,
                topology_config,
                tracer,
                storage_dir,
                chain_db,
                inbound_listen_addr: file_cfg.inbound_listen_addr,
                use_ledger_peers: Some(file_cfg.use_ledger_peers_policy()),
                peer_snapshot_path,
                metrics_port,
                base_ledger_state,
                socket_path: file_cfg.socket_path.map(PathBuf::from),
                block_producer_credentials,
                max_major_protocol_version: file_cfg.max_major_protocol_version,
            }))
        }
        #[cfg(unix)]
        Command::Query {
            socket_path,
            network_magic,
            query,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_query(socket_path, network_magic, query))
        }
        #[cfg(unix)]
        Command::SubmitTx {
            socket_path,
            network_magic,
            tx_file,
            tx_hex,
        } => {
            let tx_bytes = match (tx_file, tx_hex) {
                (Some(path), _) => std::fs::read(&path)
                    .wrap_err_with(|| format!("failed to read tx file {}", path.display()))?,
                (_, Some(hex)) => {
                    let hex = hex.trim();
                    let hex = hex.strip_prefix("0x").unwrap_or(hex);
                    hex::decode(hex).wrap_err("invalid hex in --tx-hex")?
                }
                (None, None) => bail!("one of --tx-file or --tx-hex is required"),
            };
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_submit_tx(socket_path, network_magic, tx_bytes))
        }
    }
}

/// Emit a positive-path `Node.GenesisHash.Verified` trace event after
/// `strict_base_ledger_state` has completed genesis-hash verification.
///
/// Each `(file, hash)` pair is reported so operators can confirm exactly
/// which files were checked. `byron` is counted only when an expected hash
/// is declared — Byron hash verification is still a follow-up slice
/// (requires canonical CBOR), so a present expectation is currently a
/// "declared but not yet verified" nuance we surface separately.
fn trace_genesis_hashes_verified(tracer: &NodeTracer, file_cfg: &NodeConfigFile) {
    let shelley_verified = file_cfg.shelley_genesis_file.is_some()
        && file_cfg.shelley_genesis_hash.is_some();
    let alonzo_verified = file_cfg.alonzo_genesis_file.is_some()
        && file_cfg.alonzo_genesis_hash.is_some();
    let conway_verified = file_cfg.conway_genesis_file.is_some()
        && file_cfg.conway_genesis_hash.is_some();
    let byron_declared = file_cfg.byron_genesis_file.is_some()
        && file_cfg.byron_genesis_hash.is_some();
    let verified_count = u64::from(shelley_verified)
        + u64::from(alonzo_verified)
        + u64::from(conway_verified);

    tracer.trace_runtime(
        "Node.GenesisHash.Verified",
        "Notice",
        "genesis hash integrity check passed",
        trace_fields([
            ("shelleyVerified", json!(shelley_verified)),
            ("alonzoVerified", json!(alonzo_verified)),
            ("conwayVerified", json!(conway_verified)),
            (
                "byronHashDeclaredButCanonicalCborPending",
                json!(byron_declared),
            ),
            ("verifiedCount", json!(verified_count)),
        ]),
    );
}

fn strict_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<LedgerState> {
    // Verify the operator-declared genesis hashes BEFORE loading any
    // genesis content so a wrong genesis file aborts startup cleanly
    // rather than silently corrupting subsequent ledger state. Mirrors
    // upstream `Cardano.Node.Configuration.POM.parseGenesisHash`.
    file_cfg
        .verify_known_genesis_hashes(config_base_dir)
        .wrap_err("genesis hash verification failed")?;

    let mut state = LedgerState::new(Era::Byron);
    state.set_expected_network_id(file_cfg.expected_network_id());

    // Seed the multi-era UTxO with Byron genesis distributions so the
    // first Byron transaction that spends a genesis output can resolve
    // its inputs.  Without this seeding every Byron block beyond the
    // genesis-funded first transaction would fail with `InputNotFound`.
    //
    // Reference: `Cardano.Chain.Genesis.UTxO.genesisUtxo`.
    let byron_entries = file_cfg
        .load_byron_genesis_utxo(config_base_dir)
        .wrap_err("failed to load Byron genesis UTxO")?;
    if !byron_entries.is_empty() {
        state.seed_byron_genesis_utxo(
            byron_entries
                .into_iter()
                .map(|entry| (entry.address, entry.amount)),
        );
    }
    if let Some(bootstrap) = file_cfg
        .load_shelley_genesis_bootstrap(config_base_dir)
        .wrap_err("failed to load Shelley genesis bootstrap")?
    {
        state.configure_pending_shelley_genesis_utxo(bootstrap.initial_funds);
        state.configure_pending_shelley_genesis_stake(
            bootstrap
                .staking
                .into_iter()
                .map(|(credential, pool)| (StakeCredential::AddrKeyHash(credential), pool))
                .collect(),
        );
        state.configure_pending_shelley_genesis_delegs(
            bootstrap
                .gen_delegs
                .into_iter()
                .map(|(genesis_hash, parsed)| {
                    (
                        genesis_hash,
                        GenesisDelegationState {
                            delegate: parsed.delegate,
                            vrf: parsed.vrf,
                        },
                    )
                })
                .collect(),
        );
        state.set_genesis_update_quorum(bootstrap.update_quorum);
        state.set_max_lovelace_supply(bootstrap.max_lovelace_supply);
        state.set_slots_per_epoch(bootstrap.epoch_length);
        state.set_active_slot_coeff(yggdrasil_ledger::UnitInterval {
            numerator: bootstrap.active_slots_coeff.0,
            denominator: bootstrap.active_slots_coeff.1,
        });
        // Compute stability_window = 3k/f from genesis config so the
        // ledger PPUP rule can enforce the exact upstream slot-of-no-return.
        if file_cfg.active_slot_coeff > 0.0 {
            let sw = (3.0 * file_cfg.security_param_k as f64 / file_cfg.active_slot_coeff) as u64;
            state.set_stability_window(sw);
        }
    }
    if let Some(params) = file_cfg
        .load_genesis_protocol_params(config_base_dir)
        .wrap_err("failed to load genesis protocol parameters")?
    {
        state.set_protocol_params(params);
    }
    if let Some(enact) = file_cfg
        .load_genesis_enact_state(config_base_dir)
        .wrap_err("failed to load genesis enact state")?
    {
        *state.enact_state_mut() = enact;
    }
    Ok(state)
}

fn best_effort_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> LedgerState {
    strict_base_ledger_state(file_cfg, config_base_dir)
        .unwrap_or_else(|_| LedgerState::new(Era::Byron))
}

fn forged_header_protocol_version(
    base_ledger_state: &LedgerState,
    max_major_protocol_version: u64,
) -> (u64, u64) {
    base_ledger_state
        .protocol_params()
        .and_then(|params| params.protocol_version)
        .unwrap_or((max_major_protocol_version, 0))
}

fn load_effective_config(
    config: Option<PathBuf>,
    network: Option<NetworkPreset>,
) -> Result<(NodeConfigFile, Option<PathBuf>)> {
    match config {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;
            let parsed: NodeConfigFile = match serde_json::from_str(&contents) {
                Ok(parsed) => parsed,
                Err(json_err) => serde_yaml::from_str(&contents).map_err(|yaml_err| {
                    eyre::eyre!(
                        "failed to parse config file {} as JSON ({json_err}) or YAML ({yaml_err})",
                        path.display()
                    )
                })?,
            };
            Ok((parsed, path.parent().map(PathBuf::from)))
        }
        None => Ok(match network {
            Some(preset) => (preset.to_config(), Some(preset_config_base_dir(preset))),
            None => (default_config(), None),
        }),
    }
}

fn preset_config_base_dir(preset: NetworkPreset) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("configuration")
        .join(preset.to_string())
}

/// Apply topology overrides from --topology CLI flag or TopologyFilePath config key.
///
/// If `cli_topology` is provided it takes priority.  Otherwise falls back to the
/// `TopologyFilePath` field in the config file.  The loaded topology replaces the
/// inline peer topology fields in the config.
fn apply_topology_override(
    file_cfg: &mut NodeConfigFile,
    cli_topology: Option<&std::path::Path>,
    config_base_dir: Option<&std::path::Path>,
) -> Result<()> {
    let topology_path = if let Some(path) = cli_topology {
        Some(path.to_path_buf())
    } else {
        file_cfg
            .topology_file_path
            .as_deref()
            .map(|s| resolve_config_path(std::path::Path::new(s), config_base_dir))
    };

    if let Some(path) = topology_path {
        let topology = load_topology_file(&path)
            .wrap_err_with(|| format!("failed to load topology file {}", path.display()))?;
        apply_topology_to_config(file_cfg, &topology);

        // Also update the primary peer from the topology's first bootstrap
        // or root candidate when available.
        let candidates = topology.resolved_root_providers().ordered_candidates();
        if let Some(first) = candidates.first() {
            file_cfg.peer_addr = *first;
        }
    }

    Ok(())
}

fn validate_config_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
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

    let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    let mut warnings = Vec::new();

    // Surface genesis-hash mismatches in the preflight report (without
    // bailing) so an operator running `validate-config` sees the
    // corruption flag alongside any other warnings rather than seeing
    // only the first error. The actual `run` path bails on mismatch via
    // `strict_base_ledger_state` so a misconfigured node cannot start.
    if let Err(err) = file_cfg.verify_known_genesis_hashes(config_base_dir) {
        warnings.push(format!("genesis hash verification: {err}"));
    }

    // Protocol-version floor: the Shelley-era hard fork introduced
    // protocol major version 2, so a node operating below that refuses
    // every block beyond Byron.  Warn but do not bail — an operator
    // legitimately pinned to Byron (replay testing, historical audit)
    // may still want `run` to proceed.
    //
    // Reference: upstream `MaxMajorProtVer` in
    // `Ouroboros.Consensus.Protocol.Abstract`; Shelley hard-fork was at
    // major protocol version 2, Conway at 9.
    if file_cfg.max_major_protocol_version < 2 {
        warnings.push(format!(
            "max_major_protocol_version = {} is pre-Shelley; Shelley-era \
             and later blocks will be rejected as unsupported. \
             Recommended: 10 (Conway-era default)",
            file_cfg.max_major_protocol_version,
        ));
    }

    // Cross-field sanity: every version the operator advertises in
    // `protocol_versions` must be at or below `max_major_protocol_version`.
    // Advertising a major the node itself would reject as unsupported is a
    // self-defeating config — the node would forge a block with that
    // header-version and then fail to apply its own block during the
    // verification pass. Surface the exact offending versions and the
    // accepted ceiling so the fix is obvious.
    //
    // Reference: upstream `MaxMajorProtVer` is the same value consulted in
    // header verification (`Cardano.Protocol.Praos.Rules.Prtcl.headerView`);
    // any proposed version above it causes `ObsoleteNode`.
    if let Some(&max_proposed) = file_cfg.protocol_versions.iter().max() {
        if (max_proposed as u64) > file_cfg.max_major_protocol_version {
            let offending: Vec<u32> = file_cfg
                .protocol_versions
                .iter()
                .copied()
                .filter(|&v| (v as u64) > file_cfg.max_major_protocol_version)
                .collect();
            warnings.push(format!(
                "protocol_versions contains {:?} which exceeds max_major_protocol_version = {}; \
                 blocks forged at these major versions would be rejected by this node's \
                 own header verification (ObsoleteNode). \
                 Raise max_major_protocol_version or drop the offending entries.",
                offending, file_cfg.max_major_protocol_version,
            ));
        }
    }

    if file_cfg.governor_tick_interval_secs == 0 {
        warnings.push(
            "governor_tick_interval_secs is 0; the governor loop will busy-\
             spin at runtime-scheduler resolution and pin a CPU core. \
             Recommended: 1-30"
                .to_owned(),
        );
    }

    // Validate the six governor peer-count targets against the upstream
    // `sanePeerSelectionTargets` invariants using the already-available
    // `GovernorTargets::is_sane()` predicate. Invalid configurations
    // (e.g. `target_active > target_established`) would not crash at
    // runtime but would cause the governor to churn indefinitely trying
    // to satisfy an unsatisfiable shape.
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
        // The upstream NtN KeepAlive client timeout (`keepalive::CLIENT`)
        // is 97 s; a heartbeat interval >= 97 s means the peer's
        // inactivity timer fires before our next heartbeat and tears the
        // connection down. Values <= 1 s are wasteful (constant wire
        // traffic for no benefit).  A sensible operator-tuned range is
        // 10-60 seconds; upstream defaults to around 30 s.
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

    // Defend against very-small cadences too — a slot-every-block
    // checkpoint cadence hammers the ledger-snapshot store and steals
    // fsync bandwidth from the hot sync path. A reasonable floor of 32
    // matches upstream's per-block snapshot batch size.
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
        // A checkpoint cadence longer than a full epoch means a crash
        // after a new epoch rotates stake snapshots but before the next
        // checkpoint lands forces replay of the entire prior epoch on
        // restart — wasteful at best, recovery-stalling at worst. Warn
        // so operators spot typo-shifted units (slots vs epochs vs ms).
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

    // Upstream `CheckpointsFile` integrity preflight: if the operator
    // points at a checkpoints JSON file, the file must exist; if they
    // ALSO declare `CheckpointsFileHash`, the file's raw-bytes Blake2b-256
    // must match. Same integrity story as `*GenesisFile` + `*GenesisHash`
    // — supply-chain swap or typo-shifted path would otherwise surface
    // later as "checkpoint pinning silently disabled" or a confusing
    // consensus divergence once checkpoint enforcement lands. Doing this
    // check now means the declared hash is validated BEFORE any future
    // checkpoint-loader slice wires the pinning semantics, so the
    // verification cannot regress in that follow-up.
    //
    // Reference: `Cardano.Node.Configuration.Checkpoints` in cardano-node;
    // the Blake2b-256 digest is taken over the raw JSON bytes (era-
    // agnostic — no canonical-CBOR step — so the existing
    // `verify_genesis_file_hash` helper applies unchanged).
    if let Some(ckpt_file) = file_cfg.checkpoints_file.as_deref() {
        let ckpt_path = resolve_config_path(std::path::Path::new(ckpt_file), config_base_dir);
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

    // Cross-field sanity: `RequiresNetworkMagic` must match the canonical
    // default for the configured `network_magic`. Mainnet magic (764_824_073)
    // expects `RequiresNoMagic`; every other magic expects `RequiresMagic`.
    // An explicit override in the opposite direction is almost always a
    // copy-paste bug (e.g. mainnet config repurposed for testnet without
    // updating this field) and would cause Byron-era header decoding to
    // desync between the two peers. Warn with the recommended value
    // inlined so the fix is immediately obvious.
    //
    // Reference: upstream `Cardano.Chain.Genesis.Config.mkConfigFromGenesisData`
    // derives the default from the magic; operator-supplied overrides in
    // mismatched shape are rejected at Byron handshake time on the Haskell
    // node too. Warning-only here (not a bail) so pure Shelley+ test
    // environments can still run.
    if let Some(explicit) = file_cfg.requires_network_magic {
        let expected =
            yggdrasil_node::config::RequiresNetworkMagic::default_for_magic(file_cfg.network_magic);
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
            resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
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
        let base_ledger_state = best_effort_base_ledger_state(file_cfg, config_base_dir);
        let chain_db = ChainDb::new(
            FileImmutable::open(&immutable_dir).wrap_err_with(|| {
                format!("failed to open immutable store {}", immutable_dir.display())
            })?,
            FileVolatile::open(&volatile_dir).wrap_err_with(|| {
                format!("failed to open volatile store {}", volatile_dir.display())
            })?,
            FileLedgerStore::open(&ledger_dir).wrap_err_with(|| {
                format!("failed to open ledger store {}", ledger_dir.display())
            })?,
        );
        let tip = chain_db.recovery().tip;
        let recovery =
            recover_ledger_state_chaindb(&chain_db, base_ledger_state).wrap_err_with(|| {
                format!(
                    "failed to recover ledger state from storage directory {}",
                    storage_dir.display()
                )
            })?;
        let latest_slot = point_slot(&recovery.point).or_else(|| point_slot(&tip));
        let ledger_snapshot = ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state);
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

    let fallback_peers = configured_fallback_peers(
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

// ---------------------------------------------------------------------------
// status subcommand
// ---------------------------------------------------------------------------

/// Ledger-state cardinality summary mirroring LSQ tag 23
/// `GetLedgerCounts`.  Exposed inside [`StatusReport`] when the node has
/// successfully recovered the latest ledger state from storage.
#[derive(Serialize)]
struct LedgerCountsReport {
    stake_credentials: usize,
    pools: usize,
    dreps: usize,
    committee_members: usize,
    governance_actions: usize,
    gen_delegs: usize,
}

/// On-disk node status report produced by the `status` subcommand.
#[derive(Serialize)]
struct StatusReport {
    network_magic: u32,
    storage_dir: String,
    storage_initialized: bool,
    chain_tip: String,
    chain_tip_slot: Option<u64>,
    chain_tip_hash: Option<String>,
    immutable_tip: String,
    immutable_block_count: usize,
    volatile_tip: String,
    volatile_block_count: usize,
    ledger_checkpoint_slot: Option<u64>,
    ledger_checkpoint_count: usize,
    replayed_volatile_blocks: Option<usize>,
    recovered_ledger_point: Option<String>,
    /// Era of the recovered ledger state (`Byron`, `Shelley`, …, `Conway`).
    /// `None` when storage is uninitialized or recovery fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_era: Option<String>,
    /// Current epoch number at the recovered ledger tip.
    /// `None` when storage is uninitialized or recovery fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_epoch: Option<u64>,
    /// Aggregate ledger-state cardinalities at the recovered tip.
    /// `None` when storage replay failed or no ledger state was recovered.
    #[serde(skip_serializing_if = "Option::is_none")]
    ledger_counts: Option<LedgerCountsReport>,
}

fn status_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<StatusReport> {
    let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
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
        FileImmutable::open(immutable_dir).wrap_err("failed to open immutable store")?,
        FileVolatile::open(volatile_dir).wrap_err("failed to open volatile store")?,
        FileLedgerStore::open(ledger_dir).wrap_err("failed to open ledger store")?,
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
    let recovery = recover_ledger_state_chaindb(
        &chain_db,
        best_effort_base_ledger_state(file_cfg, config_base_dir),
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
    let current_epoch = recovery.as_ref().ok().map(|r| r.ledger_state.current_epoch().0);

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

fn resolve_storage_dir(
    storage_dir: &std::path::Path,
    config_base_dir: Option<&std::path::Path>,
) -> PathBuf {
    if storage_dir.is_absolute() {
        storage_dir.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(storage_dir)
    } else {
        storage_dir.to_path_buf()
    }
}

fn resolve_config_path(
    path: &std::path::Path,
    config_base_dir: Option<&std::path::Path>,
) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(path)
    } else {
        path.to_path_buf()
    }
}

fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
}

fn extend_unique_peers(target: &mut Vec<SocketAddr>, peers: impl IntoIterator<Item = SocketAddr>) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

fn extend_unique_ledger_peers(
    target: &mut Vec<SocketAddr>,
    access_points: impl IntoIterator<Item = PoolRelayAccessPoint>,
) {
    for access_point in access_points {
        let peer_access_point = PeerAccessPoint {
            address: access_point.address,
            port: access_point.port,
        };
        extend_unique_peers(target, resolve_peer_access_points(&peer_access_point));
    }
}

fn ledger_peer_snapshot_from_ledger_state(ledger_state: &LedgerState) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(
        &mut ledger_peers,
        ledger_state.pool_state().relay_access_points(),
    );
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
}

fn configured_fallback_peers(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
    ledger_snapshot: &LedgerPeerSnapshot,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    tracer: &NodeTracer,
) -> Vec<SocketAddr> {
    let mut fallback_peers = file_cfg.ordered_fallback_peers();

    let mut snapshot_slot = None;
    let mut snapshot_available = file_cfg.peer_snapshot_file.is_none();
    let mut snapshot_path = None;
    let mut snapshot_file = None;

    if let Some(peer_snapshot_file) = file_cfg.peer_snapshot_file.as_deref() {
        let peer_snapshot_path =
            resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
        snapshot_path = Some(peer_snapshot_path.clone());

        match load_peer_snapshot_file(&peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                snapshot_file = Some(loaded_snapshot.snapshot);
            }
            Err(err) => {
                let freshness = file_cfg.peer_snapshot_freshness(None, latest_slot, false);
                let (decision, _) = file_cfg.eligible_ledger_fallback_peers(
                    ledger_snapshot,
                    latest_slot,
                    ledger_state_judgement,
                    freshness,
                );

                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to load peer snapshot fallbacks",
                    trace_fields([
                        ("decision", json!(format!("{decision:?}"))),
                        ("latestSlot", json!(latest_slot)),
                        (
                            "snapshotPath",
                            json!(peer_snapshot_path.display().to_string()),
                        ),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    let combined_snapshot = merge_ledger_peer_snapshots(ledger_snapshot, snapshot_file);
    let freshness =
        file_cfg.peer_snapshot_freshness(snapshot_slot, latest_slot, snapshot_available);
    let (decision, eligible_peers) = file_cfg.eligible_ledger_fallback_peers(
        &combined_snapshot,
        latest_slot,
        ledger_state_judgement,
        freshness,
    );
    let snapshot_peer_count = eligible_peers.len();
    extend_unique_peers(&mut fallback_peers, eligible_peers);

    tracer.trace_runtime(
        "Net.PeerSelection",
        "Info",
        "evaluated ledger-derived startup fallbacks",
        trace_fields([
            ("decision", json!(format!("{decision:?}"))),
            ("latestSlot", json!(latest_slot)),
            ("snapshotSlot", json!(snapshot_slot)),
            (
                "snapshotPath",
                json!(snapshot_path.map(|path| path.display().to_string())),
            ),
            (
                "ledgerPeerCount",
                json!(combined_snapshot.ledger_peers.len()),
            ),
            (
                "bigLedgerPeerCount",
                json!(combined_snapshot.big_ledger_peers.len()),
            ),
            ("eligiblePeerCount", json!(snapshot_peer_count)),
        ]),
    );

    fallback_peers
}

fn checkpoint_trace_config_mut(file_cfg: &mut NodeConfigFile) -> &mut TraceNamespaceConfig {
    file_cfg
        .trace_options
        .entry(CHECKPOINT_TRACE_NAMESPACE.to_owned())
        .or_default()
}

async fn run_node(request: RunNodeRequest) -> Result<()> {
    let RunNodeRequest {
        node_config,
        bootstrap_peers,
        sync_config,
        governor_config,
        topology_config,
        tracer,
        storage_dir,
        chain_db,
        inbound_listen_addr,
        use_ledger_peers,
        peer_snapshot_path,
        metrics_port,
        base_ledger_state,
        socket_path,
        block_producer_credentials,
        max_major_protocol_version,
    } = request;

    // Log block producer mode availability.
    if let Some(ref bp) = block_producer_credentials {
        tracer.trace_runtime(
            "Startup.BlockProducer",
            "Notice",
            "block producer credentials loaded",
            trace_fields([
                (
                    "vrfVerificationKeyHash",
                    json!(hex::encode(
                        yggdrasil_crypto::blake2b::hash_bytes_256(&bp.vrf_verification_key.0).0
                    )),
                ),
                (
                    "opcertSequenceNumber",
                    json!(bp.operational_cert.sequence_number),
                ),
                ("kesPeriod", json!(bp.kes_current_period)),
            ]),
        );
    }

    let block_producer_runtime_config = block_producer_credentials.as_ref().and_then(|_| {
        sync_config
            .active_slot_coeff
            .clone()
            .map(|active_slot_coeff| {
                let protocol_version =
                    forged_header_protocol_version(&base_ledger_state, max_major_protocol_version);
                let (max_block_body_size, protocol_version) = base_ledger_state
                    .protocol_params()
                    .map(|params| {
                        (
                            params.max_block_body_size,
                            params.protocol_version.unwrap_or(protocol_version),
                        )
                    })
                    .unwrap_or((65_536, protocol_version));

                yggdrasil_node::RuntimeBlockProducerConfig {
                    slot_length: std::time::Duration::from_secs_f64(
                        sync_config.slot_length_secs.unwrap_or(1.0),
                    ),
                    active_slot_coeff,
                    sigma_num: 1,
                    sigma_den: 1,
                    epoch_nonce: Nonce::Neutral,
                    max_block_body_size,
                    protocol_version,
                }
            })
    });

    if block_producer_credentials.is_some() && block_producer_runtime_config.is_none() {
        tracer.trace_runtime(
            "Startup.BlockProducer",
            "Warning",
            "block producer credentials present but active slot coefficient unavailable; producer loop disabled",
            std::collections::BTreeMap::new(),
        );
    }

    let chain_db = Arc::new(RwLock::new(chain_db));
    let peer_registry = Arc::new(RwLock::new(seed_peer_registry(
        node_config.peer_addr,
        &topology_config,
    )));

    let metrics = std::sync::Arc::new(NodeMetrics::new());

    // Optionally spawn the Prometheus metrics HTTP endpoint.
    if let Some(port) = metrics_port {
        let metrics_ref = std::sync::Arc::clone(&metrics);
        tokio::spawn(async move {
            if let Err(err) = serve_metrics(port, metrics_ref).await {
                eprintln!("metrics server error: {err}");
            }
        });
    }

    tracer.trace_runtime(
        "Startup.DiffusionInit",
        "Notice",
        "starting node runtime",
        trace_fields([
            ("primaryPeer", json!(node_config.peer_addr.to_string())),
            ("bootstrapPeerCount", json!(1 + bootstrap_peers.len())),
            ("networkMagic", json!(node_config.network_magic)),
            ("storageDir", json!(storage_dir.display().to_string())),
            (
                "protocolVersions",
                json!(
                    node_config
                        .protocol_versions
                        .iter()
                        .map(|v| v.0)
                        .collect::<Vec<_>>()
                ),
            ),
        ]),
    );

    let nonce_state = sync_config
        .nonce_config
        .as_ref()
        .map(|_| NonceEvolutionState::new(Nonce::Neutral));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn signal handler for graceful shutdown.
    let signal_tracer = tracer.clone();
    let signal_shutdown_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        signal_tracer.trace_runtime(
            "Node.Shutdown",
            "Notice",
            "shutdown signal received",
            std::collections::BTreeMap::new(),
        );
        let _ = signal_shutdown_tx.send(true);
    });

    // Shared mempool for governor TTL purge and inbound TxSubmission admission.
    let shared_mempool = SharedMempool::default();
    let shared_connection_manager = Arc::new(RwLock::new(ConnectionManagerState::new()));
    let shared_inbound_governor = Arc::new(RwLock::new(InboundGovernorState::new()));
    let shared_inbound_peers: Arc<RwLock<BTreeMap<SocketAddr, NodePeerSharing>>> =
        Arc::new(RwLock::new(BTreeMap::new()));

    let governor_task = {
        let mut governor_shutdown = shutdown_rx.clone();
        let governor_node_config = node_config.clone();
        let governor_chain_db = Arc::clone(&chain_db);
        let governor_registry = Arc::clone(&peer_registry);
        let governor_tracer = tracer.clone();
        let governor_metrics = std::sync::Arc::clone(&metrics);
        let governor_topology = topology_config.clone();
        let governor_base_ledger_state = base_ledger_state.clone();
        let governor_mempool = shared_mempool.clone();
        let governor_connection_manager = Arc::clone(&shared_connection_manager);
        let governor_inbound_peers = Arc::clone(&shared_inbound_peers);
        tokio::spawn(async move {
            let shutdown = async move {
                if *governor_shutdown.borrow() {
                    return;
                }
                while governor_shutdown.changed().await.is_ok() {
                    if *governor_shutdown.borrow() {
                        break;
                    }
                }
            };

            run_governor_loop(
                governor_node_config,
                governor_chain_db,
                governor_registry,
                governor_connection_manager,
                GovernorState::default(),
                governor_config,
                governor_topology,
                governor_base_ledger_state,
                Some(governor_mempool),
                Some(governor_inbound_peers),
                governor_tracer,
                Some(governor_metrics),
                shutdown,
            )
            .await;
        })
    };

    // Shared chain-tip notification channel.  The block producer notifies
    // waiters when it inserts a new block so inbound ChainSync servers can
    // push updates without busy-looping.  The sync service also notifies
    // after each batch so locally-connected NtN clients see progress.
    let chain_tip_notify: yggdrasil_node::ChainTipNotify =
        std::sync::Arc::new(tokio::sync::Notify::new());

    // Whether diffusion pipelining is enabled for this node.  For now
    // it is always on; a future config flag may control this.
    let diffusion_pipelining = DiffusionPipeliningSupport::DiffusionPipeliningOn;

    // Shared diffusion pipelining state.  When pipelining is enabled, the
    // sync pipeline sets a tentative header after header validation but
    // before body validation completes; the ChainSync server may serve it
    // to downstream peers immediately.
    let shared_tentative_state: Option<Arc<RwLock<TentativeState>>> = match diffusion_pipelining {
        DiffusionPipeliningSupport::DiffusionPipeliningOff => None,
        DiffusionPipeliningSupport::DiffusionPipeliningOn => {
            Some(Arc::new(RwLock::new(TentativeState::initial())))
        }
    };

    // Shared block-producer state updated by the sync pipeline so the
    // producer loop reads live epoch nonce and stake sigma values.
    let shared_bp_state = std::sync::Arc::new(std::sync::RwLock::new(
        yggdrasil_node::SharedBlockProducerState::default(),
    ));

    // Compute issuer pool-key-hash (Blake2b-224) before credentials are
    // consumed by the block-producer task.  Used by the sync pipeline to
    // push stake sigma updates to the shared producer state.
    let bp_pool_key_hash: Option<[u8; 28]> = block_producer_credentials
        .as_ref()
        .map(|bp| yggdrasil_crypto::blake2b::hash_bytes_224(&bp.issuer_vkey.0).0);

    let block_producer_task =
        if let (Some(block_producer_credentials), Some(block_producer_config)) =
            (block_producer_credentials, block_producer_runtime_config)
        {
            let mut producer_shutdown = shutdown_rx.clone();
            let producer_chain_db = Arc::clone(&chain_db);
            let producer_mempool = shared_mempool.clone();
            let producer_tracer = tracer.clone();
            let producer_metrics = std::sync::Arc::clone(&metrics);
            let producer_tip_notify = chain_tip_notify.clone();
            let producer_bp_state = std::sync::Arc::clone(&shared_bp_state);
            Some(tokio::spawn(async move {
                let shutdown = async move {
                    if *producer_shutdown.borrow() {
                        return;
                    }
                    while producer_shutdown.changed().await.is_ok() {
                        if *producer_shutdown.borrow() {
                            break;
                        }
                    }
                };

                run_block_producer_loop(
                    producer_chain_db,
                    producer_mempool,
                    block_producer_credentials,
                    block_producer_config,
                    Some(producer_tip_notify),
                    Some(producer_bp_state),
                    producer_tracer,
                    Some(producer_metrics),
                    shutdown,
                )
                .await;
            }))
        } else {
            None
        };

    // Shared TxSubmission inbound dedup state; threaded into both the inbound
    // accept loop (populated when peers advertise TxIds) and the reconnecting
    // sync request (consulted during mempool eviction to avoid re-fetching
    // transactions already confirmed on the applied chain).  Cloning is cheap
    // (Arc<RwLock<_>>).
    let inbound_tx_state = SharedTxState::default();

    let inbound_task = if let Some(listen_addr) = inbound_listen_addr {
        let listener = PeerListener::bind(
            listen_addr,
            node_config.network_magic,
            node_config.protocol_versions.clone(),
        )
        .await?;
        let bound_addr = listener.local_addr().unwrap_or(listen_addr);
        tracer.trace_runtime(
            "Net.Inbound",
            "Notice",
            "inbound listener bound",
            trace_fields([("listenAddr", json!(bound_addr.to_string()))]),
        );

        let shared_provider = if let Some(tentative) = shared_tentative_state.as_ref() {
            Arc::new(SharedChainDb::from_arc_with_tentative(
                Arc::clone(&chain_db),
                Arc::clone(tentative),
            ))
        } else {
            Arc::new(SharedChainDb::from_arc(Arc::clone(&chain_db)))
        };
        let block_provider: Arc<dyn BlockProvider> = shared_provider.clone();
        let chain_provider: Arc<dyn ChainProvider> = shared_provider;
        let tx_submission_consumer = Arc::new(
            SharedTxSubmissionConsumer::new(Arc::clone(&chain_db), shared_mempool.clone())
                .with_metrics(Arc::clone(&metrics)),
        );
        let peer_sharing = Arc::new(SharedPeerSharingProvider::with_inbound_governor(
            Arc::clone(&peer_registry),
            Some(Arc::clone(&shared_inbound_governor)),
        ));
        let inbound_connection_manager = Arc::clone(&shared_connection_manager);
        let inbound_governor = Arc::clone(&shared_inbound_governor);
        let inbound_tx_state = inbound_tx_state.clone();
        let mut inbound_shutdown = shutdown_rx.clone();
        let inbound_tracer = tracer.clone();
        let inbound_metrics = metrics.clone();
        let inbound_peers = Arc::clone(&shared_inbound_peers);
        let inbound_tip_notify = chain_tip_notify.clone();

        Some(tokio::spawn(async move {
            let shutdown = async move {
                if *inbound_shutdown.borrow() {
                    return;
                }
                while inbound_shutdown.changed().await.is_ok() {
                    if *inbound_shutdown.borrow() {
                        break;
                    }
                }
            };

            if let Err(err) = run_inbound_accept_loop(
                &listener,
                Some(block_provider),
                Some(chain_provider),
                Some(tx_submission_consumer),
                Some(peer_sharing),
                Some(inbound_peers),
                Some(inbound_connection_manager),
                Some(inbound_governor),
                Some(yggdrasil_network::AcceptedConnectionsLimit::default()),
                Some(inbound_tx_state),
                Some(inbound_tip_notify),
                Some(&inbound_tracer),
                Some(&inbound_metrics),
                shutdown,
            )
            .await
            {
                inbound_tracer.trace_runtime(
                    "Net.Inbound",
                    "Error",
                    "inbound listener stopped with error",
                    trace_fields([("error", json!(err.to_string()))]),
                );
            }
        }))
    } else {
        None
    };

    // -- NtC local server (Unix socket for CLI queries / tx submission) ----
    #[cfg(unix)]
    let ntc_task = if let Some(ref ntc_path) = socket_path {
        let ntc_chain_db = Arc::clone(&chain_db);
        let ntc_mempool = shared_mempool.clone();
        let ntc_path = ntc_path.clone();
        let ntc_tracer = tracer.clone();
        let mut ntc_shutdown = shutdown_rx.clone();
        let ntc_evaluator: Option<
            Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>,
        > = None;
        let ntc_metrics = Some(Arc::clone(&metrics));
        let ntc_network_magic = node_config.network_magic;

        tracer.trace_runtime(
            "Net.NtC",
            "Notice",
            "starting NtC local server",
            trace_fields([("socketPath", json!(ntc_path.display().to_string()))]),
        );

        Some(tokio::spawn(async move {
            let dispatcher: Arc<dyn yggdrasil_node::LocalQueryDispatcher> =
                Arc::new(yggdrasil_node::BasicLocalQueryDispatcher);
            let shutdown = async move {
                if *ntc_shutdown.borrow() {
                    return;
                }
                while ntc_shutdown.changed().await.is_ok() {
                    if *ntc_shutdown.borrow() {
                        break;
                    }
                }
            };
            if let Err(err) = yggdrasil_node::run_local_accept_loop(
                &ntc_path,
                ntc_network_magic,
                ntc_chain_db,
                ntc_mempool,
                dispatcher,
                ntc_evaluator,
                ntc_metrics,
                shutdown,
            )
            .await
            {
                ntc_tracer.trace_runtime(
                    "Net.NtC",
                    "Error",
                    "NtC local server stopped with error",
                    trace_fields([("error", json!(err.to_string()))]),
                );
            }
        }))
    } else {
        None
    };
    #[cfg(not(unix))]
    let ntc_task: Option<tokio::task::JoinHandle<()>> = {
        let _ = &socket_path;
        None
    };

    let request = ResumeReconnectingVerifiedSyncRequest::new(
        &node_config,
        &bootstrap_peers,
        base_ledger_state,
        &sync_config,
    )
    .with_nonce_state(nonce_state)
    .with_use_ledger_peers(use_ledger_peers)
    .with_peer_snapshot_path(peer_snapshot_path)
    .with_metrics(Some(&metrics))
    .with_peer_registry(Some(Arc::clone(&peer_registry)))
    .with_mempool(Some(shared_mempool.clone()))
    .with_tentative_state(shared_tentative_state.clone())
    .with_tip_notify(Some(chain_tip_notify.clone()))
    .with_bp_state(
        bp_pool_key_hash.map(|_| std::sync::Arc::clone(&shared_bp_state)),
        bp_pool_key_hash,
    )
    .with_inbound_tx_state(Some(inbound_tx_state));

    let mut sync_shutdown = shutdown_rx.clone();
    let outcome: ResumedSyncServiceOutcome =
        match resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer(
            &chain_db,
            request,
            &tracer,
            async move {
                if *sync_shutdown.borrow() {
                    return;
                }
                while sync_shutdown.changed().await.is_ok() {
                    if *sync_shutdown.borrow() {
                        break;
                    }
                }
            },
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(err) => {
                let _ = shutdown_tx.send(true);
                let _ = governor_task.await;
                if let Some(handle) = block_producer_task {
                    let _ = handle.await;
                }
                if let Some(handle) = inbound_task {
                    let _ = handle.await;
                }
                if let Some(handle) = ntc_task {
                    let _ = handle.await;
                }
                tracer.trace_runtime(
                    "Node.Sync",
                    "Error",
                    "node run failed",
                    trace_fields([
                        ("error", json!(err.to_string())),
                        ("primaryPeer", json!(node_config.peer_addr.to_string())),
                    ]),
                );
                return Err(err.into());
            }
        };

    let _ = shutdown_tx.send(true);
    let _ = governor_task.await;
    if let Some(handle) = block_producer_task {
        let _ = handle.await;
    }
    if let Some(handle) = inbound_task {
        let _ = handle.await;
    }
    if let Some(handle) = ntc_task {
        let _ = handle.await;
    }

    tracer.trace_runtime(
        "Node.Sync",
        "Notice",
        "sync complete",
        trace_fields([
            (
                "checkpointSlot",
                json!(outcome.recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(outcome.recovery.replayed_volatile_blocks),
            ),
            (
                "recoveredPoint",
                json!(format!("{:?}", outcome.recovery.point)),
            ),
            ("totalBlocks", json!(outcome.sync.total_blocks)),
            ("totalRollbacks", json!(outcome.sync.total_rollbacks)),
            ("batchesCompleted", json!(outcome.sync.batches_completed)),
            ("stableBlockCount", json!(outcome.sync.stable_block_count)),
            ("reconnectCount", json!(outcome.sync.reconnect_count)),
            (
                "lastConnectedPeer",
                json!(
                    outcome
                        .sync
                        .last_connected_peer_addr
                        .map(|addr| addr.to_string())
                ),
            ),
            (
                "finalPoint",
                json!(format!("{:?}", outcome.sync.final_point)),
            ),
        ]),
    );

    if let Some(ref nonce) = outcome.sync.nonce_state {
        tracer.trace_runtime(
            "Node.Sync",
            "Info",
            "epoch nonce state updated",
            trace_fields([
                ("epoch", json!(nonce.current_epoch.0)),
                ("epochNonce", json!(format!("{:?}", nonce.epoch_nonce))),
            ]),
        );
    }

    if let Some(ref cs) = outcome.sync.chain_state {
        tracer.trace_runtime(
            "Node.Sync",
            "Info",
            "chain state tracked",
            trace_fields([
                ("volatileEntries", json!(cs.volatile_len())),
                ("tip", json!(format!("{:?}", cs.tip()))),
            ]),
        );
    }

    Ok(())
}

struct RunNodeRequest {
    node_config: NodeConfig,
    bootstrap_peers: Vec<SocketAddr>,
    sync_config: VerifiedSyncServiceConfig,
    governor_config: RuntimeGovernorConfig,
    topology_config: yggdrasil_network::TopologyConfig,
    tracer: NodeTracer,
    storage_dir: PathBuf,
    chain_db: ChainDb<FileImmutable, FileVolatile, FileLedgerStore>,
    inbound_listen_addr: Option<SocketAddr>,
    use_ledger_peers: Option<yggdrasil_network::UseLedgerPeers>,
    peer_snapshot_path: Option<PathBuf>,
    metrics_port: Option<u16>,
    /// Genesis-seeded base ledger state used for recovery and fresh sync.
    base_ledger_state: LedgerState,
    /// NtC Unix domain socket path for local client queries.
    socket_path: Option<PathBuf>,
    /// Block producer credentials (VRF key, KES key, operational certificate).
    /// When present the node operates in block-producing mode.
    block_producer_credentials: Option<yggdrasil_node::block_producer::BlockProducerCredentials>,
    /// Maximum protocol-version major this node supports for forged headers.
    max_major_protocol_version: u64,
}

// ---------------------------------------------------------------------------
// Prometheus metrics HTTP endpoint
// ---------------------------------------------------------------------------

/// Lightweight HTTP handler that responds with Prometheus exposition text on
/// `GET /metrics`, a JSON snapshot on `GET /metrics/json`, and a simple health
/// check on `GET /health`.
///
/// Upstream-compatible debug aliases are also accepted:
/// - `GET /debug` and `GET /debug/metrics` (JSON metrics)
/// - `GET /debug/metrics/prometheus` (Prometheus text)
/// - `GET /debug/health` (health JSON)
///
/// Uses raw tokio TCP — no HTTP framework dependency required.
async fn serve_metrics(port: u16, metrics: std::sync::Arc<NodeMetrics>) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    loop {
        let (mut stream, _addr) = listener.accept().await?;
        let metrics = std::sync::Arc::clone(&metrics);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buf[..n]);
            let (status, content_type, body) = metrics_http_response(&request, &metrics);

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

fn metrics_http_response(request: &str, metrics: &NodeMetrics) -> (&'static str, &'static str, String) {
    // Route order matters: more-specific prefixes MUST be tested before
    // less-specific ones.  Before the fix, `GET /metrics` was checked
    // before `GET /metrics/json`, so every JSON request matched the
    // Prometheus-text prefix first and never reached the JSON arm,
    // silently turning `/metrics/json` into dead code.
    if request.starts_with("GET /health") || request.starts_with("GET /debug/health") {
        let snap = metrics.snapshot();
        let body = serde_json::json!({
            "status": "ok",
            "uptime_seconds": snap.uptime_ms / 1000,
            "blocks_synced": snap.blocks_synced,
            "current_slot": snap.current_slot,
        })
        .to_string();
        ("200 OK", "application/json", body)
    } else if request.starts_with("GET /metrics/json")
        || request.starts_with("GET /debug/metrics/json")
        || request.starts_with("GET /debug/metrics ")
        || request.starts_with("GET /debug ")
    {
        // JSON first — must precede the `/metrics` / `/debug/metrics`
        // Prometheus text arms below.
        let snap = metrics.snapshot();
        match serde_json::to_string_pretty(&snap) {
            Ok(json) => ("200 OK", "application/json", json),
            Err(_) => (
                "500 Internal Server Error",
                "text/plain",
                "serialization error".to_owned(),
            ),
        }
    } else if request.starts_with("GET /debug/metrics/prometheus")
        || request.starts_with("GET /debug/metrics")
        || request.starts_with("GET /metrics")
    {
        let body = metrics.snapshot().to_prometheus_text();
        ("200 OK", "text/plain; version=0.0.4; charset=utf-8", body)
    } else {
        ("404 Not Found", "text/plain", "not found\n".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHECKPOINT_TRACE_NAMESPACE, apply_topology_override, checkpoint_trace_config_mut,
        configured_fallback_peers, forged_header_protocol_version,
        ledger_peer_snapshot_from_ledger_state, load_effective_config, preset_config_base_dir,
        status_report, validate_config_report,
    };
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use yggdrasil_ledger::{
        Era, LedgerState, PoolParams, Relay, RewardAccount, StakeCredential, UnitInterval,
    };
    use yggdrasil_network::{LedgerPeerSnapshot, LedgerStateJudgement};
    use yggdrasil_node::config::default_config;
    use yggdrasil_node::tracer::{NodeMetrics, NodeTracer};

    #[test]
    fn checkpoint_trace_override_creates_namespace_when_missing() {
        let mut cfg = default_config();
        cfg.trace_options.remove(CHECKPOINT_TRACE_NAMESPACE);

        checkpoint_trace_config_mut(&mut cfg).severity = Some("Info".to_owned());

        assert_eq!(
            cfg.trace_options
                .get(CHECKPOINT_TRACE_NAMESPACE)
                .expect("checkpoint namespace")
                .severity
                .as_deref(),
            Some("Info")
        );
    }

    #[test]
    fn checkpoint_trace_override_can_disable_rate_limit() {
        let mut cfg = default_config();

        checkpoint_trace_config_mut(&mut cfg).max_frequency = None;

        assert_eq!(
            cfg.trace_options
                .get(CHECKPOINT_TRACE_NAMESPACE)
                .expect("checkpoint namespace")
                .max_frequency,
            None
        );
    }

    #[test]
    fn checkpoint_trace_override_updates_severity_and_backends() {
        let mut cfg = default_config();
        let override_cfg = checkpoint_trace_config_mut(&mut cfg);
        override_cfg.severity = Some("Silence".to_owned());
        override_cfg.backends = vec!["Stdout MachineFormat".to_owned(), "Forwarder".to_owned()];

        let checkpoint_cfg = cfg
            .trace_options
            .get(CHECKPOINT_TRACE_NAMESPACE)
            .expect("checkpoint namespace");
        assert_eq!(checkpoint_cfg.severity.as_deref(), Some("Silence"));
        assert_eq!(
            checkpoint_cfg.backends,
            vec!["Stdout MachineFormat".to_owned(), "Forwarder".to_owned(),]
        );
    }

    #[test]
    fn ledger_peer_snapshot_from_ledger_state_uses_registered_pool_relays() {
        let mut ledger_state = yggdrasil_ledger::LedgerState::new(yggdrasil_ledger::Era::Shelley);
        ledger_state.pool_state_mut().register(PoolParams {
            operator: [1; 28],
            vrf_keyhash: [2; 32],
            pledge: 1,
            cost: 1,
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([3; 28]),
            },
            pool_owners: vec![[4; 28]],
            relays: vec![Relay::SingleHostAddr(
                Some(3001),
                Some([127, 0, 0, 9]),
                None,
            )],
            pool_metadata: None,
        });

        let snapshot = ledger_peer_snapshot_from_ledger_state(&ledger_state);
        assert_eq!(
            snapshot,
            LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new(),)
        );
    }

    #[test]
    fn configured_fallback_peers_appends_eligible_ledger_state_peers() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(0);
        cfg.peer_snapshot_file = None;
        let tracer = NodeTracer::from_config(&cfg);
        let ledger_snapshot =
            LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new());

        let fallback_peers = configured_fallback_peers(
            &cfg,
            None,
            &ledger_snapshot,
            Some(1),
            LedgerStateJudgement::YoungEnough,
            &tracer,
        );

        assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("peer")));
    }

    #[test]
    fn configured_fallback_peers_merges_snapshot_big_ledger_peers() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-peer-snapshot-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let snapshot_path = dir.join("peer-snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
                "version": 2,
                "slotNo": 10,
                "bigLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.10", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("write snapshot");

        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(0);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());
        let tracer = NodeTracer::from_config(&cfg);
        let ledger_snapshot =
            LedgerPeerSnapshot::new(["127.0.0.9:3001".parse().expect("peer")], Vec::new());

        let fallback_peers = configured_fallback_peers(
            &cfg,
            Some(&dir),
            &ledger_snapshot,
            Some(10),
            LedgerStateJudgement::YoungEnough,
            &tracer,
        );

        assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("ledger")));
        assert!(fallback_peers.contains(&"127.0.0.10:3001".parse().expect("big ledger")));

        std::fs::remove_file(snapshot_path).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_when_storage_is_uninitialized() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-validate-config-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

        assert_eq!(report.storage.status, "not-initialized");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("storage directories are not initialized"))
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn metrics_http_response_supports_debug_json_alias() {
        let metrics = NodeMetrics::new();
        let (status, content_type, body) =
            super::metrics_http_response("GET /debug HTTP/1.1\r\n\r\n", &metrics);

        assert_eq!(status, "200 OK");
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"blocks_synced\""));
    }

    #[test]
    fn metrics_http_response_supports_debug_prometheus_alias() {
        let metrics = NodeMetrics::new();
        metrics.add_blocks_synced(3);
        let (status, content_type, body) = super::metrics_http_response(
            "GET /debug/metrics/prometheus HTTP/1.1\r\n\r\n",
            &metrics,
        );

        assert_eq!(status, "200 OK");
        assert_eq!(content_type, "text/plain; version=0.0.4; charset=utf-8");
        assert!(body.contains("yggdrasil_blocks_synced 3"));
    }

    #[test]
    fn metrics_http_response_supports_debug_health_alias() {
        let metrics = NodeMetrics::new();
        let (status, content_type, body) =
            super::metrics_http_response("GET /debug/health HTTP/1.1\r\n\r\n", &metrics);

        assert_eq!(status, "200 OK");
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\":\"ok\""));
    }

    #[test]
    fn validate_config_report_loads_configured_peer_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-validate-snapshot-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let snapshot_path = dir.join("peer-snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
                "version": 2,
                "slotNo": 10,
                "allLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.11", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("write snapshot");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

        assert_eq!(report.peer_snapshot.status, "loaded");
        assert_eq!(report.peer_snapshot.slot, Some(10));
        assert_eq!(report.peer_snapshot.ledger_peer_count, 1);
        assert_eq!(report.peer_snapshot.error, None);

        std::fs::remove_file(snapshot_path).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_rejects_invalid_active_slot_coeff() {
        let mut cfg = default_config();
        cfg.active_slot_coeff = 0.0;

        assert!(validate_config_report(&cfg, None).is_err());
    }

    #[test]
    fn validate_config_report_rejects_zero_slots_per_kes_period() {
        let mut cfg = default_config();
        cfg.slots_per_kes_period = 0;
        let err = validate_config_report(&cfg, None)
            .err()
            .expect("zero slots_per_kes_period must fail");
        assert!(
            err.to_string().contains("slots_per_kes_period"),
            "error should mention slots_per_kes_period: {err}",
        );
    }

    #[test]
    fn validate_config_report_rejects_zero_max_kes_evolutions() {
        let mut cfg = default_config();
        cfg.max_kes_evolutions = 0;
        let err = validate_config_report(&cfg, None)
            .err()
            .expect("zero max_kes_evolutions must fail");
        assert!(
            err.to_string().contains("max_kes_evolutions"),
            "error should mention max_kes_evolutions: {err}",
        );
    }

    #[test]
    fn validate_config_report_warns_on_pre_shelley_max_major_protocol_version() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-pv-warn-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.max_major_protocol_version = 1; // pre-Shelley
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("pre-Shelley") || w.contains("max_major_protocol_version")),
            "expected pre-Shelley warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn decode_ntc_result_shapes_typed_json_for_new_queries() {
        // Lock in the decoder output for every recently-added typed
        // response so a silent drift in the CLI-side parser (which turns
        // the server's raw CBOR into structured JSON) is caught at CI
        // time rather than showing up as wrong keys in user-facing
        // `yggdrasil-node query ...` output.
        use super::{QueryCommand, decode_ntc_result};

        // AccountState: `[treasury, reserves, total_deposits]`
        // CBOR: 0x83 0x01 0x02 0x03 → [1, 2, 3]
        let v = decode_ntc_result(&QueryCommand::AccountState, &[0x83, 0x01, 0x02, 0x03])
            .expect("decode AccountState");
        assert_eq!(v["treasury_lovelace"], 1);
        assert_eq!(v["reserves_lovelace"], 2);
        assert_eq!(v["total_deposits_lovelace"], 3);

        // StabilityWindow: unsigned u64 or null.
        let v = decode_ntc_result(&QueryCommand::StabilityWindow, &[0x19, 0x08, 0x70])
            .expect("decode StabilityWindow");
        assert_eq!(v["stability_window_slots"], 2160);
        let v = decode_ntc_result(&QueryCommand::StabilityWindow, &[0xf6])
            .expect("decode StabilityWindow null");
        assert!(v["stability_window"].is_null());

        // NumDormantEpochs: unsigned u64.
        let v = decode_ntc_result(&QueryCommand::NumDormantEpochs, &[0x03])
            .expect("decode NumDormantEpochs");
        assert_eq!(v["num_dormant_epochs"], 3);

        // ExpectedNetworkId: mainnet id (1) or null.
        let v = decode_ntc_result(&QueryCommand::ExpectedNetworkId, &[0x01])
            .expect("decode ExpectedNetworkId");
        assert_eq!(v["expected_network_id"], 1);
        let v = decode_ntc_result(&QueryCommand::ExpectedNetworkId, &[0xf6])
            .expect("decode ExpectedNetworkId null");
        assert!(v["expected_network_id"].is_null());

        // DepositPot: 4-element array with derived total.
        // CBOR: 0x84 0x01 0x02 0x03 0x04 → [1, 2, 3, 4]
        let v = decode_ntc_result(&QueryCommand::DepositPot, &[0x84, 0x01, 0x02, 0x03, 0x04])
            .expect("decode DepositPot");
        assert_eq!(v["key_deposits_lovelace"], 1);
        assert_eq!(v["pool_deposits_lovelace"], 2);
        assert_eq!(v["drep_deposits_lovelace"], 3);
        assert_eq!(v["proposal_deposits_lovelace"], 4);
        assert_eq!(v["total_lovelace"], 10);

        // LedgerCounts: 6-element array.
        let v = decode_ntc_result(
            &QueryCommand::LedgerCounts,
            &[0x86, 0x05, 0x04, 0x03, 0x02, 0x01, 0x00],
        )
        .expect("decode LedgerCounts");
        assert_eq!(v["stake_credentials"], 5);
        assert_eq!(v["pools"], 4);
        assert_eq!(v["dreps"], 3);
        assert_eq!(v["committee_members"], 2);
        assert_eq!(v["governance_actions"], 1);
        assert_eq!(v["gen_delegs"], 0);
    }

    #[test]
    #[cfg(unix)]
    fn encode_ntc_query_emits_expected_tag_bytes() {
        // Lock in the on-wire byte sequence for every QueryCommand variant
        // so silent tag drift between the CLI encoder and the
        // BasicLocalQueryDispatcher server-side arms surfaces as a failing
        // test.  Every simple (no-parameter) variant produces CBOR
        // `[tag]` == `0x81` + one-byte-unsigned(tag); the four parametric
        // variants produce `[tag, <param>]` which we spot-check separately.
        use super::{QueryCommand, encode_ntc_query};

        let cases: &[(QueryCommand, &[u8])] = &[
            (QueryCommand::CurrentEra, &[0x81, 0x00]),
            (QueryCommand::Tip, &[0x81, 0x01]),
            (QueryCommand::CurrentEpoch, &[0x81, 0x02]),
            (QueryCommand::ProtocolParams, &[0x81, 0x03]),
            (QueryCommand::StakeDistribution, &[0x81, 0x05]),
            (QueryCommand::TreasuryAndReserves, &[0x81, 0x07]),
            (QueryCommand::Constitution, &[0x81, 0x08]),
            (QueryCommand::GovState, &[0x81, 0x09]),
            (QueryCommand::DrepState, &[0x81, 0x0a]),
            (QueryCommand::CommitteeMembersState, &[0x81, 0x0b]),
            (QueryCommand::AccountState, &[0x81, 0x0d]),
            (QueryCommand::StakePools, &[0x81, 0x0f]),
            (QueryCommand::DrepStakeDistr, &[0x81, 0x11]),
            (QueryCommand::GenesisDelegations, &[0x81, 0x12]),
            (QueryCommand::StabilityWindow, &[0x81, 0x13]),
            (QueryCommand::NumDormantEpochs, &[0x81, 0x14]),
            (QueryCommand::ExpectedNetworkId, &[0x81, 0x15]),
            (QueryCommand::DepositPot, &[0x81, 0x16]),
            (QueryCommand::LedgerCounts, &[0x81, 0x17]),
        ];
        for (query, want) in cases {
            let got = encode_ntc_query(query);
            assert_eq!(
                got, *want,
                "encode_ntc_query drifted for {query:?}: expected {want:?}, got {got:?}",
            );
        }
    }

    #[test]
    fn metrics_http_response_routes_json_before_prometheus() {
        // Regression for the `starts_with("GET /metrics")` routing bug:
        // `GET /metrics/json` must reach the JSON arm, not match the
        // shorter `/metrics` prefix first.
        let metrics = yggdrasil_node::tracer::NodeMetrics::default();

        let (status, ctype, body) =
            super::metrics_http_response("GET /metrics/json HTTP/1.1\r\n", &metrics);
        assert_eq!(status, "200 OK");
        assert_eq!(ctype, "application/json");
        // JSON snapshot starts with `{` (not Prometheus `#` or metric name).
        assert!(body.trim_start().starts_with('{'));

        let (status, ctype, body) =
            super::metrics_http_response("GET /metrics HTTP/1.1\r\n", &metrics);
        assert_eq!(status, "200 OK");
        assert!(
            ctype.starts_with("text/plain"),
            "expected Prometheus text content type, got {ctype}",
        );
        assert!(body.contains("# HELP yggdrasil_blocks_synced"));

        // /debug/metrics/json is a documented JSON alias.
        let (_, ctype, body) =
            super::metrics_http_response("GET /debug/metrics/json HTTP/1.1\r\n", &metrics);
        assert_eq!(ctype, "application/json");
        assert!(body.trim_start().starts_with('{'));

        // /debug/metrics (with trailing space) is the JSON alias matching
        // the upstream cardano-tracer debug-dump convention.
        let (_, ctype, body) =
            super::metrics_http_response("GET /debug/metrics HTTP/1.1\r\n", &metrics);
        assert_eq!(ctype, "application/json");
        assert!(body.trim_start().starts_with('{'));

        // /debug/metrics/prometheus is the explicit Prometheus-text alias.
        let (_, ctype, body) = super::metrics_http_response(
            "GET /debug/metrics/prometheus HTTP/1.1\r\n",
            &metrics,
        );
        assert!(ctype.starts_with("text/plain"));
        assert!(body.contains("# HELP"));
    }

    #[test]
    fn validate_config_report_warns_when_checkpoint_interval_exceeds_epoch() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-epoch-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        // Set the cadence to 10× the epoch length — a typical "operator
        // confused slots with epochs" typo shape.
        cfg.checkpoint_interval_slots = cfg.epoch_length * 10;

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("exceeds epoch_length")),
            "expected epoch-bound warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_too_small_checkpoint_interval() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-small-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.checkpoint_interval_slots = 1; // well below the 32-slot soft floor

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("soft floor")),
            "expected soft-floor warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_accepts_checkpoint_interval_at_epoch_length() {
        // Equal-to-epoch must NOT warn; the message reads "at most one per
        // epoch (interval <= epoch_length)" and the boundary is safe.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-at-epoch-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.checkpoint_interval_slots = cfg.epoch_length;

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("exceeds epoch_length")),
            "no epoch-bound warning expected at interval == epoch_length, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_mainnet_requires_magic_override() {
        // Mainnet (magic 764_824_073) canonical default is RequiresNoMagic.
        // An explicit RequiresMagic override is a copy-paste bug that would
        // desync Byron-era header decoding with every other mainnet peer.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-mainnet-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.requires_network_magic =
            Some(yggdrasil_node::config::RequiresNetworkMagic::RequiresMagic);

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
            "expected RequiresNetworkMagic mismatch warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_testnet_requires_no_magic_override() {
        // Any non-mainnet magic's canonical default is RequiresMagic. An
        // explicit RequiresNoMagic override is a copy-paste bug that would
        // desync Byron-era header decoding with testnet peers.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-testnet-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        // Not the mainnet magic.
        cfg.network_magic = 2;
        cfg.requires_network_magic =
            Some(yggdrasil_node::config::RequiresNetworkMagic::RequiresNoMagic);

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
            "expected RequiresNetworkMagic mismatch warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_accepts_canonical_requires_network_magic() {
        // Mainnet with RequiresNoMagic AND testnet with RequiresMagic are
        // both canonical; neither must produce the mismatch warning.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-req-magic-ok-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.requires_network_magic =
            Some(yggdrasil_node::config::RequiresNetworkMagic::RequiresNoMagic);
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
            "mainnet + RequiresNoMagic must not warn, got: {:?}",
            report.warnings,
        );

        // And the None case — default inferred — must not warn either.
        cfg.requires_network_magic = None;
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("RequiresNetworkMagic") && w.contains("inconsistent")),
            "None requires_network_magic must not warn, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_protocol_versions_exceeding_max_major() {
        // Operator proposes major version 99 but max-accept is 10 →
        // forged blocks at 99 would be rejected by this node's own
        // header verification (ObsoleteNode).
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-pv-above-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.max_major_protocol_version = 10;
        cfg.protocol_versions = vec![10, 13, 99];

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report.warnings.iter().any(|w| {
                w.contains("exceeds max_major_protocol_version")
                    && w.contains("13")
                    && w.contains("99")
            }),
            "expected ObsoleteNode warning naming 13 and 99, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_accepts_protocol_versions_at_or_below_max_major() {
        // Boundary: every proposed version == or <= max is safe; equal-to
        // the ceiling is explicitly allowed (upstream `MaxMajorProtVer`
        // comparison is `<=`).
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-pv-ok-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.max_major_protocol_version = 10;
        cfg.protocol_versions = vec![9, 10];

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("exceeds max_major_protocol_version")),
            "no exceeds-max warning expected when all <= max, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_missing_checkpoints_file() {
        // CheckpointsFile set but the path does not exist → warn that
        // checkpoint pinning will be silently disabled at runtime.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-missing-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.checkpoints_file = Some("not-here.json".to_owned());
        cfg.checkpoints_file_hash = None;

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("CheckpointsFile") && w.contains("does not exist")),
            "expected missing-checkpoints-file warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_checkpoints_file_hash_mismatch() {
        // CheckpointsFile + CheckpointsFileHash set, but the file bytes
        // don't hash to the declared value → warn with the mismatch
        // surfaced from `verify_genesis_file_hash`.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-hash-bad-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("checkpoints.json"), b"{}").expect("write ckpt");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.checkpoints_file = Some("checkpoints.json".to_owned());
        // Wrong hash — `{}` does NOT hash to all-zeros.
        cfg.checkpoints_file_hash = Some("0".repeat(64));

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("CheckpointsFile hash verification")),
            "expected checkpoints hash-mismatch warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_accepts_matching_checkpoints_file_hash() {
        // CheckpointsFile points at an existing file AND CheckpointsFileHash
        // is the correct Blake2b-256 digest → no warning.
        use yggdrasil_crypto::hash_bytes_256;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-ckpt-hash-ok-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let ckpt_bytes = b"{\"checkpoints\":[]}";
        std::fs::write(dir.join("checkpoints.json"), ckpt_bytes).expect("write ckpt");
        let correct_hash = hex::encode(hash_bytes_256(ckpt_bytes).0);

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.checkpoints_file = Some("checkpoints.json".to_owned());
        cfg.checkpoints_file_hash = Some(correct_hash);

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("CheckpointsFile")),
            "correct hash must not produce any CheckpointsFile warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_zero_governor_tick() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-gov-tick-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.governor_tick_interval_secs = 0;
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("governor_tick_interval_secs")),
            "expected governor-tick warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_insane_governor_targets() {
        // `target_active > target_established` violates upstream
        // `sanePeerSelectionTargets`; the preflight should flag it.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-gov-targets-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.governor_target_active = 99; // > established, impossible to satisfy
        cfg.governor_target_established = 10;
        cfg.governor_target_known = 20;

        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("sanePeerSelectionTargets")),
            "expected sane-targets warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_on_unsafe_keepalive_interval() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();

        // A value >= 97 collides with the upstream KeepAlive client timeout.
        let dir_hi = std::env::temp_dir().join(format!("yggdrasil-keepalive-hi-{unique}"));
        std::fs::create_dir_all(&dir_hi).expect("temp dir");
        let mut cfg_hi = default_config();
        cfg_hi.storage_dir = PathBuf::from("data");
        cfg_hi.peer_snapshot_file = None;
        cfg_hi.keepalive_interval_secs = Some(120);
        let report = validate_config_report(&cfg_hi, Some(&dir_hi)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("KeepAlive") && w.contains("120")),
            "expected KeepAlive timeout warning for 120s interval, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir_hi).ok();

        // A value of 0 is also called out as wasteful.
        let dir_zero = std::env::temp_dir().join(format!("yggdrasil-keepalive-zero-{unique}"));
        std::fs::create_dir_all(&dir_zero).expect("temp dir");
        let mut cfg_zero = default_config();
        cfg_zero.storage_dir = PathBuf::from("data");
        cfg_zero.peer_snapshot_file = None;
        cfg_zero.keepalive_interval_secs = Some(0);
        let report = validate_config_report(&cfg_zero, Some(&dir_zero)).expect("report");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("keepalive_interval_secs is 0")),
            "expected zero-keepalive warning, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir_zero).ok();
    }

    #[test]
    fn validate_config_report_accepts_sensible_keepalive_interval() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-keepalive-ok-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;
        cfg.keepalive_interval_secs = Some(30); // upstream default ballpark
        let report = validate_config_report(&cfg, Some(&dir)).expect("report");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("keepalive_interval_secs") || w.contains("KeepAlive")),
            "no keepalive warning expected at 30s, got: {:?}",
            report.warnings,
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_rejects_zero_security_param_k() {
        let mut cfg = default_config();
        cfg.security_param_k = 0;
        let err = validate_config_report(&cfg, None)
            .err()
            .expect("zero k must fail");
        assert!(
            err.to_string().contains("security_param_k"),
            "error should mention security_param_k: {err}",
        );
    }

    #[test]
    fn validate_config_report_rejects_zero_epoch_length() {
        let mut cfg = default_config();
        cfg.epoch_length = 0;
        let err = validate_config_report(&cfg, None)
            .err()
            .expect("zero epoch_length must fail");
        assert!(
            err.to_string().contains("epoch_length"),
            "error should mention epoch_length: {err}",
        );
    }

    #[test]
    fn validate_config_report_rejects_zero_byron_epoch_length_with_boundary_set() {
        let mut cfg = default_config();
        cfg.byron_to_shelley_slot = Some(86_400);
        cfg.byron_epoch_length = 0;
        let err = validate_config_report(&cfg, None)
            .err()
            .expect("zero byron_epoch_length with boundary must fail");
        assert!(
            err.to_string().contains("byron_epoch_length"),
            "error should mention byron_epoch_length: {err}",
        );
    }

    #[test]
    fn validate_config_report_allows_zero_byron_epoch_length_without_boundary() {
        // When `byron_to_shelley_slot` is not set, the Byron prefix is
        // inapplicable (e.g. preview testnet) and a zero
        // byron_epoch_length should not abort the preflight.
        let mut cfg = default_config();
        cfg.byron_to_shelley_slot = None;
        cfg.byron_epoch_length = 0;
        // Other fields still sane — no bail expected from this check.
        let result = validate_config_report(&cfg, None);
        // We don't require full success here (other checks may warn or
        // fail depending on storage/genesis), but we DO require that the
        // byron_epoch_length check specifically does not fire.
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("byron_epoch_length"),
                "byron_epoch_length bail should not fire without byron_to_shelley_slot: {e}",
            );
        }
    }

    #[test]
    fn validate_config_report_warns_on_genesis_hash_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("shelley.json"), b"{}").expect("write");
        std::fs::write(dir.path().join("alonzo.json"), b"{}").expect("write");
        std::fs::write(dir.path().join("conway.json"), b"{}").expect("write");

        let mut cfg = default_config();
        cfg.shelley_genesis_file = Some("shelley.json".to_owned());
        cfg.shelley_genesis_hash = Some("0".repeat(64));
        cfg.alonzo_genesis_file = Some("alonzo.json".to_owned());
        cfg.alonzo_genesis_hash = None;
        cfg.conway_genesis_file = Some("conway.json".to_owned());
        cfg.conway_genesis_hash = None;
        // Default mainnet config sets a real Byron path; clear it so the
        // preflight does not also warn about the missing Byron UTxO file.
        cfg.byron_genesis_file = None;
        cfg.byron_genesis_hash = None;
        // storage_dir does not need to exist for the warning we want to
        // assert here; the validate path continues despite that.

        let report = validate_config_report(&cfg, Some(dir.path()))
            .expect("validate succeeds with hash warning");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("genesis hash verification")),
            "expected genesis hash warning in report, got: {:?}",
            report.warnings,
        );
    }

    #[test]
    fn load_effective_config_uses_network_preset_when_file_is_absent() {
        let (cfg, config_base_dir) =
            load_effective_config(None, Some(yggdrasil_node::config::NetworkPreset::Preview))
                .expect("preset config");

        assert_eq!(cfg.network_magic, 2);
        assert_eq!(
            config_base_dir,
            Some(preset_config_base_dir(
                yggdrasil_node::config::NetworkPreset::Preview
            ))
        );
    }

    #[test]
    fn load_effective_config_parses_yaml_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-config-yaml-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let config_path = dir.join("config.yaml");
        std::fs::write(
            &config_path,
            "peer_addr: 127.0.0.1:3001\nnetwork_magic: 42\nprotocol_versions:\n  - 13\n",
        )
        .expect("write yaml config");

        let (cfg, config_base_dir) =
            load_effective_config(Some(config_path.clone()), None).expect("yaml config");

        assert_eq!(cfg.peer_addr, "127.0.0.1:3001".parse().expect("addr"));
        assert_eq!(cfg.network_magic, 42);
        assert_eq!(cfg.protocol_versions, vec![13]);
        assert_eq!(config_base_dir, Some(dir.clone()));

        std::fs::remove_file(config_path).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn forged_header_protocol_version_uses_ledger_protocol_when_present() {
        let mut state = LedgerState::new(Era::Byron);
        let params = yggdrasil_ledger::ProtocolParameters {
            protocol_version: Some((9, 1)),
            ..yggdrasil_ledger::ProtocolParameters::default()
        };
        state.set_protocol_params(params);

        assert_eq!(forged_header_protocol_version(&state, 10), (9, 1));
    }

    #[test]
    fn forged_header_protocol_version_falls_back_to_max_major_protocol_version() {
        let state = LedgerState::new(Era::Byron);
        assert_eq!(forged_header_protocol_version(&state, 10), (10, 0));
    }

    #[test]
    fn validate_config_report_warns_when_peer_snapshot_file_is_missing() {
        let (cfg, config_base_dir) =
            load_effective_config(None, Some(yggdrasil_node::config::NetworkPreset::Preview))
                .expect("preset config");

        let report =
            validate_config_report(&cfg, config_base_dir.as_deref()).expect("validation report");

        assert_eq!(report.peer_snapshot.status, "unavailable");
        assert!(report.peer_snapshot.error.is_some());
        assert!(
            report.warnings.iter().any(
                |warning| warning.contains("configured peer snapshot file could not be loaded")
            )
        );
    }

    #[test]
    fn status_report_shows_uninitialized_when_storage_absent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-empty-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");

        assert!(!report.storage_initialized);
        assert_eq!(report.immutable_block_count, 0);
        assert_eq!(report.volatile_block_count, 0);
        assert_eq!(report.ledger_checkpoint_count, 0);
        assert!(report.chain_tip_slot.is_none());
        // Ledger-derived fields must be absent on uninitialized storage.
        assert!(report.current_era.is_none());
        assert!(report.current_epoch.is_none());
        assert!(report.ledger_counts.is_none());

        // And the JSON serialisation must elide them (skip_serializing_if
        // = Option::is_none) so pre-existing consumers see no breaking
        // change when the data is absent.
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(
            !json.contains("current_era"),
            "current_era key should be absent when unset, got: {json}",
        );
        assert!(
            !json.contains("current_epoch"),
            "current_epoch key should be absent when unset, got: {json}",
        );
        assert!(
            !json.contains("ledger_counts"),
            "ledger_counts key should be absent when unset, got: {json}",
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_report_shows_initialized_when_storage_exists() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-init-{unique}"));
        let data_dir = dir.join("data");
        std::fs::create_dir_all(data_dir.join("immutable")).expect("immutable dir");
        std::fs::create_dir_all(data_dir.join("volatile")).expect("volatile dir");
        std::fs::create_dir_all(data_dir.join("ledger")).expect("ledger dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");

        assert!(report.storage_initialized);
        assert_eq!(report.immutable_block_count, 0);
        assert_eq!(report.volatile_block_count, 0);
        assert!(report.chain_tip.contains("Origin"));

        // Storage is present so recovery succeeds from an empty state;
        // the ledger-counts summary should therefore be populated, and all
        // six cardinalities should be zero on a fresh node.
        let counts = report
            .ledger_counts
            .as_ref()
            .expect("ledger counts present when storage is initialized");
        assert_eq!(counts.stake_credentials, 0);
        assert_eq!(counts.pools, 0);
        assert_eq!(counts.dreps, 0);
        assert_eq!(counts.committee_members, 0);
        assert_eq!(counts.governance_actions, 0);
        assert_eq!(counts.gen_delegs, 0);

        // Era + epoch are populated on a successful recovery. A fresh
        // ledger starts in Byron era, epoch 0 until blocks advance it.
        assert_eq!(
            report.current_era.as_deref(),
            Some("Byron"),
            "fresh ledger should report Byron era",
        );
        assert_eq!(
            report.current_epoch,
            Some(0),
            "fresh ledger should report epoch 0",
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_report_serializes_to_json() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-json-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");
        let json = serde_json::to_string_pretty(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(
            parsed["network_magic"],
            serde_json::Value::from(764_824_073u64)
        );
        assert_eq!(
            parsed["storage_initialized"],
            serde_json::Value::Bool(false)
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_topology_override_from_cli_flag() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-topo-override-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let topo_path = dir.join("topology.json");
        std::fs::write(
            &topo_path,
            r#"{
                "bootstrapPeers": [
                    {"address": "127.0.0.50", "port": 3001}
                ],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 77000
            }"#,
        )
        .expect("write topology file");

        let mut cfg = default_config();
        cfg.use_ledger_after_slot = None;
        cfg.peer_snapshot_file = None;

        apply_topology_override(&mut cfg, Some(topo_path.as_path()), None)
            .expect("apply topology override");

        assert_eq!(cfg.use_ledger_after_slot, Some(77000));
        assert_eq!(cfg.peer_addr, "127.0.0.50:3001".parse().expect("addr"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_topology_override_from_config_topology_file_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-topo-config-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let topo_path = dir.join("my-topology.json");
        std::fs::write(
            &topo_path,
            r#"{
                "bootstrapPeers": [],
                "localRoots": [
                    {
                        "accessPoints": [
                            {"address": "127.0.0.60", "port": 3001}
                        ],
                        "advertise": false,
                        "valency": 1,
                        "trustable": true
                    }
                ],
                "publicRoots": [],
                "useLedgerAfterSlot": 55000
            }"#,
        )
        .expect("write topology file");

        let mut cfg = default_config();
        cfg.topology_file_path = Some("my-topology.json".to_owned());
        cfg.use_ledger_after_slot = None;

        apply_topology_override(&mut cfg, None, Some(dir.as_path()))
            .expect("apply topology from config key");

        assert_eq!(cfg.use_ledger_after_slot, Some(55000));
        assert_eq!(cfg.local_roots.len(), 1);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_topology_override_cli_takes_priority_over_config_key() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-topo-priority-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let config_topo = dir.join("config-topology.json");
        std::fs::write(
            &config_topo,
            r#"{
                "bootstrapPeers": [],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 11000
            }"#,
        )
        .expect("write config topology");

        let cli_topo = dir.join("cli-topology.json");
        std::fs::write(
            &cli_topo,
            r#"{
                "bootstrapPeers": [],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 22000
            }"#,
        )
        .expect("write cli topology");

        let mut cfg = default_config();
        cfg.topology_file_path = Some(config_topo.display().to_string());

        apply_topology_override(&mut cfg, Some(cli_topo.as_path()), Some(dir.as_path()))
            .expect("apply topology");

        // CLI topology (22000) should win over config TopologyFilePath (11000).
        assert_eq!(cfg.use_ledger_after_slot, Some(22000));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_topology_override_noop_when_neither_cli_nor_config() {
        let mut cfg = default_config();
        cfg.topology_file_path = None;
        let original_ledger_slot = cfg.use_ledger_after_slot;

        apply_topology_override(&mut cfg, None, None).expect("apply topology no-op");

        assert_eq!(cfg.use_ledger_after_slot, original_ledger_slot);
    }
}
