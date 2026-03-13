use super::*;

#[test]
fn ex_units_cbor_round_trip() {
    let eu = ExUnits {
        mem: 500_000,
        steps: 200_000_000,
    };
    let encoded = eu.to_cbor_bytes();
    let decoded = ExUnits::from_cbor_bytes(&encoded).expect("decode ExUnits");
    assert_eq!(decoded, eu);
}

#[test]
fn redeemer_cbor_round_trip() {
    let redeemer = Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(42),
        ex_units: ExUnits {
            mem: 100_000,
            steps: 50_000_000,
        },
    };
    let encoded = redeemer.to_cbor_bytes();
    let decoded = Redeemer::from_cbor_bytes(&encoded).expect("decode Redeemer");
    assert_eq!(decoded, redeemer);
}

#[test]
fn redeemer_mint_tag() {
    let redeemer = Redeemer {
        tag: 1,
        index: 2,
        data: PlutusData::List(vec![]),
        ex_units: ExUnits { mem: 0, steps: 0 },
    };
    let encoded = redeemer.to_cbor_bytes();
    let decoded = Redeemer::from_cbor_bytes(&encoded).expect("decode mint Redeemer");
    assert_eq!(decoded.tag, 1);
    assert_eq!(decoded.index, 2);
}

#[test]
fn redeemer_with_complex_plutus_data() {
    let complex_data = PlutusData::Constr(
        0,
        vec![
            PlutusData::Integer(42),
            PlutusData::Bytes(vec![0xDE, 0xAD]),
            PlutusData::List(vec![PlutusData::Integer(-1), PlutusData::Integer(100)]),
        ],
    );
    let redeemer = Redeemer {
        tag: 0,
        index: 3,
        data: complex_data,
        ex_units: ExUnits {
            mem: 200_000,
            steps: 100_000_000,
        },
    };
    let encoded = redeemer.to_cbor_bytes();
    let decoded = Redeemer::from_cbor_bytes(&encoded).expect("decode complex Redeemer");
    assert_eq!(decoded, redeemer);
}

#[test]
fn redeemer_with_map_plutus_data() {
    let map_data = PlutusData::Map(vec![
        (PlutusData::Integer(1), PlutusData::Bytes(vec![0x01])),
        (PlutusData::Integer(2), PlutusData::Bytes(vec![0x02])),
    ]);
    let redeemer = Redeemer {
        tag: 2,
        index: 0,
        data: map_data,
        ex_units: ExUnits { mem: 0, steps: 0 },
    };
    let encoded = redeemer.to_cbor_bytes();
    let decoded = Redeemer::from_cbor_bytes(&encoded).expect("decode map Redeemer");
    assert_eq!(decoded, redeemer);
}

#[test]
fn witness_set_with_complex_plutus_data() {
    let pd1 = PlutusData::Constr(0, vec![PlutusData::Integer(1)]);
    let pd2 = PlutusData::Map(vec![(
        PlutusData::Bytes(b"x".to_vec()),
        PlutusData::Integer(-5),
    )]);
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![pd1, pd2],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes)
        .expect("witness set complex plutus data");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_typed_redeemer_and_plutus_data() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![PlutusData::Integer(42)],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Constr(1, vec![PlutusData::Bytes(vec![0xAB])]),
            ex_units: ExUnits {
                mem: 100,
                steps: 200,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes)
        .expect("witness set typed redeemer + data");
    assert_eq!(wset, decoded);
}

#[test]
fn alonzo_txout_no_datum_cbor_round_trip() {
    let txout = AlonzoTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(3_000_000),
        datum_hash: None,
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = AlonzoTxOut::from_cbor_bytes(&encoded).expect("decode AlonzoTxOut no datum");
    assert_eq!(decoded, txout);
}

#[test]
fn alonzo_txout_with_datum_hash_cbor_round_trip() {
    use std::collections::BTreeMap;

    let mut assets = BTreeMap::new();
    assets.insert(b"Script".to_vec(), 1_u64);
    let mut ma = BTreeMap::new();
    ma.insert([0xAA; 28], assets);

    let txout = AlonzoTxOut {
        address: vec![0x71; 57],
        amount: Value::CoinAndAssets(2_000_000, ma),
        datum_hash: Some([0xDD; 32]),
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = AlonzoTxOut::from_cbor_bytes(&encoded).expect("decode AlonzoTxOut with datum");
    assert_eq!(decoded, txout);
    assert_eq!(decoded.datum_hash, Some([0xDD; 32]));
}

#[test]
fn alonzo_tx_body_required_fields_only() {
    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(1_000_000),
            datum_hash: None,
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
    };
    let encoded = body.to_cbor_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&encoded).expect("decode AlonzoTxBody required");
    assert_eq!(decoded, body);
}

#[test]
fn alonzo_tx_body_all_optional_fields() {
    use std::collections::BTreeMap;

    let mut mint_assets = BTreeMap::new();
    mint_assets.insert(b"PlutusToken".to_vec(), 100_i64);
    let mut mint = BTreeMap::new();
    mint.insert([0xCC; 28], mint_assets);

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 1,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x71; 57],
            amount: Value::Coin(5_000_000),
            datum_hash: Some([0xAA; 32]),
        }],
        fee: 300_000,
        ttl: Some(500_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some([0xBB; 32]),
        validity_interval_start: Some(100_000),
        mint: Some(mint),
        script_data_hash: Some([0xEE; 32]),
        collateral: Some(vec![ShelleyTxIn {
            transaction_id: [0x33; 32],
            index: 0,
        }]),
        required_signers: Some(vec![[0x44; 28], [0x55; 28]]),
        network_id: Some(1),
    };
    let encoded = body.to_cbor_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&encoded).expect("decode AlonzoTxBody all fields");
    assert_eq!(decoded, body);
}

#[test]
fn alonzo_tx_body_collateral_only() {
    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x66; 32],
            index: 0,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
            datum_hash: None,
        }],
        fee: 170_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: Some(vec![
            ShelleyTxIn {
                transaction_id: [0x77; 32],
                index: 0,
            },
            ShelleyTxIn {
                transaction_id: [0x88; 32],
                index: 1,
            },
        ]),
        required_signers: None,
        network_id: None,
    };
    let encoded = body.to_cbor_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&encoded).expect("decode collateral-only");
    assert_eq!(decoded.collateral.expect("collateral present").len(), 2);
}

#[test]
fn alonzo_tx_body_unknown_keys_skipped() {
    let mut enc = Encoder::new();
    enc.map(4);
    enc.unsigned(0).array(1);
    enc.array(2).bytes(&[0x99; 32]).unsigned(0);
    enc.unsigned(1).array(1);
    enc.array(2).bytes(&[0x61; 29]).unsigned(1_000_000);
    enc.unsigned(2).unsigned(100_000);
    enc.unsigned(50).bytes(&[0xFF; 8]);

    let bytes = enc.into_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&bytes).expect("decode with unknown keys");
    assert_eq!(decoded.fee, 100_000);
    assert!(decoded.script_data_hash.is_none());
    assert!(decoded.collateral.is_none());
}

#[test]
fn alonzo_tx_body_network_id_testnet() {
    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(1_000_000),
            datum_hash: None,
        }],
        fee: 150_000,
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
        network_id: Some(0),
    };
    let encoded = body.to_cbor_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&encoded).expect("decode testnet network_id");
    assert_eq!(decoded.network_id, Some(0));
}