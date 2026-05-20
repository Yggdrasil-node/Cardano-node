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
//! Wire-protocol drive is shared by the standalone cardano-cli binary
//! and the node binary's compatibility wrappers: open Unix
//! socket via `yggdrasil_network::ntc_connect`, acquire at
//! VolatileTip, send the CBOR-encoded `GetChainPoint = [3]` query,
//! decode the upstream `Ouroboros.Network.Block.encodePoint` reply
//! shape (`[]` = origin / `[slot, hash]` = block point), emit JSON
//! identical to what the node binary's `cardano-cli query-tip`
//! subcommand prints.

#![cfg_attr(not(unix), allow(dead_code))]

use std::path::Path;

use eyre::Result;
#[cfg(unix)]
use eyre::WrapErr;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use yggdrasil_network::{
    AcquireTarget, LocalStateQueryClient, LocalTxSubmissionClient, MiniProtocolNum, ntc_connect,
};

use crate::lsq::{LsqClient, NtcQuery};
#[cfg(unix)]
use crate::lsq::{QueryPlan, plan_for};

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

impl LsqClient for TokioLsqClient {
    fn run_query(&self, socket_path: &Path, network_magic: u32, query: NtcQuery) -> Result<()> {
        #[cfg(not(unix))]
        {
            let _ = (socket_path, network_magic, query);
            Err(eyre::eyre!(
                "LocalStateQuery over node-to-client sockets requires Unix-domain socket support"
            ))
        }

        #[cfg(unix)]
        {
            let QueryPlan {
                query_bytes,
                query_label,
                decode,
            } = plan_for(query);
            run_blocking(async move {
                let result =
                    acquire_query_release(socket_path, network_magic, query_bytes, query_label)
                        .await?;
                print_json(&decode(&result))
            })
        }
    }

    fn submit_tx(&self, socket_path: &Path, network_magic: u32, tx_bytes: &[u8]) -> Result<()> {
        #[cfg(not(unix))]
        {
            let _ = (socket_path, network_magic, tx_bytes);
            Err(eyre::eyre!(
                "LocalTxSubmission over node-to-client sockets requires Unix-domain socket support"
            ))
        }

        #[cfg(unix)]
        run_blocking(submit_tx_inner(socket_path, network_magic, tx_bytes))
    }
}

/// Open the NtC socket, submit `tx_bytes` over the LocalTxSubmission
/// mini-protocol, and print the accept/reject outcome as JSON.
///
/// Shared LocalTxSubmission driver for `cardano-cli transaction submit`
/// and `yggdrasil-node submit-tx`. A rejection is a normal outcome (printed as
/// `{"result":"rejected","reason":…}`), not an `Err` — only a
/// connection/transport failure bails.
#[cfg(unix)]
async fn submit_tx_inner(socket_path: &Path, network_magic: u32, tx_bytes: &[u8]) -> Result<()> {
    // `query_only = false` — a submitting client is not query-only.
    let mut conn = ntc_connect(socket_path, network_magic, false)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to connect to NtC socket {} (network_magic={network_magic})",
                socket_path.display()
            )
        })?;

    let tx_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .ok_or_else(|| eyre::eyre!("NTC_LOCAL_TX_SUBMISSION mini-protocol handle missing"))?;
    let mut client = LocalTxSubmissionClient::new(tx_handle);

    let outcome = match client.submit(tx_bytes.to_vec()).await {
        Ok(()) => json!({ "result": "accepted" }),
        Err(e) => json!({ "result": "rejected", "reason": e.to_string() }),
    };
    let _ = client.done().await;
    print_json(&outcome)
}

