// R251 forensic helper — clippy relaxations are intentional: this binary is
// not on the runtime hot path. The flagged lints (complex tuple return
// types, "too many args", and the duplicated eprintln branch) trade
// readability of a debug-only tool against clippy strictness; promoting the
// tool to production would require the standard refactor.
#![allow(
    clippy::if_same_then_else,
    clippy::manual_is_multiple_of,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
//! Forensic helper that walks a Haskell `cardano-node` immutable-DB chunk
//! file as a sequence of CBOR-encoded blocks and dumps the block at a target
//! slot — header, per-tx body bytes, witness-set bytes, and a few per-tx
//! identifying fields.
//!
//! Used during R251 to investigate Gap BQ (preview vkey witness rejection
//! at slot ~1,525,024 on tx `44ccae43…`). Yggdrasil's CEK / strict-Ed25519
//! analysis was inconclusive because the captured (vkey, msg, sig) is
//! fully canonical; the bug must be in the byte range we hash for
//! `tx_body_hash` or in how `ShelleyWitnessSet::from_cbor_bytes` parses
//! the witness CBOR. Comparing what Haskell sees on-the-wire (this dump)
//! against what `extract_block_tx_byte_spans` produces in Yggdrasil
//! pinpoints the divergence.
//!
//! The Haskell ImmutableDB chunk format is a stream of raw CBOR values:
//! one CBOR-encoded block after another, with no length-prefix wrapper
//! per block. We rely on the CBOR decoder's notion of "consumed bytes" to
//! advance the cursor.
//!
//! Reference: `Ouroboros.Consensus.Storage.ImmutableDB.Impl` (chunk file
//! format), `Cardano.Chain.Block` (Byron block CBOR), and
//! `Ouroboros.Network.Protocol.ChainSync.PipeliningHaders.RawHeader`
//! (Shelley-family era-tagged outer wrapper).
//!
//! Usage:
//! ```text
//! cargo run --release -p yggdrasil-db-analyser --bin dump_block -- \
//!     /path/to/preview/db/immutable/00017.chunk 1525057
//! ```
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side CLI binary that
//! dumps a single block's CBOR + decoded form to stdout for
//! forensic inspection. Operator-tooling-side equivalent of
//! upstream `cardano-tools/db-analyser --dump-block`. No
//! single-file upstream parallel; Yggdrasil keeps the dumper under
//! the `db-analyser` sister-tool crate instead of the node binary
//! shell.

use std::env;
use std::fs;
use std::process::ExitCode;

