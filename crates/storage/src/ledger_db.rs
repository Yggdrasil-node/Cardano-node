//! LedgerDB — restorable ledger snapshots with file-backed persistence.
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Consensus/Storage/LedgerDB.hs.
//! Filename matches upstream basename (or flattens upstream
//! directory); the module is the canonical 1:1 mirror surface
//! for the Rust port of upstream's `Ouroboros/Consensus/Storage/LedgerDB.hs` module.

use yggdrasil_ledger::SlotNo;

use crate::error::StorageError;

// ---------------------------------------------------------------------------
// LedgerSnapshotVersion — R446 format-version scaffolding
// ---------------------------------------------------------------------------

/// Format-version tag for a serialized ledger snapshot payload.
///
/// Yggdrasil owns its snapshot encoding end-to-end (no external Hackage
/// source consultation needed). When the snapshot format evolves (e.g.
/// era extensions, governance-state additions), the version tag lets a
/// future snapshot-converter round migrate older snapshots without
/// operator intervention.
///
/// Wire layout for versioned snapshots (V2+):
/// `[b"YgLS" (4-byte magic), version (u32 big-endian), payload bytes]`.
///
/// V1 snapshots (current) have no header — they're plain CBOR blobs.
/// Readers detect V1 by absence of the magic prefix; V2+ readers
/// inspect the magic + version bytes before deserializing.
///
/// R446 ships only the version-tag scaffolding (detect-version helpers +
/// trait methods). Actual V1→V2 migration logic lands when the
/// snapshot format actually evolves; until then, every snapshot in the
/// wild is V1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct LedgerSnapshotVersion(pub u32);

impl LedgerSnapshotVersion {
    /// The 4-byte magic prefix used by V2+ snapshots. ASCII `"YgLS"`
    /// (Yggdrasil Ledger Snapshot). Chosen to be human-readable in
    /// hex-dumps + unique enough to not collide with CBOR initial
    /// bytes (which start with a major-type tag in the top 3 bits;
    /// 'Y' = 0x59 has major type 2 which collides with byte-strings,
    /// but the full 4-byte sequence is unambiguous as a header).
    pub const MAGIC: [u8; 4] = *b"YgLS";

    /// V1 — the current Yggdrasil ledger snapshot format. Plain CBOR
    /// blob, no header.
    pub const V1: Self = Self(1);

    /// Alias for the most recent version. Currently V1; future rounds
    /// will bump this when V2 ships.
    pub const LATEST: Self = Self::V1;

    /// Construct from a raw `u32`. Used by [`detect_version`] when the
    /// wire bytes carry a future-version tag this build doesn't yet
    /// recognize — the unknown tag is preserved verbatim so error
    /// messages can surface "snapshot is version 7, this build
    /// supports up to LATEST.0".
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// Returns true when this version requires the V2+ magic-byte
    /// header on the wire. R446: only V1 is sub-magic; everything
    /// >= V1.0+1 carries the header.
    pub const fn has_header(self) -> bool {
        self.0 > Self::V1.0
    }
}

/// Detect the format version of a serialized snapshot payload.
///
/// Inspects the first 8 bytes for the [`LedgerSnapshotVersion::MAGIC`]
/// prefix. If present, parses the following 4 bytes as a big-endian
/// `u32` version number. If absent, returns `V1` (legacy / current
/// format).
///
/// This function is `&'static`-pure — no allocations, no I/O. Suitable
/// for header-detection in a hot loop (e.g. snapshot-converter scanning
/// a directory of pre-R446 snapshots).
pub fn detect_version(data: &[u8]) -> LedgerSnapshotVersion {
    if data.len() >= 8 && data[..4] == LedgerSnapshotVersion::MAGIC {
        let version_bytes: [u8; 4] = data[4..8]
            .try_into()
            .expect("slice of length 4 is statically guaranteed");
        LedgerSnapshotVersion::new(u32::from_be_bytes(version_bytes))
    } else {
        LedgerSnapshotVersion::V1
    }
}

