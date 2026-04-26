//! Conway-era transaction types with on-chain governance.
//!
//! Conway introduces decentralized governance (CIP-1694), adding:
//! - `Voter`: committee member, DRep, or stake pool operator.
//! - `Vote`: Yes / No / Abstain.
//! - `GovActionId`: reference to a governance action (tx_id + index).
//! - `VotingProcedure`: a vote cast on a governance action, with
//!   optional anchor metadata.
//! - `Anchor`: URL + data hash for off-chain metadata.
//! - `GovAction`: typed governance action (7 variants matching CDDL tags 0–6).
//! - `Constitution`: anchor + optional guardrails script hash.
//! - `ProposalProcedure`: governance proposal with deposit, return
//!   address, action body, and anchor.
//! - `ConwayTxBody`: extends Babbage with keys 19 (`voting_procedures`),
//!   20 (`proposal_procedures`), 21 (`current_treasury_value`),
//!   22 (`treasury_donation`).
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway>

use std::collections::{BTreeMap, HashMap};

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::babbage::BabbageTxOut;
use crate::eras::mary::{MintAsset, decode_mint_asset, encode_mint_asset};
use crate::eras::shelley::{PraosHeader, ShelleyTxIn, ShelleyWitnessSet};
use crate::error::LedgerError;
use crate::protocol_params::ProtocolParameterUpdate;
use crate::types::{Anchor, DCert, HeaderHash, RewardAccount, StakeCredential, UnitInterval};

pub const CONWAY_NAME: &str = "Conway";

// ---------------------------------------------------------------------------
// Vote
// ---------------------------------------------------------------------------

/// Vote cast by a voter on a governance action.
///
/// CDDL: `vote = 0 .. 2`
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.Vote`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Vote {
    /// Vote against the proposal (0).
    No = 0,
    /// Vote in favor of the proposal (1).
    Yes = 1,
    /// Abstain from voting (2).
    Abstain = 2,
}

impl CborEncode for Vote {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.unsigned(*self as u64);
    }
}

impl CborDecode for Vote {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let v = dec.unsigned()?;
        match v {
            0 => Ok(Self::No),
            1 => Ok(Self::Yes),
            2 => Ok(Self::Abstain),
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 2,
                actual: v as u8,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Voter
// ---------------------------------------------------------------------------

/// Entity casting a governance vote.
///
/// CDDL:
/// ```text
/// voter =
///   [ 0, addr_keyhash       ; constitutional committee member key hash
///   // 1, scripthash         ; constitutional committee member script
///   // 2, addr_keyhash       ; DRep key hash
///   // 3, scripthash         ; DRep script
///   // 4, addr_keyhash       ; stake pool operator
///   ]
/// ```
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.Voter`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Voter {
    /// Constitutional committee member identified by key hash (tag 0).
    CommitteeKeyHash([u8; 28]),
    /// Constitutional committee member identified by script hash (tag 1).
    CommitteeScript([u8; 28]),
    /// DRep identified by key hash (tag 2).
    DRepKeyHash([u8; 28]),
    /// DRep identified by script hash (tag 3).
    DRepScript([u8; 28]),
    /// Stake pool operator identified by pool key hash (tag 4).
    StakePool([u8; 28]),
}

impl CborEncode for Voter {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        match self {
            Self::CommitteeKeyHash(h) => {
                enc.unsigned(0).bytes(h);
            }
            Self::CommitteeScript(h) => {
                enc.unsigned(1).bytes(h);
            }
            Self::DRepKeyHash(h) => {
                enc.unsigned(2).bytes(h);
            }
            Self::DRepScript(h) => {
                enc.unsigned(3).bytes(h);
            }
            Self::StakePool(h) => {
                enc.unsigned(4).bytes(h);
            }
        }
    }
}

impl CborDecode for Voter {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let tag = dec.unsigned()?;
        let raw = dec.bytes()?;
        let hash: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 28,
            actual: raw.len(),
        })?;
        match tag {
            0 => Ok(Self::CommitteeKeyHash(hash)),
            1 => Ok(Self::CommitteeScript(hash)),
            2 => Ok(Self::DRepKeyHash(hash)),
            3 => Ok(Self::DRepScript(hash)),
            4 => Ok(Self::StakePool(hash)),
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 4,
                actual: tag as u8,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// GovActionId
// ---------------------------------------------------------------------------

/// Reference to a governance action within a transaction.
///
/// CDDL: `gov_action_id = [transaction_id : $hash32, gov_action_index : uint16]`
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.GovActionId`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GovActionId {
    /// Transaction hash containing the governance action.
    pub transaction_id: [u8; 32],
    /// Index of the governance action within the transaction.
    pub gov_action_index: u16,
}

impl CborEncode for GovActionId {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2)
            .bytes(&self.transaction_id)
            .unsigned(u64::from(self.gov_action_index));
    }
}

impl CborDecode for GovActionId {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let raw = dec.bytes()?;
        let transaction_id: [u8; 32] =
            raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                expected: 32,
                actual: raw.len(),
            })?;
        let gov_action_index = dec.unsigned()? as u16;
        Ok(Self {
            transaction_id,
            gov_action_index,
        })
    }
}

// ---------------------------------------------------------------------------
// Anchor — CBOR encoding is in `crate::cbor`.
// ---------------------------------------------------------------------------

// The `Anchor` struct is defined in `crate::types`; CBOR impls are in `crate::cbor`.

// ---------------------------------------------------------------------------
// Constitution
// ---------------------------------------------------------------------------

