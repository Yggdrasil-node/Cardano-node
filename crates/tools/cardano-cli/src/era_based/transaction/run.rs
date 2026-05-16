//! EraBased transaction run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/Transaction/Run.hs`.
//! R292 landed the file as an API skeleton. R508 (Phase 3.2) lands
//! the first concrete subcommand — `run_transaction_txid_cmd`,
//! mirroring upstream `runTransactionTxIdCmd`. Remaining transaction
//! subcommands (`transaction build`, `transaction build-raw`,
//! `transaction sign`, `transaction view`, …) port over subsequent
//! rounds.

use std::path::PathBuf;

use eyre::{Result, WrapErr};

/// Run `transaction txid` — print the transaction id of a serialized
/// transaction as 64-char lowercase hex.
///
/// Mirrors upstream `runTransactionTxIdCmd` from
/// `Cardano.CLI.EraBased.Transaction.Run`. The transaction is read
/// from either `--tx-file` or `--tx-hex` (clap enforces exactly one
/// via `conflicts_with`); the id is the Blake2b-256 hash of the CBOR
/// transaction *body* — the first element of the outer transaction
/// array — matching `getTxId . getTxBody` upstream.
pub fn run_transaction_txid_cmd(tx_file: Option<PathBuf>, tx_hex: Option<String>) -> Result<()> {
    let tx_bytes = read_tx_input(tx_file, tx_hex)?;
    let txid = compute_txid_from_tx_cbor(&tx_bytes)?;
    // Upstream `cardano-cli transaction txid` prints 64-char
    // lowercase hex, no `0x` prefix.
    println!("{}", hex::encode(txid));
    Ok(())
}

/// Resolve the `--tx-file` / `--tx-hex` flag pair to raw transaction
/// bytes. clap's `conflicts_with` already rejects "both"; this
/// handles the "file", "hex", and "neither" cases.
fn read_tx_input(tx_file: Option<PathBuf>, tx_hex: Option<String>) -> Result<Vec<u8>> {
    match (tx_file, tx_hex) {
        (Some(path), None) => std::fs::read(&path)
            .wrap_err_with(|| format!("failed to read --tx-file {}", path.display())),
        (None, Some(hex_str)) => {
            // Tolerate a leading `0x` + surrounding whitespace for
            // terminal-paste ergonomics.
            let stripped = hex_str.trim();
            let stripped = stripped.strip_prefix("0x").unwrap_or(stripped);
            hex::decode(stripped).wrap_err("invalid hex in --tx-hex")
        }
        (None, None) => {
            eyre::bail!("transaction txid requires either --tx-file or --tx-hex")
        }
        (Some(_), Some(_)) => {
            unreachable!("clap's conflicts_with prevents --tx-file + --tx-hex both being set")
        }
    }
}

/// Compute the transaction id from a complete CBOR transaction.
///
/// Every Cardano era encodes a transaction as a CBOR array whose
/// first element is the transaction body. The id is the Blake2b-256
/// hash of that body's *raw* CBOR bytes — so the body byte span is
/// captured verbatim via `Decoder::raw_value` rather than
/// re-encoded. Delegates the hash to `yggdrasil_ledger::compute_tx_id`.
fn compute_txid_from_tx_cbor(tx_bytes: &[u8]) -> Result<[u8; 32]> {
    use yggdrasil_ledger::cbor::Decoder;
    use yggdrasil_ledger::compute_tx_id;

    let mut dec = Decoder::new(tx_bytes);
    let _array_len = dec
        .array()
        .map_err(|e| eyre::eyre!("transaction CBOR does not start with an array: {e}"))?;
    let body_bytes = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract the transaction body bytes: {e}"))?;
    Ok(compute_tx_id(body_bytes).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_tx_input` with neither flag bails with the documented
    /// "requires --tx-file or --tx-hex" message.
    #[test]
    fn read_tx_input_rejects_no_flags() {
        let err = read_tx_input(None, None).expect_err("neither flag must bail");
        assert!(
            err.to_string().contains("--tx-file or --tx-hex"),
            "error must name both flags; got {err}"
        );
    }

    /// `read_tx_input` decodes `--tx-hex`, tolerating a `0x` prefix +
    /// surrounding whitespace.
    #[test]
    fn read_tx_input_decodes_hex_with_0x_prefix() {
        let bytes = read_tx_input(None, Some("  0xDEADbeef \n".to_string()))
            .expect("0x-prefixed hex must decode");
        assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    /// `compute_txid_from_tx_cbor` hashes the body span of a minimal
    /// CBOR transaction array `[body, …]`. We use a 2-element array
    /// whose first element is an empty map (`0xA0` — a degenerate but
    /// structurally-valid body) and assert the id is the Blake2b-256
    /// of exactly that one body byte.
    #[test]
    fn txid_hashes_the_body_byte_span() {
        // CBOR `[ {}, {} ]` = 0x82 0xA0 0xA0. The body is the first
        // element: the single byte 0xA0 (empty map).
        let tx = vec![0x82, 0xA0, 0xA0];
        let txid = compute_txid_from_tx_cbor(&tx).expect("txid from minimal tx");
        let expected = yggdrasil_ledger::compute_tx_id(&[0xA0]).0;
        assert_eq!(
            txid, expected,
            "txid must be Blake2b-256 of the body byte span (0xA0), not the whole tx"
        );
    }

    /// A transaction CBOR that does not start with an array surfaces
    /// a structured error rather than panicking.
    #[test]
    fn txid_rejects_non_array_cbor() {
        // 0x00 is the CBOR unsigned integer 0 — not an array.
        let err = compute_txid_from_tx_cbor(&[0x00]).expect_err("non-array must bail");
        assert!(
            err.to_string().contains("does not start with an array"),
            "error must explain the array requirement; got {err}"
        );
    }
}
