//! `query tx-mempool` subcommand: drive the running node's NtC
//! `LocalTxMonitor` mini-protocol to inspect the mempool snapshot.
//!
//! Mirrors upstream `Cardano.CLI.Shelley.Run.Query.runQueryTxMempool`.
//! Three subcommands:
//!
//! - `info`        → acquire snapshot + `MsgGetSizes` → JSON
//!   `{capacityInBytes, sizeInBytes, numberOfTxs, slot}`.
//! - `next-tx`     → acquire snapshot + `MsgNextTx` → JSON
//!   `{slot, tx: <hex|null>}`.
//! - `tx-exists`   → acquire snapshot + `MsgHasTx(tx_id)` → JSON
//!   `{slot, exists}`.
//!
//! Unix-only because the transport is a Unix domain socket; non-Unix
//! targets do not expose the `TxMempool` clap arm.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Shelley/Run/Query.hs>
//! and `Ouroboros.Network.Protocol.LocalTxMonitor.Client`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `query tx-mempool`
//! subcommand (NtC LocalTxMonitor client: info / next-tx /
//! tx-exists). Mirrors the dispatch half of upstream
//! `Cardano.CLI.Compatible.Run::runQueryTxMempool`. Yggdrasil's
//! binary mirrors the core tx-mempool query flow.

#![cfg(unix)]

use std::path::PathBuf;

use clap::Subcommand;
use eyre::{Result, WrapErr};
use serde_json::json;

/// `cardano-cli query tx-mempool` sub-commands, mirroring upstream
/// `Cardano.CLI.Shelley.Run.Query.runQueryTxMempool`.
#[derive(Subcommand, Debug)]
pub enum TxMempoolCommand {
    /// Acquire a mempool snapshot and report the size + capacity.
    /// Equivalent to upstream `cardano-cli query tx-mempool info`.
    Info,
    /// Acquire a mempool snapshot and emit the next transaction
    /// (hex-encoded raw CBOR), or `null` if the snapshot is empty.
    /// Equivalent to upstream `cardano-cli query tx-mempool next-tx`.
    NextTx,
    /// Acquire a mempool snapshot and report whether `tx_id` is
    /// currently present.  Equivalent to upstream
    /// `cardano-cli query tx-mempool tx-exists`.
    TxExists {
        /// Hex-encoded transaction id, 32 bytes (with or without `0x`
        /// prefix).
        #[arg(long)]
        tx_id: String,
    },
}

/// Lenient hex decoder for the `--tx-id` argument: trims whitespace,
/// accepts an optional `0x` prefix, and returns an empty `Vec<u8>` on
/// parse failure (matches the prior call-site semantics where invalid
/// hex produced an empty parameter, surfaced via `MsgHasTx`'s `exists:
/// false` rather than a typed error).
fn decode_tx_id_hex(raw: &str) -> Vec<u8> {
    let stripped = raw.trim();
    let stripped = stripped.strip_prefix("0x").unwrap_or(stripped);
    hex::decode(stripped).unwrap_or_default()
}

/// Drive the NtC `LocalTxMonitor` mini-protocol against a running node.
pub async fn run_tx_mempool(
    socket_path: PathBuf,
    network_magic: u32,
    action: TxMempoolCommand,
) -> Result<()> {
    use yggdrasil_network::{LocalTxMonitorClient, MiniProtocolNum, ntc_connect};

    let mut conn = ntc_connect(&socket_path, network_magic, false)
        .await
        .wrap_err_with(|| format!("failed to connect to NtC socket {}", socket_path.display()))?;

    let handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_MONITOR)
        .expect("NTC_LOCAL_TX_MONITOR handle missing");
    let mut client = LocalTxMonitorClient::new(handle);

    let snapshot = client
        .acquire()
        .await
        .wrap_err("LocalTxMonitor acquire failed")?;

    let out = match action {
        TxMempoolCommand::Info => {
            let sizes = client
                .get_sizes()
                .await
                .wrap_err("LocalTxMonitor get_sizes failed")?;
            json!({
                "slot": snapshot.slot_no,
                "capacityInBytes": sizes.capacity_in_bytes,
                "sizeInBytes": sizes.size_in_bytes,
                "numberOfTxs": sizes.num_txs,
            })
        }
        TxMempoolCommand::NextTx => {
            let tx = client
                .next_tx()
                .await
                .wrap_err("LocalTxMonitor next_tx failed")?;
            json!({
                "slot": snapshot.slot_no,
                "tx": tx.map(hex::encode),
            })
        }
        TxMempoolCommand::TxExists { tx_id } => {
            let id_bytes = decode_tx_id_hex(&tx_id);
            let exists = client
                .has_tx(id_bytes)
                .await
                .wrap_err("LocalTxMonitor has_tx failed")?;
            json!({
                "slot": snapshot.slot_no,
                "exists": exists,
            })
        }
    };
    println!("{}", serde_json::to_string_pretty(&out)?);

    let _ = client.release().await;
    let _ = client.done().await;
    Ok(())
}
