//! Transaction construction for the tx-generator runtime.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Tx.hs`.
//! Ports `sourceToStoreTransaction`, `sourceToStoreTransactionNew`,
//! `sourceTransactionPreview`, `genTx`, and `txSizeInBytes` for the
//! pure-Rust generator surface.

use std::collections::{BTreeMap, BTreeSet};

use crate::generator_tx::sized_metadata::TxMetadata;
use crate::script::types::SigningKeyEnvelope;
use crate::tx_generator::fund::{
    Fund, FundWitness, ScriptWitnessForSpending, get_fund_coin, get_fund_key, get_fund_tx_in,
};
use crate::tx_generator::utils::mk_tx_in;
use crate::tx_generator::utxo::ToUtxoList;
use crate::types::{AnyCardanoEra, Lovelace, TxGenError};
use yggdrasil_crypto::SigningKey;
use yggdrasil_ledger::{
    AllegraTxBody, AlonzoCompatibleSubmittedTx, AlonzoTxBody, BabbageTxBody, CborEncode,
    ConwayTxBody, ExUnits, MaryTxBody, MultiEraSubmittedTx, MultiEraTxOut, Redeemer,
    ShelleyCompatibleSubmittedTx, ShelleyTxBody, ShelleyTxIn, ShelleyVkeyWitness,
    ShelleyWitnessSet, TxId, compute_tx_id,
};

/// Result produced by upstream `TxGenerator era`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedTx {
    /// Typed submitted transaction spanning the Shelley-based eras.
    pub tx: MultiEraSubmittedTx,
    /// Transaction id derived from the serialized body bytes.
    pub tx_id: TxId,
}

impl GeneratedTx {
    pub(crate) fn new(tx: MultiEraSubmittedTx) -> Self {
        let tx_id = tx.tx_id();
        Self { tx, tx_id }
    }
}

/// Mirror of upstream `sourceToStoreTransaction`.
pub fn source_to_store_transaction<FundSource, InToOut, MkTxOut, Store>(
    tx_generator: impl FnOnce(&[Fund], &[MultiEraTxOut]) -> Result<GeneratedTx, TxGenError>,
    fund_source: FundSource,
    in_to_out: InToOut,
    mk_tx_out: MkTxOut,
    fund_to_store: Store,
) -> Result<GeneratedTx, TxGenError>
where
    FundSource: FnOnce() -> Result<Vec<Fund>, TxGenError>,
    InToOut: FnOnce(&[Lovelace]) -> Result<Vec<Lovelace>, TxGenError>,
    MkTxOut: FnOnce(&[Lovelace]) -> Result<ToUtxoList, TxGenError>,
    Store: FnOnce(Vec<Fund>) -> Result<(), TxGenError>,
{
    let input_funds = fund_source()?;
    let input_coins = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
    let out_values = in_to_out(&input_coins)?;
    let to_utxo_list = mk_tx_out(&out_values)?;
    let generated = tx_generator(&input_funds, &to_utxo_list.outputs)?;
    store(generated, to_utxo_list, fund_to_store)
}

/// Mirror of upstream `sourceToStoreTransactionNew`.
pub fn source_to_store_transaction_new<FundSource, ValueSplitter, ToStore, Split, Store>(
    tx_generator: impl FnOnce(&[Fund], &[MultiEraTxOut]) -> Result<GeneratedTx, TxGenError>,
    fund_source: FundSource,
    value_splitter: ValueSplitter,
    to_store: ToStore,
    fund_to_store: Store,
) -> Result<GeneratedTx, TxGenError>
where
    FundSource: FnOnce() -> Result<Vec<Fund>, TxGenError>,
    ValueSplitter: FnOnce(&[Lovelace]) -> Result<Split, TxGenError>,
    ToStore: FnOnce(Split) -> Result<ToUtxoList, TxGenError>,
    Store: FnOnce(Vec<Fund>) -> Result<(), TxGenError>,
{
    let input_funds = fund_source()?;
    let input_coins = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
    let split = value_splitter(&input_coins)?;
    let to_utxo_list = to_store(split)?;
    let generated = tx_generator(&input_funds, &to_utxo_list.outputs)?;
    store(generated, to_utxo_list, fund_to_store)
}

