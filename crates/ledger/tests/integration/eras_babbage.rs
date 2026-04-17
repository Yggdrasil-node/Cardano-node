use super::*;

#[test]
fn datum_option_inline_complex_plutus_data() {
    let complex = PlutusData::Constr(
        2,
        vec![PlutusData::Map(vec![(
            PlutusData::Bytes(b"key".to_vec()),
            PlutusData::Integer(999),
        )])],
    );
    let datum = DatumOption::Inline(complex);
    let mut enc = Encoder::new();
    datum.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = DatumOption::decode_cbor(&mut dec).expect("decode complex inline datum");
    assert_eq!(datum, decoded);
}

#[test]
fn datum_option_inline_nested_list() {
    let nested = PlutusData::List(vec![
        PlutusData::List(vec![PlutusData::Integer(1), PlutusData::Integer(2)]),
        PlutusData::Bytes(vec![0xFF]),
    ]);
    let datum = DatumOption::Inline(nested);
    let encoded = datum.to_cbor_bytes();
    let decoded = DatumOption::from_cbor_bytes(&encoded).expect("decode nested inline datum");
    assert_eq!(datum, decoded);
}

#[test]
fn babbage_txout_with_typed_inline_datum() {
    let txout = BabbageTxOut {
        address: vec![0x01; 28],
        amount: Value::Coin(5_000_000),
        datum_option: Some(DatumOption::Inline(PlutusData::Constr(
            0,
            vec![
                PlutusData::Integer(100),
                PlutusData::Bytes(vec![0xCA, 0xFE]),
            ],
        ))),
        script_ref: None,
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = BabbageTxOut::from_cbor_bytes(&encoded).expect("decode typed inline datum txout");
    assert_eq!(txout, decoded);
}

#[test]
fn babbage_txout_with_inline_datum_and_script_ref_typed() {
    let txout = BabbageTxOut {
        address: vec![0x03; 28],
        amount: Value::Coin(10_000_000),
        datum_option: Some(DatumOption::Inline(PlutusData::List(vec![
            PlutusData::Integer(1),
            PlutusData::Integer(2),
        ]))),
        script_ref: Some(ScriptRef(Script::Native(NativeScript::ScriptPubkey(
            [0x00; 28],
        )))),
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = BabbageTxOut::from_cbor_bytes(&encoded)
        .expect("decode typed inline datum + script ref txout");
    assert_eq!(txout, decoded);
}

#[test]
fn datum_option_hash_cbor_round_trip() {
    let datum = DatumOption::Hash([0xAA; 32]);
    let mut enc = Encoder::new();
    datum.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = DatumOption::decode_cbor(&mut dec).expect("decode");
    assert_eq!(datum, decoded);
}

#[test]
fn datum_option_inline_cbor_round_trip() {
    let inline_data = PlutusData::Integer(42);
    let datum = DatumOption::Inline(inline_data.clone());
    let mut enc = Encoder::new();
    datum.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = DatumOption::decode_cbor(&mut dec).expect("decode");
    assert_eq!(datum, decoded);
    if let DatumOption::Inline(data) = decoded {
        assert_eq!(data, inline_data);
    } else {
        panic!("expected Inline variant");
    }
}

#[test]
fn babbage_txout_map_format_cbor_round_trip() {
    let txout = BabbageTxOut {
        address: vec![0x01; 28],
        amount: Value::Coin(5_000_000),
        datum_option: None,
        script_ref: None,
    };
    let mut enc = Encoder::new();
    txout.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxOut::decode_cbor(&mut dec).expect("decode");
    assert_eq!(txout, decoded);
}

#[test]
fn babbage_txout_with_datum_hash_cbor_round_trip() {
    let txout = BabbageTxOut {
        address: vec![0x02; 28],
        amount: Value::Coin(3_000_000),
        datum_option: Some(DatumOption::Hash([0xBB; 32])),
        script_ref: None,
    };
    let mut enc = Encoder::new();
    txout.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxOut::decode_cbor(&mut dec).expect("decode");
    assert_eq!(txout, decoded);
}

#[test]
fn babbage_txout_with_inline_datum_and_script_ref() {
    let txout = BabbageTxOut {
        address: vec![0x03; 28],
        amount: Value::Coin(10_000_000),
        datum_option: Some(DatumOption::Inline(PlutusData::Integer(5))),
        script_ref: Some(ScriptRef(Script::Native(NativeScript::ScriptPubkey(
            [0x00; 28],
        )))),
    };
    let mut enc = Encoder::new();
    txout.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxOut::decode_cbor(&mut dec).expect("decode");
    assert_eq!(txout, decoded);
}

#[test]
fn babbage_txout_pre_babbage_array_decode() {
    let mut enc = Encoder::new();
    enc.array(2).bytes(&[0x04; 28]).unsigned(2_000_000);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxOut::decode_cbor(&mut dec).expect("decode");
    assert_eq!(decoded.address, vec![0x04; 28]);
    assert_eq!(decoded.amount, Value::Coin(2_000_000));
    assert!(decoded.datum_option.is_none());
    assert!(decoded.script_ref.is_none());
}

#[test]
fn babbage_txout_pre_babbage_array_with_datum_hash() {
    let mut enc = Encoder::new();
    enc.array(3)
        .bytes(&[0x05; 28])
        .unsigned(1_000_000)
        .bytes(&[0xCC; 32]);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxOut::decode_cbor(&mut dec).expect("decode");
    assert_eq!(decoded.amount, Value::Coin(1_000_000));
    assert_eq!(decoded.datum_option, Some(DatumOption::Hash([0xCC; 32])));
}

#[test]
fn babbage_tx_body_required_fields_only() {
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 28],
            amount: Value::Coin(1_000_000),
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
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxBody::decode_cbor(&mut dec).expect("decode");
    assert_eq!(body, decoded);
}

#[test]
fn babbage_tx_body_with_new_fields() {
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 1,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02; 28],
            amount: Value::Coin(5_000_000),
            datum_option: Some(DatumOption::Inline(PlutusData::Integer(66))),
            script_ref: None,
        }],
        fee: 300_000,
        ttl: Some(1_000_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: Some([0xDD; 32]),
        collateral: Some(vec![ShelleyTxIn {
            transaction_id: [0x33; 32],
            index: 0,
        }]),
        required_signers: Some(vec![[0x44; 28]]),
        network_id: Some(1),
        collateral_return: Some(BabbageTxOut {
            address: vec![0x05; 28],
            amount: Value::Coin(4_700_000),
            datum_option: None,
            script_ref: None,
        }),
        total_collateral: Some(300_000),
        reference_inputs: Some(vec![ShelleyTxIn {
            transaction_id: [0x55; 32],
            index: 2,
        }]),
    };
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxBody::decode_cbor(&mut dec).expect("decode");
    assert_eq!(body, decoded);
}

