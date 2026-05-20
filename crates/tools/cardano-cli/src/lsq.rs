//! LSQ-client abstraction for library-side `query-*` dispatch.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Rust-idiomatic indirection that upstream
//! Haskell's monomorphic call-graph doesn't need: upstream
//! `Cardano.CLI.*.Run.*` modules call
//! `Cardano.Api.queryNodeLocalState` inline, threading the network-
//! magic + socket-path through `LocalNodeConnectInfo`. The Rust port
//! needs a trait here because `yggdrasil-cardano-cli` keeps its
//! dependency footprint configurable (`tokio` and `yggdrasil-network`
//! live behind the `lsq-tokio` feature). The library dispatches
//! `query-*` and `transaction submit` through a `&dyn LsqClient` so
//! callers can plug the concrete [`crate::lsq_tokio::TokioLsqClient`]
//! when socket access is available, or use [`DeferralLsqClient`] in a
//! slim build.
//!
//! The trait is intentionally **synchronous-facing** at the library
//! boundary even though concrete impls are tokio-async internally:
//! the impl is responsible for constructing its own runtime + driving
//! the future to completion. That keeps the library `run_command`
//! signature plain `fn(...) -> Result<()>` rather than `async fn`.
//!
//! ## Wiring shape
//!
//! - Library defines the [`NtcQuery`] enum (one variant per
//!   LocalStateQuery the library dispatches) and the [`LsqClient`]
//!   trait with a single [`LsqClient::run_query`] method taking an
//!   `NtcQuery`. Adding a new `query-*` subcommand is one enum
//!   variant + one decoder in the concrete impl - the trait surface
//!   stays a single method regardless of how many queries land.
//! - Library's [`crate::run::run_command_with`] takes
//!   `&dyn LsqClient` and dispatches each `Command::Query*` variant
//!   through it. The simpler [`crate::run::run_command`] (no client)
//!   plugs in [`DeferralLsqClient`].
//! - [`DeferralLsqClient`] is the in-crate "no concrete impl wired"
//!   sentinel: its `run_query` returns the documented eyre error
//!   pointing operators at the node binary's wrapper.
//!
//! The trait method is presentation-aware: implementations own the
//! stdout formatting. The library's job is to dispatch the variant;
//! the impl's job is to drive the wire protocol + render.

use std::path::Path;

use eyre::Result;
use serde_json::json;
use yggdrasil_ledger::{Decoder, Encoder};

/// A LocalStateQuery the library can ask a running node to answer.
///
/// Each variant maps 1:1 to a `query-*` subcommand. The concrete
/// [`LsqClient`] impl owns the per-variant CBOR encode + decode; the
/// library only carries the variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NtcQuery {
    /// `query-tip` - chain point (`GetChainPoint`).
    Tip,
    /// `query-chain-block-no` - current chain block number
    /// (`GetChainBlockNo`).
    ChainBlockNo,
    /// `query-current-era` - current ledger era (`GetCurrentEra`).
    CurrentEra,
    /// `query-system-start` - system start time (`GetSystemStart`).
    SystemStart,
    /// `query-era-history` - hard-fork era interpreter
    /// (`GetInterpreter`).
    EraHistory,
    /// `query-current-epoch` - current epoch number.
    CurrentEpoch,
    /// `query-stake-distribution` - per-pool active-stake
    /// distribution.
    StakeDistribution,
    /// `query-stake-pools` - the set of registered stake-pool ids.
    StakePools,
    /// `query-protocol-parameters` - the current protocol
    /// parameters.
    ProtocolParameters,
    /// `query-utxo --address` - UTxO entries for one raw address.
    UtxoByAddress {
        /// Raw address bytes decoded from the CLI hex argument.
        address: Vec<u8>,
    },
    /// `query-reward-balance --account` - reward-account balance.
    RewardBalance {
        /// Raw reward-account bytes decoded from the CLI hex argument.
        account: Vec<u8>,
    },
    /// `query-utxo --tx-in` - UTxO entries for one transaction input.
    UtxoByTxIn {
        /// Raw 32-byte transaction id decoded from the CLI hex argument.
        tx_id: Vec<u8>,
        /// Output index within `tx_id`.
        index: u16,
    },
    /// `query-delegations-and-rewards --credential` - delegation and
    /// reward entries for one stake credential.
    DelegationsAndRewards {
        /// Raw credential hash decoded from the CLI hex argument.
        credential: Vec<u8>,
        /// `true` for key hash, `false` for script hash.
        is_key_hash: bool,
    },
    /// `query-stake-pool-params --pool-hash` - registered pool
    /// parameters for one pool id.
    StakePoolParams {
        /// Raw pool key hash decoded from the CLI hex argument.
        pool_hash: Vec<u8>,
    },
    /// `query-drep-stake-distribution` - per-DRep stake distribution
    /// (Conway governance).
    DrepStakeDistribution,
    /// `query-constitution` - the current on-chain constitution
    /// (Conway governance).
    Constitution,
    /// `query-gov-state` - the governance-action state (Conway).
    GovState,
    /// `query-drep-state` - the registered-DRep state (Conway).
    DrepState,
    /// `query-committee-state` - the constitutional-committee state
    /// (Conway governance).
    CommitteeState,
    /// `query-treasury-and-reserves` - the treasury + reserves pots.
    TreasuryAndReserves,
    /// `query-account-state` - treasury / reserves / total-deposits.
    AccountState,
    /// `query-genesis-delegations` - the genesis-delegation map.
    GenesisDelegations,
    /// `query-stability-window` - the stability-window slot count.
    StabilityWindow,
    /// `query-num-dormant-epochs` - the Conway dormant-epoch counter.
    NumDormantEpochs,
    /// `query-expected-network-id` - the node's configured network id.
    ExpectedNetworkId,
    /// `query-deposit-pot` - the key/pool/DRep/proposal deposit pots.
    DepositPot,
    /// `query-ledger-counts` - ledger-state cardinality counters.
    LedgerCounts,
}

