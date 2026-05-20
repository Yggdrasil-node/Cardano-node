//! `submit-tx` subcommand: submit a serialized transaction through
//! the shared cardano-cli NtC LocalTxSubmission client.
//!
//! Mirrors upstream `Cardano.CLI.Run.Transaction.Submit` (formerly
//! `Cardano.CLI.Shelley.Run.Transaction.runTxSubmit`). Yggdrasil's
//! variant is intentionally smaller — we only support submitting a
//! pre-serialized CBOR transaction (no era-aware envelope rewriting).
//!
//! The concrete transport is owned by
//! `yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient`; this module
//! remains as the node binary's compatibility adapter for the
//! top-level `submit-tx` command.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Run/Transaction.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `submit-tx` subcommand
//! (NtC LocalTxSubmission client). Mirrors the dispatch half of
//! upstream `Cardano.CLI.Compatible.Transaction::runSubmitTxCmd`.
//! Yggdrasil's binary covers the core submit-tx flow; the
//! broader cardano-cli transaction-construction surface is
//! scheduled for Phase F (R289-R295).

use std::path::PathBuf;

use eyre::Result;
use yggdrasil_cardano_cli::lsq::LsqClient;
use yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient;

/// Compatibility re-export for the node-level `submit-tx` wrapper.
pub use yggdrasil_cardano_cli::era_based::transaction::run::decode_tx_hex_arg;

/// Connect to the running node's NtC socket and submit a transaction
/// via the shared cardano-cli LocalTxSubmission client, printing the
/// accept/reject outcome.
///
/// Reference: `cardano-cli transaction submit` against
/// `ouroboros-network/ouroboros-network/protocols/lib` LocalTxSubmission.
pub fn run_submit_tx(socket_path: PathBuf, network_magic: u32, tx_bytes: Vec<u8>) -> Result<()> {
    TokioLsqClient.submit_tx(&socket_path, network_magic, &tx_bytes)
}
