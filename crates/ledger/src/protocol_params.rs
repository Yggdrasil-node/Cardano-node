//! Protocol parameters carried by the ledger state.
//!
//! Each Cardano era extends the parameter set, but from a validation
//! standpoint the important fields are the fee formula coefficients,
//! UTxO entry limits, execution-unit pricing, and collateral rules.
//!
//! This module intentionally stores all known parameters in a single
//! flat structure. Era-specific defaults and boundary conditions are
//! documented per field.
//!
//! Reference: `Cardano.Ledger.Shelley.PParams` and per-era extensions
//! in `cardano-ledger`.

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::alonzo::ExUnits;
use crate::error::LedgerError;
use crate::types::UnitInterval;

// ---------------------------------------------------------------------------
// Pool voting thresholds (CDDL key 25)
// ---------------------------------------------------------------------------

/// Per-action-type acceptance thresholds for stake-pool operator (SPO)
/// votes in Conway governance.
///
/// Encoded as a 5-element CBOR array of `unit_interval` values.
///
/// Reference: `pool_voting_thresholds` in the Conway CDDL and
/// `PoolVotingThresholds` in `Cardano.Ledger.Conway.PParams`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolVotingThresholds {
    /// Threshold for a motion of no-confidence.
    pub motion_no_confidence: UnitInterval,
    /// Threshold for a committee update under normal conditions.
    pub committee_normal: UnitInterval,
    /// Threshold for a committee update when in a state of no-confidence.
    pub committee_no_confidence: UnitInterval,
    /// Threshold for a hard-fork initiation.
    pub hard_fork_initiation: UnitInterval,
    /// Threshold for security-relevant parameter changes (`ppSecurityGroup`).
    pub pp_security_group: UnitInterval,
}

impl Default for PoolVotingThresholds {
    fn default() -> Self {
        Self {
            motion_no_confidence: UnitInterval { numerator: 51, denominator: 100 },
            committee_normal: UnitInterval { numerator: 51, denominator: 100 },
            committee_no_confidence: UnitInterval { numerator: 51, denominator: 100 },
            hard_fork_initiation: UnitInterval { numerator: 51, denominator: 100 },
            pp_security_group: UnitInterval { numerator: 51, denominator: 100 },
        }
    }
}

impl CborEncode for PoolVotingThresholds {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(5);
        self.motion_no_confidence.encode_cbor(enc);
        self.committee_normal.encode_cbor(enc);
        self.committee_no_confidence.encode_cbor(enc);
        self.hard_fork_initiation.encode_cbor(enc);
        self.pp_security_group.encode_cbor(enc);
    }
}

impl CborDecode for PoolVotingThresholds {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 5 {
            return Err(LedgerError::CborInvalidLength { expected: 5, actual: len as usize });
        }
        Ok(Self {
            motion_no_confidence: UnitInterval::decode_cbor(dec)?,
            committee_normal: UnitInterval::decode_cbor(dec)?,
            committee_no_confidence: UnitInterval::decode_cbor(dec)?,
            hard_fork_initiation: UnitInterval::decode_cbor(dec)?,
            pp_security_group: UnitInterval::decode_cbor(dec)?,
        })
    }
}

// ---------------------------------------------------------------------------
// DRep voting thresholds (CDDL key 26)
// ---------------------------------------------------------------------------

/// Per-action-type acceptance thresholds for DRep votes in Conway governance.
///
/// Encoded as a 10-element CBOR array of `unit_interval` values.
///
/// Reference: `drep_voting_thresholds` in the Conway CDDL and
/// `DRepVotingThresholds` in `Cardano.Ledger.Conway.PParams`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DRepVotingThresholds {
    /// Threshold for a motion of no-confidence.
    pub motion_no_confidence: UnitInterval,
    /// Threshold for a committee update under normal conditions.
    pub committee_normal: UnitInterval,
    /// Threshold for a committee update when in a state of no-confidence.
    pub committee_no_confidence: UnitInterval,
    /// Threshold for a new-constitution / guardrails-script action.
    pub update_to_constitution: UnitInterval,
    /// Threshold for a hard-fork initiation.
    pub hard_fork_initiation: UnitInterval,
    /// Threshold for protocol-parameter changes in the network group.
    pub pp_network_group: UnitInterval,
    /// Threshold for protocol-parameter changes in the economic group.
    pub pp_economic_group: UnitInterval,
    /// Threshold for protocol-parameter changes in the technical group.
    pub pp_technical_group: UnitInterval,
    /// Threshold for protocol-parameter changes in the governance group.
    pub pp_gov_group: UnitInterval,
    /// Threshold for treasury withdrawals.
    pub treasury_withdrawal: UnitInterval,
}