/// Mirror of upstream `sourceTransactionPreview`.
pub fn source_transaction_preview<ValueSplitter, ToStore, Split>(
    tx_generator: impl FnOnce(&[Fund], &[MultiEraTxOut]) -> Result<GeneratedTx, TxGenError>,
    input_funds: &[Fund],
    value_splitter: ValueSplitter,
    to_store: ToStore,
) -> Result<GeneratedTx, TxGenError>
where
    ValueSplitter: FnOnce(&[Lovelace]) -> Result<Split, TxGenError>,
    ToStore: FnOnce(Split) -> Result<ToUtxoList, TxGenError>,
{
    let input_coins = input_funds.iter().map(get_fund_coin).collect::<Vec<_>>();
    let split = value_splitter(&input_coins)?;
    let to_utxo_list = to_store(split)?;
    tx_generator(input_funds, &to_utxo_list.outputs)
}

fn store(
    generated: GeneratedTx,
    to_utxo_list: ToUtxoList,
    fund_to_store: impl FnOnce(Vec<Fund>) -> Result<(), TxGenError>,
) -> Result<GeneratedTx, TxGenError> {
    let tx_id_hex = hex::encode(generated.tx_id.0);
    fund_to_store(to_utxo_list.funds_for_tx_id(&tx_id_hex))?;
    Ok(generated)
}

/// Mirror of upstream `genTx`.
///
/// This slice constructs and signs Shelley-family transactions for key
/// witnesses. Spending Plutus-script funds remains on an explicit boundary
/// until the script-integrity hash and pre-execution slices land.
pub fn gen_tx(
    era: AnyCardanoEra,
    signing_keys: &BTreeMap<String, SigningKeyEnvelope>,
    collateral_funds: &[Fund],
    fee: Lovelace,
    metadata: Option<&TxMetadata>,
    in_funds: &[Fund],
    outputs: &[MultiEraTxOut],
) -> Result<GeneratedTx, TxGenError> {
    if matches!(era, AnyCardanoEra::Byron | AnyCardanoEra::Dijkstra) {
        return Err(TxGenError::ApiError(format!(
            "genTx: unsupported ShelleyBasedEra {era:?}"
        )));
    }
    reject_script_inputs(in_funds)?;
    reject_script_inputs(collateral_funds)?;

    let inputs = in_funds
        .iter()
        .map(|fund| mk_tx_in(get_fund_tx_in(fund)).map_err(TxGenError::ApiError))
        .collect::<Result<Vec<_>, _>>()?;
    let collateral = collateral_funds
        .iter()
        .map(|fund| mk_tx_in(get_fund_tx_in(fund)).map_err(TxGenError::ApiError))
        .collect::<Result<Vec<_>, _>>()?;
    let auxiliary_data = metadata.map(TxMetadata::to_cbor_bytes);
    let auxiliary_data_hash = auxiliary_data
        .as_ref()
        .map(|bytes| yggdrasil_crypto::hash_bytes_256(bytes).0);

    match era {
        AnyCardanoEra::Shelley => {
            let body = ShelleyTxBody {
                inputs,
                outputs: shelley_outputs(outputs)?,
                fee,
                ttl: u64::MAX,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Shelley(tx)))
        }
        AnyCardanoEra::Allegra => {
            let body = AllegraTxBody {
                inputs,
                outputs: shelley_outputs(outputs)?,
                fee,
                ttl: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash,
                validity_interval_start: None,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Allegra(tx)))
        }
        AnyCardanoEra::Mary => {
            let body = MaryTxBody {
                inputs,
                outputs: mary_outputs(outputs)?,
                fee,
                ttl: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash,
                validity_interval_start: None,
                mint: None,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Mary(tx)))
        }
        AnyCardanoEra::Alonzo => {
            let body = AlonzoTxBody {
                inputs,
                outputs: alonzo_outputs(outputs)?,
                fee,
                ttl: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(collateral),
                required_signers: None,
                network_id: None,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Alonzo(tx)))
        }
        AnyCardanoEra::Babbage => {
            let body = BabbageTxBody {
                inputs,
                outputs: babbage_outputs(outputs)?,
                fee,
                ttl: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(collateral),
                required_signers: None,
                network_id: None,
                collateral_return: None,
                total_collateral: None,
                reference_inputs: None,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Babbage(tx)))
        }
        AnyCardanoEra::Conway => {
            let body = ConwayTxBody {
                inputs,
                outputs: babbage_outputs(outputs)?,
                fee,
                ttl: None,
                certificates: None,
                withdrawals: None,
                auxiliary_data_hash,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(collateral),
                required_signers: None,
                network_id: None,
                collateral_return: None,
                total_collateral: None,
                reference_inputs: None,
                voting_procedures: None,
                proposal_procedures: None,
                current_treasury_value: None,
                treasury_donation: None,
            };
            let witness_set = make_witness_set(
                &body.to_cbor_bytes(),
                signing_keys,
                in_funds,
                collateral_funds,
            )?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, auxiliary_data);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Conway(tx)))
        }
        AnyCardanoEra::Byron | AnyCardanoEra::Dijkstra => unreachable!("checked above"),
    }
}