impl NtcQuery {
    /// The `query-*` subcommand name this query backs - used for
    /// error messages (`DeferralLsqClient`) + the error context an
    /// impl wraps a connection failure with.
    pub fn subcommand_name(&self) -> &'static str {
        match self {
            NtcQuery::Tip => "query-tip",
            NtcQuery::ChainBlockNo => "query-chain-block-no",
            NtcQuery::CurrentEra => "query-current-era",
            NtcQuery::SystemStart => "query-system-start",
            NtcQuery::EraHistory => "query-era-history",
            NtcQuery::CurrentEpoch => "query-current-epoch",
            NtcQuery::StakeDistribution => "query-stake-distribution",
            NtcQuery::StakePools => "query-stake-pools",
            NtcQuery::ProtocolParameters => "query-protocol-parameters",
            NtcQuery::UtxoByAddress { .. } | NtcQuery::UtxoByTxIn { .. } => "query-utxo",
            NtcQuery::RewardBalance { .. } => "query-reward-balance",
            NtcQuery::DelegationsAndRewards { .. } => "query-delegations-and-rewards",
            NtcQuery::StakePoolParams { .. } => "query-stake-pool-params",
            NtcQuery::DrepStakeDistribution => "query-drep-stake-distribution",
            NtcQuery::Constitution => "query-constitution",
            NtcQuery::GovState => "query-gov-state",
            NtcQuery::DrepState => "query-drep-state",
            NtcQuery::CommitteeState => "query-committee-state",
            NtcQuery::TreasuryAndReserves => "query-treasury-and-reserves",
            NtcQuery::AccountState => "query-account-state",
            NtcQuery::GenesisDelegations => "query-genesis-delegations",
            NtcQuery::StabilityWindow => "query-stability-window",
            NtcQuery::NumDormantEpochs => "query-num-dormant-epochs",
            NtcQuery::ExpectedNetworkId => "query-expected-network-id",
            NtcQuery::DepositPot => "query-deposit-pot",
            NtcQuery::LedgerCounts => "query-ledger-counts",
        }
    }
}

/// Lenient hex decoder shared by LocalStateQuery argument encoders.
///
/// Accepts whitespace around the argument and an optional `0x` prefix. Invalid
/// hex returns an empty byte vector to preserve the historical query behavior:
/// the CBOR envelope stays well-formed and the node-side query simply matches
/// no ledger entries.
pub fn decode_optional_prefixed_hex(raw: &str) -> Vec<u8> {
    let stripped = raw.trim();
    let stripped = stripped.strip_prefix("0x").unwrap_or(stripped);
    hex::decode(stripped).unwrap_or_default()
}