impl Default for DRepVotingThresholds {
    fn default() -> Self {
        Self {
            motion_no_confidence: UnitInterval { numerator: 67, denominator: 100 },
            committee_normal: UnitInterval { numerator: 67, denominator: 100 },
            committee_no_confidence: UnitInterval { numerator: 60, denominator: 100 },
            update_to_constitution: UnitInterval { numerator: 75, denominator: 100 },
            hard_fork_initiation: UnitInterval { numerator: 60, denominator: 100 },
            pp_network_group: UnitInterval { numerator: 67, denominator: 100 },
            pp_economic_group: UnitInterval { numerator: 67, denominator: 100 },
            pp_technical_group: UnitInterval { numerator: 67, denominator: 100 },
            pp_gov_group: UnitInterval { numerator: 75, denominator: 100 },
            treasury_withdrawal: UnitInterval { numerator: 67, denominator: 100 },
        }
    }
}

impl CborEncode for DRepVotingThresholds {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(10);
        self.motion_no_confidence.encode_cbor(enc);
        self.committee_normal.encode_cbor(enc);
        self.committee_no_confidence.encode_cbor(enc);
        self.update_to_constitution.encode_cbor(enc);
        self.hard_fork_initiation.encode_cbor(enc);
        self.pp_network_group.encode_cbor(enc);
        self.pp_economic_group.encode_cbor(enc);
        self.pp_technical_group.encode_cbor(enc);
        self.pp_gov_group.encode_cbor(enc);
        self.treasury_withdrawal.encode_cbor(enc);
    }
}

impl CborDecode for DRepVotingThresholds {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 10 {
            return Err(LedgerError::CborInvalidLength { expected: 10, actual: len as usize });
        }
        Ok(Self {
            motion_no_confidence: UnitInterval::decode_cbor(dec)?,
            committee_normal: UnitInterval::decode_cbor(dec)?,
            committee_no_confidence: UnitInterval::decode_cbor(dec)?,
            update_to_constitution: UnitInterval::decode_cbor(dec)?,
            hard_fork_initiation: UnitInterval::decode_cbor(dec)?,
            pp_network_group: UnitInterval::decode_cbor(dec)?,
            pp_economic_group: UnitInterval::decode_cbor(dec)?,
            pp_technical_group: UnitInterval::decode_cbor(dec)?,
            pp_gov_group: UnitInterval::decode_cbor(dec)?,
            treasury_withdrawal: UnitInterval::decode_cbor(dec)?,
        })
    }
}

