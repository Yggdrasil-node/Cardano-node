//! `LedgerState` CBOR encoder + decoder.
//!
//! Mirrors upstream EncCBOR/DecCBOR instance separation for
//! `Cardano.Ledger.Shelley.LedgerState`. Yggdrasil's codec lives in its own
//! file because the 24-element array codec is mechanical and large; isolating
//! it from the apply-path methods on `LedgerState` keeps the orchestrator
//! file focused on transition logic.
//!
//! The codec is forward-compatible with legacy 9-, 10-, and 12-23-element
//! array layouts produced by older yggdrasil releases — each tail field is
//! decoded conditionally on the array length, and missing trailing fields
//! default to safe values (empty maps, `None`, `EpochNo(0)`, etc.).
//!
//! Extracted from `state.rs` in R269 sixteenth slice as part of the strict
//! 1:1 filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269p-state-cbor-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. The `LedgerState` 24-element-array codec
//! is defined inline in upstream's
//! `Cardano.Ledger.Shelley.LedgerState` via the `EncCBOR`/`DecCBOR`
//! instances. Yggdrasil isolates the codec for cohesion (the
//! mechanical 24-element layout would dominate the apply-path
//! module). Forward-compatible with legacy 9-, 10-, and 12-23-
//! element layouts produced by older Yggdrasil releases.

use super::{
    AccountingState, CommitteeState, DepositPot, DrepState, EnactState, GenesisDelegationState,
    GovernanceActionState, InstantaneousRewards, LedgerState, PoolState, RewardAccounts,
    StakeCredentials,
};
use crate::eras::shelley::ShelleyUtxo;
use crate::types::{EpochNo, GenesisDelegateHash, GenesisHash, Point, UnitInterval, VrfKeyHash};
use crate::utxo::MultiEraUtxo;
use crate::{CborDecode, CborEncode, Decoder, Encoder, Era, LedgerError};
use std::collections::BTreeMap;

impl CborEncode for LedgerState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(24);
        self.current_era.encode_cbor(enc);
        self.tip.encode_cbor(enc);
        match self.expected_network_id {
            Some(network_id) => {
                enc.unsigned(u64::from(network_id));
            }
            None => {
                enc.null();
            }
        }
        enc.map(self.governance_actions.len() as u64);
        for (gov_action_id, state) in &self.governance_actions {
            gov_action_id.encode_cbor(enc);
            state.encode_cbor(enc);
        }
        self.pool_state.encode_cbor(enc);
        self.stake_credentials.encode_cbor(enc);
        self.committee_state.encode_cbor(enc);
        self.drep_state.encode_cbor(enc);
        self.reward_accounts.encode_cbor(enc);
        self.multi_era_utxo.encode_cbor(enc);
        self.shelley_utxo.encode_cbor(enc);
        // Encode protocol_params as either the params map or CBOR null.
        match &self.protocol_params {
            Some(pp) => pp.encode_cbor(enc),
            None => {
                enc.null();
            }
        }
        self.deposit_pot.encode_cbor(enc);
        self.accounting.encode_cbor(enc);
        self.current_epoch.encode_cbor(enc);
        self.enact_state.encode_cbor(enc);
        // gen_delegs: map of genesis-hash → (delegate, vrf)
        enc.map(self.gen_delegs.len() as u64);
        for (genesis_hash, deleg) in &self.gen_delegs {
            enc.bytes(genesis_hash);
            enc.array(2);
            enc.bytes(&deleg.delegate);
            enc.bytes(&deleg.vrf);
        }
        // pending_pparam_updates: map epoch → map genesis-hash → update
        enc.map(self.pending_pparam_updates.len() as u64);
        for (epoch, proposals) in &self.pending_pparam_updates {
            epoch.encode_cbor(enc);
            enc.map(proposals.len() as u64);
            for (genesis_hash, update) in proposals {
                enc.bytes(genesis_hash);
                update.encode_cbor(enc);
            }
        }
        // utxos_donation: accumulated treasury donations (Conway).
        enc.unsigned(self.utxos_donation);
        // instantaneous_rewards: accumulated MIR state (Shelley–Babbage).
        self.instantaneous_rewards.encode_cbor(enc);
        // genesis_update_quorum: MIR cert signature threshold.
        enc.unsigned(self.genesis_update_quorum);
        // num_dormant_epochs: consecutive dormant epoch count (Conway).
        enc.unsigned(self.num_dormant_epochs);
        // blocks_made: per-pool block production counts (current epoch).
        // Reference: NewEpochState.nesBcur.
        enc.map(self.blocks_made.len() as u64);
        for (pool_hash, &count) in &self.blocks_made {
            enc.bytes(pool_hash);
            enc.unsigned(count);
        }
        // blocks_made_prev: delayed per-pool block counts used by rewards.
        // Reference: NewEpochState.nesBprev.
        enc.map(self.blocks_made_prev.len() as u64);
        for (pool_hash, &count) in &self.blocks_made_prev {
            enc.bytes(pool_hash);
            enc.unsigned(count);
        }
    }
}

