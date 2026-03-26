//! Genesis configuration file loading and protocol-parameter derivation.
//!
//! The official Cardano node reads separate genesis files per era at startup:
//! - `ShelleyGenesisFile` — fee constants, staking deposits, epoch/security params.
//! - `AlonzoGenesisFile` — Plutus execution prices, ex-unit limits, collateral rules.
//! - `ConwayGenesisFile` — governance deposits, DRep activity threshold.
//!
//! This module provides typed serde representations for the fields we consume
//! and a `build_protocol_parameters` function that assembles a
//! [`ProtocolParameters`] from the loaded values so the node can seed the
//! initial ledger state with network-accurate validation rules rather than
//! hardcoded defaults.
//!
//! Reference:
//! - <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/>
//! - <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Genesis.hs>
//! - <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Genesis.hs>
//! - <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Genesis.hs>

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use yggdrasil_ledger::ProtocolParameters;
use yggdrasil_ledger::protocol_params::{DRepVotingThresholds, PoolVotingThresholds};
use yggdrasil_ledger::types::UnitInterval;
use yggdrasil_ledger::eras::alonzo::ExUnits;
use yggdrasil_plutus::{CostModel, CostModelError};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error returned while loading or parsing a genesis file.
#[derive(Debug, Error)]
pub enum GenesisLoadError {
    /// The genesis file could not be read.
    #[error("failed to read genesis file {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The genesis file contained invalid JSON.
    #[error("failed to parse genesis file {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Error returned while deriving the node's simplified CEK cost model from
/// genesis configuration.
#[derive(Debug, Error)]
pub enum GenesisCostModelError {
    /// Reading or parsing the genesis file failed.
    #[error(transparent)]
    Load(#[from] GenesisLoadError),
    /// The upstream named cost-model parameters could not be mapped onto the
    /// current simplified flat CEK cost model.
    #[error(transparent)]
    CostModel(#[from] CostModelError),
}

// ---------------------------------------------------------------------------
// Shelley genesis
// ---------------------------------------------------------------------------

/// Subset of `shelley-genesis.json` used to seed initial protocol parameters.
///
/// Unrecognised fields are ignored via `#[serde(default)]` and
/// `deny_unknown_fields` is intentionally absent so the parser is
/// forward-compatible with extended upstream formats.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyGenesis {
    /// Active slot coefficient `f` (Ouroboros Praos, mainnet: 0.05).
    #[serde(default = "default_active_slots_coeff")]
    pub active_slots_coeff: f64,

    /// Slots per epoch (mainnet Shelley: 432000).
    #[serde(default = "default_epoch_length")]
    pub epoch_length: u64,

    /// Slots per KES period (mainnet: 129600).
    #[serde(default = "default_slots_per_kes_period")]
    pub slots_per_kes_period: u64,

    /// Maximum KES evolutions (mainnet: 62).
    #[serde(default = "default_max_kes_evolutions")]
    pub max_kes_evolutions: u64,

    /// Security parameter `k` (mainnet: 2160).
    #[serde(default = "default_security_param")]
    pub security_param: u64,

    /// Initial Shelley protocol parameters embedded in the genesis file.
    #[serde(default)]
    pub protocol_params: ShelleyGenesisProtocolParams,
}

/// The `protocolParams` object from `shelley-genesis.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyGenesisProtocolParams {
    /// Linear fee coefficient (lovelace per byte of tx body).  Key 0.
    #[serde(default = "default_min_fee_a")]
    pub min_fee_a: u64,
    /// Constant fee component (lovelace).  Key 1.
    #[serde(default = "default_min_fee_b")]
    pub min_fee_b: u64,
    /// Maximum block body size in bytes.  Key 2.
    #[serde(default = "default_max_block_body_size")]
    pub max_block_body_size: u32,
    /// Maximum transaction size in bytes.  Key 3.
    #[serde(default = "default_max_tx_size")]
    pub max_tx_size: u32,
    /// Maximum block header size in bytes.  Key 4.
    #[serde(default = "default_max_block_header_size")]
    pub max_block_header_size: u16,
    /// Stake key deposit (lovelace).  Key 5.
    #[serde(default = "default_key_deposit")]
    pub key_deposit: u64,
    /// Pool registration deposit (lovelace).  Key 6.
    #[serde(default = "default_pool_deposit")]
    pub pool_deposit: u64,
    /// Maximum pool retirement epoch lag.  Key 7.
    #[serde(default = "default_e_max", rename = "eMax")]
    pub e_max: u64,
    /// Desired number of stake pools.  Key 8.
    #[serde(default = "default_n_opt", rename = "nOpt")]
    pub n_opt: u64,
    /// Pool pledge influence `a0`.  Key 9.
    #[serde(default = "default_a0", rename = "a0")]
    pub a0: GenesisRational,
    /// Monetary expansion rate `ρ`.  Key 10.
    #[serde(default = "default_rho")]
    pub rho: GenesisRational,
    /// Treasury growth rate `τ`.  Key 11.
    #[serde(default = "default_tau")]
    pub tau: GenesisRational,
    /// Active ledger protocol version.
    #[serde(default = "default_protocol_version")]
    pub protocol_version: GenesisProtocolVersion,
    /// Minimum UTxO value (lovelace, Shelley–Mary).
    #[serde(default = "default_min_utxo_value", rename = "minUTxOValue")]
    pub min_utxo_value: u64,
    /// Minimum pool operating cost (lovelace per epoch).  Key 16.
    #[serde(default = "default_min_pool_cost")]
    pub min_pool_cost: u64,
}

/// A rational number as serialised in genesis JSON.
///
/// Shelley genesis represents rationals as literal floats; Alonzo genesis
/// uses explicit `{"numerator": n, "denominator": d}` objects.
/// This type attempts to deserialise both forms.
#[derive(Clone, Debug)]
pub struct GenesisRational {
    pub numerator: u64,
    pub denominator: u64,
}

/// Protocol version object used by Shelley genesis JSON.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenesisProtocolVersion {
    pub major: u64,
    pub minor: u64,
}

