//! BlockFetch fetch primitives and ChainSync point/range helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side BlockFetch range adapters
//! that wrap the typed `BlockFetchClient` from `yggdrasil_network`
//! and bridge to either raw bytes, typed `ShelleyBlock`, or
//! multi-era decoded payloads. Surfaces upstream
//! `Ouroboros.Network.BlockFetch.Client` /
//! `Ouroboros.Consensus.MiniProtocol.BlockFetch.Client` semantics —
//! in particular the upstream-Origin-lower-bound normalization
//! (collapse `[Origin, upper]` to `[upper, upper]` because the
//! BlockFetch server cannot resolve `Point::Origin` as a virtual
//! genesis tip).
//!
//! `point_from_raw_header` handles every ChainSync header envelope
//! flavor Yggdrasil has seen on the wire — typed Shelley/Praos,
//! CBOR-in-CBOR-wrapped, Byron raw EBB / main, and the multi-era
//! 2-tuple envelope — synthesising a `Point::BlockPoint(slot, hash)`
//! with era-correct hash-prefix selection (R211 lesson: Byron EBB
//! uses `0x82 0x00`, main uses `0x82 0x01`).
//!
//! Public functions moved from `node/src/sync.rs`:
//!
//! - `fetch_range_blocks` — raw byte BlockFetch range.
//! - `fetch_range_blocks_typed` — typed `ShelleyBlock` range.
//! - `fetch_range_blocks_multi_era_raw_decoded` — `pub(crate)`,
//!   used by `blockfetch_worker.rs`.
//! - `fetch_range_blocks_decoded` — decoded `ShelleyBlock` range.
//! - `normalize_blockfetch_range_points` — Origin → upper collapse,
//!   typed.
//! - `normalize_blockfetch_range_bytes` — same, byte-encoded.
//! - `point_from_raw_header` — header envelope → `Point` extractor.
//! - `point_bytes_from_raw_header_or_tip` — same, with tip fallback.
//! - `map_blockfetch_error` — typed-decode-error reclassification.
//!
//! Extracted from `node/src/sync.rs` in R500 (sync.rs R-arc, 3rd
//! slice). See `docs/operational-runs/2026-05-12-round-498-plan-sync-rs-split-arc.md`
//! for the multi-round plan.

use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_ledger::{
    BYRON_SLOTS_PER_EPOCH, CborDecode, CborEncode, Decoder, HeaderHash, Point, PraosHeader,
    ShelleyBlock, ShelleyHeader, SlotNo,
};
use yggdrasil_network::{BlockFetchClient, BlockFetchClientError, ChainRange};

use super::{
    MultiEraBlock, SyncError, decode_multi_era_block_ledger, drop_raw_range_lower_boundary,
};

pub(super) fn map_blockfetch_error(err: BlockFetchClientError) -> SyncError {
    match err {
        BlockFetchClientError::BlockDecode(err) => SyncError::LedgerDecode(err),
        other => SyncError::BlockFetch(other),
    }
}