/// Build a single-threaded tokio runtime and drive `fut` to
/// completion. Each `TokioLsqClient` call is one-shot (matches
/// upstream `cardano-cli`'s per-invocation behavior), so the runtime
/// is constructed + torn down per call.
#[cfg(unix)]
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
/// Mirrors `crates/node/cardano-node/src/commands/query.rs::run_query`'s socket-drive
/// flow. `query_label` names the query for the error context only.
#[cfg(unix)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsq::{
        decode_account_state_result, decode_chain_block_no_result, decode_chain_point_result,
        decode_committee_state_result, decode_constitution_result, decode_current_epoch_result,
        decode_current_era_result, decode_delegations_and_rewards_result,
        decode_deposit_pot_result, decode_drep_stake_distribution_result, decode_drep_state_result,
        decode_era_history_result, decode_expected_network_id_result, decode_gov_state_result,
        decode_ledger_counts_result, decode_num_dormant_epochs_result,
        decode_protocol_parameters_result, decode_reward_balance_result,
        decode_stability_window_result, decode_stake_distribution_result,
        decode_stake_pool_params_result, decode_stake_pools_result, decode_system_start_result,
        decode_treasury_and_reserves_result, decode_utxo_result,
        encode_delegations_and_rewards_query, encode_get_chain_block_no_query,
        encode_get_chain_point_query, encode_get_current_era_query, encode_get_era_history_query,
        encode_get_system_start_query, encode_reward_balance_query, encode_single_tag_query,
        encode_stake_pool_params_query, encode_utxo_by_address_query, encode_utxo_by_tx_in_query,
        format_utc_time,
    };
    use serde_json::json;
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
        #[cfg(unix)]
        assert!(
            msg.contains("failed to connect to NtC socket")
                && msg.contains("network_magic=764824073"),
            "error must carry the eyre socket-path + magic context; got {msg}"
        );
        #[cfg(not(unix))]
        assert!(
            msg.contains("requires Unix-domain socket support"),
            "error must describe unsupported platform; got {msg}"
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

    /// `encode_get_era_history_query` produces the hard-fork
    /// `GetInterpreter` query shape `[0, [2, [0]]]`.
    #[test]
    fn encode_get_era_history_query_emits_canonical_cbor() {
        assert_eq!(
            encode_get_era_history_query(),
            vec![0x82, 0x00, 0x82, 0x02, 0x81, 0x00]
        );
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

    /// Parameterized query plans preserve the node wrapper's prior
    /// tag-and-argument CBOR envelopes and result JSON keys.
    #[test]
    fn parameterized_query_plans_match_node_wrapper_shapes() {
        assert_eq!(
            encode_utxo_by_address_query(&[0xaa, 0xbb]),
            vec![0x82, 0x04, 0x42, 0xaa, 0xbb]
        );
        assert_eq!(
            encode_reward_balance_query(&[0xcc]),
            vec![0x82, 0x06, 0x41, 0xcc]
        );
        assert_eq!(
            encode_utxo_by_tx_in_query(&[0xdd], 2),
            vec![0x82, 0x0e, 0x81, 0x82, 0x41, 0xdd, 0x02]
        );
        assert_eq!(
            encode_delegations_and_rewards_query(&[0xee, 0xff], false),
            vec![0x82, 0x10, 0x81, 0x82, 0x01, 0x42, 0xee, 0xff]
        );
        assert_eq!(
            encode_stake_pool_params_query(&[0x11]),
            vec![0x82, 0x0c, 0x41, 0x11]
        );

        assert_eq!(decode_utxo_result(&[0x82]), json!({ "utxo_cbor": "82" }));
        assert_eq!(
            decode_reward_balance_result(&[0x19, 0x03, 0xe8]),
            json!({ "reward_balance_lovelace": 1000 })
        );
        assert_eq!(
            decode_delegations_and_rewards_result(&[0xaa]),
            json!({ "delegations_and_rewards_cbor": "aa" })
        );
        assert_eq!(
            decode_stake_pool_params_result(&[0xf6]),
            json!({ "pool": serde_json::Value::Null })
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

    /// `decode_current_epoch_result` reads a plain unsigned epoch number.
    #[test]
    fn decode_current_epoch_reads_count() {
        assert_eq!(decode_current_epoch_result(&[0x05]), json!({ "epoch": 5 }));
    }

    /// `decode_era_history_result` surfaces the interpreter as raw CBOR.
    #[test]
    fn decode_era_history_is_raw_hex() {
        assert_eq!(
            decode_era_history_result(&[0x82, 0x01, 0x02]),
            json!({ "era_history_cbor": "820102" })
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

    /// `TokioLsqClient::submit_tx` against a missing socket bails
    /// with the wrapped NtC-connect error — same failure-mode
    /// contract as the query path.
    #[test]
    fn submit_tx_against_missing_socket_returns_wrapped_error() {
        let client = TokioLsqClient;
        let err = client
            .submit_tx(
                &PathBuf::from("/tmp/yggdrasil-cardano-cli-nonexistent-socket"),
                764_824_073,
                &[0x82, 0xa0, 0xa0],
            )
            .expect_err("missing socket must bail");
        let msg = format!("{err:#}");
        #[cfg(unix)]
        assert!(
            msg.contains("failed to connect to NtC socket"),
            "error must carry the eyre socket-path context; got {msg}"
        );
        #[cfg(not(unix))]
        assert!(
            msg.contains("requires Unix-domain socket support"),
            "error must describe unsupported platform; got {msg}"
        );
    }
}