impl GenesisRational {
    fn to_unit_interval(&self) -> UnitInterval {
        UnitInterval {
            numerator: self.numerator,
            denominator: self.denominator,
        }
    }
}

impl Default for GenesisRational {
    fn default() -> Self {
        Self { numerator: 0, denominator: 1 }
    }
}

impl<'de> serde::Deserialize<'de> for GenesisRational {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct Vis;
        impl<'de> Visitor<'de> for Vis {
            type Value = GenesisRational;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a rational as a float or {{numerator, denominator}} object")
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                // Convert float to a rational with denominator 1_000_000.
                let denom = 1_000_000u64;
                let numer = (v * denom as f64).round() as u64;
                Ok(GenesisRational { numerator: numer, denominator: denom })
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(GenesisRational { numerator: v as u64, denominator: 1 })
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(GenesisRational { numerator: v, denominator: 1 })
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut numerator: Option<u64> = None;
                let mut denominator: Option<u64> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "numerator" => numerator = Some(map.next_value()?),
                        "denominator" => denominator = Some(map.next_value()?),
                        _ => { map.next_value::<serde_json::Value>()?; }
                    }
                }
                let n = numerator.ok_or_else(|| serde::de::Error::missing_field("numerator"))?;
                let d = denominator.ok_or_else(|| serde::de::Error::missing_field("denominator"))?;
                Ok(GenesisRational { numerator: n, denominator: d })
            }
        }
        de.deserialize_any(Vis)
    }
}

impl Serialize for GenesisRational {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("numerator", &self.numerator)?;
        m.serialize_entry("denominator", &self.denominator)?;
        m.end()
    }
}

// ---------------------------------------------------------------------------
// Alonzo genesis
// ---------------------------------------------------------------------------