impl CborDecode for LedgerState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        // Accept legacy 9/10-element arrays and current 12-24-element arrays.
        if len != 9 && len != 10 && !(12..=24).contains(&len) {
            return Err(LedgerError::CborInvalidLength {
                expected: 24,
                actual: len as usize,
            });
        }

        let current_era = Era::decode_cbor(dec)?;
        let tip = Point::decode_cbor(dec)?;
        let expected_network_id = if len >= 13 {
            if dec.peek_is_null() {
                dec.skip()?;
                None
            } else {
                Some(dec.unsigned()? as u8)
            }
        } else {
            None
        };
        let governance_actions = if len >= 14 {
            let map_len = dec.map()?;
            let mut governance_actions = BTreeMap::new();
            for _ in 0..map_len {
                let gov_action_id = crate::eras::conway::GovActionId::decode_cbor(dec)?;
                let state = GovernanceActionState::decode_cbor(dec)?;
                governance_actions.insert(gov_action_id, state);
            }
            governance_actions
        } else {
            BTreeMap::new()
        };
        let pool_state = PoolState::decode_cbor(dec)?;
        let stake_credentials = StakeCredentials::decode_cbor(dec)?;
        let committee_state = CommitteeState::decode_cbor(dec)?;
        let drep_state = DrepState::decode_cbor(dec)?;
        let reward_accounts = RewardAccounts::decode_cbor(dec)?;
        let multi_era_utxo = MultiEraUtxo::decode_cbor(dec)?;
        let shelley_utxo = ShelleyUtxo::decode_cbor(dec)?;

        let protocol_params = if len >= 10 {
            if dec.peek_is_null() {
                dec.skip()?;
                None
            } else {
                Some(crate::protocol_params::ProtocolParameters::decode_cbor(
                    dec,
                )?)
            }
        } else {
            None
        };

        let deposit_pot = if len >= 12 {
            DepositPot::decode_cbor(dec)?
        } else {
            DepositPot::default()
        };

        let accounting = if len >= 12 {
            AccountingState::decode_cbor(dec)?
        } else {
            AccountingState::default()
        };

        let current_epoch = if len >= 15 {
            EpochNo::decode_cbor(dec)?
        } else {
            EpochNo(0)
        };

        let enact_state = if len >= 16 {
            EnactState::decode_cbor(dec)?
        } else {
            EnactState::default()
        };

        let gen_delegs = if len >= 17 {
            let map_len = dec.map()?;
            let mut delegs = BTreeMap::new();
            for _ in 0..map_len {
                let genesis_hash: GenesisHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 28];
                    arr.copy_from_slice(bytes);
                    arr
                };
                let inner_len = dec.array()?;
                if inner_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: inner_len as usize,
                    });
                }
                let delegate: GenesisDelegateHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 28];
                    arr.copy_from_slice(bytes);
                    arr
                };
                let vrf: VrfKeyHash = {
                    let bytes = dec.bytes()?;
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    arr
                };
                delegs.insert(genesis_hash, GenesisDelegationState { delegate, vrf });
            }
            delegs
        } else {
            BTreeMap::new()
        };

        let pending_pparam_updates = if len >= 18 {
            let outer_len = dec.map()?;
            let mut updates = BTreeMap::new();
            for _ in 0..outer_len {
                let epoch = EpochNo::decode_cbor(dec)?;
                let inner_len = dec.map()?;
                let mut proposals = BTreeMap::new();
                for _ in 0..inner_len {
                    let genesis_hash: GenesisHash = {
                        let bytes = dec.bytes()?;
                        let mut arr = [0u8; 28];
                        arr.copy_from_slice(bytes);
                        arr
                    };
                    let update = crate::protocol_params::ProtocolParameterUpdate::decode_cbor(dec)?;
                    proposals.insert(genesis_hash, update);
                }
                updates.insert(epoch, proposals);
            }
            updates
        } else {
            BTreeMap::new()
        };

        let utxos_donation = if len >= 19 { dec.unsigned()? } else { 0 };

        let instantaneous_rewards = if len >= 20 {
            InstantaneousRewards::decode_cbor(dec)?
        } else {
            InstantaneousRewards::default()
        };

        let genesis_update_quorum = if len >= 21 {
            dec.unsigned()?
        } else {
            5 // upstream default (mainnet)
        };

        let num_dormant_epochs = if len >= 22 { dec.unsigned()? } else { 0 };

        let blocks_made = if len >= 23 {
            let map_len = dec.map()?;
            let mut bm = BTreeMap::new();
            for _ in 0..map_len {
                let bytes = dec.bytes()?;
                let mut arr = [0u8; 28];
                arr.copy_from_slice(bytes);
                let count = dec.unsigned()?;
                bm.insert(arr, count);
            }
            bm
        } else {
            BTreeMap::new()
        };

        let blocks_made_prev = if len >= 24 {
            let map_len = dec.map()?;
            let mut bm = BTreeMap::new();
            for _ in 0..map_len {
                let bytes = dec.bytes()?;
                let mut arr = [0u8; 28];
                arr.copy_from_slice(bytes);
                let count = dec.unsigned()?;
                bm.insert(arr, count);
            }
            bm
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            current_era,
            tip,
            latest_block_protocol_version: None,
            tip_block_no: None,
            current_epoch,
            expected_network_id,
            governance_actions,
            pool_state,
            stake_credentials,
            committee_state,
            drep_state,
            reward_accounts,
            multi_era_utxo,
            shelley_utxo,
            protocol_params,
            // Reconstructing from a checkpoint: the snapshot only
            // captured curPParams, not prevPParams.  Initialise prev = cur
            // so the first reward calc after checkpoint resume falls back
            // to current params; once UPEC fires again the field will
            // hold the proper pre-update value.
            previous_protocol_params: None,
            deposit_pot,
            accounting,
            enact_state,
            gen_delegs,
            future_gen_delegs: BTreeMap::new(),
            pending_pparam_updates,
            utxos_donation,
            instantaneous_rewards,
            genesis_update_quorum,
            num_dormant_epochs,
            blocks_made,
            blocks_made_prev,
            pending_shelley_genesis_utxo: None,
            pending_shelley_genesis_stake: None,
            pending_shelley_genesis_delegs: None,
            // Runtime-only fields — not serialized, re-set from genesis.
            max_lovelace_supply: 0,
            slots_per_epoch: 0,
            active_slot_coeff: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            stability_window: None,
            byron_shelley_transition: None,
        })
    }
}