use yggdrasil_ledger::Decoder;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump_block <path-to-immutable.chunk> <target-slot> [target-tx-id-hex]");
        return ExitCode::FAILURE;
    }
    let path = &args[1];
    let target_slot: u64 = match args[2].parse() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("invalid slot: {e}");
            return ExitCode::FAILURE;
        }
    };
    let target_tx: Option<String> = args.get(3).cloned();

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "[dump_block] file={} size={}B target_slot={} target_tx={:?}",
        path,
        bytes.len(),
        target_slot,
        target_tx,
    );

    let mut cursor: usize = 0;
    let mut block_index: usize = 0;
    while cursor < bytes.len() {
        // Each CBOR block in the chunk file is the on-wire era-tagged outer
        // wrapper: `[era_tag, era_block_cbor]`. We decode the outer pair and
        // peek into the inner block to get the slot.
        let block_start = cursor;
        let mut dec = Decoder::new(&bytes[cursor..]);
        let outer_pos_before = dec.position();
        let (era_tag, slot, tx_body_spans, witness_set_spans, block_size) =
            match try_decode_block(&mut dec, &bytes[cursor..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "[dump_block] decode failed at offset {} (block #{}): {}",
                        block_start, block_index, e
                    );
                    return ExitCode::FAILURE;
                }
            };

        let consumed = dec.position() - outer_pos_before;
        let block_bytes = &bytes[cursor..cursor + consumed];

        // Scan all tx-ids in this block for the target prefix.
        if let Some(prefix) = target_tx.as_deref() {
            let prefix_lc = prefix.to_ascii_lowercase();
            // Inner-block bytes: re-decode outer to find them.
            let mut decoder = Decoder::new(&bytes[cursor..]);
            let _ = decoder.array();
            let _ = decoder.unsigned();
            let probe = Decoder::new(&bytes[cursor + decoder.position()..]).tag();
            let inner_bytes: Vec<u8> = if probe.ok() == Some(24) {
                let _ = decoder.tag();
                match decoder.bytes() {
                    Ok(b) => b.to_vec(),
                    Err(_) => Vec::new(),
                }
            } else {
                let s = decoder.position();
                let _ = decoder.skip();
                let e = decoder.position();
                bytes[cursor + s..cursor + e].to_vec()
            };
            for &(s, e) in &tx_body_spans {
                if e > inner_bytes.len() {
                    continue;
                }
                let body = &inner_bytes[s..e];
                let tx_id = yggdrasil_crypto::hash_bytes_256(body).0;
                let tx_id_hex = bytes_to_hex(&tx_id);
                if tx_id_hex.starts_with(&prefix_lc) {
                    eprintln!(
                        "[dump_block] FOUND target tx={} at block #{} (era={} slot={} chunk_offset={})",
                        tx_id_hex, block_index, era_tag, slot, block_start
                    );
                    print_block_dump(
                        era_tag,
                        slot,
                        cursor,
                        block_size,
                        block_bytes,
                        &tx_body_spans,
                        &witness_set_spans,
                        &bytes[cursor..],
                        Some(prefix),
                    );
                    return ExitCode::SUCCESS;
                }
            }
        }
        // Print every block when target_tx is "list" — useful to locate
        // a block by chunk offset.
        if target_tx.as_deref() == Some("list") {
            eprintln!(
                "[dump_block] block #{} at offset {} era={} slot={} bodies={}",
                block_index,
                block_start,
                era_tag,
                slot,
                tx_body_spans.len()
            );
        } else if block_index < 3 || block_index % 100 == 0 {
            eprintln!(
                "[dump_block] block #{} at offset {} era={} slot={} bodies={}",
                block_index,
                block_start,
                era_tag,
                slot,
                tx_body_spans.len()
            );
        }
        if slot == target_slot {
            print_block_dump(
                era_tag,
                slot,
                cursor,
                block_size,
                block_bytes,
                &tx_body_spans,
                &witness_set_spans,
                &bytes[cursor..],
                target_tx.as_deref(),
            );
            return ExitCode::SUCCESS;
        }

        cursor += consumed;
        block_index += 1;
    }

    eprintln!(
        "[dump_block] reached end of chunk after {} blocks without finding slot {}",
        block_index, target_slot
    );
    ExitCode::FAILURE
}

/// Decode just enough of the era-tagged outer block to extract the era tag,
/// slot, and the tx-body / witness-set byte spans. Returns the consumed
/// length so the caller can advance.
fn try_decode_block(
    dec: &mut Decoder<'_>,
    chunk_remainder: &[u8],
) -> Result<(u64, u64, Vec<(usize, usize)>, Vec<(usize, usize)>, usize), String> {
    // Outer wrapper: `[era_tag, era_block_cbor_or_inner]`
    // Era tags (per `Ouroboros.Network.Protocol.LocalStateQuery`):
    //   0 = EBB, 1 = Byron, 2 = Shelley, 3 = Allegra, 4 = Mary,
    //   5 = Alonzo, 6 = Babbage, 7 = Conway
    let outer_arr_len = dec.array().map_err(|e| format!("outer arr: {e:?}"))?;
    if outer_arr_len != 2 {
        return Err(format!("expected 2-elem outer, got {outer_arr_len}"));
    }
    let era_tag = dec.unsigned().map_err(|e| format!("era tag: {e:?}"))?;
    // The inner is a tag-24 CBOR-encoded byte string of the actual era block,
    // OR for some chunk encodings it's the raw block array directly.
    let block_size = chunk_remainder.len();

    // Try tag-24 wrapped form first.
    let mut peek = Decoder::new(&chunk_remainder[dec.position()..]);
    let probe = peek.tag().ok();
    if probe == Some(24) {
        // Skip the tag and consume the byte-string-wrapped inner block.
        let _ = dec.tag();
        let inner_bytes = dec.bytes().map_err(|e| format!("inner bs: {e:?}"))?;
        let (slot, body_spans, ws_spans) = decode_inner_block(era_tag, inner_bytes)?;
        return Ok((era_tag, slot, body_spans, ws_spans, block_size));
    }

    // Otherwise the inner block is directly inline as a CBOR array.
    // Save start position then walk the inner block.
    let inner_start = dec.position();
    decode_block_inline(dec, era_tag).map_err(|e| format!("inline block: {e}"))?;
    let inner_end = dec.position();
    // Reconstruct the inner block bytes from the chunk remainder.
    let inner_bytes = &chunk_remainder[inner_start..inner_end];
    let (slot, body_spans, ws_spans) = decode_inner_block(era_tag, inner_bytes)?;
    Ok((era_tag, slot, body_spans, ws_spans, block_size))
}

