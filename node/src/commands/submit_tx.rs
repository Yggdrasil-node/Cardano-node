//! `submit-tx` subcommand: drive the running node's NtC
//! `LocalTxSubmission` mini-protocol with a serialized transaction.
//!
//! Mirrors upstream `Cardano.CLI.Run.Transaction.Submit` (formerly
//! `Cardano.CLI.Shelley.Run.Transaction.runTxSubmit`). Yggdrasil's
//! variant is intentionally smaller — we only support submitting a
//! pre-serialized CBOR transaction (no era-aware envelope rewriting).
//!
//! The subcommand is Unix-only because the transport is a Unix domain
//! socket; non-Unix targets do not expose the `SubmitTx` clap arm.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Run/Transaction.hs>

#![cfg(unix)]

use std::path::PathBuf;

use eyre::{Result, WrapErr};
use serde_json::json;

/// Lenient hex decoder for the `--tx-hex` CLI argument.
///
/// Accepts whitespace-trimmed hex with an optional `0x` prefix (the
/// upstream `cardano-cli` accepts the bare form, but operators
/// frequently paste signed tx bytes via clipboard tooling that adds
/// `0x` even though upstream does not strictly require it).
///
/// Any non-hex input or odd-length body surfaces via the `hex` crate's
/// error, wrapped with "invalid hex in --tx-hex" so the operator sees
/// which CLI flag is at fault.
pub fn decode_tx_hex_arg(raw: &str) -> Result<Vec<u8>> {
    let stripped = raw.trim();
    let stripped = stripped.strip_prefix("0x").unwrap_or(stripped);
    hex::decode(stripped).wrap_err("invalid hex in --tx-hex")
}

/// Connect to the running node's NtC Unix socket and submit a transaction
/// via the LocalTxSubmission protocol, printing the accept/reject outcome.
///
/// Reference: `cardano-cli transaction submit` against
/// `ouroboros-network-protocols` LocalTxSubmission.
pub async fn run_submit_tx(
    socket_path: PathBuf,
    network_magic: u32,
    tx_bytes: Vec<u8>,
) -> Result<()> {
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
