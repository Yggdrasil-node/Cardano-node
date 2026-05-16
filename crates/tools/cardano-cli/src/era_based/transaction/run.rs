//! EraBased transaction run.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/Transaction/Run.hs`.
//! R292 landed the file as an API skeleton. R508–R513 (Phase 3.2/3.3)
//! land `run_transaction_txid_cmd` / `run_transaction_sign_cmd` /
//! `run_transaction_submit_cmd` / `run_transaction_view_cmd`,
//! mirroring the corresponding upstream `runTransaction*Cmd`.
//! `transaction build` / `build-raw` (full tx construction) port
//! over subsequent rounds.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

use crate::era_independent::address::run::read_verification_key_text_envelope;
use crate::lsq::LsqClient;

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

/// Run `transaction sign` — sign a transaction with one Ed25519
/// signing key and write the signed CBOR to `out_file`.
///
/// Mirrors upstream `runTransactionSignCmd` from
/// `Cardano.CLI.EraBased.Transaction.Run`. Yggdrasil's surface (the
/// node binary's `cardano-cli transaction-sign` wrapper is the
/// parity reference) is the single-signer form: it *replaces* the
/// transaction's witness set with a fresh one carrying exactly the
/// one `[vkey, signature]` pair, rather than appending to an
/// existing multi-witness set.
pub fn run_transaction_sign_cmd(
    tx_file: Option<PathBuf>,
    tx_hex: Option<String>,
    signing_key_file: &Path,
    out_file: &Path,
) -> Result<()> {
    let tx_bytes = read_tx_input(tx_file, tx_hex)?;
    let sk_envelope = std::fs::read(signing_key_file).wrap_err_with(|| {
        format!(
            "failed to read --signing-key-file {}",
            signing_key_file.display()
        )
    })?;
    // A signing-key TextEnvelope carries the same `5820`-prefixed
    // 32-byte payload shape as a verification-key envelope, so the
    // shared decoder applies.
    let sk_bytes = read_verification_key_text_envelope(&sk_envelope)?;
    let signed_tx = sign_tx_with_fresh_witness_set(&tx_bytes, &sk_bytes)?;
    std::fs::write(out_file, &signed_tx)
        .wrap_err_with(|| format!("failed to write --out-file {}", out_file.display()))?;
    Ok(())
}

/// Sign a transaction, replacing its witness set with a fresh
/// single-signer set `{0: [[vkey, signature]]}`.
///
/// The transaction is the CBOR array `[body, witness_set, …tail]`.
/// The body byte span is captured verbatim (`Decoder::raw_value`),
/// the original witness set is skipped, and everything after it (the
/// `tail` — `is_valid` flag, auxiliary data) is preserved. The
/// txid (Blake2b-256 of the body) is Ed25519-signed; the result is
/// re-assembled as `array(L) || body || fresh_wits || tail`.
fn sign_tx_with_fresh_witness_set(tx_bytes: &[u8], sk_bytes: &[u8; 32]) -> Result<Vec<u8>> {
    use yggdrasil_crypto::SigningKey;
    use yggdrasil_ledger::cbor::{Decoder, Encoder};

    // Parse the outer array, capture the body span, skip the old
    // witness set, and remember the tail offset.
    let mut dec = Decoder::new(tx_bytes);
    let array_len = dec
        .array()
        .map_err(|e| eyre::eyre!("tx CBOR does not start with an array: {e}"))?;
    if array_len < 2 {
        eyre::bail!(
            "tx CBOR outer array must have at least 2 elements (body + witness set); got {array_len}"
        );
    }
    let body_bytes = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract the transaction body bytes: {e}"))?
        .to_vec();
    dec.skip()
        .map_err(|e| eyre::eyre!("failed to skip the original witness set: {e}"))?;
    let tail = &tx_bytes[dec.position()..];

    // Sign the txid (Blake2b-256 of the body) with the supplied SK.
    let sk = SigningKey::from_bytes(*sk_bytes);
    let vk = sk
        .verification_key()
        .map_err(|e| eyre::eyre!("derive VK from SK failed: {e}"))?;
    let txid = yggdrasil_ledger::compute_tx_id(&body_bytes);
    let sig = sk
        .sign(&txid.0)
        .map_err(|e| eyre::eyre!("sign txid failed: {e}"))?;

    // Fresh witness set = `{0: [[vk_bytes, sig_bytes]]}`.
    let mut wits = Encoder::new();
    wits.map(1);
    wits.unsigned(0);
    wits.array(1);
    wits.array(2);
    wits.bytes(&vk.to_bytes());
    wits.bytes(&sig.to_bytes());
    let wits_bytes = wits.into_bytes();

    // Re-assemble: outer array(L) || body || fresh wits || tail.
    let mut header = Encoder::new();
    header.array(array_len);
    let mut out = header.into_bytes();
    out.extend_from_slice(&body_bytes);
    out.extend_from_slice(&wits_bytes);
    out.extend_from_slice(tail);
    Ok(out)
}