/// Walk an inner block CBOR value just enough to advance the decoder past it.
fn decode_block_inline(dec: &mut Decoder<'_>, _era_tag: u64) -> Result<(), String> {
    // Inner block layout (Shelley-family): `[header, [tx_bodies], [witness_sets], aux_data, [invalid_txs]]`
    // For Byron it's similar but with different header shape. We just walk.
    dec.skip().map_err(|e| format!("skip inner: {e:?}"))
}

/// Decode an inner block to extract slot, tx-body byte spans, and
/// witness-set byte spans. Spans are (start, end) byte offsets within the
/// `inner_bytes` slice.
///
/// Returns `(slot, tx_body_spans, witness_set_spans)`.
fn decode_inner_block(
    era_tag: u64,
    inner_bytes: &[u8],
) -> Result<(u64, Vec<(usize, usize)>, Vec<(usize, usize)>), String> {
    let mut dec = Decoder::new(inner_bytes);

    if era_tag == 1 {
        // Byron — header layout differs; we don't dive in here.
        let _arr = dec.array().map_err(|e| format!("byron arr: {e:?}"))?;
        // Byron header is far more complex; for now return slot=0 and empty spans.
        return Ok((0, Vec::new(), Vec::new()));
    }

    // Shelley-family: outer 4 or 5-element array.
    let _block_arr = dec.array().map_err(|e| format!("block arr: {e:?}"))?;
    // header — extract slot from header.body.slot (inside the typed header).
    let header_start = dec.position();
    dec.skip().map_err(|e| format!("skip header: {e:?}"))?;
    let header_end = dec.position();
    let slot = extract_slot_from_header(&inner_bytes[header_start..header_end]);

    // Capture tx-body spans.
    let bodies_start = dec.position();
    let body_count = dec.array().map_err(|e| format!("body arr: {e:?}"))?;
    let mut body_spans = Vec::with_capacity(body_count as usize);
    for _ in 0..body_count {
        let s = dec.position();
        dec.skip().map_err(|e| format!("skip body: {e:?}"))?;
        let e = dec.position();
        body_spans.push((s, e));
    }
    let _ = bodies_start;

    // Capture witness-set spans.
    let ws_count = dec.array().map_err(|e| format!("ws arr: {e:?}"))?;
    let mut ws_spans = Vec::with_capacity(ws_count as usize);
    for _ in 0..ws_count {
        let s = dec.position();
        dec.skip().map_err(|e| format!("skip ws: {e:?}"))?;
        let e = dec.position();
        ws_spans.push((s, e));
    }

    Ok((slot, body_spans, ws_spans))
}