/// Mirror of upstream `txSizeInBytes`.
pub fn tx_size_in_bytes(tx: &GeneratedTx) -> usize {
    tx.tx.raw_cbor().len()
}

fn reject_script_inputs(funds: &[Fund]) -> Result<(), TxGenError> {
    for fund in funds {
        if matches!(fund.witness, FundWitness::ScriptWitness(_)) {
            return Err(TxGenError::PlutusError(
                "genTx: script witnesses require script-integrity hash support".to_string(),
            ));
        }
    }
    Ok(())
}

fn make_witness_set(
    body_cbor: &[u8],
    signing_keys: &BTreeMap<String, SigningKeyEnvelope>,
    in_funds: &[Fund],
    collateral_funds: &[Fund],
) -> Result<ShelleyWitnessSet, TxGenError> {
    let tx_id = compute_tx_id(body_cbor);
    let mut seen = BTreeSet::new();
    let mut vkey_witnesses = Vec::new();
    for key_name in in_funds
        .iter()
        .chain(collateral_funds)
        .filter_map(get_fund_key)
    {
        if !seen.insert(key_name.to_string()) {
            continue;
        }
        let envelope = signing_keys.get(key_name).ok_or_else(|| {
            TxGenError::ApiError(format!("genTx: signing key `{key_name}` not loaded"))
        })?;
        vkey_witnesses.push(make_vkey_witness(envelope, &tx_id)?);
    }

    Ok(ShelleyWitnessSet {
        vkey_witnesses,
        ..empty_witness_set()
    })
}

pub(crate) fn make_vkey_witness(
    signing_key: &SigningKeyEnvelope,
    tx_id: &TxId,
) -> Result<ShelleyVkeyWitness, TxGenError> {
    let seed = signing_key_seed(signing_key)?;
    let signing_key = SigningKey::from_bytes(seed);
    let verification_key = signing_key
        .verification_key()
        .map_err(|err| TxGenError::ApiError(format!("genTx: verification key failed: {err}")))?;
    let signature = signing_key
        .sign(&tx_id.0)
        .map_err(|err| TxGenError::ApiError(format!("genTx: signing failed: {err}")))?;
    Ok(ShelleyVkeyWitness {
        vkey: verification_key.to_bytes(),
        signature: signature.to_bytes(),
    })
}

pub(crate) fn signing_key_seed(signing_key: &SigningKeyEnvelope) -> Result<[u8; 32], TxGenError> {
    signing_key
        .raw_ed25519_signing_key_seed("genTx")
        .map_err(TxGenError::ApiError)
}

