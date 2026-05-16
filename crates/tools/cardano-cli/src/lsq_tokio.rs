//! Concrete tokio + yggdrasil-network LSQ-client impl.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side concrete implementation
//! of the [`crate::lsq::LsqClient`] trait. Upstream Haskell's
//! `Cardano.Api.queryNodeLocalState` is the call-site analogue but
//! upstream doesn't carry a separate "concrete client struct" — the
//! impl is inline in each subcommand handler. The Rust split exists
//! because the library crate stays tokio-free; this module is gated
//! behind the `lsq-tokio` Cargo feature so opting out of it
//! (`cargo build --no-default-features`) produces a binary that
//! gracefully falls back to the deferral path rather than failing to
//! compile.
//!
//! Wire-protocol drive mirrors `node/src/commands/query.rs::run_query`
//! (the node binary's NtC LocalStateQuery dispatcher): open Unix
//! socket via `yggdrasil_network::ntc_connect`, acquire at
//! VolatileTip, send the CBOR-encoded `GetChainPoint = [3]` query,
//! decode the upstream `Ouroboros.Network.Block.encodePoint` reply
//! shape (`[]` = origin / `[slot, hash]` = block point), emit JSON
//! identical to what the node binary's `cardano-cli query-tip`
//! subcommand prints.

use std::path::Path;

use eyre::{Result, WrapErr};
use serde_json::json;
use yggdrasil_ledger::{Decoder, Encoder};
use yggdrasil_network::{AcquireTarget, LocalStateQueryClient, MiniProtocolNum, ntc_connect};

use crate::lsq::{LsqClient, NtcQuery};

/// Concrete LSQ client that opens a Unix-socket NtC connection on
/// each call, drives the LocalStateQuery mini-protocol, and renders
/// the result as JSON on stdout. Constructed in
/// [`crate::main`][standalone-main] when the `lsq-tokio` feature is
/// on; ignored otherwise.
///
/// Zero-sized — all per-call state (socket, runtime, mini-protocol
/// handles) is constructed and torn down inside `run_query`. The
/// upstream parity choice is to match `cardano-cli`'s one-shot
/// behavior: one query per invocation, no persistent client state
/// across calls.
///
/// [standalone-main]: ../main/index.html
pub struct TokioLsqClient;

/// Decoder for one LSQ reply — turns the raw CBOR result bytes into
/// the JSON value the subcommand prints. One per [`NtcQuery`].
type LsqReplyDecoder = fn(&[u8]) -> serde_json::Value;

/// The per-query plan: the CBOR query envelope, an upstream query
/// label for error context, and the reply decoder.
struct QueryPlan {
    query_bytes: Vec<u8>,
    query_label: &'static str,
    decode: LsqReplyDecoder,
}

/// Map an [`NtcQuery`] variant to its CBOR encode + reply decoder.
fn plan_for(query: NtcQuery) -> QueryPlan {
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

/// Decode the `GetTreasuryAndReserves` reply — the 2-element CBOR
/// array `[treasury, reserves]` (lovelace). Mirrors the node
/// binary's `decode_ntc_result` `TreasuryAndReserves` arm.
fn decode_treasury_and_reserves_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    if dec.array().ok() == Some(2) {
        let treasury = dec.unsigned().unwrap_or(0);
        let reserves = dec.unsigned().unwrap_or(0);
        json!({ "treasury_lovelace": treasury, "reserves_lovelace": reserves })
    } else {
        json!({ "result_cbor": hex::encode(result) })
    }
}

/// Decode the `GetAccountState` reply — `[treasury, reserves,
/// total_deposits]`. Mirrors the node binary's `AccountState` arm.
fn decode_account_state_result(result: &[u8]) -> serde_json::Value {
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

/// Decode the `GetGenesisDelegations` reply (raw CBOR hex — the
/// value side carries multiple sub-entries per genesis key).
fn decode_genesis_delegations_result(result: &[u8]) -> serde_json::Value {
    json!({ "genesis_delegations_cbor": hex::encode(result) })
}

/// Decode the `GetStabilityWindow` reply — CBOR null (`f6`) when
/// unset, otherwise a plain unsigned slot count.
fn decode_stability_window_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "stability_window": serde_json::Value::Null })
    } else {
        let mut dec = Decoder::new(result);
        json!({ "stability_window_slots": dec.unsigned().unwrap_or(0) })
    }
}

