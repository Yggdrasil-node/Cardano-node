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

use crate::lsq::LsqClient;

/// Concrete LSQ client that opens a Unix-socket NtC connection on
/// each call, drives the LocalStateQuery mini-protocol, and renders
/// the result as JSON on stdout. Constructed in
/// [`crate::main`][standalone-main] when the `lsq-tokio` feature is
/// on; ignored otherwise.
///
/// Zero-sized — all per-call state (socket, runtime, mini-protocol
/// handles) is constructed and torn down inside `query_tip`. The
/// upstream parity choice is to match `cardano-cli`'s one-shot
/// behavior: one query per invocation, no persistent client state
/// across calls.
///
/// [standalone-main]: ../main/index.html
pub struct TokioLsqClient;

impl LsqClient for TokioLsqClient {
    fn query_tip(&self, socket_path: &Path, network_magic: u32) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .wrap_err("failed to build tokio current-thread runtime")?;

        rt.block_on(query_tip_inner(socket_path, network_magic))
    }
}

/// Async body of `TokioLsqClient::query_tip`. Open + handshake +
/// acquire + query + decode + print + release + done.
///
/// Mirrors `node/src/commands/query.rs::run_query`'s flow restricted
/// to the `QueryCommand::Tip` arm.
async fn query_tip_inner(socket_path: &Path, network_magic: u32) -> Result<()> {
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

    let query_bytes = encode_get_chain_point_query();
    let result = client
        .query(query_bytes)
        .await
        .wrap_err("LocalStateQuery `GetChainPoint` query failed")?;

    let json_val = decode_chain_point_result(&result);
    println!("{}", serde_json::to_string_pretty(&json_val)?);

    // Best-effort cleanup; failures here are non-fatal because the
    // remote may already have torn the socket down by the time we
    // get here.
    let _ = client.release().await;
    let _ = client.done().await;
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

    /// Calling `query_tip` against a nonexistent socket path bails
    /// with a wrapped IO error rather than panicking. Pins the
    /// failure-mode contract: errors travel through `eyre` with the
    /// socket-path + network-magic context.
    #[test]
    fn query_tip_against_missing_socket_returns_wrapped_error() {
        let client = TokioLsqClient;
        let result = client.query_tip(
            &PathBuf::from("/tmp/yggdrasil-cardano-cli-nonexistent-socket"),
            764_824_073,
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
}