pub(crate) fn shelley_outputs(
    outputs: &[MultiEraTxOut],
) -> Result<Vec<yggdrasil_ledger::ShelleyTxOut>, TxGenError> {
    outputs
        .iter()
        .map(|output| match output {
            MultiEraTxOut::Shelley(out) => Ok(out.clone()),
            other => Err(output_era_error("Shelley", other)),
        })
        .collect()
}

pub(crate) fn mary_outputs(
    outputs: &[MultiEraTxOut],
) -> Result<Vec<yggdrasil_ledger::MaryTxOut>, TxGenError> {
    outputs
        .iter()
        .map(|output| match output {
            MultiEraTxOut::Mary(out) => Ok(out.clone()),
            other => Err(output_era_error("Mary", other)),
        })
        .collect()
}

pub(crate) fn alonzo_outputs(
    outputs: &[MultiEraTxOut],
) -> Result<Vec<yggdrasil_ledger::AlonzoTxOut>, TxGenError> {
    outputs
        .iter()
        .map(|output| match output {
            MultiEraTxOut::Alonzo(out) => Ok(out.clone()),
            other => Err(output_era_error("Alonzo", other)),
        })
        .collect()
}

pub(crate) fn babbage_outputs(
    outputs: &[MultiEraTxOut],
) -> Result<Vec<yggdrasil_ledger::BabbageTxOut>, TxGenError> {
    outputs
        .iter()
        .map(|output| match output {
            MultiEraTxOut::Babbage(out) => Ok(out.clone()),
            other => Err(output_era_error("Babbage", other)),
        })
        .collect()
}

fn output_era_error(expected: &str, actual: &MultiEraTxOut) -> TxGenError {
    TxGenError::ApiError(format!(
        "genTx: expected {expected}-family output, got {actual:?}"
    ))
}

pub(crate) fn optional_inputs(inputs: Vec<ShelleyTxIn>) -> Option<Vec<ShelleyTxIn>> {
    if inputs.is_empty() {
        None
    } else {
        Some(inputs)
    }
}

pub(crate) fn empty_witness_set() -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: Vec::new(),
        native_scripts: Vec::new(),
        bootstrap_witnesses: Vec::new(),
        plutus_v1_scripts: Vec::new(),
        plutus_data: Vec::new(),
        redeemers: Vec::new(),
        plutus_v2_scripts: Vec::new(),
        plutus_v3_scripts: Vec::new(),
    }
}