/// On-chain constitution reference.
///
/// CDDL: `constitution = [anchor, guardrails_script_hash / null]`
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.Constitution`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Constitution {
    /// Anchor pointing to off-chain constitution text.
    pub anchor: Anchor,
    /// Optional guardrails script hash (28 bytes).
    pub guardrails_script_hash: Option<[u8; 28]>,
}

impl CborEncode for Constitution {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        self.anchor.encode_cbor(enc);
        match &self.guardrails_script_hash {
            Some(h) => {
                enc.bytes(h);
            }
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for Constitution {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let anchor = Anchor::decode_cbor(dec)?;
        let guardrails_script_hash = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            let raw = dec.bytes()?;
            let hash: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                expected: 28,
                actual: raw.len(),
            })?;
            Some(hash)
        };
        Ok(Self {
            anchor,
            guardrails_script_hash,
        })
    }
}

// ---------------------------------------------------------------------------
// GovAction
// ---------------------------------------------------------------------------

/// Typed governance action body.
///
/// CDDL:
/// ```text
/// gov_action =
///   [ parameter_change_action
///   // hard_fork_initiation_action
///   // treasury_withdrawals_action
///   // no_confidence
///   // update_committee
///   // new_constitution
///   // info_action
///   ]
///
/// parameter_change_action = (0, gov_action_id / null, protocol_param_update, policy_hash / null)
/// hard_fork_initiation_action = (1, gov_action_id / null, [uint, uint])
/// treasury_withdrawals_action = (2, { * reward_account => coin }, policy_hash / null)
/// no_confidence = (3, gov_action_id / null)
/// update_committee = (4, gov_action_id / null, set<committee_cold_credential>,
///                      { * committee_cold_credential => epoch }, unit_interval)
/// new_constitution = (5, gov_action_id / null, constitution)
/// info_action = 6
/// ```
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.GovAction`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovAction {
    /// Propose a protocol parameter change (tag 0).
    ParameterChange {
        /// Previous governance action ID, or `None` for the first such action.
        prev_action_id: Option<GovActionId>,
        /// Typed protocol parameter update delta.
        protocol_param_update: ProtocolParameterUpdate,
        /// Optional guardrails (policy) script hash.
        guardrails_script_hash: Option<[u8; 28]>,
    },
    /// Propose a hard fork (tag 1).
    HardForkInitiation {
        /// Previous governance action ID, or `None`.
        prev_action_id: Option<GovActionId>,
        /// Target protocol version `(major, minor)`.
        protocol_version: (u64, u64),
    },
    /// Propose treasury withdrawals (tag 2).
    TreasuryWithdrawals {
        /// Map from reward account to withdrawal amount in lovelace.
        withdrawals: BTreeMap<RewardAccount, u64>,
        /// Optional guardrails (policy) script hash.
        guardrails_script_hash: Option<[u8; 28]>,
    },
    /// Motion of no-confidence in the current committee (tag 3).
    NoConfidence {
        /// Previous governance action ID, or `None`.
        prev_action_id: Option<GovActionId>,
    },
    /// Update the constitutional committee (tag 4).
    UpdateCommittee {
        /// Previous governance action ID, or `None`.
        prev_action_id: Option<GovActionId>,
        /// Committee members to remove.
        members_to_remove: Vec<StakeCredential>,
        /// Committee members to add, mapped to their term-limit epoch.
        members_to_add: BTreeMap<StakeCredential, u64>,
        /// New quorum threshold.
        quorum: UnitInterval,
    },
    /// Propose a new constitution (tag 5).
    NewConstitution {
        /// Previous governance action ID, or `None`.
        prev_action_id: Option<GovActionId>,
        /// The new constitution.
        constitution: Constitution,
    },
    /// Informational action with no on-chain effect (tag 6).
    InfoAction,
}

/// Encode an optional `GovActionId` or CBOR null.
fn encode_optional_gov_action_id(enc: &mut Encoder, id: &Option<GovActionId>) {
    match id {
        Some(gid) => gid.encode_cbor(enc),
        None => {
            enc.null();
        }
    }
}

/// Decode an optional `GovActionId` (CBOR null → None).
fn decode_optional_gov_action_id(
    dec: &mut Decoder<'_>,
) -> Result<Option<GovActionId>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        Ok(Some(GovActionId::decode_cbor(dec)?))
    }
}

/// Encode an optional 28-byte script hash or CBOR null.
fn encode_optional_script_hash(enc: &mut Encoder, h: &Option<[u8; 28]>) {
    match h {
        Some(hash) => {
            enc.bytes(hash);
        }
        None => {
            enc.null();
        }
    }
}

/// Decode an optional 28-byte script hash (CBOR null → None).
fn decode_optional_script_hash(dec: &mut Decoder<'_>) -> Result<Option<[u8; 28]>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        let raw = dec.bytes()?;
        let hash: [u8; 28] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 28,
            actual: raw.len(),
        })?;
        Ok(Some(hash))
    }
}

impl CborEncode for GovAction {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::ParameterChange {
                prev_action_id,
                protocol_param_update,
                guardrails_script_hash,
            } => {
                enc.array(4);
                enc.unsigned(0);
                encode_optional_gov_action_id(enc, prev_action_id);
                protocol_param_update.encode_cbor(enc);
                encode_optional_script_hash(enc, guardrails_script_hash);
            }
            Self::HardForkInitiation {
                prev_action_id,
                protocol_version,
            } => {
                enc.array(3);
                enc.unsigned(1);
                encode_optional_gov_action_id(enc, prev_action_id);
                enc.array(2)
                    .unsigned(protocol_version.0)
                    .unsigned(protocol_version.1);
            }
            Self::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash,
            } => {
                enc.array(3);
                enc.unsigned(2);
                enc.map(withdrawals.len() as u64);
                for (acct, coin) in withdrawals {
                    acct.encode_cbor(enc);
                    enc.unsigned(*coin);
                }
                encode_optional_script_hash(enc, guardrails_script_hash);
            }
            Self::NoConfidence { prev_action_id } => {
                enc.array(2);
                enc.unsigned(3);
                encode_optional_gov_action_id(enc, prev_action_id);
            }
            Self::UpdateCommittee {
                prev_action_id,
                members_to_remove,
                members_to_add,
                quorum,
            } => {
                enc.array(5);
                enc.unsigned(4);
                encode_optional_gov_action_id(enc, prev_action_id);
                // set<committee_cold_credential>
                enc.array(members_to_remove.len() as u64);
                for cred in members_to_remove {
                    cred.encode_cbor(enc);
                }
                // { committee_cold_credential => epoch }
                enc.map(members_to_add.len() as u64);
                for (cred, epoch) in members_to_add {
                    cred.encode_cbor(enc);
                    enc.unsigned(*epoch);
                }
                quorum.encode_cbor(enc);
            }
            Self::NewConstitution {
                prev_action_id,
                constitution,
            } => {
                enc.array(3);
                enc.unsigned(5);
                encode_optional_gov_action_id(enc, prev_action_id);
                constitution.encode_cbor(enc);
            }
            Self::InfoAction => {
                enc.array(1);
                enc.unsigned(6);
            }
        }
    }
}

impl CborDecode for GovAction {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        match tag {
            0 => {
                // parameter_change_action = (0, gov_action_id / null, protocol_param_update, policy_hash / null)
                if len != 4 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 4,
                        actual: len as usize,
                    });
                }
                let prev_action_id = decode_optional_gov_action_id(dec)?;
                let protocol_param_update = ProtocolParameterUpdate::decode_cbor(dec)?;
                let guardrails_script_hash = decode_optional_script_hash(dec)?;
                Ok(Self::ParameterChange {
                    prev_action_id,
                    protocol_param_update,
                    guardrails_script_hash,
                })
            }
            1 => {
                // hard_fork_initiation_action = (1, gov_action_id / null, [uint, uint])
                if len != 3 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 3,
                        actual: len as usize,
                    });
                }
                let prev_action_id = decode_optional_gov_action_id(dec)?;
                let pv_len = dec.array()?;
                if pv_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: pv_len as usize,
                    });
                }
                let major = dec.unsigned()?;
                let minor = dec.unsigned()?;
                Ok(Self::HardForkInitiation {
                    prev_action_id,
                    protocol_version: (major, minor),
                })
            }
            2 => {
                // treasury_withdrawals_action = (2, { * reward_account => coin }, policy_hash / null)
                if len != 3 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 3,
                        actual: len as usize,
                    });
                }
                let map_len = dec.map()?;
                let mut withdrawals = BTreeMap::new();
                for _ in 0..map_len {
                    let acct = RewardAccount::decode_cbor(dec)?;
                    let coin = dec.unsigned()?;
                    withdrawals.insert(acct, coin);
                }
                let guardrails_script_hash = decode_optional_script_hash(dec)?;
                Ok(Self::TreasuryWithdrawals {
                    withdrawals,
                    guardrails_script_hash,
                })
            }
            3 => {
                // no_confidence = (3, gov_action_id / null)
                if len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: len as usize,
                    });
                }
                let prev_action_id = decode_optional_gov_action_id(dec)?;
                Ok(Self::NoConfidence { prev_action_id })
            }
            4 => {
                // update_committee = (4, gov_action_id / null, set<credential>, { credential => epoch }, unit_interval)
                if len != 5 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 5,
                        actual: len as usize,
                    });
                }
                let prev_action_id = decode_optional_gov_action_id(dec)?;
                // set<committee_cold_credential>
                let remove_count = dec.array()?;
                let mut members_to_remove = Vec::with_capacity(remove_count as usize);
                for _ in 0..remove_count {
                    members_to_remove.push(StakeCredential::decode_cbor(dec)?);
                }
                // { committee_cold_credential => epoch }
                let add_count = dec.map()?;
                let mut members_to_add = BTreeMap::new();
                for _ in 0..add_count {
                    let cred = StakeCredential::decode_cbor(dec)?;
                    let epoch = dec.unsigned()?;
                    members_to_add.insert(cred, epoch);
                }
                let quorum = UnitInterval::decode_cbor(dec)?;
                Ok(Self::UpdateCommittee {
                    prev_action_id,
                    members_to_remove,
                    members_to_add,
                    quorum,
                })
            }
            5 => {
                // new_constitution = (5, gov_action_id / null, constitution)
                if len != 3 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 3,
                        actual: len as usize,
                    });
                }
                let prev_action_id = decode_optional_gov_action_id(dec)?;
                let constitution = Constitution::decode_cbor(dec)?;
                Ok(Self::NewConstitution {
                    prev_action_id,
                    constitution,
                })
            }
            6 => {
                // info_action = 6
                if len != 1 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 1,
                        actual: len as usize,
                    });
                }
                Ok(Self::InfoAction)
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 6,
                actual: tag as u8,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// VotingProcedure
// ---------------------------------------------------------------------------

/// A single vote cast on a governance action, with optional anchor metadata.
///
/// CDDL: `voting_procedure = [vote, anchor / null]`
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.VotingProcedure`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VotingProcedure {
    /// The vote itself.
    pub vote: Vote,
    /// Optional anchor with off-chain rationale.
    pub anchor: Option<Anchor>,
}