/// Decode the `GetNumDormantEpochs` reply — a plain unsigned count.
fn decode_num_dormant_epochs_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    json!({ "num_dormant_epochs": dec.unsigned().unwrap_or(0) })
}

/// Decode the `GetExpectedNetworkId` reply — CBOR null (`f6`) when
/// no expectation is configured, otherwise a plain unsigned id.
fn decode_expected_network_id_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "expected_network_id": serde_json::Value::Null })
    } else {
        let mut dec = Decoder::new(result);
        json!({ "expected_network_id": dec.unsigned().unwrap_or(0) })
    }
}

/// Decode the `GetDepositPot` reply — the 4-element CBOR array
/// `[key, pool, drep, proposal]` (lovelace), surfaced with a derived
/// total. Mirrors the node binary's `DepositPot` arm.
fn decode_deposit_pot_result(result: &[u8]) -> serde_json::Value {
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

/// Decode the `GetLedgerCounts` reply — the 6-element CBOR array of
/// ledger-state cardinality counters. Mirrors the node binary's
/// `LedgerCounts` arm.
fn decode_ledger_counts_result(result: &[u8]) -> serde_json::Value {
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

// The five Conway governance queries surface complex per-era ledger
// structures (`Constitution`, `GovActionsState`, DRep maps, …) as
// raw CBOR hex under a descriptive key — matching the node binary's
// `decode_ntc_result` arms. Decoding them into typed JSON is a
// follow-on; the hex passthrough lets clients decode them today.

/// Decode the `GetDRepStakeDistribution` reply (raw CBOR hex).
fn decode_drep_stake_distribution_result(result: &[u8]) -> serde_json::Value {
    json!({ "drep_stake_distribution_cbor": hex::encode(result) })
}

/// Decode the `GetConstitution` reply (raw CBOR hex).
fn decode_constitution_result(result: &[u8]) -> serde_json::Value {
    json!({ "constitution_cbor": hex::encode(result) })
}

/// Decode the `GetGovState` reply (raw CBOR hex).
fn decode_gov_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "governance_actions_cbor": hex::encode(result) })
}

/// Decode the `GetDRepState` reply (raw CBOR hex).
fn decode_drep_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "drep_state_cbor": hex::encode(result) })
}

/// Decode the `GetCommitteeState` reply (raw CBOR hex).
fn decode_committee_state_result(result: &[u8]) -> serde_json::Value {
    json!({ "committee_state_cbor": hex::encode(result) })
}

/// CBOR-encode a single-element `[tag]` query envelope.
///
/// `query-stake-distribution` / `query-stake-pools` /
/// `query-protocol-parameters` use yggdrasil-node's NtC dispatcher
/// tags (5 / 15 / 102) — these are yggdrasil-node-specific (not the
/// upstream `BlockQuery` wrapper), so these subcommands target a
/// running `yggdrasil-node`. Mirrors the encoding the node binary's
/// own `cardano-cli` wrapper sends (`node/src/commands/query.rs`).
fn encode_single_tag_query(tag: u64) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(tag);
    enc.into_bytes()
}

/// Decode the `GetStakeDistribution` reply — a complex per-pool
/// structure surfaced as raw CBOR hex (matches the node binary's
/// `decode_ntc_result` `StakeDistribution` arm).
fn decode_stake_distribution_result(result: &[u8]) -> serde_json::Value {
    json!({ "stake_distribution_cbor": hex::encode(result) })
}