/// Protocol parameters governing transaction and block validation.
///
/// All fields are optional so that the struct can represent any era's
/// parameter subset. Validation helpers treat a `None` value as "rule
/// not enforced in this era".
///
/// Reference: upstream `PParams` per era in `cardano-ledger`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolParameters {
    // -- Fee formula (Shelley+) --------------------------------------------

    /// Linear fee coefficient (lovelace per byte).
    ///
    /// CDDL key 0 — `min_fee_a`.
    pub min_fee_a: u64,

    /// Constant fee addend (lovelace).
    ///
    /// CDDL key 1 — `min_fee_b`.
    pub min_fee_b: u64,

    // -- Block limits (Shelley+) -------------------------------------------

    /// Maximum block body size in bytes.
    ///
    /// CDDL key 2 — `max_block_body_size`.
    pub max_block_body_size: u32,

    /// Maximum transaction size in bytes.
    ///
    /// CDDL key 3 — `max_tx_size`.
    pub max_tx_size: u32,

    /// Maximum block header size in bytes.
    ///
    /// CDDL key 4 — `max_block_header_size`.
    pub max_block_header_size: u16,

    // -- Staking (Shelley+) ------------------------------------------------

    /// Key deposit (lovelace).
    ///
    /// CDDL key 5 — `key_deposit`.
    pub key_deposit: u64,

    /// Pool deposit (lovelace).
    ///
    /// CDDL key 6 — `pool_deposit`.
    pub pool_deposit: u64,

    /// Maximum epoch for pool retirement.
    ///
    /// CDDL key 7 — `e_max`.
    pub e_max: u64,

    /// Desired number of stake pools.
    ///
    /// CDDL key 8 — `n_opt`.
    pub n_opt: u64,

    /// Pool pledge influence (a0).
    ///
    /// CDDL key 9 — `a0`.
    pub a0: UnitInterval,

    /// Monetary expansion (rho).
    ///
    /// CDDL key 10 — `rho`.
    pub rho: UnitInterval,

    /// Treasury growth rate (tau).
    ///
    /// CDDL key 11 — `tau`.
    pub tau: UnitInterval,

    /// Current ledger protocol version `(major, minor)`.
    ///
    /// This tracks the active protocol version carried in protocol parameters
    /// and is used by Conway governance bootstrap checks.
    ///
    /// CDDL key 14 — `protocol_version`.
    pub protocol_version: Option<(u64, u64)>,

    // -- Min UTxO (Shelley–Mary) -------------------------------------------

    /// Minimum UTxO value (lovelace). Applied in Shelley through Mary.
    /// Replaced by `coins_per_utxo_byte` from Alonzo onward.
    ///
    /// CDDL key 15 — `min_utxo_value`.
    pub min_utxo_value: Option<u64>,

    // -- Pool cost (Shelley+) ----------------------------------------------

    /// Minimum pool cost (lovelace per epoch).
    ///
    /// CDDL key 16 — `min_pool_cost`.
    pub min_pool_cost: u64,

    // -- Alonzo+ -----------------------------------------------------------

    /// Coins per UTxO byte (replaces `min_utxo_value` from Alonzo).
    ///
    /// CDDL key 17 — `coins_per_utxo_byte` (Babbage name; Alonzo used
    /// `coins_per_utxo_word` which is 8× this value).
    pub coins_per_utxo_byte: Option<u64>,

    /// Execution unit prices: (price_mem, price_steps) as rationals.
    ///
    /// CDDL key 19 — `prices`.
    pub price_mem: Option<UnitInterval>,
    /// CPU step price.
    pub price_step: Option<UnitInterval>,

    /// Maximum execution units per transaction.
    ///
    /// CDDL key 20 — `max_tx_ex_units`.
    pub max_tx_ex_units: Option<ExUnits>,

    /// Maximum execution units per block.
    ///
    /// CDDL key 21 — `max_block_ex_units`.
    pub max_block_ex_units: Option<ExUnits>,

    /// Maximum value size (serialized bytes) for an output.
    ///
    /// CDDL key 22 — `max_val_size`.
    pub max_val_size: Option<u32>,

    /// Collateral percentage (e.g. 150 = 150%).
    ///
    /// CDDL key 23 — `collateral_percentage`.
    pub collateral_percentage: Option<u64>,

    /// Maximum number of collateral inputs.
    ///
    /// CDDL key 24 — `max_collateral_inputs`.
    pub max_collateral_inputs: Option<u32>,

    /// Governance action lifetime in epochs for Conway proposal procedures.
    ///
    /// CDDL key 29 — `gov_action_lifetime`.
    pub gov_action_lifetime: Option<u64>,

    /// Governance action deposit required for Conway proposal procedures.
    ///
    /// CDDL key 30 — `gov_action_deposit`.
    pub gov_action_deposit: Option<u64>,

    /// DRep registration deposit (lovelace).
    ///
    /// CDDL key 31 — `drep_deposit`.
    pub drep_deposit: Option<u64>,

    /// Pool voting thresholds for Conway governance actions.
    ///
    /// CDDL key 25 — `pool_voting_thresholds`.
    pub pool_voting_thresholds: Option<PoolVotingThresholds>,

    /// DRep voting thresholds for Conway governance actions.
    ///
    /// CDDL key 26 — `drep_voting_thresholds`.
    pub drep_voting_thresholds: Option<DRepVotingThresholds>,

    /// Minimum number of active committee members required.
    ///
    /// CDDL key 27 — `min_committee_size`.
    pub min_committee_size: Option<u64>,

    /// Maximum term length for committee members in epochs.
    ///
    /// CDDL key 28 — `committee_term_limit`.
    pub committee_term_limit: Option<u64>,

    /// DRep activity period in epochs.  A DRep that has not voted or
    /// updated for this many epochs is treated as inactive.
    ///
    /// CDDL key 32 — `drep_activity`.
    pub drep_activity: Option<u64>,
}

