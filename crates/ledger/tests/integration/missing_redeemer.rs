//! Integration tests for MissingRedeemer predicate failures.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`

use std::collections::BTreeMap;

use super::*;

fn permissive_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.max_collateral_inputs = Some(3);
    params.collateral_percentage = Some(150);
    params
}

fn mint_one(policy_hash: [u8; 28]) -> BTreeMap<[u8; 28], BTreeMap<Vec<u8>, i64>> {
    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(policy_hash, assets);
    mint
}

fn output_assets(policy_hash: [u8; 28]) -> BTreeMap<[u8; 28], BTreeMap<Vec<u8>, u64>> {
    let mut assets = BTreeMap::new();
    let mut policy_assets = BTreeMap::new();
    policy_assets.insert(b"nft".to_vec(), 1u64);
    assets.insert(policy_hash, policy_assets);
    assets
}

#[test]
fn alonzo_block_rejects_missing_minting_redeemer_for_plutus_policy() {
    let signer = TestSigner::new([0x71; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x11; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x12; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    let script_bytes = vec![0x51, 0x52, 0x53];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V1,
        &script_bytes,
    );
    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
            datum_hash: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_one(policy_hash)),
        script_data_hash: None,
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };

    let body_bytes = body.to_cbor_bytes();
    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0x21; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x31; 32],
        },
        transactions: vec![Tx {
            id: compute_tx_id(&body_bytes),
            body: body_bytes,
            witnesses: Some(ws.to_cbor_bytes()),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(
            result,
            Err(LedgerError::MissingRedeemer { hash, ref purpose })
                if hash == policy_hash && purpose == "minting index 0"
        ),
        "expected MissingRedeemer for Alonzo block, got: {result:?}",
    );
}

#[test]
fn babbage_submitted_tx_rejects_missing_minting_redeemer_for_plutus_policy() {
    let signer = TestSigner::new([0x72; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x41; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x42; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let script_bytes = vec![0x61, 0x62, 0x63];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V2,
        &script_bytes,
    );
    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_one(policy_hash)),
        script_data_hash: None,
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(
            result,
            Err(LedgerError::MissingRedeemer { hash, ref purpose })
                if hash == policy_hash && purpose == "minting index 0"
        ),
        "expected MissingRedeemer for Babbage submitted tx, got: {result:?}",
    );
}