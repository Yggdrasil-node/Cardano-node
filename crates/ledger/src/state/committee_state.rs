//! Constitutional-committee state — `CommitteeAuthorization`,
//! `CommitteeMemberState`, and `CommitteeState`.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Conway.Governance.Committee`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs)
//! plus the `csCommitteeCreds` authorization map from
//! [`Cardano.Ledger.Conway.Governance::CommitteeState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs).
//!
//! Yggdrasil's `CommitteeState` combines the upstream `committeeMembers`
//! (`Map Credential EpochNo` term-tracking) and `csCommitteeCreds`
//! (authorization-tracking) into a single map keyed by cold credential.
//! `expires_at` carries the term epoch (matches upstream's
//! `committeeMembers` value); `authorization` carries the hot-key /
//! resignation state (matches `csCommitteeCreds`).
//!
//! Extracted from `state.rs` in R269 eleventh slice as part of the strict
//! 1:1 filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269k-state-committee-state-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Combines two upstream maps from
//! `Cardano.Ledger.Conway.Governance`: `committeeMembers` (cold-
//! credential -> term epoch) and `csCommitteeCreds` (cold -> hot
//! authorization / resignation). Yggdrasil unifies them under one
//! `CommitteeState` map keyed by cold credential; upstream keeps them
//! as parallel maps inside the `Conway.Governance` struct.

use super::{decode_optional_anchor, encode_optional_anchor};
use crate::types::{Anchor, EpochNo, StakeCredential};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Committee-member authorization state visible from the ledger.
///
/// This mirrors the Conway cert-state split where a known cold credential may
/// have no hot key, an authorized hot key, or a recorded resignation anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitteeAuthorization {
    /// The member has an authorized hot credential.
    CommitteeHotCredential(StakeCredential),
    /// The member has resigned, optionally carrying an anchor.
    CommitteeMemberResigned(Option<Anchor>),
}

impl CborEncode for CommitteeAuthorization {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::CommitteeHotCredential(credential) => {
                enc.array(2).unsigned(0);
                credential.encode_cbor(enc);
            }
            Self::CommitteeMemberResigned(anchor) => {
                enc.array(2).unsigned(1);
                encode_optional_anchor(anchor.as_ref(), enc);
            }
        }
    }
}

impl CborDecode for CommitteeAuthorization {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        match dec.unsigned()? {
            0 => Ok(Self::CommitteeHotCredential(StakeCredential::decode_cbor(
                dec,
            )?)),
            1 => Ok(Self::CommitteeMemberResigned(decode_optional_anchor(dec)?)),
            tag => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

/// State for a known constitutional-committee cold credential.
///
/// Upstream reference: `Cardano.Ledger.Conway.Governance.Committee`
/// — members are stored as `Map Credential EpochNo` where the epoch
/// is the term expiry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitteeMemberState {
    pub(super) authorization: Option<CommitteeAuthorization>,
    /// The epoch at which this member's term expires (inclusive).
    /// Upstream: the per-member `EpochNo` value in `committeeMembers`.
    pub(super) expires_at: Option<u64>,
}

impl CborEncode for CommitteeMemberState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // New format: 3-element array [version=2, authorization_or_null, expires_at_or_null].
        enc.array(3).unsigned(2);
        match self.authorization.as_ref() {
            Some(authorization) => authorization.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
        match self.expires_at {
            Some(epoch) => {
                enc.unsigned(epoch);
            }
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for CommitteeMemberState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        if major == 7 {
            // Legacy format: bare null → no authorization, no term.
            dec.null()?;
            return Ok(Self {
                authorization: None,
                expires_at: None,
            });
        }
        // Must be an array.
        let len = dec.array()?;
        match len {
            2 => {
                // Legacy format: CommitteeAuthorization [tag, data].
                let tag = dec.unsigned()?;
                let auth = match tag {
                    0 => CommitteeAuthorization::CommitteeHotCredential(
                        StakeCredential::decode_cbor(dec)?,
                    ),
                    1 => CommitteeAuthorization::CommitteeMemberResigned(decode_optional_anchor(
                        dec,
                    )?),
                    _ => return Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
                };
                Ok(Self {
                    authorization: Some(auth),
                    expires_at: None,
                })
            }
            3 => {
                // New format: [version=2, authorization_or_null, expires_at_or_null].
                let _version = dec.unsigned()?;
                let authorization = if dec.peek_major()? == 7 {
                    dec.null()?;
                    None
                } else {
                    Some(CommitteeAuthorization::decode_cbor(dec)?)
                };
                let expires_at = if dec.peek_major()? == 7 {
                    dec.null()?;
                    None
                } else {
                    Some(dec.unsigned()?)
                };
                Ok(Self {
                    authorization,
                    expires_at,
                })
            }
            _ => Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            }),
        }
    }
}

