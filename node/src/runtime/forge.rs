//! Forge / block-producer helpers — slot-driven body assembly,
//! self-validation, and KES expiry surveillance.
//!
//! Mirrors upstream `Cardano.Node.Forge` block-producer helpers:
//!
//! - `tip_context_from_chain_db` — derive `(Option<SlotNo>, Option<BlockNo>,
//!   Option<HeaderHash>)` from the current `ChainDb` tip for forge-context
//!   construction.
//! - `mempool_entries_for_forging` — extract the fee-ordered mempool slice
//!   for body assembly.
//! - `extract_inner_block_bytes` / `self_validate_forged_block` — re-decode
//!   the freshly-forged block and run protocol-version, body-hash,
//!   body-size, header-hash, slot, and block-number sanity checks.
//! - `KesExpiryWarning` + `kes_expiry_warning` / `kes_expiry_warning_from_periods`
//!   — operator-observability helper that fires when the operational
//!   certificate's KES window is within `KES_EXPIRY_WARNING_THRESHOLD_PERIODS`
//!   of expiry.
//!
//! Extracted from `runtime.rs` in R271p (Phase γ §R271 sixteenth slice).

use yggdrasil_consensus::kes_period_of_slot;
use yggdrasil_consensus::mempool::{MEMPOOL_ZERO_IDX, MempoolEntry, SharedMempool};
use yggdrasil_ledger::{BlockNo, Decoder, HeaderHash, Point, SlotNo};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::block_producer::{BlockProducerCredentials, ForgedBlock, serialize_forged_block_cbor};
use crate::sync::{
    SyncError, decode_multi_era_block, multi_era_block_to_block, validate_block_body_size,
    validate_block_protocol_version, verify_block_body_hash,
};

pub(super) fn tip_context_from_chain_db<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
) -> (Option<SlotNo>, Option<BlockNo>, Option<HeaderHash>)
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    match chain_db.tip() {
        Point::Origin => (None, None, None),
        Point::BlockPoint(slot, hash) => {
            let block_no = chain_db
                .volatile()
                .get_block(&hash)
                .or_else(|| chain_db.immutable().get_block(&hash))
                .map(|block| block.header.block_no);
            (Some(slot), block_no, Some(hash))
        }
    }
}

pub(super) fn mempool_entries_for_forging(mempool: &SharedMempool) -> Vec<MempoolEntry> {
    let snapshot = mempool.snapshot();
    let mut entries = snapshot
        .mempool_txids_after(MEMPOOL_ZERO_IDX)
        .into_iter()
        .filter_map(|(_, idx, _)| snapshot.mempool_lookup_tx(idx).cloned())
        .collect::<Vec<_>>();
    // Keep forge-body assembly deterministic and fee-ordered (descending).
    entries.sort_by_key(|e| std::cmp::Reverse(e.fee));
    entries
}

pub(super) fn extract_inner_block_bytes(raw_envelope: &[u8]) -> Result<&[u8], SyncError> {
    let mut dec = Decoder::new(raw_envelope);
    let _ = dec.array().map_err(SyncError::LedgerDecode)?;
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_start = dec.position();
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_end = dec.position();
    dec.slice(body_start, body_end)
        .map_err(SyncError::LedgerDecode)
}

pub(super) fn self_validate_forged_block(forged: &ForgedBlock) -> Result<(), SyncError> {
    let raw_envelope = serialize_forged_block_cbor(forged);
    let decoded = decode_multi_era_block(&raw_envelope)?;

    validate_block_protocol_version(&decoded)?;
    verify_block_body_hash(&raw_envelope)?;

    let raw_inner_block = extract_inner_block_bytes(&raw_envelope)?;
    validate_block_body_size(&decoded, raw_inner_block)?;

    let decoded_block = multi_era_block_to_block(&decoded, &raw_envelope);
    if decoded_block.header.hash != forged.header_hash {
        return Err(SyncError::Recovery(
            "forged header hash mismatch".to_owned(),
        ));
    }
    if decoded_block.header.slot_no != forged.slot {
        return Err(SyncError::Recovery("forged slot mismatch".to_owned()));
    }
    if decoded_block.header.block_no != forged.block_number {
        return Err(SyncError::Recovery(
            "forged block number mismatch".to_owned(),
        ));
    }

    Ok(())
}

/// Emit a warning when the operational certificate is close to KES expiry.
///
/// Upstream reference: `praosCheckCanForge` / `KESInfo` style operator
/// observability around certificate validity windows.
const KES_EXPIRY_WARNING_THRESHOLD_PERIODS: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct KesExpiryWarning {
    pub(super) current_period: u64,
    pub(super) cert_start_period: u64,
    pub(super) cert_end_period: u64,
    pub(super) remaining_periods: u64,
    pub(super) remaining_slots: u64,
}

pub(super) fn kes_expiry_warning(
    creds: &BlockProducerCredentials,
    current_slot: SlotNo,
) -> Option<KesExpiryWarning> {
    let current_period = kes_period_of_slot(current_slot.0, creds.slots_per_kes_period).ok()?;
    kes_expiry_warning_from_periods(
        current_period,
        creds.operational_cert.kes_period,
        creds.max_kes_evolutions,
        creds.slots_per_kes_period,
    )
}

pub(super) fn kes_expiry_warning_from_periods(
    current_period: u64,
    cert_start_period: u64,
    max_kes_evolutions: u64,
    slots_per_kes_period: u64,
) -> Option<KesExpiryWarning> {
    let cert_end_period = cert_start_period.checked_add(max_kes_evolutions)?;
    let remaining_periods = cert_end_period.saturating_sub(current_period);
    if remaining_periods > KES_EXPIRY_WARNING_THRESHOLD_PERIODS {
        return None;
    }

    Some(KesExpiryWarning {
        current_period,
        cert_start_period,
        cert_end_period,
        remaining_periods,
        remaining_slots: remaining_periods.saturating_mul(slots_per_kes_period),
    })
}
