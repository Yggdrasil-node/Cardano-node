//! Shelley genesis initial-fund spending for tx-generator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Genesis.hs`.
//! Ports `genesisSecureInitialFund`, `genesisInitialFundForKey`,
//! `genesisExpenditure`, and `mkGenesisTransaction` for the pure-Rust
//! transaction-generator runtime.

use crate::script::env::GenesisInitialFund;
use crate::script::types::{NetworkId, SigningKeyEnvelope};
use crate::tx_generator::fund::Fund;
use crate::tx_generator::tx::{
    GeneratedTx, alonzo_outputs, babbage_outputs, empty_witness_set, make_vkey_witness,
    mary_outputs, optional_inputs, shelley_outputs,
};
use crate::tx_generator::utils::mk_tx_in;
use crate::tx_generator::utxo::{key_address, mk_utxo_variant};
use crate::types::{AnyCardanoEra, Lovelace, TxGenError, TxGenTxParams};
use yggdrasil_ledger::{
    AllegraTxBody, AlonzoCompatibleSubmittedTx, AlonzoTxBody, BabbageTxBody, CborEncode,
    ConwayTxBody, MaryTxBody, MultiEraSubmittedTx, MultiEraTxOut, ShelleyCompatibleSubmittedTx,
    ShelleyTxBody, ShelleyTxIn, ShelleyWitnessSet, TxId, compute_tx_id,
};

/// Mirror of upstream `genesisSecureInitialFund`.
pub fn genesis_secure_initial_fund(
    era: AnyCardanoEra,
    network_id: &NetworkId,
    genesis: &[GenesisInitialFund],
    src_key: &SigningKeyEnvelope,
    dest_key_name: &str,
    dest_key: &SigningKeyEnvelope,
    tx_params: &TxGenTxParams,
) -> Result<(GeneratedTx, Fund), TxGenError> {
    if matches!(era, AnyCardanoEra::Byron | AnyCardanoEra::Dijkstra) {
        return Err(TxGenError::ApiError(format!(
            "genesisSecureInitialFund: unsupported ShelleyBasedEra {era:?}"
        )));
    }

    let initial_fund =
        genesis_initial_fund_for_key(network_id, genesis, src_key)?.ok_or_else(|| {
            TxGenError::TxGenError(
                "genesisSecureInitialFund: no fund found for given key in genesis".to_string(),
            )
        })?;
    let value = initial_fund
        .lovelace
        .checked_sub(tx_params.tx_param_fee)
        .ok_or_else(|| {
            TxGenError::TxGenError(format!(
                "genesisSecureInitialFund: insufficient funds, input={}, fee={}",
                initial_fund.lovelace, tx_params.tx_param_fee
            ))
        })?;
    let output_builder = mk_utxo_variant(
        era,
        network_id.clone(),
        dest_key_name.to_string(),
        dest_key.clone(),
    )
    .map_err(TxGenError::ApiError)?;
    let (output, pending_fund) = output_builder.build(value).map_err(TxGenError::ApiError)?;
    let generated = genesis_expenditure(
        era,
        src_key,
        &initial_fund.tx_in,
        output,
        tx_params.tx_param_fee,
        tx_params.tx_param_ttl,
    )?;
    let fund = pending_fund.fund_for_tx_id(0, &hex::encode(generated.tx_id.0));

    Ok((generated, fund))
}

/// Mirror of upstream `genesisInitialFundForKey`.
pub fn genesis_initial_fund_for_key<'a>(
    network_id: &NetworkId,
    genesis: &'a [GenesisInitialFund],
    key: &SigningKeyEnvelope,
) -> Result<Option<&'a GenesisInitialFund>, TxGenError> {
    let address = key_address(network_id, key).map_err(TxGenError::ApiError)?;
    Ok(genesis.iter().find(|fund| fund.address == address))
}

/// Mirror of upstream `genesisExpenditure`.
pub fn genesis_expenditure(
    era: AnyCardanoEra,
    input_key: &SigningKeyEnvelope,
    pseudo_tx_in: &str,
    output: MultiEraTxOut,
    fee: Lovelace,
    ttl: u64,
) -> Result<GeneratedTx, TxGenError> {
    let input = mk_tx_in(pseudo_tx_in).map_err(TxGenError::ApiError)?;
    mk_genesis_transaction(era, input_key, ttl, fee, vec![input], vec![output])
}