impl CommitteeMemberState {
    /// Creates member state with no authorized hot credential.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates member state with a term expiry epoch.
    pub fn with_term(expires_at: u64) -> Self {
        Self {
            authorization: None,
            expires_at: Some(expires_at),
        }
    }

    /// Returns the epoch at which this member's term expires, if known.
    pub fn expires_at(&self) -> Option<u64> {
        self.expires_at
    }

    /// Returns `true` when the member's term has expired at the given epoch.
    ///
    /// Upstream: `currentEpoch <= expirationEpoch` means active.
    /// So expired means `current_epoch > expires_at`.
    pub fn is_expired(&self, current_epoch: EpochNo) -> bool {
        self.expires_at.is_some_and(|term| current_epoch.0 > term)
    }

    /// Returns the member authorization state, if any.
    pub fn authorization(&self) -> Option<&CommitteeAuthorization> {
        self.authorization.as_ref()
    }

    /// Returns the authorized hot credential, if present.
    pub fn hot_credential(&self) -> Option<StakeCredential> {
        match self.authorization.as_ref() {
            Some(CommitteeAuthorization::CommitteeHotCredential(credential)) => Some(*credential),
            _ => None,
        }
    }

    /// Returns the resignation anchor, if the member has resigned.
    pub fn resignation_anchor(&self) -> Option<&Anchor> {
        match self.authorization.as_ref() {
            Some(CommitteeAuthorization::CommitteeMemberResigned(anchor)) => anchor.as_ref(),
            _ => None,
        }
    }

    /// Returns true when the member has a recorded resignation.
    pub fn is_resigned(&self) -> bool {
        matches!(
            self.authorization,
            Some(CommitteeAuthorization::CommitteeMemberResigned(_))
        )
    }

    /// Returns true when this credential is an enacted committee member.
    ///
    /// Upstream: `committeeMembers` stores `Map Credential EpochNo`.
    /// A credential is an enacted member if and only if it has a term
    /// epoch (set by `register_with_term` during `UpdateCommittee`
    /// enactment).  Credentials that only have authorization/resignation
    /// state but no term (e.g., auto-registered via `isPotentialFutureMember`
    /// or membership-cleared via `NoConfidence`) are NOT enacted members.
    pub fn is_enacted_member(&self) -> bool {
        self.expires_at.is_some()
    }

    pub(crate) fn set_authorization(&mut self, authorization: Option<CommitteeAuthorization>) {
        self.authorization = authorization;
    }
}

/// Known constitutional-committee members visible from the ledger.
///
/// Membership itself is governed elsewhere in Conway state. This narrow local
/// container tracks known cold credentials plus their hot-key authorization or
/// resignation status so committee certificates can be applied atomically.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitteeState {
    pub(super) entries: BTreeMap<StakeCredential, CommitteeMemberState>,
}

impl CborEncode for CommitteeState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (credential, state) in &self.entries {
            enc.array(2);
            credential.encode_cbor(enc);
            state.encode_cbor(enc);
        }
    }
}

impl CborDecode for CommitteeState {
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