/// Persistent store for ledger state snapshots.
///
/// Snapshots allow the node to resume from a recent ledger state without
/// replaying the entire chain. Each snapshot is tagged with the slot at
/// which it was taken.
///
/// Reference: snapshot handling in `Ouroboros.Consensus.Storage.LedgerDB`.
pub trait LedgerStore {
    /// Persists a serialized ledger snapshot taken at the given slot.
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError>;

    /// Returns the most recently stored snapshot (slot + payload), or `None`
    /// if no snapshot has been taken.
    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])>;

    /// Returns the most recent snapshot at or before `slot`.
    fn latest_snapshot_before_or_at(&self, slot: SlotNo) -> Option<(SlotNo, &[u8])>;

    /// Deletes snapshots newer than `slot`.
    ///
    /// Passing `None` clears all snapshots.
    fn truncate_after(&mut self, slot: Option<SlotNo>) -> Result<(), StorageError>;

    /// Retains only the newest `max_snapshots` snapshots.
    /// Passing `0` clears all snapshots.
    fn retain_latest(&mut self, max_snapshots: usize) -> Result<(), StorageError>;

    /// Returns the total number of stored snapshots.
    fn count(&self) -> usize;

    /// Returns the format-version of the most recently stored snapshot,
    /// or `None` if no snapshot has been taken. R446 scaffolding —
    /// used by a future snapshot-converter round to decide whether a
    /// migration pass is required before opening a ledger DB for the
    /// node's runtime.
    ///
    /// Default impl delegates to [`detect_version`] over the latest
    /// snapshot's bytes. Implementations that already carry the
    /// version in a cheaper-to-read sidecar can override.
    fn latest_snapshot_version(&self) -> Option<LedgerSnapshotVersion> {
        self.latest_snapshot().map(|(_, data)| detect_version(data))
    }
}

/// In-memory ledger snapshot store for tests and interface stabilization.
#[derive(Clone, Debug, Default)]
pub struct InMemoryLedgerStore {
    snapshots: Vec<(SlotNo, Vec<u8>)>,
}

impl LedgerStore for InMemoryLedgerStore {
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError> {
        if let Some((_, existing)) = self
            .snapshots
            .iter_mut()
            .find(|(snapshot_slot, _)| *snapshot_slot == slot)
        {
            *existing = data;
        } else {
            self.snapshots.push((slot, data));
            self.snapshots
                .sort_by_key(|(snapshot_slot, _)| *snapshot_slot);
        }
        Ok(())
    }

    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])> {
        self.snapshots.last().map(|(s, d)| (*s, d.as_slice()))
    }

    fn latest_snapshot_before_or_at(&self, slot: SlotNo) -> Option<(SlotNo, &[u8])> {
        self.snapshots
            .iter()
            .rev()
            .find(|(snapshot_slot, _)| *snapshot_slot <= slot)
            .map(|(snapshot_slot, data)| (*snapshot_slot, data.as_slice()))
    }

    fn truncate_after(&mut self, slot: Option<SlotNo>) -> Result<(), StorageError> {
        match slot {
            Some(slot) => self
                .snapshots
                .retain(|(snapshot_slot, _)| *snapshot_slot <= slot),
            None => self.snapshots.clear(),
        }
        Ok(())
    }

    fn retain_latest(&mut self, max_snapshots: usize) -> Result<(), StorageError> {
        if max_snapshots == 0 {
            self.snapshots.clear();
        } else if self.snapshots.len() > max_snapshots {
            let remove_count = self.snapshots.len() - max_snapshots;
            self.snapshots.drain(..remove_count);
        }
        Ok(())
    }

    fn count(&self) -> usize {
        self.snapshots.len()
    }
}

#[cfg(test)]
mod r446_version_tests {
    use super::*;

    #[test]
    fn version_constants_are_canonical() {
        assert_eq!(LedgerSnapshotVersion::V1.0, 1);
        assert_eq!(LedgerSnapshotVersion::LATEST, LedgerSnapshotVersion::V1);
        assert_eq!(LedgerSnapshotVersion::MAGIC, *b"YgLS");
    }