/// Mirror of upstream `mkGenesisTransaction`.
pub fn mk_genesis_transaction(
    era: AnyCardanoEra,
    key: &SigningKeyEnvelope,
    ttl: u64,
    fee: Lovelace,
    inputs: Vec<ShelleyTxIn>,
    outputs: Vec<MultiEraTxOut>,
) -> Result<GeneratedTx, TxGenError> {
    match era {
        AnyCardanoEra::Shelley => {
            let body = ShelleyTxBody {
                inputs,
                outputs: shelley_outputs(&outputs)?,
                fee,
                ttl,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
            };
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Shelley(tx)))
        }
        AnyCardanoEra::Allegra => {
            let body = AllegraTxBody {
                inputs,
                outputs: shelley_outputs(&outputs)?,
                fee,
                ttl: Some(ttl),
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                validity_interval_start: None,
            };
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Allegra(tx)))
        }
        AnyCardanoEra::Mary => {
            let body = MaryTxBody {
                inputs,
                outputs: mary_outputs(&outputs)?,
                fee,
                ttl: Some(ttl),
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                validity_interval_start: None,
                mint: None,
            };
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = ShelleyCompatibleSubmittedTx::new(body, witness_set, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Mary(tx)))
        }
        AnyCardanoEra::Alonzo => {
            let body = AlonzoTxBody {
                inputs,
                outputs: alonzo_outputs(&outputs)?,
                fee,
                ttl: Some(ttl),
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(Vec::new()),
                required_signers: None,
                network_id: None,
            };
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Alonzo(tx)))
        }
        AnyCardanoEra::Babbage => {
            let body = BabbageTxBody {
                inputs,
                outputs: babbage_outputs(&outputs)?,
                fee,
                ttl: Some(ttl),
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(Vec::new()),
                required_signers: None,
                network_id: None,
                collateral_return: None,
                total_collateral: None,
                reference_inputs: None,
            };
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Babbage(tx)))
        }
        AnyCardanoEra::Conway => {
            let body = ConwayTxBody {
                inputs,
                outputs: babbage_outputs(&outputs)?,
                fee,
                ttl: Some(ttl),
                certificates: None,
                withdrawals: None,
                auxiliary_data_hash: None,
                validity_interval_start: None,
                mint: None,
                script_data_hash: None,
                collateral: optional_inputs(Vec::new()),
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
            let witness_set = genesis_witness_set(key, &body.to_cbor_bytes())?;
            let tx = AlonzoCompatibleSubmittedTx::new(body, witness_set, true, None);
            Ok(GeneratedTx::new(MultiEraSubmittedTx::Conway(tx)))
        }
        AnyCardanoEra::Byron | AnyCardanoEra::Dijkstra => Err(TxGenError::ApiError(format!(
            "mkGenesisTransaction: unsupported ShelleyBasedEra {era:?}"
        ))),
    }
}

fn genesis_witness_set(
    key: &SigningKeyEnvelope,
    body_cbor: &[u8],
) -> Result<ShelleyWitnessSet, TxGenError> {
    let tx_id = genesis_tx_id(body_cbor);
    Ok(ShelleyWitnessSet {
        vkey_witnesses: vec![make_vkey_witness(key, &tx_id)?],
        ..empty_witness_set()
    })
}

fn genesis_tx_id(body_cbor: &[u8]) -> TxId {
    compute_tx_id(body_cbor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::witnesses::verify_vkey_signatures;

    fn signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::payment_signing_key_shelley(format!("5820{}", hex::encode([byte; 32])))
    }

    fn genesis_signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::genesis_utxo_signing_key(format!("5820{}", hex::encode([byte; 32])))
    }

    fn genesis_fund_for_key(
        network_id: &NetworkId,
        key: &SigningKeyEnvelope,
        lovelace: Lovelace,
    ) -> GenesisInitialFund {
        let address = key_address(network_id, key).expect("address");
        let tx_in = yggdrasil_node_genesis::initial_funds_pseudo_txin(&address);
        GenesisInitialFund {
            address,
            tx_in: format!("{}#{}", hex::encode(tx_in.transaction_id), tx_in.index),
            lovelace,
        }
    }

    #[test]
    fn genesis_secure_initial_fund_spends_matching_conway_initial_fund() {
        let network_id = NetworkId::Testnet(42);
        let src_key = genesis_signing_key(7);
        let dest_key = signing_key(9);
        let genesis = vec![genesis_fund_for_key(&network_id, &src_key, 2_000_000)];

        let (generated, fund) = genesis_secure_initial_fund(
            AnyCardanoEra::Conway,
            &network_id,
            &genesis,
            &src_key,
            "dest-key",
            &dest_key,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 77,
            },
        )
        .expect("secure genesis");

        assert_eq!(
            generated.tx.inputs()[0],
            mk_tx_in(&genesis[0].tx_in).expect("test genesis tx input")
        );
        assert_eq!(generated.tx.fee(), 10);
        assert_eq!(generated.tx.expires_at().map(|slot| slot.0), Some(77));
        assert_eq!(fund.lovelace, 1_999_990);
        assert_eq!(fund.key_name, "dest-key");
        assert_eq!(fund.tx_in, format!("{}#0", hex::encode(generated.tx_id.0)));
        let MultiEraSubmittedTx::Conway(tx) = &generated.tx else {
            panic!("expected Conway tx");
        };
        assert_eq!(tx.body.outputs[0].amount.coin(), 1_999_990);
        assert_eq!(tx.witness_set.vkey_witnesses.len(), 1);
        verify_vkey_signatures(&generated.tx_id.0, &tx.witness_set.vkey_witnesses)
            .expect("witness verifies");
    }

    #[test]
    fn genesis_secure_initial_fund_requires_matching_genesis_key() {
        let network_id = NetworkId::Testnet(42);
        let src_key = genesis_signing_key(7);
        let other_key = genesis_signing_key(8);
        let dest_key = signing_key(9);
        let genesis = vec![genesis_fund_for_key(&network_id, &other_key, 2_000_000)];

        let err = genesis_secure_initial_fund(
            AnyCardanoEra::Conway,
            &network_id,
            &genesis,
            &src_key,
            "dest-key",
            &dest_key,
            &TxGenTxParams {
                tx_param_fee: 10,
                tx_param_add_tx_size: 0,
                tx_param_ttl: 77,
            },
        )
        .expect_err("missing genesis fund");

        assert!(err.to_string().contains("no fund found for given key"));
    }
}