/// Format a Cardano `SystemStart` triple as an ISO 8601 UTC timestamp.
///
/// The inputs mirror upstream `Cardano.Slotting.Time.SystemStart`: Gregorian
/// `year`, 1-based `dayOfYear`, and `picosecondsOfDay`. The conversion uses
/// proleptic Gregorian leap-year rules and floors picoseconds to whole seconds
/// for the rendered `YYYY-MM-DDThh:mm:ssZ` value.
pub fn format_utc_time(year: u64, day_of_year: u64, picoseconds_of_day: u64) -> String {
    let is_leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days: [u64; 12] = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut remaining = day_of_year.saturating_sub(1);
    let mut month: u32 = 1;
    let mut day_of_month: u32 = 1;
    for (idx, &md) in month_days.iter().enumerate() {
        if remaining < md {
            month = (idx as u32) + 1;
            day_of_month = (remaining as u32) + 1;
            break;
        }
        remaining -= md;
    }
    let total_seconds = picoseconds_of_day / 1_000_000_000_000;
    let hour = (total_seconds / 3600) % 24;
    let minute = (total_seconds / 60) % 60;
    let second = total_seconds % 60;
    format!("{year:04}-{month:02}-{day_of_month:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Decoder for one LSQ reply. It turns raw CBOR result bytes into
/// the JSON value the subcommand prints.
pub(crate) type LsqReplyDecoder = fn(&[u8]) -> serde_json::Value;

/// The per-query wire plan: CBOR query envelope, upstream query label
/// for error context, and the matching reply decoder.
pub(crate) struct QueryPlan {
    pub(crate) query_bytes: Vec<u8>,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) query_label: &'static str,
    pub(crate) decode: LsqReplyDecoder,
}

/// CBOR-encode a migrated LocalStateQuery variant.
///
/// This is the shared wire surface used by the standalone
/// `yggdrasil-cardano-cli` binary and the node binary's compatibility
/// query bridge for variants that have moved into this crate.
pub fn encode_query(query: NtcQuery) -> Vec<u8> {
    plan_for(query).query_bytes
}

/// Decode the CBOR reply for a migrated LocalStateQuery variant.
pub fn decode_query_result(query: NtcQuery, result: &[u8]) -> serde_json::Value {
    let plan = plan_for(query);
    (plan.decode)(result)
}

/// Map an [`NtcQuery`] variant to its CBOR encode + reply decoder.
pub(crate) fn plan_for(query: NtcQuery) -> QueryPlan {
    match query {
        NtcQuery::Tip => QueryPlan {
            query_bytes: encode_get_chain_point_query(),
            query_label: "GetChainPoint",
            decode: decode_chain_point_result,
        },
        NtcQuery::ChainBlockNo => QueryPlan {
            query_bytes: encode_get_chain_block_no_query(),
            query_label: "GetChainBlockNo",
            decode: decode_chain_block_no_result,
        },
        NtcQuery::CurrentEra => QueryPlan {
            query_bytes: encode_get_current_era_query(),
            query_label: "GetCurrentEra",
            decode: decode_current_era_result,
        },
        NtcQuery::SystemStart => QueryPlan {
            query_bytes: encode_get_system_start_query(),
            query_label: "GetSystemStart",
            decode: decode_system_start_result,
        },
        NtcQuery::EraHistory => QueryPlan {
            query_bytes: encode_get_era_history_query(),
            query_label: "GetInterpreter",
            decode: decode_era_history_result,
        },
        NtcQuery::CurrentEpoch => QueryPlan {
            query_bytes: encode_single_tag_query(101),
            query_label: "GetCurrentEpoch",
            decode: decode_current_epoch_result,
        },
        NtcQuery::StakeDistribution => QueryPlan {
            query_bytes: encode_single_tag_query(5),
            query_label: "GetStakeDistribution",
            decode: decode_stake_distribution_result,
        },
        NtcQuery::StakePools => QueryPlan {
            query_bytes: encode_single_tag_query(15),
            query_label: "GetStakePools",
            decode: decode_stake_pools_result,
        },
        NtcQuery::ProtocolParameters => QueryPlan {
            query_bytes: encode_single_tag_query(102),
            query_label: "GetProtocolParameters",
            decode: decode_protocol_parameters_result,
        },
        NtcQuery::UtxoByAddress { address } => QueryPlan {
            query_bytes: encode_utxo_by_address_query(&address),
            query_label: "GetUTxOByAddress",
            decode: decode_utxo_result,
        },
        NtcQuery::RewardBalance { account } => QueryPlan {
            query_bytes: encode_reward_balance_query(&account),
            query_label: "GetRewardBalance",
            decode: decode_reward_balance_result,
        },
        NtcQuery::UtxoByTxIn { tx_id, index } => QueryPlan {
            query_bytes: encode_utxo_by_tx_in_query(&tx_id, index),
            query_label: "GetUTxOByTxIn",
            decode: decode_utxo_result,
        },
        NtcQuery::DelegationsAndRewards {
            credential,
            is_key_hash,
        } => QueryPlan {
            query_bytes: encode_delegations_and_rewards_query(&credential, is_key_hash),
            query_label: "GetDelegationsAndRewards",
            decode: decode_delegations_and_rewards_result,
        },
        NtcQuery::StakePoolParams { pool_hash } => QueryPlan {
            query_bytes: encode_stake_pool_params_query(&pool_hash),
            query_label: "GetStakePoolParams",
            decode: decode_stake_pool_params_result,
        },
        NtcQuery::DrepStakeDistribution => QueryPlan {
            query_bytes: encode_single_tag_query(17),
            query_label: "GetDRepStakeDistribution",
            decode: decode_drep_stake_distribution_result,
        },
        NtcQuery::Constitution => QueryPlan {
            query_bytes: encode_single_tag_query(8),
            query_label: "GetConstitution",
            decode: decode_constitution_result,
        },
        NtcQuery::GovState => QueryPlan {
            query_bytes: encode_single_tag_query(9),
            query_label: "GetGovState",
            decode: decode_gov_state_result,
        },
        NtcQuery::DrepState => QueryPlan {
            query_bytes: encode_single_tag_query(10),
            query_label: "GetDRepState",
            decode: decode_drep_state_result,
        },
        NtcQuery::CommitteeState => QueryPlan {
            query_bytes: encode_single_tag_query(11),
            query_label: "GetCommitteeState",
            decode: decode_committee_state_result,
        },
        NtcQuery::TreasuryAndReserves => QueryPlan {
            query_bytes: encode_single_tag_query(7),
            query_label: "GetTreasuryAndReserves",
            decode: decode_treasury_and_reserves_result,
        },
        NtcQuery::AccountState => QueryPlan {
            query_bytes: encode_single_tag_query(13),
            query_label: "GetAccountState",
            decode: decode_account_state_result,
        },
        NtcQuery::GenesisDelegations => QueryPlan {
            query_bytes: encode_single_tag_query(18),
            query_label: "GetGenesisDelegations",
            decode: decode_genesis_delegations_result,
        },
        NtcQuery::StabilityWindow => QueryPlan {
            query_bytes: encode_single_tag_query(19),
            query_label: "GetStabilityWindow",
            decode: decode_stability_window_result,
        },
        NtcQuery::NumDormantEpochs => QueryPlan {
            query_bytes: encode_single_tag_query(20),
            query_label: "GetNumDormantEpochs",
            decode: decode_num_dormant_epochs_result,
        },
        NtcQuery::ExpectedNetworkId => QueryPlan {
            query_bytes: encode_single_tag_query(21),
            query_label: "GetExpectedNetworkId",
            decode: decode_expected_network_id_result,
        },
        NtcQuery::DepositPot => QueryPlan {
            query_bytes: encode_single_tag_query(22),
            query_label: "GetDepositPot",
            decode: decode_deposit_pot_result,
        },
        NtcQuery::LedgerCounts => QueryPlan {
            query_bytes: encode_single_tag_query(23),
            query_label: "GetLedgerCounts",
            decode: decode_ledger_counts_result,
        },
    }
}

pub(crate) fn decode_treasury_and_reserves_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    if dec.array().ok() == Some(2) {
        let treasury = dec.unsigned().unwrap_or(0);
        let reserves = dec.unsigned().unwrap_or(0);
        json!({ "treasury_lovelace": treasury, "reserves_lovelace": reserves })
    } else {
        json!({ "result_cbor": hex::encode(result) })
    }
}

