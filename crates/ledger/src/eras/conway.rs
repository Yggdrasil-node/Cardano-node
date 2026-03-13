//! Conway-era transaction types with on-chain governance.
//!
//! Conway introduces decentralized governance (CIP-1694), adding:
//! - `Voter`: committee member, DRep, or stake pool operator.
//! - `Vote`: Yes / No / Abstain.
//! - `GovActionId`: reference to a governance action (tx_id + index).
//! - `VotingProcedure`: a vote cast on a governance action, with
//!   optional anchor metadata.
//! - `Anchor`: URL + data hash for off-chain metadata.
//! - `ProposalProcedure`: governance proposal with deposit, return
//!   address, action body, and anchor.
//! - `ConwayTxBody`: extends Babbage with keys 19 (`voting_procedures`),
//!   20 (`proposal_procedures`), 21 (`current_treasury_value`),
//!   22 (`treasury_donation`).
//!
//! Governance action bodies are kept as opaque CBOR bytes at this stage.
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway>

use std::collections::{BTreeMap, HashMap};

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::babbage::BabbageTxOut;
use crate::eras::mary::{MintAsset, decode_mint_asset, encode_mint_asset};
use crate::eras::shelley::{ShelleyHeader, ShelleyTxIn, ShelleyWitnessSet};
use crate::error::LedgerError;
use crate::types::{Anchor, HeaderHash};

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
            raw.try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
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
/// The `gov_action` field is stored as opaque CBOR bytes to avoid
/// modeling the full recursive governance action grammar at this stage.
///
/// Reference: `Cardano.Ledger.Conway.Governance.Procedures.ProposalProcedure`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalProcedure {
    /// Deposit required for the proposal in lovelace.
    pub deposit: u64,
    /// Reward account receiving the deposit refund.
    pub reward_account: Vec<u8>,
    /// Governance action body as opaque CBOR bytes.
    pub gov_action: Vec<u8>,
    /// Off-chain metadata anchor.
    pub anchor: Anchor,
}

impl CborEncode for ProposalProcedure {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        enc.unsigned(self.deposit);
        enc.bytes(&self.reward_account);
        enc.raw(&self.gov_action);
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
        // Capture the governance action as opaque CBOR bytes.
        let start = dec.position();
        dec.skip()?;
        let end = dec.position();
        let gov_action = dec.slice(start, end)?.to_vec();
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
/// Certificates (4) and withdrawals (5) remain future work.
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
                    let count = dec.array()?;
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
                7 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into()
                            .map_err(|_| LedgerError::CborInvalidLength {
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
                        raw.try_into()
                            .map_err(|_| LedgerError::CborInvalidLength {
                                expected: 32,
                                actual: raw.len(),
                            })?;
                    script_data_hash = Some(hash);
                }
                13 => {
                    let count = dec.array()?;
                    let mut cols = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        cols.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    collateral = Some(cols);
                }
                14 => {
                    let count = dec.array()?;
                    let mut sigs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let raw = dec.bytes()?;
                        let hash: [u8; 28] =
                            raw.try_into()
                                .map_err(|_| LedgerError::CborInvalidLength {
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
                    let count = dec.array()?;
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
                    let count = dec.array()?;
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
/// Shares the Shelley block envelope structure but carries `ConwayTxBody`
/// transaction bodies with governance extensions.
///
/// CDDL:
/// ```text
/// block = [
///   header,
///   transaction_bodies       : [* transaction_body],
///   transaction_witness_sets : [* transaction_witness_set],
///   transaction_metadata_set : {* uint => metadata}
/// ]
/// ```
///
/// Reference: `Cardano.Ledger.Conway.TxBody` and
/// `Ouroboros.Consensus.Shelley.Ledger.Block`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConwayBlock {
    /// The signed block header (same format as Shelley).
    pub header: ShelleyHeader,
    /// Transaction bodies decoded with Conway-era key-map CBOR.
    pub transaction_bodies: Vec<ConwayTxBody>,
    /// Witness sets (parallel to transaction_bodies).
    pub witness_sets: Vec<ShelleyWitnessSet>,
    /// Metadata map: transaction index → raw CBOR metadata bytes.
    pub transaction_metadata: HashMap<u64, Vec<u8>>,
}

impl ConwayBlock {
    /// Compute the Blake2b-256 header hash for this block.
    pub fn header_hash(&self) -> HeaderHash {
        self.header.header_hash()
    }
}

impl CborEncode for ConwayBlock {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        self.header.encode_cbor(enc);

        enc.array(self.transaction_bodies.len() as u64);
        for body in &self.transaction_bodies {
            body.encode_cbor(enc);
        }

        enc.array(self.witness_sets.len() as u64);
        for ws in &self.witness_sets {
            ws.encode_cbor(enc);
        }

        enc.map(self.transaction_metadata.len() as u64);
        for (&idx, meta) in &self.transaction_metadata {
            enc.unsigned(idx);
            enc.raw(meta);
        }
    }
}

impl CborDecode for ConwayBlock {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }

        let header = ShelleyHeader::decode_cbor(dec)?;

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

        Ok(Self {
            header,
            transaction_bodies,
            witness_sets,
            transaction_metadata,
        })
    }
}
