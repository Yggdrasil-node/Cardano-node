use super::*;

pub(super) fn sample_praos_header() -> PraosHeader {
    PraosHeader {
        body: PraosHeaderBody {
            block_number: 42,
            slot: 1000,
            prev_hash: Some([0xAA; 32]),
            issuer_vkey: [0x11; 32],
            vrf_vkey: [0x22; 32],
            vrf_result: ShelleyVrfCert {
                output: vec![0x30; 32],
                proof: [0x31; 80],
            },
            block_body_size: 512,
            block_body_hash: [0x55; 32],
            operational_cert: ShelleyOpCert {
                hot_vkey: [0x60; 32],
                sequence_number: 1,
                kes_period: 0,
                sigma: [0x61; 64],
            },
            protocol_version: (8, 0),
        },
        signature: vec![0xDD; 448],
    }
}

#[test]
fn babbage_block_cbor_round_trip_empty() {
    let block = BabbageBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let bytes = block.to_cbor_bytes();
    let decoded = BabbageBlock::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.header.body.slot, 1000);
    assert_eq!(decoded.header.body.block_number, 42);
    assert!(decoded.transaction_bodies.is_empty());
}

#[test]
fn babbage_block_cbor_round_trip_with_tx() {
    let tx_body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 29],
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
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
    };
    let block = BabbageBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![tx_body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let bytes = block.to_cbor_bytes();
    let decoded = BabbageBlock::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.transaction_bodies.len(), 1);
    assert_eq!(decoded.transaction_bodies[0].fee, 200_000);
    assert_eq!(decoded.transaction_witness_sets.len(), 1);
}

#[test]
fn babbage_block_header_hash() {
    let block = BabbageBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let h1 = block.header_hash();
    let h2 = block.header.header_hash();
    assert_eq!(h1, h2);
}

#[test]
fn conway_block_cbor_round_trip_empty() {
    let block = ConwayBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let bytes = block.to_cbor_bytes();
    let decoded = ConwayBlock::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.header.body.slot, 1000);
    assert_eq!(decoded.header.body.block_number, 42);
    assert!(decoded.transaction_bodies.is_empty());
}

#[test]
fn conway_block_cbor_round_trip_with_tx() {
    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xBB; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 29],
            amount: Value::Coin(3_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 300_000,
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
    };
    let block = ConwayBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![tx_body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let bytes = block.to_cbor_bytes();
    let decoded = ConwayBlock::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.transaction_bodies.len(), 1);
    assert_eq!(decoded.transaction_bodies[0].fee, 300_000);
}

#[test]
fn conway_block_header_hash() {
    let block = ConwayBlock {
        header: sample_praos_header(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let h1 = block.header_hash();
    let h2 = block.header.header_hash();
    assert_eq!(h1, h2);
}