pub(crate) fn decode_account_state_result(result: &[u8]) -> serde_json::Value {
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
        json!({ "account_state_cbor": hex::encode(result) })
    }
}

pub(crate) fn decode_genesis_delegations_result(result: &[u8]) -> serde_json::Value {
    json!({ "genesis_delegations_cbor": hex::encode(result) })
}

pub(crate) fn decode_stability_window_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "stability_window": serde_json::Value::Null })
    } else {
        let mut dec = Decoder::new(result);
        json!({ "stability_window_slots": dec.unsigned().unwrap_or(0) })
    }
}

pub(crate) fn decode_num_dormant_epochs_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    json!({ "num_dormant_epochs": dec.unsigned().unwrap_or(0) })
}

pub(crate) fn encode_utxo_by_address_query(address: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(4u64).bytes(address);
    enc.into_bytes()
}

pub(crate) fn decode_utxo_result(result: &[u8]) -> serde_json::Value {
    json!({ "utxo_cbor": hex::encode(result) })
}

pub(crate) fn encode_reward_balance_query(account: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(6u64).bytes(account);
    enc.into_bytes()
}

pub(crate) fn decode_reward_balance_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    json!({ "reward_balance_lovelace": dec.unsigned().unwrap_or(0) })
}

pub(crate) fn encode_utxo_by_tx_in_query(tx_id: &[u8], index: u16) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(14u64);
    enc.array(1);
    enc.array(2).bytes(tx_id).unsigned(index as u64);
    enc.into_bytes()
}

pub(crate) fn encode_delegations_and_rewards_query(
    credential: &[u8],
    is_key_hash: bool,
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(16u64);
    enc.array(1);
    enc.array(2);
    enc.unsigned(if is_key_hash { 0u64 } else { 1u64 });
    enc.bytes(credential);
    enc.into_bytes()
}