impl CborEncode for VotingProcedure {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        self.vote.encode_cbor(enc);
        match &self.anchor {
            Some(a) => a.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for VotingProcedure {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let vote = Vote::decode_cbor(dec)?;
        // Major type 7 (simple/float) with value 22 = null.
        let anchor = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            Some(Anchor::decode_cbor(dec)?)
        };
        Ok(Self { vote, anchor })
    }
}

// ---------------------------------------------------------------------------
// ProposalProcedure
// ---------------------------------------------------------------------------

/// A governance proposal submitted as part of a Conway transaction.
///
/// CDDL:
/// ```text
/// proposal_procedure =
///   [ deposit : coin
///   , reward_account
///   , gov_action
///   , anchor
///   ]
/// ```
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.ProposalProcedure`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalProcedure {
    /// Deposit required for the proposal in lovelace.
    pub deposit: u64,
    /// Reward account receiving the deposit refund.
    pub reward_account: Vec<u8>,
    /// Typed governance action body.
    pub gov_action: GovAction,
    /// Off-chain metadata anchor.
    pub anchor: Anchor,
}

impl CborEncode for ProposalProcedure {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        enc.unsigned(self.deposit);
        enc.bytes(&self.reward_account);
        self.gov_action.encode_cbor(enc);
        self.anchor.encode_cbor(enc);
    }
}

impl CborDecode for ProposalProcedure {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }
        let deposit = dec.unsigned()?;
        let reward_account = dec.bytes()?.to_vec();
        let gov_action = GovAction::decode_cbor(dec)?;
        let anchor = Anchor::decode_cbor(dec)?;
        Ok(Self {
            deposit,
            reward_account,
            gov_action,
            anchor,
        })
    }
}

// ---------------------------------------------------------------------------
// VotingProcedures (nested map)
// ---------------------------------------------------------------------------

/// Full voting procedures map carried in a Conway transaction body.
///
/// CDDL: `voting_procedures = { + voter => { + gov_action_id => voting_procedure } }`
///
/// Uses `BTreeMap` for deterministic CBOR ordering.
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.VotingProcedures`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VotingProcedures {
    pub procedures: BTreeMap<Voter, BTreeMap<GovActionId, VotingProcedure>>,
}