impl Default for ProtocolParameters {
    /// Returns Shelley-era mainnet genesis defaults.
    ///
    /// Alonzo+ fields are `None` — callers must set them for
    /// script-validation-era blocks.
    fn default() -> Self {
        Self {
            min_fee_a: 44,
            min_fee_b: 155_381,
            max_block_body_size: 65_536,
            max_tx_size: 16_384,
            max_block_header_size: 1100,
            key_deposit: 2_000_000,
            pool_deposit: 500_000_000,
            e_max: 18,
            n_opt: 150,
            a0: UnitInterval {
                numerator: 3,
                denominator: 10,
            },
            rho: UnitInterval {
                numerator: 3,
                denominator: 1000,
            },
            tau: UnitInterval {
                numerator: 2,
                denominator: 10,
            },
            protocol_version: Some((2, 0)),
            min_utxo_value: Some(1_000_000),
            min_pool_cost: 340_000_000,
            coins_per_utxo_byte: None,
            price_mem: None,
            price_step: None,
            max_tx_ex_units: None,
            max_block_ex_units: None,
            max_val_size: None,
            collateral_percentage: None,
            max_collateral_inputs: None,
            gov_action_lifetime: None,
            gov_action_deposit: None,
            drep_deposit: None,
            drep_activity: None,
            pool_voting_thresholds: None,
            drep_voting_thresholds: None,
            min_committee_size: None,
            committee_term_limit: None,
        }
    }
}

impl ProtocolParameters {
    /// Returns mainnet Alonzo-era defaults (extends Shelley defaults with
    /// script-era parameters).
    pub fn alonzo_defaults() -> Self {
        Self {
            protocol_version: Some((6, 0)),
            min_utxo_value: None,
            coins_per_utxo_byte: Some(4_310),
            price_mem: Some(UnitInterval {
                numerator: 577,
                denominator: 10_000,
            }),
            price_step: Some(UnitInterval {
                numerator: 721,
                denominator: 10_000_000,
            }),
            max_tx_ex_units: Some(ExUnits {
                mem: 10_000_000_000,
                steps: 10_000_000_000_000,
            }),
            max_block_ex_units: Some(ExUnits {
                mem: 50_000_000_000,
                steps: 40_000_000_000_000,
            }),
            max_val_size: Some(5000),
            collateral_percentage: Some(150),
            max_collateral_inputs: Some(3),
            ..Self::default()
        }
    }

    /// Returns the minimum lovelace required for a UTxO entry.
    ///
    /// - **Shelley–Mary**: `min_utxo_value`.
    /// - **Alonzo+**: `coins_per_utxo_byte × serialized_size`.
    ///
    /// Returns `None` when neither parameter is set.
    pub fn min_lovelace_for_utxo(&self, serialized_output_size: usize) -> Option<u64> {
        if let Some(per_byte) = self.coins_per_utxo_byte {
            // Alonzo+: per-byte costing with a 160-byte overhead per upstream
            // `utxoEntrySizeWithoutVal + 27`.
            let overhead = 160u64;
            let size = serialized_output_size as u64 + overhead;
            Some(per_byte.saturating_mul(size))
        } else {
            self.min_utxo_value
        }
    }
}

// -- CBOR codec ---------------------------------------------------------------

impl CborEncode for ProtocolParameters {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // Encode as a map of (key → value) pairs, matching the upstream
        // update-proposal encoding.
        // Always-present keys: 0-11 (12) + 16 (1) = 13
        let mut count: u64 = 13;
        if self.min_utxo_value.is_some() {
            count += 1;
        }
        if self.protocol_version.is_some() {
            count += 1;
        }
        if self.coins_per_utxo_byte.is_some() {
            count += 1;
        }
        if self.price_mem.is_some() {
            count += 2; // price_mem + price_step
        }
        if self.max_tx_ex_units.is_some() {
            count += 1;
        }
        if self.max_block_ex_units.is_some() {
            count += 1;
        }
        if self.max_val_size.is_some() {
            count += 1;
        }
        if self.collateral_percentage.is_some() {
            count += 1;
        }
        if self.max_collateral_inputs.is_some() {
            count += 1;
        }
        if self.gov_action_lifetime.is_some() {
            count += 1;
        }
        if self.gov_action_deposit.is_some() {
            count += 1;
        }
        if self.drep_deposit.is_some() {
            count += 1;
        }
        if self.drep_activity.is_some() {
            count += 1;
        }
        if self.pool_voting_thresholds.is_some() {
            count += 1;
        }
        if self.drep_voting_thresholds.is_some() {
            count += 1;
        }
        if self.min_committee_size.is_some() {
            count += 1;
        }
        if self.committee_term_limit.is_some() {
            count += 1;
        }