            let credential = StakeCredential::decode_cbor(dec)?;
            let state = CommitteeMemberState::decode_cbor(dec)?;
            entries.insert(credential, state);
        }
        Ok(Self { entries })
    }
}

impl CommitteeState {
    /// Creates an empty committee-state container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the state for `credential`, if present.
    pub fn get(&self, credential: &StakeCredential) -> Option<&CommitteeMemberState> {
        self.entries.get(credential)
    }

    /// Returns mutable state for `credential`, if present.
    pub fn get_mut(&mut self, credential: &StakeCredential) -> Option<&mut CommitteeMemberState> {
        self.entries.get_mut(credential)
    }

    /// Returns true when `credential` is a known committee member.
    pub fn is_member(&self, credential: &StakeCredential) -> bool {
        self.entries.contains_key(credential)
    }

    /// Iterates over known committee members in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&StakeCredential, &CommitteeMemberState)> {
        self.entries.iter()
    }

    /// Inserts a known committee member with no authorized hot credential.
    pub fn register(&mut self, credential: StakeCredential) -> bool {
        self.entries
            .insert(credential, CommitteeMemberState::new())
            .is_none()
    }

    /// Sets the term expiry epoch for a committee member, preserving any
    /// existing authorization/resignation state.
    ///
    /// Upstream: `committeeMembers` stores `Map Credential EpochNo` which
    /// is separate from `csCommitteeCreds` (authorization state).  When
    /// `UpdateCommittee` is enacted, only `committeeMembers` is modified —
    /// `csCommitteeCreds` is untouched.  In our combined model we preserve
    /// the existing authorization when the entry already exists.
    pub fn register_with_term(&mut self, credential: StakeCredential, expires_at: u64) -> bool {
        use std::collections::btree_map::Entry;
        match self.entries.entry(credential) {
            Entry::Occupied(mut entry) => {
                // Preserve authorization/resignation — only update term.
                entry.get_mut().expires_at = Some(expires_at);
                false
            }
            Entry::Vacant(entry) => {
                entry.insert(CommitteeMemberState::with_term(expires_at));
                true
            }
        }
    }

    /// Removes a known committee member entirely (entry + authorization).
    pub fn unregister(&mut self, credential: &StakeCredential) -> Option<CommitteeMemberState> {
        self.entries.remove(credential)
    }

    /// Clears enacted membership for a single credential by setting
    /// `expires_at = None`, while preserving its authorization/resignation
    /// state.
    ///
    /// Upstream: removing from `committeeMembers` does not touch
    /// `csCommitteeCreds`.
    pub fn clear_membership(&mut self, credential: &StakeCredential) {
        if let Some(member) = self.entries.get_mut(credential) {
            member.expires_at = None;
        }
    }

    /// Clears enacted membership for all credentials by setting every
    /// entry's `expires_at = None`, preserving authorization/resignation
    /// state.
    ///
    /// Upstream: `NoConfidence` sets `ensCommittee = SNothing` which removes
    /// all `committeeMembers` but leaves `csCommitteeCreds` untouched.
    pub fn clear_all_membership(&mut self) {
        for member in self.entries.values_mut() {
            member.expires_at = None;
        }
    }

    /// Removes all entries whose credential is not a current committee
    /// member (i.e., `expires_at` is `None`).
    ///
    /// This implements upstream `updateCommitteeState` from
    /// `Cardano.Ledger.Conway.Rules.Epoch`:
    ///
    /// ```haskell
    /// updateCommitteeState committee (CommitteeState creds) =
    ///   CommitteeState $ Map.intersection creds members
    ///   where members = foldMap' committeeMembers committee
    /// ```
    ///
    /// Must be called at each epoch boundary after governance enactment
    /// so that hot-key authorizations for removed committee members are
    /// cleaned up.  Without this, re-elected members would retain their
    /// old authorization instead of having to re-register.
    pub fn prune_non_members(&mut self) {
        self.entries.retain(|_, m| m.expires_at.is_some());
    }

    /// Returns the number of known committee members.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no known committee members.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