impl CborEncode for VotingProcedures {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.map(self.procedures.len() as u64);
        for (voter, actions) in &self.procedures {
            voter.encode_cbor(enc);
            enc.map(actions.len() as u64);
            for (action_id, procedure) in actions {
                action_id.encode_cbor(enc);
                procedure.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for VotingProcedures {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let outer_len = dec.map()?;
        let mut procedures = BTreeMap::new();
        for _ in 0..outer_len {
            let voter = Voter::decode_cbor(dec)?;
            let inner_len = dec.map()?;
            let mut actions = BTreeMap::new();
            for _ in 0..inner_len {
                let action_id = GovActionId::decode_cbor(dec)?;
                let procedure = VotingProcedure::decode_cbor(dec)?;
                actions.insert(action_id, procedure);
            }
            procedures.insert(voter, actions);
        }
        Ok(Self { procedures })
    }
}

// ---------------------------------------------------------------------------
// Conway transaction body
// ---------------------------------------------------------------------------

/// Conway-era transaction body.
///
/// Extends Babbage by adding:
/// - Key 19: `voting_procedures` — votes cast on governance actions.
/// - Key 20: `proposal_procedures` — new governance proposals.
/// - Key 21: `current_treasury_value` — optional current treasury value.
/// - Key 22: `treasury_donation` — optional positive lovelace donation.
///
/// ```text
/// transaction_body =
///   { 0  : set<transaction_input>
///   , 1  : [* transaction_output]
///   , 2  : coin
///   , ? 3  : uint                        ; ttl
///   , ? 4  : [* certificate]
///   , ? 5  : withdrawals
///   , ? 7  : auxiliary_data_hash
///   , ? 8  : uint                        ; validity interval start
///   , ? 9  : mint
///   , ? 11 : script_data_hash
///   , ? 13 : set<transaction_input>      ; collateral inputs
///   , ? 14 : required_signers
///   , ? 15 : network_id
///   , ? 16 : transaction_output          ; collateral return
///   , ? 17 : coin                        ; total collateral
///   , ? 18 : set<transaction_input>      ; reference inputs
///   , ? 19 : voting_procedures           ; (NEW)
///   , ? 20 : proposal_procedures         ; (NEW)
///   , ? 21 : coin                        ; current treasury value (NEW)
///   , ? 22 : coin                        ; treasury donation     (NEW)
///   }
/// ```
///
/// Note: key 6 (update) is removed in the Conway era.
/// Certificates (4) and withdrawals (5) are now modeled.
///
/// Reference: `Cardano.Ledger.Conway.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConwayTxBody {
    // --- Inherited from Babbage (keys 0–18) ---
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<BabbageTxOut>,
    /// Transaction fee in lovelace (CDDL key 2).
    pub fee: u64,
    /// Optional TTL slot (CDDL key 3).
    pub ttl: Option<u64>,
    /// Optional certificates (CDDL key 4).
    pub certificates: Option<Vec<DCert>>,
    /// Optional withdrawals: reward-account → lovelace (CDDL key 5).
    pub withdrawals: Option<BTreeMap<RewardAccount, u64>>,
    /// Optional auxiliary data hash (CDDL key 7).
    pub auxiliary_data_hash: Option<[u8; 32]>,
    /// Optional validity interval start (CDDL key 8).
    pub validity_interval_start: Option<u64>,
    /// Optional mint field for native tokens (CDDL key 9).
    pub mint: Option<MintAsset>,
    /// Optional hash of script integrity data (CDDL key 11).
    pub script_data_hash: Option<[u8; 32]>,
    /// Optional collateral inputs (CDDL key 13).
    pub collateral: Option<Vec<ShelleyTxIn>>,
    /// Optional required signer key hashes (CDDL key 14).
    pub required_signers: Option<Vec<[u8; 28]>>,
    /// Optional network ID: 0 = testnet, 1 = mainnet (CDDL key 15).
    pub network_id: Option<u8>,
    /// Optional collateral return output (CDDL key 16).
    pub collateral_return: Option<BabbageTxOut>,
    /// Optional total collateral in lovelace (CDDL key 17).
    pub total_collateral: Option<u64>,
    /// Optional reference inputs (CDDL key 18).
    pub reference_inputs: Option<Vec<ShelleyTxIn>>,
    // --- Conway governance extensions (keys 19–22) ---
    /// Optional voting procedures (CDDL key 19).
    pub voting_procedures: Option<VotingProcedures>,
    /// Optional proposal procedures (CDDL key 20).
    pub proposal_procedures: Option<Vec<ProposalProcedure>>,
    /// Optional current treasury value in lovelace (CDDL key 21).
    pub current_treasury_value: Option<u64>,
    /// Optional treasury donation in lovelace (CDDL key 22).
    pub treasury_donation: Option<u64>,
}

impl CborEncode for ConwayTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 3; // keys 0, 1, 2
        if self.ttl.is_some() {
            field_count += 1;
        }
        if self.certificates.is_some() {
            field_count += 1;
        }
        if self.withdrawals.is_some() {
            field_count += 1;
        }
        if self.auxiliary_data_hash.is_some() {
            field_count += 1;
        }
        if self.validity_interval_start.is_some() {
            field_count += 1;
        }
        if self.mint.is_some() {
            field_count += 1;
        }
        if self.script_data_hash.is_some() {
            field_count += 1;
        }
        if self.collateral.is_some() {
            field_count += 1;
        }
        if self.required_signers.is_some() {
            field_count += 1;
        }
        if self.network_id.is_some() {
            field_count += 1;
        }
        if self.collateral_return.is_some() {
            field_count += 1;
        }
        if self.total_collateral.is_some() {
            field_count += 1;
        }
        if self.reference_inputs.is_some() {
            field_count += 1;
        }
        if self.voting_procedures.is_some() {
            field_count += 1;
        }
        if self.proposal_procedures.is_some() {
            field_count += 1;
        }
        if self.current_treasury_value.is_some() {
            field_count += 1;
        }
        if self.treasury_donation.is_some() {
            field_count += 1;
        }
        enc.map(field_count);

        // Key 0: inputs.
        enc.unsigned(0).array(self.inputs.len() as u64);
        for input in &self.inputs {
            input.encode_cbor(enc);
        }

        // Key 1: outputs.
        enc.unsigned(1).array(self.outputs.len() as u64);
        for output in &self.outputs {
            output.encode_cbor(enc);
        }

        // Key 2: fee.
        enc.unsigned(2).unsigned(self.fee);

        // Key 3: ttl.
        if let Some(ttl) = self.ttl {
            enc.unsigned(3).unsigned(ttl);
        }