        enc.map(count);

        enc.unsigned(0).unsigned(self.min_fee_a);
        enc.unsigned(1).unsigned(self.min_fee_b);
        enc.unsigned(2).unsigned(self.max_block_body_size as u64);
        enc.unsigned(3).unsigned(self.max_tx_size as u64);
        enc.unsigned(4).unsigned(self.max_block_header_size as u64);
        enc.unsigned(5).unsigned(self.key_deposit);
        enc.unsigned(6).unsigned(self.pool_deposit);
        enc.unsigned(7).unsigned(self.e_max);
        enc.unsigned(8).unsigned(self.n_opt);
        enc.unsigned(9);
        self.a0.encode_cbor(enc);
        enc.unsigned(10);
        self.rho.encode_cbor(enc);
        enc.unsigned(11);
        self.tau.encode_cbor(enc);

        if let Some((major, minor)) = self.protocol_version {
            enc.unsigned(14).array(2).unsigned(major).unsigned(minor);
        }

        if let Some(val) = self.min_utxo_value {
            enc.unsigned(15).unsigned(val);
        }

        enc.unsigned(16).unsigned(self.min_pool_cost);

        if let Some(val) = self.coins_per_utxo_byte {
            enc.unsigned(17).unsigned(val);
        }
        if let (Some(pm), Some(ps)) = (&self.price_mem, &self.price_step) {
            enc.unsigned(18);
            pm.encode_cbor(enc);
            enc.unsigned(19);
            ps.encode_cbor(enc);
        }
        if let Some(ref units) = self.max_tx_ex_units {
            enc.unsigned(20);
            units.encode_cbor(enc);
        }
        if let Some(ref units) = self.max_block_ex_units {
            enc.unsigned(21);
            units.encode_cbor(enc);
        }
        if let Some(val) = self.max_val_size {
            enc.unsigned(22).unsigned(val as u64);
        }
        if let Some(val) = self.collateral_percentage {
            enc.unsigned(23).unsigned(val);
        }
        if let Some(val) = self.max_collateral_inputs {
            enc.unsigned(24).unsigned(val as u64);
        }
        if let Some(ref thresholds) = self.pool_voting_thresholds {
            enc.unsigned(25);
            thresholds.encode_cbor(enc);
        }
        if let Some(ref thresholds) = self.drep_voting_thresholds {
            enc.unsigned(26);
            thresholds.encode_cbor(enc);
        }
        if let Some(val) = self.min_committee_size {
            enc.unsigned(27).unsigned(val);
        }
        if let Some(val) = self.committee_term_limit {
            enc.unsigned(28).unsigned(val);
        }
        if let Some(val) = self.gov_action_lifetime {
            enc.unsigned(29).unsigned(val);
        }
        if let Some(val) = self.gov_action_deposit {
            enc.unsigned(30).unsigned(val);
        }
        if let Some(val) = self.drep_deposit {
            enc.unsigned(31).unsigned(val);
        }
        if let Some(val) = self.drep_activity {
            enc.unsigned(32).unsigned(val);
        }
    }
}