/// Decode the `GetStakePools` reply — a CBOR array of 28-byte pool
/// hashes, surfaced as a hex-string list plus a count. Mirrors the
/// node binary's `decode_ntc_result` `StakePools` arm.
fn decode_stake_pools_result(result: &[u8]) -> serde_json::Value {
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

/// Decode the `GetProtocolParameters` reply — `f6` (CBOR null) when
/// no parameters are available yet, otherwise raw CBOR hex. Mirrors
/// the node binary's `decode_ntc_result` `ProtocolParams` arm.
fn decode_protocol_parameters_result(result: &[u8]) -> serde_json::Value {
    if result == [0xf6] {
        json!({ "protocol_parameters": serde_json::Value::Null })
    } else {
        json!({ "protocol_parameters": hex::encode(result) })
    }
}

impl LsqClient for TokioLsqClient {
    fn run_query(&self, socket_path: &Path, network_magic: u32, query: NtcQuery) -> Result<()> {
        let QueryPlan {
            query_bytes,
            query_label,
            decode,
        } = plan_for(query);
        run_blocking(async move {
            let result =
                acquire_query_release(socket_path, network_magic, query_bytes, query_label).await?;
            print_json(&decode(&result))
        })
    }
}

/// Build a single-threaded tokio runtime and drive `fut` to
/// completion. Each `TokioLsqClient` call is one-shot (matches
/// upstream `cardano-cli`'s per-invocation behavior), so the runtime
/// is constructed + torn down per call.
fn run_blocking<F: std::future::Future<Output = Result<()>>>(fut: F) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .wrap_err("failed to build tokio current-thread runtime")?
        .block_on(fut)
}

/// Open the NtC socket, acquire at VolatileTip, run one
/// LocalStateQuery, release + done, and return the raw CBOR result.
///
/// Mirrors `node/src/commands/query.rs::run_query`'s socket-drive
/// flow. `query_label` names the query for the error context only.
async fn acquire_query_release(
    socket_path: &Path,
    network_magic: u32,
    query_bytes: Vec<u8>,
    query_label: &str,
) -> Result<Vec<u8>> {
    let mut conn = ntc_connect(socket_path, network_magic, true)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to connect to NtC socket {} (network_magic={network_magic})",
                socket_path.display()
            )
        })?;

    let sq_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .ok_or_else(|| eyre::eyre!("NTC_LOCAL_STATE_QUERY mini-protocol handle missing"))?;
    let mut client = LocalStateQueryClient::new(sq_handle);

    client
        .acquire(AcquireTarget::VolatileTip)
        .await
        .wrap_err("LocalStateQuery acquire failed")?;

    let result = client
        .query(query_bytes)
        .await
        .wrap_err_with(|| format!("LocalStateQuery `{query_label}` query failed"))?;

    // Best-effort cleanup; failures here are non-fatal because the
    // remote may already have torn the socket down by the time we
    // get here.
    let _ = client.release().await;
    let _ = client.done().await;
    Ok(result)
}

/// Pretty-print a `serde_json::Value` to stdout.
fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// CBOR-encode the `GetChainPoint = [3]` query envelope.
///
/// Mirrors upstream `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`:
/// `[3]` is the single-element array with tag 3 = `GetChainPoint`.
fn encode_get_chain_point_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(3u64);
    enc.into_bytes()
}

/// CBOR-encode the `GetChainBlockNo = [2]` query envelope.
///
/// Mirrors upstream `Ouroboros.Consensus.Ledger.Query.queryEncodeNodeToClient`:
/// `[2]` is the single-element array with tag 2 = `GetChainBlockNo`.
fn encode_get_chain_block_no_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(2u64);
    enc.into_bytes()
}

/// Decode the upstream `Cardano.Slotting.Block.encodeChainBlockNo`
/// reply: `Origin = [0]`, `At b = [1, b]` (mirrors
/// `Ouroboros.Network.Block.Tip.tipBlockNo`).
///
/// Mirrors `node/src/commands/query.rs::decode_ntc_result`'s
/// `QueryCommand::ChainBlockNo` arm.
fn decode_chain_block_no_result(result: &[u8]) -> serde_json::Value {
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

/// CBOR-encode the `GetCurrentEra` query envelope.
///
/// Mirrors upstream `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query` —
/// `BlockQuery (QueryHardFork GetCurrentEra)` is the nested
/// `[0, [2, [1]]]`: outer `BlockQuery` tag 0, `QueryHardFork` tag 2,
/// `GetCurrentEra` tag 1.
fn encode_get_current_era_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0u64);
    enc.array(2).unsigned(2u64);
    enc.array(1).unsigned(1u64);
    enc.into_bytes()
}