        // Key 4: certificates.
        if let Some(certs) = &self.certificates {
            enc.unsigned(4).array(certs.len() as u64);
            for cert in certs {
                cert.encode_cbor(enc);
            }
        }

        // Key 5: withdrawals.
        if let Some(withdrawals) = &self.withdrawals {
            enc.unsigned(5).map(withdrawals.len() as u64);
            for (acct, coin) in withdrawals {
                acct.encode_cbor(enc);
                enc.unsigned(*coin);
            }
        }

        // Key 7: auxiliary_data_hash.
        if let Some(hash) = &self.auxiliary_data_hash {
            enc.unsigned(7).bytes(hash);
        }

        // Key 8: validity_interval_start.
        if let Some(start) = self.validity_interval_start {
            enc.unsigned(8).unsigned(start);
        }

        // Key 9: mint.
        if let Some(mint) = &self.mint {
            enc.unsigned(9);
            encode_mint_asset(enc, mint);
        }

        // Key 11: script_data_hash.
        if let Some(hash) = &self.script_data_hash {
            enc.unsigned(11).bytes(hash);
        }

        // Key 13: collateral.
        if let Some(collateral) = &self.collateral {
            enc.unsigned(13).array(collateral.len() as u64);
            for input in collateral {
                input.encode_cbor(enc);
            }
        }

        // Key 14: required_signers.
        if let Some(signers) = &self.required_signers {
            enc.unsigned(14).array(signers.len() as u64);
            for signer in signers {
                enc.bytes(signer);
            }
        }

        // Key 15: network_id.
        if let Some(nid) = self.network_id {
            enc.unsigned(15).unsigned(u64::from(nid));
        }

        // Key 16: collateral_return.
        if let Some(ret) = &self.collateral_return {
            enc.unsigned(16);
            ret.encode_cbor(enc);
        }

        // Key 17: total_collateral.
        if let Some(total) = self.total_collateral {
            enc.unsigned(17).unsigned(total);
        }

        // Key 18: reference_inputs.
        if let Some(refs) = &self.reference_inputs {
            enc.unsigned(18).array(refs.len() as u64);
            for input in refs {
                input.encode_cbor(enc);
            }
        }

        // Key 19: voting_procedures.
        if let Some(vp) = &self.voting_procedures {
            enc.unsigned(19);
            vp.encode_cbor(enc);
        }

        // Key 20: proposal_procedures.
        if let Some(pp) = &self.proposal_procedures {
            enc.unsigned(20).array(pp.len() as u64);
            for proposal in pp {
                proposal.encode_cbor(enc);
            }
        }

        // Key 21: current_treasury_value.
        if let Some(val) = self.current_treasury_value {
            enc.unsigned(21).unsigned(val);
        }

        // Key 22: treasury_donation.
        if let Some(don) = self.treasury_donation {
            enc.unsigned(22).unsigned(don);
        }
    }
}

impl CborDecode for ConwayTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<BabbageTxOut>> = None;
        let mut fee: Option<u64> = None;
        let mut ttl: Option<u64> = None;
        let mut certificates: Option<Vec<DCert>> = None;
        let mut withdrawals: Option<BTreeMap<RewardAccount, u64>> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;
        let mut validity_interval_start: Option<u64> = None;
        let mut mint: Option<MintAsset> = None;
        let mut script_data_hash: Option<[u8; 32]> = None;
        let mut collateral: Option<Vec<ShelleyTxIn>> = None;
        let mut required_signers: Option<Vec<[u8; 28]>> = None;
        let mut network_id: Option<u8> = None;
        let mut collateral_return: Option<BabbageTxOut> = None;
        let mut total_collateral: Option<u64> = None;
        let mut reference_inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut voting_procedures: Option<VotingProcedures> = None;
        let mut proposal_procedures: Option<Vec<ProposalProcedure>> = None;
        let mut current_treasury_value: Option<u64> = None;
        let mut treasury_donation: Option<u64> = None;

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array_or_set()?;
                    let mut ins = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        ins.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    inputs = Some(ins);
                }
                1 => {
                    let count = dec.array()?;
                    let mut outs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        outs.push(BabbageTxOut::decode_cbor(dec)?);
                    }
                    outputs = Some(outs);
                }
                2 => {
                    fee = Some(dec.unsigned()?);
                }
                3 => {
                    ttl = Some(dec.unsigned()?);
                }
                4 => {
                    let count = dec.array_or_set()?;
                    let mut certs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        certs.push(DCert::decode_cbor(dec)?);
                    }
                    certificates = Some(certs);
                }
                5 => {
                    let count = dec.map()?;
                    let mut wdrl = BTreeMap::new();
                    for _ in 0..count {
                        let acct = RewardAccount::decode_cbor(dec)?;
                        let coin = dec.unsigned()?;
                        wdrl.insert(acct, coin);
                    }
                    withdrawals = Some(wdrl);
                }
                7 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                            expected: 32,
                            actual: raw.len(),
                        })?;
                    auxiliary_data_hash = Some(hash);
                }
                8 => {
                    validity_interval_start = Some(dec.unsigned()?);
                }
                9 => {
                    mint = Some(decode_mint_asset(dec)?);
                }
                11 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                            expected: 32,
                            actual: raw.len(),
                        })?;
                    script_data_hash = Some(hash);
                }
                13 => {
                    let count = dec.array_or_set()?;
                    let mut cols = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        cols.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    collateral = Some(cols);
                }
                14 => {
                    let count = dec.array_or_set()?;
                    let mut sigs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let raw = dec.bytes()?;
                        let hash: [u8; 28] =
                            raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
                                expected: 28,
                                actual: raw.len(),
                            })?;
                        sigs.push(hash);
                    }
                    required_signers = Some(sigs);
                }
                15 => {
                    network_id = Some(dec.unsigned()? as u8);
                }
                16 => {
                    collateral_return = Some(BabbageTxOut::decode_cbor(dec)?);
                }
                17 => {
                    total_collateral = Some(dec.unsigned()?);
                }
                18 => {
                    let count = dec.array_or_set()?;
                    let mut refs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        refs.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    reference_inputs = Some(refs);
                }
                19 => {
                    voting_procedures = Some(VotingProcedures::decode_cbor(dec)?);
                }
                20 => {
                    let count = dec.array_or_set()?;
                    let mut props = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        props.push(ProposalProcedure::decode_cbor(dec)?);
                    }
                    proposal_procedures = Some(props);
                }
                21 => {
                    current_treasury_value = Some(dec.unsigned()?);
                }
                22 => {
                    treasury_donation = Some(dec.unsigned()?);
                }
                // Conway removes the Shelley `update` field (CDDL key 6).
                // Upstream: `UpdateNotAllowed` from
                // `Cardano.Ledger.Conway.Rules.Utxos`.
                6 => {
                    return Err(LedgerError::UpdateNotAllowedConway);
                }
                _ => {
                    dec.skip()?;
                }
            }
        }

        Ok(Self {
            inputs: inputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            outputs: outputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            fee: fee.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            ttl,
            certificates,
            withdrawals,
            auxiliary_data_hash,
            validity_interval_start,
            mint,
            script_data_hash,
            collateral,
            required_signers,
            network_id,
            collateral_return,
            total_collateral,
            reference_inputs,
            voting_procedures,
            proposal_procedures,
            current_treasury_value,
            treasury_donation,
        })
    }
}