#[test]
fn babbage_tx_body_unknown_keys_skipped() {
    let mut enc = Encoder::new();
    enc.map(4);
    enc.unsigned(0).array(1);
    ShelleyTxIn {
        transaction_id: [0x11; 32],
        index: 0,
    }
    .encode_cbor(&mut enc);
    enc.unsigned(1).array(1);
    enc.map(2)
        .unsigned(0)
        .bytes(&[0x01; 28])
        .unsigned(1)
        .unsigned(500_000);
    enc.unsigned(2).unsigned(100_000);
    enc.unsigned(99).unsigned(42);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxBody::decode_cbor(&mut dec).expect("decode");
    assert_eq!(decoded.fee, 100_000);
    assert_eq!(decoded.inputs.len(), 1);
}

#[test]
fn babbage_tx_body_reference_inputs_only() {
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 28],
            amount: Value::Coin(1_000_000),
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
        reference_inputs: Some(vec![
            ShelleyTxIn {
                transaction_id: [0x66; 32],
                index: 0,
            },
            ShelleyTxIn {
                transaction_id: [0x77; 32],
                index: 3,
            },
        ]),
    };
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = BabbageTxBody::decode_cbor(&mut dec).expect("decode");
    assert_eq!(decoded.reference_inputs.as_ref().map(Vec::len), Some(2));
    assert_eq!(body, decoded);
}