/// Run `transaction submit` — submit a serialized transaction to a
/// running node over the NtC LocalTxSubmission mini-protocol.
///
/// Mirrors upstream `runTransactionSubmitCmd` from
/// `Cardano.CLI.EraBased.Transaction.Run`. The transaction is read
/// from `--tx-file` / `--tx-hex`; the actual socket drive is the
/// `client`'s job (the `LsqClient` trait — see `crate::lsq`). The
/// accept/reject outcome is printed by the client as JSON.
pub fn run_transaction_submit_cmd(
    tx_file: Option<PathBuf>,
    tx_hex: Option<String>,
    socket_path: &Path,
    network_magic: u32,
    client: &dyn LsqClient,
) -> Result<()> {
    let tx_bytes = read_tx_input(tx_file, tx_hex)?;
    client.submit_tx(socket_path, network_magic, &tx_bytes)
}

/// Run `transaction view` — print the structural breakdown of a
/// serialized transaction as JSON.
///
/// Mirrors upstream `runTransactionViewCmd` from
/// `Cardano.CLI.EraBased.Transaction.Run`, but **shallow**: rather
/// than a full era-aware decode of every tx-body field, it surfaces
/// the txid plus each top-level CBOR array element (body / witness
/// set / tail) as hex. A full typed pretty-printer is a follow-on —
/// this gives operators the txid + structure without depending on a
/// per-era tx-body decoder. The shallow shape is deliberate and
/// documented in the output's `view` field.
pub fn run_transaction_view_cmd(tx_file: Option<PathBuf>, tx_hex: Option<String>) -> Result<()> {
    let tx_bytes = read_tx_input(tx_file, tx_hex)?;
    let view = decode_tx_structure(&tx_bytes)?;
    println!("{}", serde_json::to_string_pretty(&view)?);
    Ok(())
}