/// Subset of `alonzo-genesis.json` used to seed Alonzo+ protocol parameters.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlonzoGenesis {
    /// Lovelace per UTxO word (Alonzo). Converts to `coins_per_utxo_byte` by
    /// dividing by 8 (one word = 8 bytes).
    #[serde(default)]
    pub lovelace_per_utxo_word: Option<u64>,

    /// Execution unit prices.
    pub execution_prices: AlonzoExecPrices,

    /// Maximum execution units per transaction.
    #[serde(rename = "maxTxExUnits")]
    pub max_tx_ex_units: AlonzoExUnits,

    /// Maximum execution units per block.
    #[serde(rename = "maxBlockExUnits")]
    pub max_block_ex_units: AlonzoExUnits,

    /// Maximum serialised value size in bytes.
    #[serde(rename = "maxValueSize", default = "default_max_value_size")]
    pub max_value_size: u32,

    /// Collateral percentage (150 = 150%).
    #[serde(rename = "collateralPercentage", default = "default_collateral_percentage")]
    pub collateral_percentage: u64,

    /// Maximum collateral inputs.
    #[serde(rename = "maxCollateralInputs", default = "default_max_collateral_inputs")]
    pub max_collateral_inputs: u32,

    /// PlutusV1 and PlutusV2 cost model parameter maps (named string → integer).
    /// Keys are `"PlutusV1"` and `"PlutusV2"`.
    #[serde(rename = "costModels", default)]
    pub cost_models: BTreeMap<String, BTreeMap<String, i64>>,
}

/// `exUnitsMem` / `exUnitsSteps` object used in Alonzo genesis.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlonzoExUnits {
    #[serde(rename = "exUnitsMem")]
    pub ex_units_mem: u64,
    #[serde(rename = "exUnitsSteps")]
    pub ex_units_steps: u64,
}

/// `prMem` / `prSteps` rational pair from Alonzo genesis `executionPrices`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlonzoExecPrices {
    #[serde(rename = "prMem")]
    pub pr_mem: GenesisRational,
    #[serde(rename = "prSteps")]
    pub pr_steps: GenesisRational,
}

// ---------------------------------------------------------------------------
// Conway genesis
// ---------------------------------------------------------------------------

/// Subset of `conway-genesis.json` used to seed governance-era parameters.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConwayGenesis {
    /// Pool voting thresholds per governance action type.
    #[serde(default)]
    pub pool_voting_thresholds: Option<GenesisPoolVotingThresholds>,

    /// DRep voting thresholds per governance action type.
    #[serde(default, rename = "dRepVotingThresholds")]
    pub drep_voting_thresholds: Option<GenesisDRepVotingThresholds>,

    /// Minimum number of active committee members.
    #[serde(default)]
    pub committee_min_size: Option<u64>,

    /// Maximum term length for committee members in epochs.
    #[serde(default)]
    pub committee_max_term_length: Option<u64>,

    /// Governance action lifetime in epochs.
    #[serde(default)]
    pub gov_action_lifetime: Option<u64>,

    /// Governance action deposit (lovelace).
    #[serde(default)]
    pub gov_action_deposit: Option<u64>,

    /// DRep registration deposit (lovelace).
    #[serde(default)]
    pub d_rep_deposit: Option<u64>,

    /// Minimum DRep activity window in epochs.
    #[serde(default)]
    pub d_rep_activity: Option<u64>,

    /// Minimum reference script cost per byte (Babbage+, lovelace).
    #[serde(default)]
    pub min_fee_ref_script_cost_per_byte: Option<u64>,
}

/// Pool voting thresholds as serialised in `conway-genesis.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisPoolVotingThresholds {
    #[serde(default)]
    pub motion_no_confidence: GenesisRational,
    #[serde(default)]
    pub committee_normal: GenesisRational,
    #[serde(default)]
    pub committee_no_confidence: GenesisRational,
    #[serde(default)]
    pub hard_fork_initiation: GenesisRational,
    #[serde(default)]
    pub pp_security_group: GenesisRational,
}