/// Decode the upstream `GetCurrentEra` reply — an `EraIndex`,
/// encoded as the single-element array `[era_index]`.
///
/// Mirrors `node/src/commands/query.rs::decode_ntc_result`'s
/// `QueryCommand::CurrentEra` arm.
fn decode_current_era_result(result: &[u8]) -> serde_json::Value {
    let mut dec = Decoder::new(result);
    let era = match dec.array() {
        Ok(1) => dec.unsigned().unwrap_or(0),
        _ => 0,
    };
    json!({ "era": era })
}

/// CBOR-encode the `GetSystemStart = [1]` query envelope.
///
/// Mirrors upstream `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query` —
/// `[1]` is the single-element array with tag 1 = `GetSystemStart`.
fn encode_get_system_start_query() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(1u64);
    enc.into_bytes()
}

/// Decode the upstream `GetSystemStart` reply — the 3-element CBOR
/// array `[year, dayOfYear, picosecondsOfDay]` (`UTCTime` as
/// `Cardano.Slotting.Time.SystemStart`).
///
/// Surfaces both the raw structured fields and a derived ISO-8601
/// `time` string. Mirrors `node/src/commands/query.rs::decode_ntc_result`'s
/// `QueryCommand::SystemStart` arm.
fn decode_system_start_result(result: &[u8]) -> serde_json::Value {
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

/// Format `(year, day-of-year, picoseconds-of-day)` as an ISO-8601
/// `YYYY-MM-DDThh:mm:ssZ` string.
///
/// Civil-date arithmetic with proleptic-Gregorian leap-year rules —
/// mirrors `node/src/commands/query.rs::format_utc_time` so the
/// standalone binary's `query system-start` output matches the node
/// binary's wrapper byte-for-byte.
fn format_utc_time(year: u64, day_of_year: u64, picoseconds_of_day: u64) -> String {
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

/// Decode the upstream `Ouroboros.Network.Block.encodePoint` reply.
///
/// - Origin: `encodeListLen 0` → `[]` → empty array.
/// - BlockPoint: `encodeListLen 2 <> slot <> hash` → `[slot, hash]`.
///
/// Mirrors `node/src/commands/query.rs::decode_ntc_result`'s
/// `QueryCommand::Tip` arm. Captured against `cardano-node 10.7.1`
/// socat-proxy bytes in the upstream-regression test
/// `upstream_get_chain_point_returns_encoded_tip_point`.
fn decode_chain_point_result(result: &[u8]) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Running a query against a nonexistent socket path bails with
    /// a wrapped IO error rather than panicking. Pins the
    /// failure-mode contract: errors travel through `eyre` with the
    /// socket-path + network-magic context.
    #[test]
    fn run_query_against_missing_socket_returns_wrapped_error() {
        let client = TokioLsqClient;
        let result = client.run_query(
            &PathBuf::from("/tmp/yggdrasil-cardano-cli-nonexistent-socket"),
            764_824_073,
            NtcQuery::Tip,
        );
        let err = result.expect_err("missing socket must bail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("failed to connect to NtC socket")
                && msg.contains("network_magic=764824073"),
            "error must carry the eyre socket-path + magic context; got {msg}"
        );
    }

    /// `decode_chain_point_result` recognizes the upstream `[]` =
    /// origin shape.
    #[test]
    fn decode_origin_chain_point() {
        // CBOR for empty array: 0x80.
        let bytes = vec![0x80];
        let v = decode_chain_point_result(&bytes);
        assert_eq!(v, json!({"tip": {"origin": true}}));
    }

    /// `decode_chain_point_result` recognizes the upstream `[slot, hash]`
    /// = block-point shape and renders hash as hex.
    #[test]
    fn decode_block_point_chain_point() {
        // CBOR for [10, h'aabbcc'] = 0x82 0x0a 0x43 0xaa 0xbb 0xcc.
        let bytes = vec![0x82, 0x0a, 0x43, 0xaa, 0xbb, 0xcc];
        let v = decode_chain_point_result(&bytes);
        assert_eq!(
            v,
            json!({"tip": {"origin": false, "slot": 10, "hash": "aabbcc"}})
        );
    }

    /// `decode_chain_point_result` falls back to a raw-hex dump for
    /// unrecognized payloads — defensive surface that matches the
    /// node binary's `decode_ntc_result` fallback exactly.
    #[test]
    fn decode_unknown_shape_falls_back_to_raw_hex() {
        // CBOR for a single integer: 0x05 (the tag-only "5").
        let bytes = vec![0x05];
        let v = decode_chain_point_result(&bytes);
        assert_eq!(v, json!({"tip_cbor": "05"}));
    }

    /// `encode_get_chain_point_query` produces the canonical CBOR
    /// `[3]` byte sequence upstream's
    /// `GetChainPoint` query envelope expects.
    #[test]
    fn encode_get_chain_point_query_emits_canonical_cbor() {
        let bytes = encode_get_chain_point_query();
        // CBOR encoding of `[3]` is `0x81 0x03` — 1-element array
        // containing the unsigned int 3.
        assert_eq!(bytes, vec![0x81, 0x03]);
    }

    /// `encode_get_chain_block_no_query` produces the canonical CBOR
    /// `[2]` byte sequence.
    #[test]
    fn encode_get_chain_block_no_query_emits_canonical_cbor() {
        assert_eq!(encode_get_chain_block_no_query(), vec![0x81, 0x02]);
    }

    /// `decode_chain_block_no_result` recognizes `Origin = [0]` (a
    /// 1-element array) → null block number.
    #[test]
    fn decode_chain_block_no_origin() {
        // CBOR `[0]` = 0x81 0x00.
        let v = decode_chain_block_no_result(&[0x81, 0x00]);
        assert_eq!(v, json!({ "chain_block_no": serde_json::Value::Null }));
    }

    /// `decode_chain_block_no_result` recognizes `At b = [1, b]` (a
    /// 2-element array) → the block number.
    #[test]
    fn decode_chain_block_no_at_block() {
        // CBOR `[1, 42]` = 0x82 0x01 0x18 0x2a.
        let v = decode_chain_block_no_result(&[0x82, 0x01, 0x18, 0x2a]);
        assert_eq!(v, json!({ "chain_block_no": 42 }));
    }

    /// `decode_chain_block_no_result` falls back to a raw-hex dump
    /// for unrecognized payloads.
    #[test]
    fn decode_chain_block_no_unknown_shape() {
        // CBOR single integer 0x05 — not an array.
        let v = decode_chain_block_no_result(&[0x05]);
        assert_eq!(v, json!({ "chain_block_no_cbor": "05" }));
    }

    /// `encode_get_current_era_query` produces the nested CBOR
    /// `[0, [2, [1]]]` byte sequence.
    #[test]
    fn encode_get_current_era_query_emits_canonical_cbor() {
        // `[0,[2,[1]]]` = 0x82 0x00 0x82 0x02 0x81 0x01.
        assert_eq!(
            encode_get_current_era_query(),
            vec![0x82, 0x00, 0x82, 0x02, 0x81, 0x01]
        );
    }

    /// `decode_current_era_result` reads the `[era_index]`
    /// single-element array.
    #[test]
    fn decode_current_era_reads_era_index() {
        // CBOR `[6]` = 0x81 0x06 — era index 6 (Conway).
        let v = decode_current_era_result(&[0x81, 0x06]);
        assert_eq!(v, json!({ "era": 6 }));
    }

    /// `decode_current_era_result` defaults to era 0 for an
    /// unrecognized shape rather than panicking.
    #[test]
    fn decode_current_era_defaults_on_unknown_shape() {
        let v = decode_current_era_result(&[0x00]);
        assert_eq!(v, json!({ "era": 0 }));
    }

    /// `encode_get_system_start_query` produces the canonical CBOR
    /// `[1]` byte sequence.
    #[test]
    fn encode_get_system_start_query_emits_canonical_cbor() {
        assert_eq!(encode_get_system_start_query(), vec![0x81, 0x01]);
    }

    /// `format_utc_time` renders civil dates correctly, including
    /// the leap-year boundary. 2024 is a leap year, so day-of-year
    /// 60 is Feb 29; 2023 is not, so day 60 is Mar 1.
    #[test]
    fn format_utc_time_handles_leap_years() {
        // 2024-01-01T00:00:00Z — day-of-year 1.
        assert_eq!(format_utc_time(2024, 1, 0), "2024-01-01T00:00:00Z");
        // Day 60 of a leap year (2024) is Feb 29.
        assert_eq!(format_utc_time(2024, 60, 0), "2024-02-29T00:00:00Z");
        // Day 60 of a non-leap year (2023) is Mar 1.
        assert_eq!(format_utc_time(2023, 60, 0), "2023-03-01T00:00:00Z");
        // Picoseconds-of-day → hh:mm:ss: 3661 s = 01:01:01.
        assert_eq!(
            format_utc_time(2022, 1, 3661 * 1_000_000_000_000),
            "2022-01-01T01:01:01Z"
        );
    }

    /// `decode_system_start_result` reads the 3-element
    /// `[year, dayOfYear, picosecondsOfDay]` reply and derives the
    /// ISO-8601 `time` string.
    #[test]
    fn decode_system_start_reads_three_element_array() {
        // CBOR `[2017, 244, 0]` = 0x83 0x19 0x07 0xE1 0x18 0xF4 0x00.
        let bytes = vec![0x83, 0x19, 0x07, 0xE1, 0x18, 0xF4, 0x00];
        let v = decode_system_start_result(&bytes);
        assert_eq!(v["system_start"]["year"], 2017);
        assert_eq!(v["system_start"]["dayOfYear"], 244);
        assert_eq!(v["system_start"]["picosecondsOfDay"], 0);
        assert_eq!(v["system_start"]["time"], "2017-09-01T00:00:00Z");
    }

    /// `decode_system_start_result` falls back to a raw-hex dump for
    /// an unrecognized payload shape.
    #[test]
    fn decode_system_start_unknown_shape() {
        let v = decode_system_start_result(&[0x00]);
        assert_eq!(v, json!({ "system_start_cbor": "00" }));
    }

    /// `encode_single_tag_query` produces the canonical `[tag]` CBOR
    /// for the yggdrasil-node dispatcher tags.
    #[test]
    fn encode_single_tag_query_emits_canonical_cbor() {
        assert_eq!(encode_single_tag_query(5), vec![0x81, 0x05]);
        assert_eq!(encode_single_tag_query(15), vec![0x81, 0x0f]);
        // tag 102 needs the 1-byte-uint CBOR prefix 0x18.
        assert_eq!(encode_single_tag_query(102), vec![0x81, 0x18, 0x66]);
    }

    /// `decode_stake_distribution_result` surfaces the raw reply as
    /// hex.
    #[test]
    fn decode_stake_distribution_is_raw_hex() {
        let v = decode_stake_distribution_result(&[0xaa, 0xbb]);
        assert_eq!(v, json!({ "stake_distribution_cbor": "aabb" }));
    }

    /// `decode_stake_pools_result` reads a CBOR array of pool hashes
    /// into a hex-string list + count.
    #[test]
    fn decode_stake_pools_reads_hash_array() {
        // CBOR `[ h'aa…(28)', h'bb…(28)' ]`.
        let mut bytes = vec![0x82];
        bytes.push(0x58);
        bytes.push(0x1c);
        bytes.extend(std::iter::repeat_n(0xaa, 28));
        bytes.push(0x58);
        bytes.push(0x1c);
        bytes.extend(std::iter::repeat_n(0xbb, 28));
        let v = decode_stake_pools_result(&bytes);
        assert_eq!(v["count"], 2);
        assert_eq!(v["stake_pools"][0], "aa".repeat(28));
        assert_eq!(v["stake_pools"][1], "bb".repeat(28));
    }

    /// `decode_protocol_parameters_result` maps CBOR null (`f6`) to
    /// a JSON null and any other payload to raw hex.
    #[test]
    fn decode_protocol_parameters_null_vs_hex() {
        assert_eq!(
            decode_protocol_parameters_result(&[0xf6]),
            json!({ "protocol_parameters": serde_json::Value::Null })
        );
        assert_eq!(
            decode_protocol_parameters_result(&[0x01, 0x02]),
            json!({ "protocol_parameters": "0102" })
        );
    }

    /// The 5 Conway governance decoders each surface the reply under
    /// their descriptive raw-hex key.
    #[test]
    fn governance_decoders_surface_raw_hex() {
        assert_eq!(
            decode_drep_stake_distribution_result(&[0xde, 0xad]),
            json!({ "drep_stake_distribution_cbor": "dead" })
        );
        assert_eq!(
            decode_constitution_result(&[0xbe, 0xef]),
            json!({ "constitution_cbor": "beef" })
        );
        assert_eq!(
            decode_gov_state_result(&[0x01]),
            json!({ "governance_actions_cbor": "01" })
        );
        assert_eq!(
            decode_drep_state_result(&[0x02]),
            json!({ "drep_state_cbor": "02" })
        );
        assert_eq!(
            decode_committee_state_result(&[0x03]),
            json!({ "committee_state_cbor": "03" })
        );
    }

    /// `decode_treasury_and_reserves_result` reads the 2-element
    /// `[treasury, reserves]` array.
    #[test]
    fn decode_treasury_and_reserves_reads_pair() {
        // CBOR `[10, 20]` = 0x82 0x0a 0x14.
        let v = decode_treasury_and_reserves_result(&[0x82, 0x0a, 0x14]);
        assert_eq!(
            v,
            json!({ "treasury_lovelace": 10, "reserves_lovelace": 20 })
        );
    }

    /// `decode_account_state_result` reads the 3-element
    /// `[treasury, reserves, total_deposits]` array.
    #[test]
    fn decode_account_state_reads_triple() {
        // CBOR `[1, 2, 3]` = 0x83 0x01 0x02 0x03.
        let v = decode_account_state_result(&[0x83, 0x01, 0x02, 0x03]);
        assert_eq!(v["treasury_lovelace"], 1);
        assert_eq!(v["reserves_lovelace"], 2);
        assert_eq!(v["total_deposits_lovelace"], 3);
    }

    /// `decode_stability_window_result` maps CBOR null to JSON null
    /// and a plain unsigned to the slot count.
    #[test]
    fn decode_stability_window_null_vs_slots() {
        assert_eq!(
            decode_stability_window_result(&[0xf6]),
            json!({ "stability_window": serde_json::Value::Null })
        );
        // CBOR `129600` = 0x1a 0x00 0x01 0xfa 0x40.
        assert_eq!(
            decode_stability_window_result(&[0x1a, 0x00, 0x01, 0xfa, 0x40]),
            json!({ "stability_window_slots": 129_600 })
        );
    }

    /// `decode_num_dormant_epochs_result` reads a plain unsigned.
    #[test]
    fn decode_num_dormant_epochs_reads_count() {
        assert_eq!(
            decode_num_dormant_epochs_result(&[0x05]),
            json!({ "num_dormant_epochs": 5 })
        );
    }

    /// `decode_expected_network_id_result` maps CBOR null to JSON
    /// null and a plain unsigned to the id.
    #[test]
    fn decode_expected_network_id_null_vs_id() {
        assert_eq!(
            decode_expected_network_id_result(&[0xf6]),
            json!({ "expected_network_id": serde_json::Value::Null })
        );
        assert_eq!(
            decode_expected_network_id_result(&[0x01]),
            json!({ "expected_network_id": 1 })
        );
    }

    /// `decode_deposit_pot_result` reads the 4-element pot array and
    /// derives the total.
    #[test]
    fn decode_deposit_pot_reads_four_pots() {
        // CBOR `[1, 2, 3, 4]` = 0x84 0x01 0x02 0x03 0x04.
        let v = decode_deposit_pot_result(&[0x84, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(v["key_deposits_lovelace"], 1);
        assert_eq!(v["proposal_deposits_lovelace"], 4);
        assert_eq!(v["total_lovelace"], 10);
    }

    /// `decode_ledger_counts_result` reads the 6-element counter
    /// array.
    #[test]
    fn decode_ledger_counts_reads_six_counters() {
        // CBOR `[1, 2, 3, 4, 5, 6]`.
        let v = decode_ledger_counts_result(&[0x86, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(v["stake_credentials"], 1);
        assert_eq!(v["pools"], 2);
        assert_eq!(v["gen_delegs"], 6);
    }

    /// The structured decoders fall back to a raw-hex dump when the
    /// reply isn't the expected array shape.
    #[test]
    fn structured_decoders_fall_back_to_hex() {
        assert_eq!(
            decode_deposit_pot_result(&[0x00]),
            json!({ "deposit_pot_cbor": "00" })
        );
        assert_eq!(
            decode_ledger_counts_result(&[0x00]),
            json!({ "ledger_counts_cbor": "00" })
        );
    }
}