pub(crate) fn decode_delegations_and_rewards_result(result: &[u8]) -> serde_json::Value {
    json!({ "delegations_and_rewards_cbor": hex::encode(result) })
}

pub(crate) fn encode_stake_pool_params_query(pool_hash: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(12u64).bytes(pool_hash);
    enc.into_bytes()
}

pub(crate) fn decode_stake_pool_params_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "pool": serde_json::Value::Null })
    } else {
        json!({ "pool_cbor": hex::encode(result) })
    }
}

/// Build the shared LSQ variant for `query-utxo --address`.
pub fn utxo_by_address_query_arg(address: &str) -> NtcQuery {
    NtcQuery::UtxoByAddress {
        address: decode_optional_prefixed_hex(address),
    }
}

/// Build the shared LSQ variant for `query-utxo --tx-in TX_HASH#INDEX`.
pub fn utxo_by_tx_in_query_arg(tx_in: &str) -> Result<NtcQuery> {
    let (tx_id, index_str) = tx_in.split_once('#').ok_or_else(|| {
        eyre::eyre!("--tx-in expects TX_HASH#INDEX (e.g. 0123ab...#0); got {tx_in:?}")
    })?;
    let index: u16 = index_str
        .parse()
        .map_err(|e| eyre::eyre!("--tx-in index {index_str:?} is not a valid u16: {e}"))?;
    Ok(NtcQuery::UtxoByTxIn {
        tx_id: decode_optional_prefixed_hex(tx_id),
        index,
    })
}

/// Build the shared LSQ variant for `query-reward-balance --account`.
pub fn reward_balance_query_arg(account: &str) -> NtcQuery {
    NtcQuery::RewardBalance {
        account: decode_optional_prefixed_hex(account),
    }
}

/// Build the shared LSQ variant for
/// `query-delegations-and-rewards --credential`.
pub fn delegations_and_rewards_query_arg(credential: &str, is_key_hash: bool) -> NtcQuery {
    NtcQuery::DelegationsAndRewards {
        credential: decode_optional_prefixed_hex(credential),
        is_key_hash,
    }
}

/// Build the shared LSQ variant for `query-stake-pool-params --pool-hash`.
pub fn stake_pool_params_query_arg(pool_hash: &str) -> NtcQuery {
    NtcQuery::StakePoolParams {
        pool_hash: decode_optional_prefixed_hex(pool_hash),
    }
}

pub(crate) fn decode_current_epoch_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    json!({ "epoch": dec.unsigned().unwrap_or(0) })
}

pub(crate) fn decode_era_history_result(result: &[u8]) -> serde_json::Value {
    json!({ "era_history_cbor": hex::encode(result) })
}

pub(crate) fn decode_expected_network_id_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "expected_network_id": serde_json::Value::Null })
    } else {
        let mut dec = Decoder::new(result);
        json!({ "expected_network_id": dec.unsigned().unwrap_or(0) })
    }
}

pub(crate) fn decode_deposit_pot_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    if dec.array().ok() == Some(4) {
        let key = dec.unsigned().unwrap_or(0);
        let pool = dec.unsigned().unwrap_or(0);
        let drep = dec.unsigned().unwrap_or(0);
        let proposal = dec.unsigned().unwrap_or(0);
        json!({
            "key_deposits_lovelace": key,
            "pool_deposits_lovelace": pool,
            "drep_deposits_lovelace": drep,
            "proposal_deposits_lovelace": proposal,
            "total_lovelace": key + pool + drep + proposal,
        })
    } else {
        json!({ "deposit_pot_cbor": hex::encode(result) })
    }
}

pub(crate) fn decode_ledger_counts_result(result: &[u8]) -> serde_json::Value {
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
        json!({ "ledger_counts_cbor": hex::encode(result) })
    }
}

pub(crate) fn decode_drep_stake_distribution_result(result: &[u8]) -> serde_json::Value {
    json!({ "drep_stake_distribution_cbor": hex::encode(result) })
}

pub(crate) fn decode_constitution_result(result: &[u8]) -> serde_json::Value {
    json!({ "constitution_cbor": hex::encode(result) })
}

pub(crate) fn decode_gov_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "governance_actions_cbor": hex::encode(result) })
}

pub(crate) fn decode_drep_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "drep_state_cbor": hex::encode(result) })
}

pub(crate) fn decode_committee_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "committee_state_cbor": hex::encode(result) })
}

pub(crate) fn encode_single_tag_query(tag: u64) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(tag);
    enc.into_bytes()
}

pub(crate) fn decode_stake_distribution_result(result: &[u8]) -> serde_json::Value {
    json!({ "stake_distribution_cbor": hex::encode(result) })
}