/// DRep voting thresholds as serialised in `conway-genesis.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisDRepVotingThresholds {
    #[serde(default)]
    pub motion_no_confidence: GenesisRational,
    #[serde(default)]
    pub committee_normal: GenesisRational,
    #[serde(default)]
    pub committee_no_confidence: GenesisRational,
    #[serde(default)]
    pub update_to_constitution: GenesisRational,
    #[serde(default)]
    pub hard_fork_initiation: GenesisRational,
    #[serde(default)]
    pub pp_network_group: GenesisRational,
    #[serde(default)]
    pub pp_economic_group: GenesisRational,
    #[serde(default)]
    pub pp_technical_group: GenesisRational,
    #[serde(default)]
    pub pp_gov_group: GenesisRational,
    #[serde(default)]
    pub treasury_withdrawal: GenesisRational,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Assemble a [`ProtocolParameters`] from loaded genesis files.
///
/// Shelley genesis provides the foundational fee/staking/block parameters.
/// Alonzo genesis adds Plutus execution prices, collateral rules, and
/// ex-unit limits. Conway parameters can be layered on top when available.
///
/// Note: fields absent in the genesis JSON are filled from upstream mainnet
/// defaults in [`ProtocolParameters::default`].
pub fn build_protocol_parameters(
    shelley: &ShelleyGenesis,
    alonzo: &AlonzoGenesis,
    conway: Option<&ConwayGenesis>,
) -> ProtocolParameters {
    let pp = &shelley.protocol_params;

    // Derive coins_per_utxo_byte from lovelacePerUTxOWord.
    // 1 UTxO word = 8 bytes.  The Alonzo field is per word; Babbage uses per byte.
    let coins_per_utxo_byte = alonzo
        .lovelace_per_utxo_word
        .map(|v| v / 8)
        .or(Some(4_310)); // Babbage mainnet default

    ProtocolParameters {
        min_fee_a: pp.min_fee_a,
        min_fee_b: pp.min_fee_b,
        max_block_body_size: pp.max_block_body_size,
        max_tx_size: pp.max_tx_size,
        max_block_header_size: pp.max_block_header_size,
        key_deposit: pp.key_deposit,
        pool_deposit: pp.pool_deposit,
        e_max: pp.e_max,
        n_opt: pp.n_opt,
        a0: pp.a0.to_unit_interval(),
        rho: pp.rho.to_unit_interval(),
        tau: pp.tau.to_unit_interval(),
        protocol_version: Some((pp.protocol_version.major, pp.protocol_version.minor)),
        min_utxo_value: Some(pp.min_utxo_value),
        min_pool_cost: pp.min_pool_cost,
        // Alonzo and later: clear Shelley min_utxo_value, use coins_per_utxo_byte.
        // Both are stored here; validation helpers use whichever is relevant per era.
        coins_per_utxo_byte,
        price_mem: Some(alonzo.execution_prices.pr_mem.to_unit_interval()),
        price_step: Some(alonzo.execution_prices.pr_steps.to_unit_interval()),
        max_tx_ex_units: Some(ExUnits {
            mem: alonzo.max_tx_ex_units.ex_units_mem,
            steps: alonzo.max_tx_ex_units.ex_units_steps,
        }),
        max_block_ex_units: Some(ExUnits {
            mem: alonzo.max_block_ex_units.ex_units_mem,
            steps: alonzo.max_block_ex_units.ex_units_steps,
        }),
        max_val_size: Some(alonzo.max_value_size),
        collateral_percentage: Some(alonzo.collateral_percentage),
        max_collateral_inputs: Some(alonzo.max_collateral_inputs),
        gov_action_lifetime: conway.and_then(|params| params.gov_action_lifetime),
        gov_action_deposit: conway.and_then(|params| params.gov_action_deposit),
        drep_deposit: conway.and_then(|params| params.d_rep_deposit),
        drep_activity: conway.and_then(|params| params.d_rep_activity),
        pool_voting_thresholds: conway
            .and_then(|c| c.pool_voting_thresholds.as_ref())
            .map(|t| PoolVotingThresholds {
                motion_no_confidence: t.motion_no_confidence.to_unit_interval(),
                committee_normal: t.committee_normal.to_unit_interval(),
                committee_no_confidence: t.committee_no_confidence.to_unit_interval(),
                hard_fork_initiation: t.hard_fork_initiation.to_unit_interval(),
                pp_security_group: t.pp_security_group.to_unit_interval(),
            }),
        drep_voting_thresholds: conway
            .and_then(|c| c.drep_voting_thresholds.as_ref())
            .map(|t| DRepVotingThresholds {
                motion_no_confidence: t.motion_no_confidence.to_unit_interval(),
                committee_normal: t.committee_normal.to_unit_interval(),
                committee_no_confidence: t.committee_no_confidence.to_unit_interval(),
                update_to_constitution: t.update_to_constitution.to_unit_interval(),
                hard_fork_initiation: t.hard_fork_initiation.to_unit_interval(),
                pp_network_group: t.pp_network_group.to_unit_interval(),
                pp_economic_group: t.pp_economic_group.to_unit_interval(),
                pp_technical_group: t.pp_technical_group.to_unit_interval(),
                pp_gov_group: t.pp_gov_group.to_unit_interval(),
                treasury_withdrawal: t.treasury_withdrawal.to_unit_interval(),
            }),
        min_committee_size: conway.and_then(|c| c.committee_min_size),
        committee_term_limit: conway.and_then(|c| c.committee_max_term_length),
    }
}