pub(super) async fn fetch_range_blocks(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<Vec<u8>>, SyncError> {
    block_fetch
        .request_range_collect(ChainRange { lower, upper })
        .await
        .map_err(SyncError::BlockFetch)
}

pub(super) async fn fetch_range_blocks_typed(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
) -> Result<Vec<(Vec<u8>, ShelleyBlock)>, SyncError> {
    block_fetch
        .request_range_collect_points_raw_with(lower, upper, ShelleyBlock::from_cbor_bytes)
        .await
        .map_err(map_blockfetch_error)
}

pub async fn fetch_range_blocks_multi_era_raw_decoded(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
) -> Result<Vec<(Vec<u8>, MultiEraBlock)>, SyncError> {
    fetch_range_blocks_multi_era_raw_decoded_excluding_lower(block_fetch, lower, upper, None).await
}

pub(super) async fn fetch_range_blocks_multi_era_raw_decoded_excluding_lower(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
    excluded_lower: Option<Point>,
) -> Result<Vec<(Vec<u8>, MultiEraBlock)>, SyncError> {
    let mut raw_blocks = block_fetch
        .request_range_collect_points(lower, upper)
        .await
        .map_err(SyncError::BlockFetch)?;

    if let Some(point) = excluded_lower {
        drop_raw_range_lower_boundary(&mut raw_blocks, point);
    }

    raw_blocks
        .into_iter()
        .map(|raw| {
            let decoded = decode_multi_era_block_ledger(&raw).map_err(SyncError::LedgerDecode)?;
            Ok((raw, decoded))
        })
        .collect()
}

pub(super) async fn fetch_range_blocks_decoded(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<ShelleyBlock>, SyncError> {
    block_fetch
        .request_range_collect_decoded::<ShelleyBlock>(ChainRange { lower, upper })
        .await
        .map_err(map_blockfetch_error)
}

pub(super) fn normalize_blockfetch_range_points(
    lower: Point,
    upper: Point,
) -> Option<(Point, Point)> {
    let upper_is_block = matches!(upper, Point::BlockPoint(_, _));
    if !upper_is_block {
        return None;
    }

    if let (Point::BlockPoint(lower_slot, _), Point::BlockPoint(upper_slot, _)) = (lower, upper) {
        if upper_slot < lower_slot {
            return None;
        }
    }

    // Upstream BlockFetch (Ouroboros.Network.BlockFetch.Server) cannot resolve
    // `Point::Origin` as a lower bound — genesis is a virtual point with no
    // fetchable header. When the caller has no prior known tip we collapse the
    // range to `[upper, upper]` so the wire `MsgRequestRange` requests just the
    // single block at `upper`. Callers that need to detect this case and avoid
    // dropping the fetched block must inspect the *original* `from_point`
    // before normalization (see the dedup gate in
    // `blockfetch_range_for_pending_forwards`).
    let normalized_lower = if matches!(lower, Point::Origin) {
        upper
    } else {
        lower
    };

    Some((normalized_lower, upper))
}

pub(super) fn normalize_blockfetch_range_bytes(
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Option<(Vec<u8>, Vec<u8>)> {
    // When both bounds decode as proper Points, apply upstream Origin->upper
    // normalization. Otherwise (synthetic/opaque payloads used by lower-level
    // tests and pass-through call sites) hand the raw bytes through unchanged
    // so the BlockFetch wire request still carries the caller's intent.
    match (
        Point::from_cbor_bytes(&lower).ok(),
        Point::from_cbor_bytes(&upper).ok(),
    ) {
        (Some(lower_point), Some(upper_point)) => {
            let (normalized_lower, normalized_upper) =
                normalize_blockfetch_range_points(lower_point, upper_point)?;
            Some((
                normalized_lower.to_cbor_bytes(),
                normalized_upper.to_cbor_bytes(),
            ))
        }
        _ => Some((lower, upper)),
    }
}

pub(super) fn point_from_raw_header(raw_header: &[u8]) -> Option<Point> {
    fn raw_header_hash(raw_header: &[u8]) -> HeaderHash {
        HeaderHash(hash_bytes_256(raw_header).0)
    }

    fn decode_slot_from_with_origin(value: &[u8]) -> Option<u64> {
        let mut dec = Decoder::new(value);
        if let Ok(2) = dec.array() {
            let tag = dec.unsigned().ok()?;
            if tag != 0 && tag != 1 {
                return None;
            }
            let slot = dec.unsigned().ok()?;
            if dec.is_empty() {
                return Some(slot);
            }
        }
        None
    }

    fn decode_cbor_in_cbor_bytes(value: &[u8]) -> Option<Vec<u8>> {
        let mut dec = Decoder::new(value);
        if let Ok(unwrapped) = dec.wrapped() {
            if dec.is_empty() {
                return Some(unwrapped.to_vec());
            }
        }

        let mut dec = Decoder::new(value);
        if let Ok(bytes) = dec.bytes() {
            if dec.is_empty() {
                return Some(bytes.to_vec());
            }
        }

        None
    }

    fn byron_main_header_hash(raw_header: &[u8]) -> HeaderHash {
        // Byron main header hash uses prefix ++ raw annotated header bytes.
        // Reference: `Cardano.Chain.Block.Header.headerHashAnnotated` —
        // `wrapHeader = encodeListLen 2 <> encodeWord 1 <> annotation`.
        const MAIN_HASH_PREFIX: [u8; 2] = [0x82, 0x01];
        let mut bytes = Vec::with_capacity(MAIN_HASH_PREFIX.len() + raw_header.len());
        bytes.extend_from_slice(&MAIN_HASH_PREFIX);
        bytes.extend_from_slice(raw_header);
        HeaderHash(hash_bytes_256(&bytes).0)
    }

    fn byron_ebb_header_hash(raw_header: &[u8]) -> HeaderHash {
        // Byron EBB header hash uses prefix ++ raw annotated header bytes.
        // Reference: `Cardano.Chain.Block.Header.boundaryHeaderHashAnnotated`
        // — `wrapBoundaryBytes = encodeListLen 2 <> encodeWord 0 <>
        // annotation`.  R211 — was previously fallback-routed through
        // `byron_main_header_hash`, which produced a wrong hash for EBBs
        // and broke BlockFetch on mainnet's first epoch boundary block.
        const EBB_HASH_PREFIX: [u8; 2] = [0x82, 0x00];
        let mut bytes = Vec::with_capacity(EBB_HASH_PREFIX.len() + raw_header.len());
        bytes.extend_from_slice(&EBB_HASH_PREFIX);
        bytes.extend_from_slice(raw_header);
        HeaderHash(hash_bytes_256(&bytes).0)
    }

    fn decode_point_from_byron_raw_header(raw_header: &[u8]) -> Option<Point> {
        let mut dec = Decoder::new(raw_header);
        if dec.array().ok()? < 5 {
            return None;
        }

        // protocol_magic, prev_hash, body_proof
        dec.skip().ok()?;
        dec.skip().ok()?;
        dec.skip().ok()?;

        let consensus_len = dec.array().ok()?;
        if consensus_len >= 4 {
            // Main block header consensus_data: [slot_id, pubkey, difficulty, signature]
            let slot_id_len = dec.array().ok()?;
            if slot_id_len != 2 {
                return None;
            }

            let epoch = dec.unsigned().ok()?;
            let slot_in_epoch = dec.unsigned().ok()?;
            let slot = epoch
                .checked_mul(BYRON_SLOTS_PER_EPOCH)
                .and_then(|base| base.checked_add(slot_in_epoch))?;
            return Some(Point::BlockPoint(
                SlotNo(slot),
                byron_main_header_hash(raw_header),
            ));
        }

        if consensus_len >= 2 {
            // R211 — Byron EBB consensus_data is `[epoch, [difficulty]]`
            // (2 elements; second is a 1-element array wrapping the
            // difficulty value).  EBB header doesn't carry slot directly:
            // its slot equals `epoch * BYRON_SLOTS_PER_EPOCH` (the start
            // of the epoch).  Critically, the *hash* must be computed
            // with `EBB_HASH_PREFIX` (0x82 0x00), not the main-block
            // prefix (0x82 0x01) — using the wrong prefix produces a
            // hash the upstream BlockFetch server can't resolve, which
            // is exactly the mainnet stall surfaced by R208 + narrowed
            // by R210.  The outer chain-sync envelope sometimes carries
            // a `[?, ?]` 2-tuple where the second element looks like a
            // slot but is actually a different value (block number /
            // chain difficulty / similar) — empirically the inner
            // `epoch * BYRON_SLOTS_PER_EPOCH` derivation yields the
            // hash the upstream peer accepts in `MsgRequestRange`.
            // Reference: `Cardano.Chain.Block.Header.Boundary
            // .ConsensusData`.
            let epoch = dec.unsigned().ok()?;
            let slot = epoch.checked_mul(BYRON_SLOTS_PER_EPOCH)?;
            return Some(Point::BlockPoint(
                SlotNo(slot),
                byron_ebb_header_hash(raw_header),
            ));
        }

        None
    }

    fn decode_header_point_bytes(bytes: &[u8]) -> Option<Point> {
        if let Ok(header) = ShelleyHeader::from_cbor_bytes(bytes) {
            return Some(Point::BlockPoint(
                SlotNo(header.body.slot),
                raw_header_hash(bytes),
            ));
        }

        if let Ok(header) = PraosHeader::from_cbor_bytes(bytes) {
            return Some(Point::BlockPoint(
                SlotNo(header.body.slot),
                raw_header_hash(bytes),
            ));
        }

        None
    }

    fn decode_point_bytes(bytes: &[u8]) -> Option<Point> {
        Point::from_cbor_bytes(bytes).ok()
    }

    fn decode_point_from_serialised_header(bytes: &[u8]) -> Option<Point> {
        let mut dec = Decoder::new(bytes);
        if let Ok(2) = dec.array() {
            let first = dec.raw_value().ok()?;
            let second = dec.raw_value().ok()?;
            if !dec.is_empty() {
                return None;
            }

            // Common serialised-header form: [point, header-bytes]
            if let Ok(point) = Point::from_cbor_bytes(first) {
                return Some(point);
            }

            // Some serialised forms store the header in CBOR-in-CBOR.
            let mut second_dec = Decoder::new(second);
            if let Ok(unwrapped) = second_dec.wrapped() {
                if second_dec.is_empty() {
                    if let Some(point) = decode_header_point_bytes(unwrapped) {
                        return Some(point);
                    }
                }
            }

            // Fallback: second field might already be a direct header payload.
            if let Some(point) = decode_header_point_bytes(second) {
                return Some(point);
            }

            // Byron serialised-header envelope observed in preprod:
            // [[withOriginSlot], tag24(rawHeaderBytes)]
            let raw_header_bytes = decode_cbor_in_cbor_bytes(second)?;
            if let Some(point) = decode_point_from_byron_raw_header(&raw_header_bytes) {
                return Some(point);
            }

            // Last-resort fallback for envelope variants where only slot
            // is recoverable from the outer with-origin field.  EBB and
            // main Byron headers are handled by the explicit byron-raw
            // decoder above (which selects the correct hash prefix
            // based on consensus_data length).  This path applies only
            // to opaque envelope variants whose inner header doesn't
            // match the standard Byron 5-tuple shape, in which case
            // assuming a main-style hash is the safe default.
            let slot = decode_slot_from_with_origin(first)?;
            Some(Point::BlockPoint(
                SlotNo(slot),
                byron_main_header_hash(&raw_header_bytes),
            ))
        } else {
            None
        }
    }

    if let Some(point) = decode_header_point_bytes(raw_header) {
        return Some(point);
    }

    // ChainSync roll-forward headers may arrive in a 2-element envelope. In
    // practice we see multiple layouts depending on the negotiated header
    // flavor (typed header, serialised header, multi-era envelope), so try a
    // small set of upstream-compatible heuristics.
    let mut dec = Decoder::new(raw_header);
    if let Ok(2) = dec.array() {
        if let (Ok(first), Ok(second)) = (dec.raw_value(), dec.raw_value()) {
            if dec.is_empty() {
                // Layout A: [eraTag, headerPayload]
                let mut first_dec = Decoder::new(first);
                if first_dec.unsigned().is_ok() && first_dec.is_empty() {
                    if let Some(point) = decode_point_from_serialised_header(second) {
                        return Some(point);
                    }
                    if let Some(point) = decode_header_point_bytes(second) {
                        return Some(point);
                    }
                    // NTN ChainSync wraps the actual header body as
                    // tag(24, bytes(<header-cbor>)) (CBOR-in-CBOR). Unwrap
                    // and retry Shelley/Praos point extraction. Without
                    // this, post-Byron headers fail to decode and the
                    // BlockFetch upper falls back to the chain tip, which
                    // upstream rejects as an unfetchable range.
                    if let Some(unwrapped) = decode_cbor_in_cbor_bytes(second) {
                        if let Some(point) = decode_header_point_bytes(&unwrapped) {
                            return Some(point);
                        }
                        if let Some(point) = decode_point_from_byron_raw_header(&unwrapped) {
                            return Some(point);
                        }
                    }
                }

                // Layout B: [point, serialised-header-bytes]
                if let Some(point) = decode_point_bytes(first) {
                    return Some(point);
                }

                // Layout C: [serialised-header-bytes, point]
                if let Some(point) = decode_point_bytes(second) {
                    return Some(point);
                }

                // Layout D: [x, headerPayload] or [headerPayload, x]
                if let Some(point) = decode_point_from_serialised_header(first) {
                    return Some(point);
                }
                if let Some(point) = decode_point_from_serialised_header(second) {
                    return Some(point);
                }
                if let Some(point) = decode_header_point_bytes(first) {
                    return Some(point);
                }
                if let Some(point) = decode_header_point_bytes(second) {
                    return Some(point);
                }
            }
        }
    }

    None
}

pub(super) fn point_bytes_from_raw_header_or_tip(raw_header: &[u8], tip: Vec<u8>) -> Vec<u8> {
    point_from_raw_header(raw_header)
        .map(|point| point.to_cbor_bytes())
        .unwrap_or(tip)
}