impl CborDecode for ProtocolParameters {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;
        let mut params = ProtocolParameters {
            min_fee_a: 0,
            min_fee_b: 0,
            max_block_body_size: 0,
            max_tx_size: 0,
            max_block_header_size: 0,
            key_deposit: 0,
            pool_deposit: 0,
            e_max: 0,
            n_opt: 0,
            a0: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            rho: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            tau: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            protocol_version: None,
            min_utxo_value: None,
            min_pool_cost: 0,
            coins_per_utxo_byte: None,
            price_mem: None,
            price_step: None,
            max_tx_ex_units: None,
            max_block_ex_units: None,
            max_val_size: None,
            collateral_percentage: None,
            max_collateral_inputs: None,
            gov_action_lifetime: None,
            gov_action_deposit: None,
            drep_deposit: None,
            drep_activity: None,
            pool_voting_thresholds: None,
            drep_voting_thresholds: None,
            min_committee_size: None,
            committee_term_limit: None,
        };

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => params.min_fee_a = dec.unsigned()?,
                1 => params.min_fee_b = dec.unsigned()?,
                2 => params.max_block_body_size = dec.unsigned()? as u32,
                3 => params.max_tx_size = dec.unsigned()? as u32,
                4 => params.max_block_header_size = dec.unsigned()? as u16,
                5 => params.key_deposit = dec.unsigned()?,
                6 => params.pool_deposit = dec.unsigned()?,
                7 => params.e_max = dec.unsigned()?,
                8 => params.n_opt = dec.unsigned()?,
                9 => params.a0 = UnitInterval::decode_cbor(dec)?,
                10 => params.rho = UnitInterval::decode_cbor(dec)?,
                11 => params.tau = UnitInterval::decode_cbor(dec)?,
                14 => {
                    if dec.peek_major()? == 4 {
                        let len = dec.array()?;
                        if len != 2 {
                            return Err(LedgerError::CborInvalidLength {
                                expected: 2,
                                actual: len as usize,
                            });
                        }
                        params.protocol_version = Some((dec.unsigned()?, dec.unsigned()?));
                    } else {
                        // Backward compatibility for checkpoints written before
                        // protocol_version was added, when min_utxo_value was
                        // encoded at key 14.
                        params.min_utxo_value = Some(dec.unsigned()?);
                    }
                }
                15 => params.min_utxo_value = Some(dec.unsigned()?),
                16 => params.min_pool_cost = dec.unsigned()?,
                17 => params.coins_per_utxo_byte = Some(dec.unsigned()?),
                18 => params.price_mem = Some(UnitInterval::decode_cbor(dec)?),
                19 => params.price_step = Some(UnitInterval::decode_cbor(dec)?),
                20 => params.max_tx_ex_units = Some(ExUnits::decode_cbor(dec)?),
                21 => params.max_block_ex_units = Some(ExUnits::decode_cbor(dec)?),
                22 => params.max_val_size = Some(dec.unsigned()? as u32),
                23 => params.collateral_percentage = Some(dec.unsigned()?),
                24 => params.max_collateral_inputs = Some(dec.unsigned()? as u32),
                25 => params.pool_voting_thresholds = Some(PoolVotingThresholds::decode_cbor(dec)?),
                26 => params.drep_voting_thresholds = Some(DRepVotingThresholds::decode_cbor(dec)?),
                27 => params.min_committee_size = Some(dec.unsigned()?),
                28 => params.committee_term_limit = Some(dec.unsigned()?),
                29 => params.gov_action_lifetime = Some(dec.unsigned()?),
                30 => params.gov_action_deposit = Some(dec.unsigned()?),
                31 => params.drep_deposit = Some(dec.unsigned()?),
                32 => params.drep_activity = Some(dec.unsigned()?),
                _ => {
                    // Skip unknown keys: consume one value.
                    dec.skip()?;
                }
            }
        }

        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shelley_params_round_trip() {
        let params = ProtocolParameters::default();
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    #[test]
    fn alonzo_params_round_trip() {
        let params = ProtocolParameters::alonzo_defaults();
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    #[test]
    fn conway_gov_action_deposit_round_trip() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.gov_action_deposit = Some(100_000_000_000);
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    #[test]
    fn conway_gov_action_lifetime_round_trip() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.gov_action_lifetime = Some(6);
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    #[test]
    fn protocol_version_round_trip() {
        let mut params = ProtocolParameters::default();
        params.protocol_version = Some((9, 0));
        let bytes = params.to_cbor_bytes();
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
        assert_eq!(params, decoded);
    }

    #[test]
    fn decode_legacy_min_utxo_value_at_key_14() {
        let bytes = vec![0xA1, 0x0E, 0x1A, 0x00, 0x0F, 0x42, 0x40];
        let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("decode legacy");
        assert_eq!(decoded.min_utxo_value, Some(1_000_000));
        assert_eq!(decoded.protocol_version, None);
    }

    #[test]
    fn min_lovelace_shelley() {
        let params = ProtocolParameters::default();
        assert_eq!(params.min_lovelace_for_utxo(100), Some(1_000_000));
    }

    #[test]
    fn min_lovelace_alonzo() {
        let params = ProtocolParameters::alonzo_defaults();
        // 4310 * (100 + 160) = 1_120_600
        assert_eq!(params.min_lovelace_for_utxo(100), Some(1_120_600));
    }
}