/// Build the current simplified CEK [`CostModel`] from the Alonzo genesis
/// named Plutus cost-model map.
///
/// The node currently uses a single flat CEK model for all script versions,
/// so only named maps that expose the shared CEK structural costs can be
/// consumed. We prefer `PlutusV1` because the vendored network genesis files
/// expose it with stable upstream key names.
pub fn build_plutus_cost_model(
    alonzo: &AlonzoGenesis,
) -> Result<Option<CostModel>, CostModelError> {
    let named_params = alonzo
        .cost_models
        .get("PlutusV1")
        .or_else(|| alonzo.cost_models.get("PlutusV2"));

    match named_params {
        Some(params) => Ok(Some(CostModel::from_alonzo_genesis_params(params)?)),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Loader helpers
// ---------------------------------------------------------------------------

/// Load and deserialise a JSON genesis file.
fn load_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, GenesisLoadError> {
    let contents = fs::read_to_string(path).map_err(|source| GenesisLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&contents).map_err(|source| GenesisLoadError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Load `shelley-genesis.json` from the given path.
pub fn load_shelley_genesis(path: &Path) -> Result<ShelleyGenesis, GenesisLoadError> {
    load_json(path)
}

/// Load `alonzo-genesis.json` from the given path.
pub fn load_alonzo_genesis(path: &Path) -> Result<AlonzoGenesis, GenesisLoadError> {
    load_json(path)
}

/// Load `conway-genesis.json` from the given path.
pub fn load_conway_genesis(path: &Path) -> Result<ConwayGenesis, GenesisLoadError> {
    load_json(path)
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

fn default_active_slots_coeff() -> f64 { 0.05 }
fn default_epoch_length() -> u64 { 432_000 }
fn default_slots_per_kes_period() -> u64 { 129_600 }
fn default_max_kes_evolutions() -> u64 { 62 }
fn default_security_param() -> u64 { 2_160 }
fn default_min_fee_a() -> u64 { 44 }
fn default_min_fee_b() -> u64 { 155_381 }
fn default_max_block_body_size() -> u32 { 65_536 }
fn default_max_tx_size() -> u32 { 16_384 }
fn default_max_block_header_size() -> u16 { 1_100 }
fn default_key_deposit() -> u64 { 2_000_000 }
fn default_pool_deposit() -> u64 { 500_000_000 }
fn default_e_max() -> u64 { 18 }
fn default_n_opt() -> u64 { 150 }
fn default_a0() -> GenesisRational { GenesisRational { numerator: 3, denominator: 10 } }
fn default_rho() -> GenesisRational { GenesisRational { numerator: 3, denominator: 1_000 } }
fn default_tau() -> GenesisRational { GenesisRational { numerator: 2, denominator: 10 } }
fn default_protocol_version() -> GenesisProtocolVersion { GenesisProtocolVersion { major: 2, minor: 0 } }
fn default_min_utxo_value() -> u64 { 1_000_000 }
fn default_min_pool_cost() -> u64 { 340_000_000 }
fn default_max_value_size() -> u32 { 5_000 }
fn default_collateral_percentage() -> u64 { 150 }
fn default_max_collateral_inputs() -> u32 { 3 }

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_shelley() -> ShelleyGenesis {
        ShelleyGenesis {
            active_slots_coeff: 0.05,
            epoch_length: 432_000,
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            security_param: 2_160,
            protocol_params: ShelleyGenesisProtocolParams {
                min_fee_a: 44,
                min_fee_b: 155_381,
                max_block_body_size: 65_536,
                max_tx_size: 16_384,
                max_block_header_size: 1_100,
                key_deposit: 2_000_000,
                pool_deposit: 500_000_000,
                e_max: 18,
                n_opt: 150,
                a0: GenesisRational { numerator: 3, denominator: 10 },
                rho: GenesisRational { numerator: 3, denominator: 1_000 },
                tau: GenesisRational { numerator: 2, denominator: 10 },
                protocol_version: GenesisProtocolVersion { major: 2, minor: 0 },
                min_utxo_value: 1_000_000,
                min_pool_cost: 340_000_000,
            },
        }
    }

    fn sample_alonzo() -> AlonzoGenesis {
        let mut cost_models = BTreeMap::new();
        cost_models.insert(
            "PlutusV1".to_owned(),
            BTreeMap::from([
                ("cekVarCost-exBudgetCPU".to_owned(), 29_773),
                ("cekConstCost-exBudgetCPU".to_owned(), 29_773),
                ("cekLamCost-exBudgetCPU".to_owned(), 29_773),
                ("cekDelayCost-exBudgetCPU".to_owned(), 29_773),
                ("cekForceCost-exBudgetCPU".to_owned(), 29_773),
                ("cekApplyCost-exBudgetCPU".to_owned(), 29_773),
                ("cekVarCost-exBudgetMemory".to_owned(), 100),
                ("cekConstCost-exBudgetMemory".to_owned(), 100),
                ("cekLamCost-exBudgetMemory".to_owned(), 100),
                ("cekDelayCost-exBudgetMemory".to_owned(), 100),
                ("cekForceCost-exBudgetMemory".to_owned(), 100),
                ("cekApplyCost-exBudgetMemory".to_owned(), 100),
                ("cekBuiltinCost-exBudgetCPU".to_owned(), 29_773),
                ("cekBuiltinCost-exBudgetMemory".to_owned(), 100),
            ]),
        );

        AlonzoGenesis {
            lovelace_per_utxo_word: Some(34_482),
            execution_prices: AlonzoExecPrices {
                pr_mem: GenesisRational { numerator: 577, denominator: 10_000 },
                pr_steps: GenesisRational { numerator: 721, denominator: 10_000_000 },
            },
            max_tx_ex_units: AlonzoExUnits {
                ex_units_mem: 10_000_000,
                ex_units_steps: 10_000_000_000,
            },
            max_block_ex_units: AlonzoExUnits {
                ex_units_mem: 50_000_000,
                ex_units_steps: 40_000_000_000,
            },
            max_value_size: 5_000,
            collateral_percentage: 150,
            max_collateral_inputs: 3,
            cost_models,
        }
    }

    fn sample_conway() -> ConwayGenesis {
        ConwayGenesis {
            pool_voting_thresholds: Some(GenesisPoolVotingThresholds {
                motion_no_confidence: GenesisRational { numerator: 510_000, denominator: 1_000_000 },
                committee_normal: GenesisRational { numerator: 510_000, denominator: 1_000_000 },
                committee_no_confidence: GenesisRational { numerator: 510_000, denominator: 1_000_000 },
                hard_fork_initiation: GenesisRational { numerator: 510_000, denominator: 1_000_000 },
                pp_security_group: GenesisRational { numerator: 510_000, denominator: 1_000_000 },
            }),
            drep_voting_thresholds: Some(GenesisDRepVotingThresholds {
                motion_no_confidence: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
                committee_normal: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
                committee_no_confidence: GenesisRational { numerator: 600_000, denominator: 1_000_000 },
                update_to_constitution: GenesisRational { numerator: 750_000, denominator: 1_000_000 },
                hard_fork_initiation: GenesisRational { numerator: 600_000, denominator: 1_000_000 },
                pp_network_group: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
                pp_economic_group: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
                pp_technical_group: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
                pp_gov_group: GenesisRational { numerator: 750_000, denominator: 1_000_000 },
                treasury_withdrawal: GenesisRational { numerator: 670_000, denominator: 1_000_000 },
            }),
            committee_min_size: Some(7),
            committee_max_term_length: Some(146),
            gov_action_lifetime: Some(6),
            gov_action_deposit: Some(100_000_000_000),
            d_rep_deposit: Some(500_000_000),
            d_rep_activity: Some(20),
            min_fee_ref_script_cost_per_byte: Some(15),
        }
    }

    #[test]
    fn build_protocol_parameters_shelley_fields() {
        let shelley = sample_shelley();
        let alonzo = sample_alonzo();
        let params = build_protocol_parameters(&shelley, &alonzo, None);

        assert_eq!(params.min_fee_a, 44);
        assert_eq!(params.min_fee_b, 155_381);
        assert_eq!(params.key_deposit, 2_000_000);
        assert_eq!(params.pool_deposit, 500_000_000);
        assert_eq!(params.max_tx_size, 16_384);
        assert_eq!(params.max_block_body_size, 65_536);
        assert_eq!(params.e_max, 18);
        assert_eq!(params.n_opt, 150);
        assert_eq!(params.protocol_version, Some((2, 0)));
    }

    #[test]
    fn build_protocol_parameters_alonzo_fields() {
        let shelley = sample_shelley();
        let alonzo = sample_alonzo();
        let params = build_protocol_parameters(&shelley, &alonzo, None);

        // lovelacePerUTxOWord = 34482 → coins_per_utxo_byte = 34482 / 8 = 4310
        assert_eq!(params.coins_per_utxo_byte, Some(4_310));

        let price_mem = params.price_mem.unwrap();
        assert_eq!(price_mem.numerator, 577);
        assert_eq!(price_mem.denominator, 10_000);

        let price_step = params.price_step.unwrap();
        assert_eq!(price_step.numerator, 721);
        assert_eq!(price_step.denominator, 10_000_000);

        let max_tx = params.max_tx_ex_units.unwrap();
        assert_eq!(max_tx.mem, 10_000_000);
        assert_eq!(max_tx.steps, 10_000_000_000);

        assert_eq!(params.collateral_percentage, Some(150));
        assert_eq!(params.max_collateral_inputs, Some(3));
        assert_eq!(params.max_val_size, Some(5_000));
    }

    #[test]
    fn build_protocol_parameters_conway_fields() {
        let shelley = sample_shelley();
        let alonzo = sample_alonzo();
        let conway = sample_conway();
        let params = build_protocol_parameters(&shelley, &alonzo, Some(&conway));

        assert_eq!(params.gov_action_lifetime, Some(6));
        assert_eq!(params.gov_action_deposit, Some(100_000_000_000));
        assert_eq!(params.min_committee_size, Some(7));
        assert_eq!(params.committee_term_limit, Some(146));

        // Pool voting thresholds.
        let pvt = params.pool_voting_thresholds.as_ref().expect("pool_voting_thresholds");
        assert_eq!(pvt.motion_no_confidence.numerator, 510_000);
        assert_eq!(pvt.motion_no_confidence.denominator, 1_000_000);
        assert_eq!(pvt.pp_security_group.numerator, 510_000);

        // DRep voting thresholds.
        let dvt = params.drep_voting_thresholds.as_ref().expect("drep_voting_thresholds");
        assert_eq!(dvt.motion_no_confidence.numerator, 670_000);
        assert_eq!(dvt.update_to_constitution.numerator, 750_000);
        assert_eq!(dvt.treasury_withdrawal.numerator, 670_000);
    }

    #[test]
    fn build_plutus_cost_model_from_alonzo_named_params() {
        let alonzo = sample_alonzo();
        let model = build_plutus_cost_model(&alonzo)
            .expect("build cost model")
            .expect("plutus v1 cost model");
        // Verify machine step costs via the public API.
        let step = model.machine_step_cost();
        assert_eq!(step.cpu, 29_773);
        assert_eq!(step.mem, 100);
        // The sample fixture only provides CEK step cost keys; no per-builtin
        // params are present, so builtins fall back to the default cost.
        use yggdrasil_plutus::types::{Constant, DefaultFun, Value};
        let args = [Value::Constant(Constant::ByteString(vec![]))];
        let bc = model.builtin_cost(DefaultFun::Sha2_256, &args);
        assert_eq!(bc.cpu, 29_773); // fallback to default builtin cost
    }

    #[test]
    fn genesis_rational_deserialises_from_map() {
        let json = r#"{"numerator": 577, "denominator": 10000}"#;
        let r: GenesisRational = serde_json::from_str(json).unwrap();
        assert_eq!(r.numerator, 577);
        assert_eq!(r.denominator, 10_000);
    }

    #[test]
    fn genesis_rational_deserialises_from_float() {
        // Shelley genesis uses raw floats for rho/tau/a0.
        let json = "0.05";
        let r: GenesisRational = serde_json::from_str(json).unwrap();
        // 0.05 * 1_000_000 = 50_000
        assert_eq!(r.numerator, 50_000);
        assert_eq!(r.denominator, 1_000_000);
    }

    #[test]
    fn shelley_genesis_json_round_trip() {
        let shelley = sample_shelley();
        let json = serde_json::to_string(&shelley).unwrap();
        let parsed: ShelleyGenesis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.active_slots_coeff, shelley.active_slots_coeff);
        assert_eq!(parsed.security_param, shelley.security_param);
        assert_eq!(parsed.protocol_params.min_fee_a, shelley.protocol_params.min_fee_a);
    }

    #[test]
    fn parse_real_mainnet_shelley_genesis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("configuration/mainnet/shelley-genesis.json");
        if !path.exists() {
            return; // skip if not present in CI
        }
        let genesis = load_shelley_genesis(&path).expect("load shelley genesis");
        assert_eq!(genesis.protocol_params.min_fee_a, 44);
        assert_eq!(genesis.protocol_params.key_deposit, 2_000_000);
        assert_eq!(genesis.protocol_params.protocol_version.major, 2);
        assert_eq!(genesis.security_param, 2_160);
    }

    #[test]
    fn parse_real_mainnet_alonzo_genesis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("configuration/mainnet/alonzo-genesis.json");
        if !path.exists() {
            return; // skip if not present in CI
        }
        let genesis = load_alonzo_genesis(&path).expect("load alonzo genesis");
        assert_eq!(genesis.collateral_percentage, 150);
        assert_eq!(genesis.max_collateral_inputs, 3);
        assert_eq!(genesis.max_tx_ex_units.ex_units_mem, 10_000_000);
    }

    #[test]
    fn parse_real_mainnet_conway_genesis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("configuration/mainnet/conway-genesis.json");
        if !path.exists() {
            return; // skip if not present in CI
        }
        let genesis = load_conway_genesis(&path).expect("load conway genesis");
        assert_eq!(genesis.gov_action_deposit, Some(100_000_000_000));
        assert_eq!(genesis.d_rep_deposit, Some(500_000_000));
        assert_eq!(genesis.committee_min_size, Some(7));
        assert_eq!(genesis.committee_max_term_length, Some(146));
        // Verify voting thresholds parsed successfully.
        let pvt = genesis.pool_voting_thresholds.as_ref().expect("poolVotingThresholds");
        assert!(pvt.motion_no_confidence.numerator > 0);
        let dvt = genesis.drep_voting_thresholds.as_ref().expect("dRepVotingThresholds");
        assert!(dvt.motion_no_confidence.numerator > 0);
        assert!(dvt.update_to_constitution.numerator > 0);
    }
}