// ---------------------------------------------------------------------------
// Block envelope
// ---------------------------------------------------------------------------

/// A complete Conway-era block as it appears on the wire.
///
/// Uses the Praos header format (14-element body with single `vrf_result`)
/// instead of the Shelley header (15-element body with `nonce_vrf` +
/// `leader_vrf`).
///
/// CDDL:
/// ```text
/// block = [
///   header,
///   transaction_bodies       : [* transaction_body],
///   transaction_witness_sets : [* transaction_witness_set],
///   auxiliary_data_set       : {* uint => auxiliary_data},
///   invalid_transactions     : [* transaction_index]
/// ]
/// ```
///
/// Reference: `Cardano.Ledger.Conway.TxBody` and
/// `Ouroboros.Consensus.Shelley.Ledger.Block`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConwayBlock {
    /// The signed block header (Praos format).
    pub header: PraosHeader,
    /// Transaction bodies decoded with Conway-era key-map CBOR.
    pub transaction_bodies: Vec<ConwayTxBody>,
    /// Witness sets (parallel to transaction_bodies).
    pub transaction_witness_sets: Vec<ShelleyWitnessSet>,
    /// Auxiliary data map: transaction index → raw CBOR auxiliary data bytes.
    pub auxiliary_data_set: HashMap<u64, Vec<u8>>,
    /// Indices of transactions whose Phase-2 scripts failed validation.
    pub invalid_transactions: Vec<u64>,
}

impl ConwayBlock {
    /// Compute the Blake2b-256 header hash for this block.
    pub fn header_hash(&self) -> HeaderHash {
        self.header.header_hash()
    }
}

impl CborEncode for ConwayBlock {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(5);
        self.header.encode_cbor(enc);

        enc.array(self.transaction_bodies.len() as u64);
        for body in &self.transaction_bodies {
            body.encode_cbor(enc);
        }

        enc.array(self.transaction_witness_sets.len() as u64);
        for ws in &self.transaction_witness_sets {
            ws.encode_cbor(enc);
        }

        enc.map(self.auxiliary_data_set.len() as u64);
        for (&idx, meta) in &self.auxiliary_data_set {
            enc.unsigned(idx);
            enc.raw(meta);
        }

        enc.array(self.invalid_transactions.len() as u64);
        for &idx in &self.invalid_transactions {
            enc.unsigned(idx);
        }
    }
}