pub(crate) fn decode_stake_pools_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    let mut pools: Vec<String> = Vec::new();
    if let Ok(n) = dec.array() {
        for _ in 0..n {
            if let Ok(hash) = dec.bytes() {
                pools.push(hex::encode(hash));
            }
        }
    }
    json!({ "stake_pools": pools, "count": pools.len() })
}

pub(crate) fn decode_protocol_parameters_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "protocol_parameters": serde_json::Value::Null })
    } else {
        json!({ "protocol_parameters": hex::encode(result) })
    }
}

pub(crate) fn encode_get_chain_point_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(3u64);
    enc.into_bytes()
}

pub(crate) fn encode_get_chain_block_no_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(2u64);
    enc.into_bytes()
}

pub(crate) fn decode_chain_block_no_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    match dec.array() {
        Ok(1) => json!({ "chain_block_no": serde_json::Value::Null }),
        Ok(2) => {
            let _tag = dec.unsigned().unwrap_or(1);
            let block_no = dec.unsigned().unwrap_or(0);
            json!({ "chain_block_no": block_no })
        }
        _ => json!({ "chain_block_no_cbor": hex::encode(result) }),
    }
}

pub(crate) fn encode_get_current_era_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0u64);
    enc.array(2).unsigned(2u64);
    enc.array(1).unsigned(1u64);
    enc.into_bytes()
}

pub(crate) fn decode_current_era_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    let era = match dec.array() {
        Ok(1) => dec.unsigned().unwrap_or(0),
        _ => 0,
    };
    json!({ "era": era })
}

pub(crate) fn encode_get_system_start_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(1u64);
    enc.into_bytes()
}

pub(crate) fn encode_get_era_history_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0u64);
    enc.array(2).unsigned(2u64);
    enc.array(1).unsigned(0u64);
    enc.into_bytes()
}

pub(crate) fn decode_system_start_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    match dec.array() {
        Ok(3) => {
            let year = dec.unsigned().unwrap_or(0);
            let day_of_year = dec.unsigned().unwrap_or(1);
            let picoseconds_of_day = dec.unsigned().unwrap_or(0);
            json!({
                "system_start": {
                    "year": year,
                    "dayOfYear": day_of_year,
                    "picosecondsOfDay": picoseconds_of_day,
                    "time": format_utc_time(year, day_of_year, picoseconds_of_day),
                }
            })
        }
        _ => json!({ "system_start_cbor": hex::encode(result) }),
    }
}