#[allow(dead_code)]
fn script_redeemer(index: u64, witness: &ScriptWitnessForSpending) -> Redeemer {
    Redeemer {
        tag: 0,
        index,
        data: witness.redeemer.clone(),
        ex_units: ExUnits {
            mem: witness.execution_units.execution_memory,
            steps: witness.execution_units.execution_steps,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_generator::utxo::{make_to_utxo_list, mk_utxo_variant};
    use crate::types::PayWithChange;
    use yggdrasil_ledger::witnesses::verify_vkey_signatures;

    const INPUT_TX_ID: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::payment_signing_key_shelley(format!("5820{}", hex::encode([byte; 32])))
    }

    fn key_map() -> BTreeMap<String, SigningKeyEnvelope> {
        BTreeMap::from([("pay".to_string(), signing_key(7))])
    }

    fn input_fund() -> Fund {
        Fund::key_fund(
            AnyCardanoEra::Conway,
            format!("{INPUT_TX_ID}#0"),
            2_000_000,
            "pay",
        )
    }

    fn conway_outputs(value: Lovelace) -> ToUtxoList {
        let builder = mk_utxo_variant(
            AnyCardanoEra::Conway,
            crate::script::types::NetworkId::Testnet(42),
            "pay",
            signing_key(7),
        )
        .expect("builder");
        make_to_utxo_list(&[builder], &[value]).expect("outputs")
    }

    #[test]
    fn source_transaction_preview_does_not_store_generated_funds() {
        let input = input_fund();
        let generated = source_transaction_preview(
            |funds, outputs| {
                gen_tx(
                    AnyCardanoEra::Conway,
                    &key_map(),
                    &[],
                    10,
                    None,
                    funds,
                    outputs,
                )
            },
            std::slice::from_ref(&input),
            |coins| Ok(vec![coins[0] - 10]),
            |values| Ok(conway_outputs(values[0])),
        )
        .expect("preview");

        assert_eq!(generated.tx.inputs().len(), 1);
        assert!(tx_size_in_bytes(&generated) > generated.tx.body_cbor().len());
    }

    #[test]
    fn source_to_store_transaction_new_stores_funds_under_generated_tx_id() {
        let mut stored = Vec::new();
        let generated = source_to_store_transaction_new(
            |funds, outputs| {
                gen_tx(
                    AnyCardanoEra::Conway,
                    &key_map(),
                    &[],
                    10,
                    None,
                    funds,
                    outputs,
                )
            },
            || Ok(vec![input_fund()]),
            |coins| Ok(PayWithChange::PayExact(vec![coins[0] - 10])),
            |split| match split {
                PayWithChange::PayExact(values) => Ok(conway_outputs(values[0])),
                PayWithChange::PayWithChange(_, _) => unreachable!("test uses exact"),
            },
            |funds| {
                stored = funds;
                Ok(())
            },
        )
        .expect("generated");

        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].tx_in,
            format!("{}#0", hex::encode(generated.tx_id.0))
        );
        assert_eq!(stored[0].lovelace, 1_999_990);
    }

    #[test]
    fn gen_tx_builds_signed_conway_transaction() {
        let outputs = conway_outputs(1_000_000);
        let generated = gen_tx(
            AnyCardanoEra::Conway,
            &key_map(),
            &[],
            10,
            None,
            &[input_fund()],
            &outputs.outputs,
        )
        .expect("tx");

        let MultiEraSubmittedTx::Conway(tx) = &generated.tx else {
            panic!("expected Conway tx");
        };
        assert_eq!(tx.body.inputs.len(), 1);
        assert_eq!(tx.body.outputs.len(), 1);
        assert_eq!(tx.body.fee, 10);
        assert_eq!(tx.witness_set.vkey_witnesses.len(), 1);
        verify_vkey_signatures(&generated.tx_id.0, &tx.witness_set.vkey_witnesses)
            .expect("witness verifies");
    }

    #[test]
    fn gen_tx_rejects_script_spends_until_script_integrity_slice() {
        let witness = FundWitness::ScriptWitness(ScriptWitnessForSpending {
            language: "PlutusV2".to_string(),
            script_bytes: vec![1, 2, 3],
            datum: yggdrasil_ledger::PlutusData::integer(0),
            redeemer: yggdrasil_ledger::PlutusData::integer(1),
            execution_units: crate::types::ExecutionUnits {
                execution_steps: 1,
                execution_memory: 1,
            },
        });
        let fund = Fund::script_fund(
            AnyCardanoEra::Conway,
            format!("{INPUT_TX_ID}#0"),
            2_000_000,
            witness,
        );
        let outputs = conway_outputs(1_000_000);
        let err = gen_tx(
            AnyCardanoEra::Conway,
            &key_map(),
            &[],
            10,
            None,
            &[fund],
            &outputs.outputs,
        )
        .expect_err("script spend boundary");

        assert!(err.to_string().contains("script-integrity hash support"));
    }

    #[test]
    fn gen_tx_rejects_missing_signing_key() {
        let outputs = conway_outputs(1_000_000);
        let err = gen_tx(
            AnyCardanoEra::Conway,
            &BTreeMap::new(),
            &[],
            10,
            None,
            &[input_fund()],
            &outputs.outputs,
        )
        .expect_err("missing key");

        assert!(err.to_string().contains("signing key `pay` not loaded"));
    }
}