impl CborDecode for ConwayBlock {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: len as usize,
            });
        }

        let header = PraosHeader::decode_cbor(dec)?;

        let tb_count = dec.array()?;
        let mut transaction_bodies = Vec::with_capacity(tb_count as usize);
        for _ in 0..tb_count {
            transaction_bodies.push(ConwayTxBody::decode_cbor(dec)?);
        }

        let ws_count = dec.array()?;
        let mut witness_sets = Vec::with_capacity(ws_count as usize);
        for _ in 0..ws_count {
            witness_sets.push(ShelleyWitnessSet::decode_cbor(dec)?);
        }

        let meta_count = dec.map()?;
        let mut transaction_metadata = HashMap::with_capacity(meta_count as usize);
        for _ in 0..meta_count {
            let idx = dec.unsigned()?;
            let start = dec.position();
            dec.skip()?;
            let end = dec.position();
            let raw = dec.slice(start, end)?.to_vec();
            transaction_metadata.insert(idx, raw);
        }

        let inv_count = dec.array()?;
        let mut invalid_transactions = Vec::with_capacity(inv_count as usize);
        for _ in 0..inv_count {
            invalid_transactions.push(dec.unsigned()?);
        }

        Ok(Self {
            header,
            transaction_bodies,
            transaction_witness_sets: witness_sets,
            auxiliary_data_set: transaction_metadata,
            invalid_transactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::mary::Value;

    fn mk_txin(idx: u16) -> ShelleyTxIn {
        ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: idx,
        }
    }

    fn mk_babbage_txout() -> BabbageTxOut {
        BabbageTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }
    }

    fn mk_anchor() -> Anchor {
        Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xDD; 32],
        }
    }

    fn mk_gov_action_id() -> GovActionId {
        GovActionId {
            transaction_id: [0xBB; 32],
            gov_action_index: 0,
        }
    }

    // ── Vote ───────────────────────────────────────────────────────────

    #[test]
    fn vote_no_round_trip() {
        let v = Vote::No;
        let decoded = Vote::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn vote_yes_round_trip() {
        let v = Vote::Yes;
        let decoded = Vote::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn vote_abstain_round_trip() {
        let v = Vote::Abstain;
        let decoded = Vote::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn vote_all_variants_differ() {
        let a = Vote::No.to_cbor_bytes();
        let b = Vote::Yes.to_cbor_bytes();
        let c = Vote::Abstain.to_cbor_bytes();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    // ── Voter ──────────────────────────────────────────────────────────

    #[test]
    fn voter_committee_key_hash_round_trip() {
        let v = Voter::CommitteeKeyHash([0x01; 28]);
        let decoded = Voter::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn voter_committee_script_round_trip() {
        let v = Voter::CommitteeScript([0x02; 28]);
        let decoded = Voter::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn voter_drep_key_hash_round_trip() {
        let v = Voter::DRepKeyHash([0x03; 28]);
        let decoded = Voter::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn voter_drep_script_round_trip() {
        let v = Voter::DRepScript([0x04; 28]);
        let decoded = Voter::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn voter_stake_pool_round_trip() {
        let v = Voter::StakePool([0x05; 28]);
        let decoded = Voter::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn voter_all_tags_differ() {
        let h = [0xAA; 28];
        let variants = [
            Voter::CommitteeKeyHash(h).to_cbor_bytes(),
            Voter::CommitteeScript(h).to_cbor_bytes(),
            Voter::DRepKeyHash(h).to_cbor_bytes(),
            Voter::DRepScript(h).to_cbor_bytes(),
            Voter::StakePool(h).to_cbor_bytes(),
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    // ── GovActionId ────────────────────────────────────────────────────

    #[test]
    fn gov_action_id_round_trip() {
        let gid = mk_gov_action_id();
        let decoded = GovActionId::from_cbor_bytes(&gid.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, gid);
    }

    #[test]
    fn gov_action_id_different_index() {
        let a = GovActionId {
            transaction_id: [0x00; 32],
            gov_action_index: 0,
        };
        let b = GovActionId {
            transaction_id: [0x00; 32],
            gov_action_index: 1,
        };
        assert_ne!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }

    // ── Constitution ───────────────────────────────────────────────────

    #[test]
    fn constitution_with_guardrails_round_trip() {
        let c = Constitution {
            anchor: mk_anchor(),
            guardrails_script_hash: Some([0xEE; 28]),
        };
        let decoded = Constitution::from_cbor_bytes(&c.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, c);
    }

    #[test]
    fn constitution_no_guardrails_round_trip() {
        let c = Constitution {
            anchor: mk_anchor(),
            guardrails_script_hash: None,
        };
        let decoded = Constitution::from_cbor_bytes(&c.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, c);
    }

    // ── GovAction ──────────────────────────────────────────────────────

    #[test]
    fn gov_action_info_round_trip() {
        let a = GovAction::InfoAction;
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_no_confidence_round_trip() {
        let a = GovAction::NoConfidence {
            prev_action_id: Some(mk_gov_action_id()),
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_no_confidence_null_prev_round_trip() {
        let a = GovAction::NoConfidence {
            prev_action_id: None,
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_hard_fork_round_trip() {
        let a = GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_new_constitution_round_trip() {
        let a = GovAction::NewConstitution {
            prev_action_id: None,
            constitution: Constitution {
                anchor: mk_anchor(),
                guardrails_script_hash: None,
            },
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_parameter_change_round_trip() {
        let a = GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: ProtocolParameterUpdate {
                min_fee_a: Some(44),
                ..Default::default()
            },
            guardrails_script_hash: None,
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_treasury_withdrawals_round_trip() {
        let mut wdrl = BTreeMap::new();
        let ra = RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xE1; 28]),
        };
        wdrl.insert(ra, 1_000_000u64);
        let a = GovAction::TreasuryWithdrawals {
            withdrawals: wdrl,
            guardrails_script_hash: Some([0xFF; 28]),
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    #[test]
    fn gov_action_update_committee_round_trip() {
        let cred = StakeCredential::AddrKeyHash([0x11; 28]);
        let mut to_add = BTreeMap::new();
        to_add.insert(cred, 300u64);
        let a = GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![StakeCredential::ScriptHash([0x22; 28])],
            members_to_add: to_add,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        };
        let decoded = GovAction::from_cbor_bytes(&a.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, a);
    }

    // ── VotingProcedure ────────────────────────────────────────────────

    #[test]
    fn voting_procedure_with_anchor_round_trip() {
        let vp = VotingProcedure {
            vote: Vote::Yes,
            anchor: Some(mk_anchor()),
        };
        let decoded = VotingProcedure::from_cbor_bytes(&vp.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, vp);
    }

    #[test]
    fn voting_procedure_null_anchor_round_trip() {
        let vp = VotingProcedure {
            vote: Vote::No,
            anchor: None,
        };
        let decoded = VotingProcedure::from_cbor_bytes(&vp.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, vp);
    }

    // ── ProposalProcedure ──────────────────────────────────────────────

    #[test]
    fn proposal_procedure_round_trip() {
        let pp = ProposalProcedure {
            deposit: 500_000_000,
            reward_account: vec![0xE1; 29],
            gov_action: GovAction::InfoAction,
            anchor: mk_anchor(),
        };
        let decoded = ProposalProcedure::from_cbor_bytes(&pp.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, pp);
    }

    // ── VotingProcedures ───────────────────────────────────────────────

    #[test]
    fn voting_procedures_round_trip() {
        let voter = Voter::DRepKeyHash([0x44; 28]);
        let gid = mk_gov_action_id();
        let vp = VotingProcedure {
            vote: Vote::Abstain,
            anchor: None,
        };
        let mut inner = BTreeMap::new();
        inner.insert(gid, vp);
        let mut procs = BTreeMap::new();
        procs.insert(voter, inner);
        let vps = VotingProcedures { procedures: procs };
        let decoded = VotingProcedures::from_cbor_bytes(&vps.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, vps);
    }

    #[test]
    fn voting_procedures_empty_round_trip() {
        let vps = VotingProcedures {
            procedures: BTreeMap::new(),
        };
        let decoded = VotingProcedures::from_cbor_bytes(&vps.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, vps);
    }

    // ── ConwayTxBody ───────────────────────────────────────────────────

    fn mk_conway_body() -> ConwayTxBody {
        ConwayTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_babbage_txout()],
            fee: 200_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
            voting_procedures: None,
            proposal_procedures: None,
            current_treasury_value: None,
            treasury_donation: None,
        }
    }

    #[test]
    fn conway_tx_body_minimal_round_trip() {
        let body = mk_conway_body();
        let decoded = ConwayTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn conway_tx_body_with_governance_round_trip() {
        let voter = Voter::StakePool([0x77; 28]);
        let gid = mk_gov_action_id();
        let mut inner = BTreeMap::new();
        inner.insert(
            gid,
            VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
            },
        );
        let mut procs = BTreeMap::new();
        procs.insert(voter, inner);

        let body = ConwayTxBody {
            voting_procedures: Some(VotingProcedures { procedures: procs }),
            proposal_procedures: Some(vec![ProposalProcedure {
                deposit: 500_000_000,
                reward_account: vec![0xE1; 29],
                gov_action: GovAction::InfoAction,
                anchor: mk_anchor(),
            }]),
            current_treasury_value: Some(1_000_000_000),
            treasury_donation: Some(100_000),
            ..mk_conway_body()
        };
        let decoded = ConwayTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn conway_tx_body_no_governance_vs_with_differ() {
        let base = mk_conway_body();
        let with_gov = ConwayTxBody {
            current_treasury_value: Some(42),
            ..base.clone()
        };
        assert_ne!(base.to_cbor_bytes(), with_gov.to_cbor_bytes());
    }

    /// Upstream: `UpdateNotAllowed` — Conway removes the Shelley
    /// `update` field (CDDL key 6).  A valid-looking map that
    /// includes key 6 must be rejected at decode time.
    #[test]
    fn conway_tx_body_rejects_key_6_update() {
        // Build a minimal ConwayTxBody and encode it.
        let good = mk_conway_body();
        let good_bytes = good.to_cbor_bytes();
        // The good encoding is a CBOR map.  We now manually create a
        // map with one extra entry: key 6 → integer 0.
        let mut enc = Encoder::new();
        let mut dec = Decoder::new(&good_bytes);
        let original_count = dec.map().expect("map header");
        enc.map(original_count + 1);
        // Copy existing entries and inject key 6.
        let mut injected = false;
        for _ in 0..original_count {
            let key = dec.unsigned().expect("key");
            if !injected && key > 6 {
                enc.unsigned(6);
                enc.unsigned(0); // dummy value
                injected = true;
            }
            enc.unsigned(key);
            // copy the value verbatim
            let val_start = dec.position();
            dec.skip().expect("skip value");
            let val_end = dec.position();
            enc.raw(&good_bytes[val_start..val_end]);
        }
        if !injected {
            enc.unsigned(6);
            enc.unsigned(0);
        }

        let bad_bytes = enc.into_bytes();
        let result = ConwayTxBody::from_cbor_bytes(&bad_bytes);
        assert!(result.is_err(), "key 6 should be rejected");
        let err = result.unwrap_err();
        assert!(
            matches!(err, LedgerError::UpdateNotAllowedConway),
            "expected UpdateNotAllowedConway, got: {err:?}"
        );
    }

    /// Encoder-side drift guard for the Conway `GovAction` wire-tag space.
    ///
    /// Mirror of `cbor::tests::dcert_encoder_tag_and_arity_match_canonical_cddl`
    /// (Round 110) for the governance-action surface. `GovAction` carries
    /// 7 variants (tags 0..=6) across two independent hand-coded sites
    /// (the encode and decode cascades). A coupled encoder/decoder typo
    /// would round-trip cleanly while silently breaking on-chain wire
    /// compat with upstream — and because governance proposals drive
    /// real treasury movements, a tag-misinterpretation regression is
    /// equivalent to fund redirection.
    ///
    /// For every variant in 0..=6 this test constructs a representative
    /// value, encodes via `to_cbor_bytes`, then INDEPENDENTLY decodes the
    /// array header + first unsigned and asserts both the array length
    /// AND the tag against the literal CDDL-specified values. The
    /// per-variant lengths (4/3/3/2/5/3/1) come straight from upstream
    /// `gov_action` CDDL constructor arities.
    ///
    /// Bidirectional completeness: pins `cases.len() == 7` so a future
    /// tag-7 upstream variant added without extending this table fails
    /// immediately, and asserts the sorted observed-tag set is exactly
    /// `0..=6` so duplicate-tag and missing-tag regressions both surface
    /// with a clear "drifted from canonical CDDL" diagnostic.
    ///
    /// Reference: `Cardano.Ledger.Conway.Governance.Procedures.GovAction`;
    /// CDDL `gov_action` rule in `cardano-ledger-conway/cddl-files/conway.cddl`.
    #[test]
    fn gov_action_encoder_tag_and_arity_match_canonical_cddl() {
        use crate::ProtocolParameterUpdate;
        use crate::types::{Anchor, RewardAccount, UnitInterval};

        let constitution = Constitution {
            anchor: Anchor {
                url: "ipfs://constitution".to_string(),
                data_hash: [0xaa; 32],
            },
            guardrails_script_hash: None,
        };

        let mut withdrawals: BTreeMap<RewardAccount, u64> = BTreeMap::new();
        withdrawals.insert(
            RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x11; 28]),
            },
            1_000_000_000,
        );

        // (canonical tag, canonical array length, variant)
        let cases: Vec<(u64, u64, GovAction)> = vec![
            (
                0,
                4,
                GovAction::ParameterChange {
                    prev_action_id: None,
                    protocol_param_update: ProtocolParameterUpdate::default(),
                    guardrails_script_hash: None,
                },
            ),
            (
                1,
                3,
                GovAction::HardForkInitiation {
                    prev_action_id: None,
                    protocol_version: (10, 1),
                },
            ),
            (
                2,
                3,
                GovAction::TreasuryWithdrawals {
                    withdrawals,
                    guardrails_script_hash: None,
                },
            ),
            (3, 2, GovAction::NoConfidence { prev_action_id: None }),
            (
                4,
                5,
                GovAction::UpdateCommittee {
                    prev_action_id: None,
                    members_to_remove: vec![],
                    members_to_add: BTreeMap::new(),
                    quorum: UnitInterval { numerator: 1, denominator: 2 },
                },
            ),
            (
                5,
                3,
                GovAction::NewConstitution {
                    prev_action_id: None,
                    constitution,
                },
            ),
            (6, 1, GovAction::InfoAction),
        ];

        // Pin: exactly 7 cases (tags 0..=6).
        assert_eq!(
            cases.len(),
            7,
            "GovAction tag space must be 0..=6 (7 variants)",
        );

        let mut seen_tags: Vec<u64> = Vec::with_capacity(7);
        for (canonical_tag, canonical_len, action) in cases {
            let bytes = action.to_cbor_bytes();
            let mut dec = Decoder::new(&bytes);
            let len = dec.array().expect("GovAction encodes as a CBOR array");
            assert_eq!(
                len, canonical_len,
                "GovAction::{:?} encoded with array length {len}, expected {canonical_len}",
                action,
            );
            let tag = dec.unsigned().expect("first array element is the tag");
            assert_eq!(
                tag, canonical_tag,
                "GovAction::{:?} encoded with tag {tag}, expected {canonical_tag}",
                action,
            );
            seen_tags.push(tag);
        }

        seen_tags.sort();
        let expected_tags: Vec<u64> = (0..=6).collect();
        assert_eq!(
            seen_tags, expected_tags,
            "encoded GovAction tag set must be exactly 0..=6 with no duplicates",
        );
    }
}