pub(crate) fn decode_chain_point_result(result: &[u8]) -> serde_json::Value {
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

/// Node-to-client operations surface the library dispatches through.
///
/// Strict mirror: none. Rust-side trait abstraction over the NtC
/// mini-protocols `Cardano.Api` exposes inline.
///
/// The name retains the historical `Lsq` (it began as a
/// LocalStateQuery-only abstraction); it now also carries
/// [`LsqClient::submit_tx`] (LocalTxSubmission). The two NtC
/// operations the standalone binary needs - query + submit - share
/// the same socket / runtime construction, so one trait + one
/// concrete impl serves both.
///
/// Concrete implementations:
///
/// - [`DeferralLsqClient`] - bails with a structured deferral error;
///   in-crate sentinel used until a real impl is wired.
/// - `TokioLsqClient` (in this crate, behind the `lsq-tokio`
///   feature) - builds a `tokio` runtime per call, opens a
///   Unix-socket NtC connection through `yggdrasil-network`, drives
///   the relevant mini-protocol, prints the JSON envelope.
pub trait LsqClient {
    /// Run one [`NtcQuery`] against the node and render the result
    /// as JSON.
    ///
    /// The impl owns stdout formatting + socket-connection
    /// construction; the library only dispatches the variant.
    ///
    /// # Parameters
    ///
    /// - `socket_path` - NtC Unix domain socket path
    ///   (`$CARDANO_NODE_SOCKET_PATH`).
    /// - `network_magic` - protocol magic for the handshake
    ///   (mainnet=764_824_073 / preprod=1 / preview=2 / custom).
    /// - `query` - which [`NtcQuery`] to run.
    fn run_query(&self, socket_path: &Path, network_magic: u32, query: NtcQuery) -> Result<()>;

    /// Submit a serialized transaction via the LocalTxSubmission
    /// mini-protocol and render the accept/reject outcome as JSON.
    ///
    /// Shared submit path used by the standalone cardano-cli binary
    /// and the node binary's compatibility wrapper. `tx_bytes` is
    /// the complete CBOR transaction.
    fn submit_tx(&self, socket_path: &Path, network_magic: u32, tx_bytes: &[u8]) -> Result<()>;
}

/// In-crate "no concrete LSQ impl wired" sentinel.
///
/// Used by library-side tests + by callers (e.g. the standalone
/// `yggdrasil-cardano-cli` binary's `main.rs`) that don't plug a
/// real client through. Both methods return the documented
/// deferral error pointing operators at the node binary's wrapper.
pub struct DeferralLsqClient;

impl LsqClient for DeferralLsqClient {
    fn run_query(&self, _socket_path: &Path, _network_magic: u32, query: NtcQuery) -> Result<()> {
        deferral_bail(query.subcommand_name())
    }

    fn submit_tx(&self, _socket_path: &Path, _network_magic: u32, _tx_bytes: &[u8]) -> Result<()> {
        deferral_bail("transaction-submit")
    }
}

/// Shared deferral error for [`DeferralLsqClient`] - every NtC
/// operation bails the same way when no concrete impl is wired.
fn deferral_bail(subcommand: &str) -> Result<()> {
    eyre::bail!(
        "{subcommand}: today's library crate doesn't carry the tokio + yggdrasil-network \
         deps needed to open a NtC socket; use the node binary's \
         `yggdrasil-node cardano-cli {subcommand} ...` subcommand for now. \
         Library-side wiring lands once a concrete `LsqClient` impl is plugged in \
         at the binary entry-point."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `DeferralLsqClient::run_query` returns the structured deferral
    /// error - naming the right subcommand - for every `NtcQuery`
    /// variant. Pins the operator-facing message.
    #[test]
    fn deferral_client_bails_with_structured_error() {
        let client = DeferralLsqClient;
        let socket = PathBuf::from("/unused.socket");
        for query in [
            NtcQuery::Tip,
            NtcQuery::ChainBlockNo,
            NtcQuery::CurrentEra,
            NtcQuery::SystemStart,
            NtcQuery::EraHistory,
            NtcQuery::CurrentEpoch,
            NtcQuery::StakeDistribution,
            NtcQuery::StakePools,
            NtcQuery::ProtocolParameters,
            NtcQuery::UtxoByAddress {
                address: vec![0xaa],
            },
            NtcQuery::RewardBalance {
                account: vec![0xbb],
            },
            NtcQuery::UtxoByTxIn {
                tx_id: vec![0xcc],
                index: 1,
            },
            NtcQuery::DelegationsAndRewards {
                credential: vec![0xdd],
                is_key_hash: true,
            },
            NtcQuery::StakePoolParams {
                pool_hash: vec![0xee],
            },
            NtcQuery::DrepStakeDistribution,
            NtcQuery::Constitution,
            NtcQuery::GovState,
            NtcQuery::DrepState,
            NtcQuery::CommitteeState,
            NtcQuery::TreasuryAndReserves,
            NtcQuery::AccountState,
            NtcQuery::GenesisDelegations,
            NtcQuery::StabilityWindow,
            NtcQuery::NumDormantEpochs,
            NtcQuery::ExpectedNetworkId,
            NtcQuery::DepositPot,
            NtcQuery::LedgerCounts,
        ] {
            let subcommand = query.subcommand_name();
            let err = client
                .run_query(&socket, 764_824_073, query)
                .expect_err("DeferralLsqClient must bail");
            let msg = err.to_string();
            assert!(
                msg.contains(subcommand) && msg.contains("LsqClient"),
                "error must name the subcommand + point at LsqClient wiring; got {msg}"
            );
        }
    }

    /// A custom `LsqClient` impl can plug in arbitrary behavior.
    /// Smoke-tests that the trait is implementable in a third-party
    /// crate and that the `NtcQuery` variant + magic are forwarded.
    #[test]
    fn custom_lsq_impl_can_be_plugged() {
        use std::cell::{Cell, RefCell};

        struct StubClient {
            expected_magic: u32,
            last_query: RefCell<Option<NtcQuery>>,
            last_submit_len: Cell<Option<usize>>,
        }
        impl LsqClient for StubClient {
            fn run_query(&self, _socket: &Path, magic: u32, query: NtcQuery) -> Result<()> {
                if magic != self.expected_magic {
                    eyre::bail!(
                        "magic mismatch: got {magic}, expected {}",
                        self.expected_magic
                    );
                }
                *self.last_query.borrow_mut() = Some(query);
                Ok(())
            }
            fn submit_tx(&self, _socket: &Path, magic: u32, tx_bytes: &[u8]) -> Result<()> {
                if magic != self.expected_magic {
                    eyre::bail!(
                        "magic mismatch: got {magic}, expected {}",
                        self.expected_magic
                    );
                }
                self.last_submit_len.set(Some(tx_bytes.len()));
                Ok(())
            }
        }
        let client = StubClient {
            expected_magic: 1,
            last_query: RefCell::new(None),
            last_submit_len: Cell::new(None),
        };
        client
            .run_query(&PathBuf::from("/x"), 1, NtcQuery::CurrentEra)
            .expect("stub with matching magic succeeds");
        assert_eq!(
            client.last_query.borrow().clone(),
            Some(NtcQuery::CurrentEra),
            "the NtcQuery variant must reach the impl"
        );
        let err = client
            .run_query(&PathBuf::from("/x"), 2, NtcQuery::Tip)
            .expect_err("stub with mismatched magic bails");
        assert!(err.to_string().contains("magic mismatch"));
        // `submit_tx` reaches the impl with the tx bytes intact.
        client
            .submit_tx(&PathBuf::from("/x"), 1, &[0xaa, 0xbb, 0xcc])
            .expect("submit_tx with matching magic succeeds");
        assert_eq!(
            client.last_submit_len.get(),
            Some(3),
            "submit_tx must forward the tx bytes to the impl"
        );
    }

    #[test]
    fn decode_optional_prefixed_hex_accepts_cli_shapes() {
        assert_eq!(
            decode_optional_prefixed_hex("deadbeef"),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert_eq!(
            decode_optional_prefixed_hex("  0xdeadbeef\n"),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert_eq!(decode_optional_prefixed_hex("zzzz"), Vec::<u8>::new());
        assert_eq!(decode_optional_prefixed_hex("0x"), Vec::<u8>::new());
    }

    #[test]
    fn current_epoch_query_uses_yggdrasil_extension_tag() {
        assert_eq!(encode_query(NtcQuery::CurrentEpoch), vec![0x81, 0x18, 0x65]);
        assert_eq!(
            decode_query_result(NtcQuery::CurrentEpoch, &[0x05]),
            serde_json::json!({ "epoch": 5 })
        );
    }

    #[test]
    fn era_history_query_uses_get_interpreter_shape() {
        assert_eq!(
            encode_query(NtcQuery::EraHistory),
            vec![0x82, 0x00, 0x82, 0x02, 0x81, 0x00]
        );
        assert_eq!(
            decode_query_result(NtcQuery::EraHistory, &[0x82, 0x01, 0x02]),
            serde_json::json!({ "era_history_cbor": "820102" })
        );
    }

    #[test]
    fn parameterized_queries_encode_and_decode_shared_plan_shapes() {
        assert_eq!(
            encode_query(utxo_by_address_query_arg("0xaabb")),
            vec![0x82, 0x04, 0x42, 0xaa, 0xbb]
        );
        assert_eq!(
            decode_query_result(
                NtcQuery::UtxoByAddress {
                    address: vec![0xaa, 0xbb],
                },
                &[0x82, 0x01, 0x02],
            ),
            serde_json::json!({ "utxo_cbor": "820102" })
        );

        assert_eq!(
            encode_query(reward_balance_query_arg("cc")),
            vec![0x82, 0x06, 0x41, 0xcc]
        );
        assert_eq!(
            decode_query_result(
                NtcQuery::RewardBalance {
                    account: vec![0xcc],
                },
                &[0x19, 0x03, 0xe8],
            ),
            serde_json::json!({ "reward_balance_lovelace": 1000 })
        );

        assert_eq!(
            encode_query(utxo_by_tx_in_query_arg("dd#2").expect("tx-in query")),
            vec![0x82, 0x0e, 0x81, 0x82, 0x41, 0xdd, 0x02]
        );
        assert_eq!(
            encode_query(delegations_and_rewards_query_arg("eeff", false)),
            vec![0x82, 0x10, 0x81, 0x82, 0x01, 0x42, 0xee, 0xff]
        );
        assert_eq!(
            decode_query_result(
                NtcQuery::DelegationsAndRewards {
                    credential: vec![0xee],
                    is_key_hash: true,
                },
                &[0xaa],
            ),
            serde_json::json!({ "delegations_and_rewards_cbor": "aa" })
        );

        assert_eq!(
            encode_query(stake_pool_params_query_arg("11")),
            vec![0x82, 0x0c, 0x41, 0x11]
        );
        assert_eq!(
            decode_query_result(
                NtcQuery::StakePoolParams {
                    pool_hash: vec![0x11],
                },
                &[0xf6],
            ),
            serde_json::json!({ "pool": serde_json::Value::Null })
        );
    }

    #[test]
    fn format_utc_time_matches_system_start_rendering() {
        assert_eq!(format_utc_time(2024, 1, 0), "2024-01-01T00:00:00Z");
        assert_eq!(format_utc_time(2024, 60, 0), "2024-02-29T00:00:00Z");
        assert_eq!(format_utc_time(2023, 60, 0), "2023-03-01T00:00:00Z");
        assert_eq!(
            format_utc_time(2022, 1, 3661 * 1_000_000_000_000),
            "2022-01-01T01:01:01Z"
        );
    }
}