/// Extract slot from a Shelley-family header. The header CBOR is a
/// 2-element array `[header_body, kes_signature]` where header_body's
/// 2nd field (index 1) is the slot number.
fn extract_slot_from_header(header_bytes: &[u8]) -> u64 {
    let mut dec = Decoder::new(header_bytes);
    let Ok(_outer_len) = dec.array() else {
        return 0;
    };
    let Ok(_body_arr_len) = dec.array() else {
        return 0;
    };
    // Field 0: block number (skip).
    if dec.skip().is_err() {
        return 0;
    }
    // Field 1: slot number.
    dec.unsigned().unwrap_or(0)
}

/// Print a structured forensic dump of the target block.
fn print_block_dump(
    era_tag: u64,
    slot: u64,
    chunk_offset: usize,
    block_size: usize,
    block_bytes: &[u8],
    body_spans: &[(usize, usize)],
    ws_spans: &[(usize, usize)],
    block_remainder: &[u8],
    target_tx_hex: Option<&str>,
) {
    println!("# Yggdrasil R251 forensic block dump");
    println!("era_tag: {era_tag}");
    println!("slot: {slot}");
    println!("chunk_offset: {chunk_offset}");
    println!("block_size_in_chunk_remainder: {block_size}");
    println!("block_outer_bytes_len: {}", block_bytes.len());
    println!("tx_count: {}", body_spans.len());
    println!();
    println!("# Each tx body / witness set span is offset within the INNER block bytes.");

    // Re-decode inner so we can print the inner-block hex too.
    // The inner block bytes start somewhere after the outer 2-element array
    // header + era_tag CBOR; for diagnostic clarity we just print the full
    // outer CBOR plus per-tx details below.
    println!();
    println!("block_outer_cbor_hex:");
    println!("{}", hex_lines(block_bytes, 64));

    // Find inner block bytes by re-decoding outer.
    let mut dec = Decoder::new(block_remainder);
    let _ = dec.array(); // outer
    let _ = dec.unsigned(); // era tag
    let inner_start_outer = dec.position();
    let probe = Decoder::new(&block_remainder[inner_start_outer..]).tag();
    let inner_bytes = if probe.ok() == Some(24) {
        let _ = dec.tag();
        match dec.bytes() {
            Ok(b) => b.to_vec(),
            Err(_) => return,
        }
    } else {
        let s = dec.position();
        let _ = dec.skip();
        let e = dec.position();
        block_remainder[s..e].to_vec()
    };

    println!();
    println!("inner_block_bytes_len: {}", inner_bytes.len());

    let mut matched_target = false;
    for (i, &(start, end)) in body_spans.iter().enumerate() {
        if end > inner_bytes.len() {
            continue;
        }
        let body = &inner_bytes[start..end];
        let tx_id = yggdrasil_crypto::hash_bytes_256(body).0;
        let tx_id_hex = bytes_to_hex(&tx_id);
        let is_target = target_tx_hex
            .map(|h| tx_id_hex.starts_with(&h.to_ascii_lowercase()))
            .unwrap_or(false);
        if is_target {
            matched_target = true;
        }
        println!();
        println!(
            "## tx[{}]: id={} (len={}, span=[{},{})){}",
            i,
            tx_id_hex,
            body.len(),
            start,
            end,
            if is_target { " ← TARGET" } else { "" }
        );
        println!("body_hex:");
        println!("{}", hex_lines(body, 64));

        if let Some((ws_start, ws_end)) = ws_spans.get(i).copied() {
            if ws_end <= inner_bytes.len() {
                let ws = &inner_bytes[ws_start..ws_end];
                println!();
                println!(
                    "witness_set_hex (len={}, span=[{},{})):",
                    ws.len(),
                    ws_start,
                    ws_end
                );
                println!("{}", hex_lines(ws, 64));
            }
        }
    }

    if let Some(t) = target_tx_hex {
        if matched_target {
            println!("\n# matched target tx prefix: {t}");
        } else {
            println!("\n# WARNING: no tx in this block matched target prefix {t}");
        }
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn hex_lines(bytes: &[u8], width: usize) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(width) {
        for b in chunk {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{b:02x}");
        }
        out.push('\n');
    }
    out
}
