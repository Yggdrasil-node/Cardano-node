//! DRep registry — `RegisteredDrep` (per-DRep deposit + anchor + activity)
//! and the `DrepState` map container.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Conway.Governance::DRepState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs)
//! and the `vsDReps` map from `VState` (DRep voting state). The `last_active_epoch`
//! field tracks the upstream DRep `drepExpiry` activity rule used by the Conway
//! EPOCH transition to identify inactive DReps.
//!
//! Extracted from `state.rs` in R269 ninth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269i-state-drep-state-extraction.md`.

use super::{decode_optional_anchor, encode_optional_anchor};
use crate::types::{Anchor, DRep, EpochNo};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Registered DRep state visible from the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredDrep {
    pub(super) anchor: Option<Anchor>,
    pub(super) deposit: u64,
    /// The most recent epoch in which this DRep was considered active
    /// (registration, vote cast, or update).  `None` for legacy entries
    /// that predate activity tracking.
    pub(super) last_active_epoch: Option<EpochNo>,
}

impl CborEncode for RegisteredDrep {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        encode_optional_anchor(self.anchor.as_ref(), enc);
        enc.unsigned(self.deposit);
        match self.last_active_epoch {
            Some(e) => enc.unsigned(e.0),
            None => enc.null(),
        };
    }
}

impl CborDecode for RegisteredDrep {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let anchor = decode_optional_anchor(dec)?;
        let deposit = dec.unsigned()?;
        let last_active_epoch = if len >= 3 {
            if dec.peek_is_null() {
                dec.null()?;
                None
            } else {
                Some(EpochNo(dec.unsigned()?))
            }
        } else {
            None
        };

        Ok(Self {
            anchor,
            deposit,
            last_active_epoch,
        })
    }
}

impl RegisteredDrep {
    /// Creates registered DRep state.
    pub fn new(deposit: u64, anchor: Option<Anchor>) -> Self {
        Self {
            anchor,
            deposit,
            last_active_epoch: None,
        }
    }

    /// Creates registered DRep state with an initial activity epoch.
    pub fn new_active(deposit: u64, anchor: Option<Anchor>, epoch: EpochNo) -> Self {
        Self {
            anchor,
            deposit,
            last_active_epoch: Some(epoch),
        }
    }

    /// Returns the current DRep anchor, if any.
    pub fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    /// Returns the current DRep deposit value.
    pub fn deposit(&self) -> u64 {
        self.deposit
    }

    /// Returns the last epoch in which this DRep was active.
    pub fn last_active_epoch(&self) -> Option<EpochNo> {
        self.last_active_epoch
    }

    /// Records that this DRep was active in `epoch`.
    pub fn touch_activity(&mut self, epoch: EpochNo) {
        self.last_active_epoch = Some(epoch);
    }

    /// Replaces the current DRep anchor.
    pub fn set_anchor(&mut self, anchor: Option<Anchor>) {
        self.anchor = anchor;
    }
}

/// DRep registry visible from the ledger.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DrepState {
    pub(super) entries: BTreeMap<DRep, RegisteredDrep>,
}

impl CborEncode for DrepState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (drep, state) in &self.entries {
            enc.array(2);
            drep.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for DrepState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut entries = BTreeMap::new();
        for _ in 0..len {
            let pair_len = dec.array()?;
            if pair_len != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: pair_len as usize,
                });
            }

            let drep = DRep::decode_cbor(dec)?;
            let state = RegisteredDrep::decode_cbor(dec)?;
            entries.insert(drep, state);
        }
        Ok(Self { entries })
    }
}

impl DrepState {
    /// Creates an empty DRep registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the registered state for `drep`, if present.
    pub fn get(&self, drep: &DRep) -> Option<&RegisteredDrep> {
        self.entries.get(drep)
    }

    /// Returns mutable registered state for `drep`, if present.
    pub fn get_mut(&mut self, drep: &DRep) -> Option<&mut RegisteredDrep> {
        self.entries.get_mut(drep)
    }

    /// Returns true when `drep` is registered.
    pub fn is_registered(&self, drep: &DRep) -> bool {
        self.entries.contains_key(drep)
    }

    /// Iterates over registered DReps in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&DRep, &RegisteredDrep)> {
        self.entries.iter()
    }

    /// Returns a mutable iterator over registered DRep entries.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut RegisteredDrep> {
        self.entries.values_mut()
    }

    /// Registers a DRep.
    ///
    /// Returns `true` when the DRep was freshly registered.
    /// Returns `false` (already registered) **without** overwriting the
    /// existing `RegisteredDrep` entry — upstream never destroys the
    /// existing deposit / anchor / activity state on duplicate registration.
    pub fn register(&mut self, drep: DRep, state: RegisteredDrep) -> bool {
        if self.entries.contains_key(&drep) {
            return false;
        }
        self.entries.insert(drep, state);
        true
    }

    /// Unregisters a DRep.
    pub fn unregister(&mut self, drep: &DRep) -> Option<RegisteredDrep> {
        self.entries.remove(drep)
    }

    /// Returns the number of registered DReps.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no registered DReps.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the set of DReps that are inactive according to the
    /// upstream Conway `drepExpiry` rule.
    ///
    /// A DRep is inactive if its `last_active_epoch + drep_activity < epoch`.
    /// DReps without a recorded `last_active_epoch` (legacy entries) are
    /// treated as active to avoid false expiry.
    ///
    /// Upstream reference: `Cardano.Ledger.Conway.Rules.Epoch` — the
    /// `drepExpiry` function used when computing the active voting stake.
    pub fn inactive_dreps(&self, epoch: EpochNo, drep_activity: u64) -> Vec<DRep> {
        self.entries
            .iter()
            .filter(|(_, state)| {
                state
                    .last_active_epoch
                    .is_some_and(|e| e.0.saturating_add(drep_activity) < epoch.0)
            })
            .map(|(drep, _)| *drep)
            .collect()
    }
}