/// Decode the *structure* of a CBOR transaction `[body, witness_set,
/// …tail]` into a JSON object: the txid (Blake2b-256 of the body),
/// the outer array length, and each element as hex.
fn decode_tx_structure(tx_bytes: &[u8]) -> Result<serde_json::Value> {
    use yggdrasil_ledger::cbor::Decoder;

    let mut dec = Decoder::new(tx_bytes);
    let array_len = dec
        .array()
        .map_err(|e| eyre::eyre!("transaction CBOR does not start with an array: {e}"))?;
    if array_len < 2 {
        eyre::bail!(
            "tx CBOR outer array must have at least 2 elements (body + witness set); got {array_len}"
        );
    }
    let body = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract the transaction body bytes: {e}"))?
        .to_vec();
    let witness_set = dec
        .raw_value()
        .map_err(|e| eyre::eyre!("failed to extract the witness-set bytes: {e}"))?
        .to_vec();
    let tail = &tx_bytes[dec.position()..];
    let txid = yggdrasil_ledger::compute_tx_id(&body).0;

    Ok(serde_json::json!({
        "view": "shallow — txid + top-level CBOR structure; not a full per-era field decode",
        "txid": hex::encode(txid),
        "tx_array_len": array_len,
        "body_cbor": hex::encode(&body),
        "witness_set_cbor": hex::encode(&witness_set),
        "tail_cbor": hex::encode(tail),
    }))
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
            eyre::bail!("this transaction subcommand requires either --tx-file or --tx-hex")
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

    /// `sign_tx_with_fresh_witness_set` preserves the body, keeps the
    /// outer array length, and installs a `{0: [[vk, sig]]}` witness
    /// set carrying the VK derived from the supplied SK.
    #[test]
    fn sign_installs_fresh_single_signer_witness_set() {
        use yggdrasil_ledger::cbor::Decoder;

        // Minimal unsigned tx `[ {}, {} ]` — body + (empty) witness set.
        let tx = vec![0x82, 0xA0, 0xA0];
        let sk_bytes = [7_u8; 32];
        let signed = sign_tx_with_fresh_witness_set(&tx, &sk_bytes).expect("sign minimal tx");

        let mut dec = Decoder::new(&signed);
        assert_eq!(
            dec.array().expect("signed tx is an array"),
            2,
            "outer length preserved"
        );
        assert_eq!(
            dec.raw_value().expect("body span"),
            &[0xA0],
            "the body must be byte-identical to the unsigned tx"
        );
        // Witness set: a 1-entry map keyed 0, value a 1-element array
        // of a 2-element [vk, sig] array.
        assert_eq!(dec.map().expect("witness map"), 1);
        assert_eq!(dec.unsigned().expect("witness key"), 0);
        assert_eq!(dec.array().expect("vkey-witness list"), 1);
        assert_eq!(dec.array().expect("vkey-witness pair"), 2);
        let vk = dec.bytes().expect("vk bytes");
        let sig = dec.bytes().expect("sig bytes");
        assert_eq!(vk.len(), 32, "Ed25519 verification key is 32 bytes");
        assert_eq!(sig.len(), 64, "Ed25519 signature is 64 bytes");

        let expected_vk = yggdrasil_crypto::SigningKey::from_bytes(sk_bytes)
            .verification_key()
            .expect("derive VK")
            .to_bytes();
        assert_eq!(vk, expected_vk, "witness VK must match the supplied SK");
    }

    /// Ed25519 signing is deterministic — signing the same tx with
    /// the same key twice yields byte-identical output.
    #[test]
    fn sign_is_deterministic() {
        let tx = vec![0x82, 0xA0, 0xA0];
        let a = sign_tx_with_fresh_witness_set(&tx, &[9_u8; 32]).expect("sign a");
        let b = sign_tx_with_fresh_witness_set(&tx, &[9_u8; 32]).expect("sign b");
        assert_eq!(
            a, b,
            "deterministic Ed25519 signing must reproduce the signed tx"
        );
    }

    /// `sign_tx_with_fresh_witness_set` rejects an outer array with
    /// fewer than 2 elements (no witness-set slot).
    #[test]
    fn sign_rejects_too_short_outer_array() {
        // `[ {} ]` = 0x81 0xA0 — a 1-element array.
        let err = sign_tx_with_fresh_witness_set(&[0x81, 0xA0], &[1_u8; 32])
            .expect_err("1-element tx array must bail");
        assert!(
            err.to_string().contains("at least 2 elements"),
            "error must explain the ≥2-element requirement; got {err}"
        );
    }

    /// `decode_tx_structure` breaks a CBOR tx into its txid + the
    /// hex of each top-level element.
    #[test]
    fn tx_structure_splits_body_witnesses_tail() {
        // `[ {}, {}, true ]` = 0x83 0xA0 0xA0 0xF5 — body, witness
        // set, and a 1-byte tail (the `is_valid` flag).
        let tx = vec![0x83, 0xA0, 0xA0, 0xF5];
        let v = decode_tx_structure(&tx).expect("structure of a 3-element tx");
        assert_eq!(v["tx_array_len"], 3);
        assert_eq!(v["body_cbor"], "a0");
        assert_eq!(v["witness_set_cbor"], "a0");
        assert_eq!(v["tail_cbor"], "f5");
        // The txid is Blake2b-256 of the body byte span (`0xa0`).
        let expected = hex::encode(yggdrasil_ledger::compute_tx_id(&[0xA0]).0);
        assert_eq!(v["txid"], expected);
    }

    /// `decode_tx_structure` rejects a tx whose outer array has
    /// fewer than 2 elements.
    #[test]
    fn tx_structure_rejects_too_short_array() {
        let err = decode_tx_structure(&[0x81, 0xA0]).expect_err("1-element array must bail");
        assert!(
            err.to_string().contains("at least 2 elements"),
            "error must explain the ≥2-element requirement; got {err}"
        );
    }
}