    #[test]
    fn has_header_false_for_v1() {
        assert!(!LedgerSnapshotVersion::V1.has_header());
    }

    #[test]
    fn has_header_true_for_v2_plus() {
        // V2+ requires the magic-byte header. R446 doesn't define V2
        // yet, but the helper correctly classifies any version > V1.
        assert!(LedgerSnapshotVersion::new(2).has_header());
        assert!(LedgerSnapshotVersion::new(99).has_header());
    }

    #[test]
    fn detect_version_returns_v1_for_legacy_no_header_payload() {
        // CBOR-shaped payload without the magic prefix — typical of
        // existing pre-R446 snapshots on disk.
        let cbor_payload = vec![0x80, 0xa0, 0x18, 0x2a];
        assert_eq!(detect_version(&cbor_payload), LedgerSnapshotVersion::V1);
    }

    #[test]
    fn detect_version_returns_v1_for_empty_payload() {
        // Defensive: zero-byte payloads are degenerate but shouldn't
        // panic the detector. Returns V1 as a safe legacy fallback.
        assert_eq!(detect_version(&[]), LedgerSnapshotVersion::V1);
    }

    #[test]
    fn detect_version_returns_v1_for_short_legacy_payload() {
        // Payloads shorter than 8 bytes can't carry a magic+version
        // header — treat as V1 legacy.
        assert_eq!(detect_version(&[0x80]), LedgerSnapshotVersion::V1);
        assert_eq!(detect_version(&[0xa0, 0xa0]), LedgerSnapshotVersion::V1);
    }

    #[test]
    fn detect_version_parses_v2_with_header() {
        // Construct a V2-shaped payload: magic + big-endian u32(2).
        let mut payload = LedgerSnapshotVersion::MAGIC.to_vec();
        payload.extend_from_slice(&2u32.to_be_bytes());
        payload.extend_from_slice(b"future-cbor-payload");
        assert_eq!(detect_version(&payload), LedgerSnapshotVersion::new(2));
    }

    #[test]
    fn detect_version_preserves_unknown_future_version() {
        // A version this build doesn't recognize yet (e.g. operator
        // is running an older yggdrasil against a future-format
        // snapshot) should preserve the tag for diagnostic surface.
        let mut payload = LedgerSnapshotVersion::MAGIC.to_vec();
        payload.extend_from_slice(&99u32.to_be_bytes());
        payload.extend_from_slice(b"future-payload");
        assert_eq!(detect_version(&payload), LedgerSnapshotVersion::new(99));
    }

    #[test]
    fn detect_version_returns_v1_for_almost_magic_prefix() {
        // A payload that starts with bytes similar to but not exactly
        // the magic must NOT be misclassified.
        let payload = b"YgLT\x00\x00\x00\x02more-payload".to_vec();
        assert_eq!(detect_version(&payload), LedgerSnapshotVersion::V1);
    }

    #[test]
    fn latest_snapshot_version_returns_none_for_empty_store() {
        let store = InMemoryLedgerStore::default();
        assert_eq!(store.latest_snapshot_version(), None);
    }

    #[test]
    fn latest_snapshot_version_detects_v1_legacy_payload() {
        let mut store = InMemoryLedgerStore::default();
        // Plain CBOR payload — no header — should detect as V1.
        store
            .save_snapshot(SlotNo(100), vec![0x80, 0xa0, 0x18, 0x2a])
            .expect("save");
        assert_eq!(
            store.latest_snapshot_version(),
            Some(LedgerSnapshotVersion::V1)
        );
    }

    #[test]
    fn latest_snapshot_version_detects_versioned_payload() {
        let mut store = InMemoryLedgerStore::default();
        // Construct a V2-shaped payload and persist it.
        let mut payload = LedgerSnapshotVersion::MAGIC.to_vec();
        payload.extend_from_slice(&2u32.to_be_bytes());
        payload.extend_from_slice(b"future-state-cbor");
        store.save_snapshot(SlotNo(200), payload).expect("save");
        assert_eq!(
            store.latest_snapshot_version(),
            Some(LedgerSnapshotVersion::new(2))
        );
    }
}
