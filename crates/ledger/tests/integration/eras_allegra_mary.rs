use super::*;

#[test]
fn allegra_tx_body_roundtrip_all_fields() {
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x60, 0x01],
            amount: 5_000_000,
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some([0xBB; 32]),
        validity_interval_start: Some(500),
    };

    let encoded = body.to_cbor_bytes();
    let decoded = AllegraTxBody::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, body);
}

#[test]
fn allegra_tx_body_roundtrip_minimal() {
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 3,
        }],
        outputs: vec![],
        fee: 100,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let encoded = body.to_cbor_bytes();
    let decoded = AllegraTxBody::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, body);
}

#[test]
fn allegra_tx_body_optional_ttl_only() {
    let body = AllegraTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 50,
        ttl: Some(999),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let encoded = body.to_cbor_bytes();
    let decoded = AllegraTxBody::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, body);
}

#[test]
fn allegra_tx_body_validity_interval_start_only() {
    let body = AllegraTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 75,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: Some(42),
    };

    let encoded = body.to_cbor_bytes();
    let decoded = AllegraTxBody::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, body);
}

#[test]
fn native_script_pubkey_roundtrip() {
    let script = NativeScript::ScriptPubkey([0xCC; 28]);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_all_roundtrip() {
    let script = NativeScript::ScriptAll(vec![
        NativeScript::ScriptPubkey([0x01; 28]),
        NativeScript::ScriptPubkey([0x02; 28]),
    ]);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_any_roundtrip() {
    let script = NativeScript::ScriptAny(vec![
        NativeScript::InvalidBefore(100),
        NativeScript::InvalidHereafter(200),
    ]);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_n_of_k_roundtrip() {
    let script = NativeScript::ScriptNOfK(
        2,
        vec![
            NativeScript::ScriptPubkey([0xAA; 28]),
            NativeScript::ScriptPubkey([0xBB; 28]),
            NativeScript::ScriptPubkey([0xCC; 28]),
        ],
    );
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_invalid_before_roundtrip() {
    let script = NativeScript::InvalidBefore(12345);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_invalid_hereafter_roundtrip() {
    let script = NativeScript::InvalidHereafter(99999);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_nested_roundtrip() {
    let script = NativeScript::ScriptAll(vec![
        NativeScript::ScriptAny(vec![
            NativeScript::ScriptPubkey([0x01; 28]),
            NativeScript::ScriptPubkey([0x02; 28]),
        ]),
        NativeScript::InvalidBefore(100),
        NativeScript::InvalidHereafter(500),
        NativeScript::ScriptNOfK(1, vec![NativeScript::ScriptPubkey([0x03; 28])]),
    ]);
    let encoded = script.to_cbor_bytes();
    let decoded = NativeScript::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded, script);
}

#[test]
fn native_script_unknown_tag_errors() {
    // Construct a native_script array with tag 6 (unknown)
    let mut enc = Encoder::new();
    enc.array(2).unsigned(6).unsigned(0);
    let bytes = enc.into_bytes();
    let result = NativeScript::from_cbor_bytes(&bytes);
    assert!(result.is_err());
}

#[test]
fn cbor_integer_roundtrip() {
    // Positive
    let mut enc = Encoder::new();
    enc.integer(42);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.integer().expect("positive"), 42);

    // Negative
    let mut enc = Encoder::new();
    enc.integer(-3);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.integer().expect("negative"), -3);

    // Zero
    let mut enc = Encoder::new();
    enc.integer(0);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.integer().expect("zero"), 0);
}

// ===========================================================================
// Mary era types
// ===========================================================================

#[test]
fn value_coin_cbor_round_trip() {
    let val = Value::Coin(5_000_000);
    let encoded = val.to_cbor_bytes();
    let decoded = Value::from_cbor_bytes(&encoded).expect("decode Value::Coin");
    assert_eq!(decoded, val);
    assert_eq!(val.coin(), 5_000_000);
    assert!(val.multi_asset().is_none());
}

#[test]
fn value_coin_and_assets_cbor_round_trip() {
    use std::collections::BTreeMap;
    let mut assets = BTreeMap::new();
    assets.insert(vec![0x41, 0x42, 0x43], 100_u64); // asset name "ABC"
    let mut ma = BTreeMap::new();
    ma.insert([0xAA; 28], assets);
    let val = Value::CoinAndAssets(2_000_000, ma);
    let encoded = val.to_cbor_bytes();
    let decoded = Value::from_cbor_bytes(&encoded).expect("decode Value::CoinAndAssets");
    assert_eq!(decoded, val);
    assert_eq!(val.coin(), 2_000_000);
    assert!(val.multi_asset().is_some());
    let inner = val.multi_asset().expect("multi_asset present");
    assert_eq!(inner.len(), 1);
    let policy_assets = inner.get(&[0xAA; 28]).expect("policy exists");
    assert_eq!(*policy_assets.get(&vec![0x41, 0x42, 0x43]).expect("asset exists"), 100);
}

#[test]
fn value_coin_and_assets_multiple_policies() {
    use std::collections::BTreeMap;
    let mut assets1 = BTreeMap::new();
    assets1.insert(vec![], 500_u64); // empty asset name (ADA-like)
    let mut assets2 = BTreeMap::new();
    assets2.insert(b"TokenA".to_vec(), 1000_u64);
    assets2.insert(b"TokenB".to_vec(), 2000_u64);
    let mut ma = BTreeMap::new();
    ma.insert([0x01; 28], assets1);
    ma.insert([0x02; 28], assets2);
    let val = Value::CoinAndAssets(10_000_000, ma);
    let encoded = val.to_cbor_bytes();
    let decoded = Value::from_cbor_bytes(&encoded).expect("decode multi-policy Value");
    assert_eq!(decoded, val);
}

#[test]
fn mary_txout_coin_only_cbor_round_trip() {
    let txout = MaryTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(3_000_000),
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = MaryTxOut::from_cbor_bytes(&encoded).expect("decode MaryTxOut coin-only");
    assert_eq!(decoded, txout);
}

#[test]
fn mary_txout_with_assets_cbor_round_trip() {
    use std::collections::BTreeMap;
    let mut assets = BTreeMap::new();
    assets.insert(b"NFT".to_vec(), 1_u64);
    let mut ma = BTreeMap::new();
    ma.insert([0xBB; 28], assets);
    let txout = MaryTxOut {
        address: vec![0x00; 57],
        amount: Value::CoinAndAssets(1_500_000, ma),
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = MaryTxOut::from_cbor_bytes(&encoded).expect("decode MaryTxOut with assets");
    assert_eq!(decoded, txout);
}

#[test]
fn mary_tx_body_required_fields_only() {
    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(1_000_000),
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };
    let encoded = body.to_cbor_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&encoded).expect("decode MaryTxBody required");
    assert_eq!(decoded, body);
}

#[test]
fn mary_tx_body_all_optional_fields() {
    use std::collections::BTreeMap;

    // Mint: create 50 tokens of one asset, burn 10 of another
    let mut mint_assets = BTreeMap::new();
    mint_assets.insert(b"Gold".to_vec(), 50_i64);
    mint_assets.insert(b"Silver".to_vec(), -10_i64);
    let mut mint = BTreeMap::new();
    mint.insert([0xCC; 28], mint_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 1,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x00; 57],
            amount: Value::Coin(5_000_000),
        }],
        fee: 180_000,
        ttl: Some(100_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some([0xDD; 32]),
        validity_interval_start: Some(50_000),
        mint: Some(mint),
    };
    let encoded = body.to_cbor_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&encoded).expect("decode MaryTxBody all fields");
    assert_eq!(decoded, body);
}

#[test]
fn mary_tx_body_with_multi_asset_outputs() {
    use std::collections::BTreeMap;
    let mut assets = BTreeMap::new();
    assets.insert(b"HOSKY".to_vec(), 1_000_000_u64);
    let mut ma = BTreeMap::new();
    ma.insert([0xFF; 28], assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x33; 32],
            index: 0,
        }],
        outputs: vec![
            MaryTxOut {
                address: vec![0x61; 29],
                amount: Value::CoinAndAssets(2_000_000, ma.clone()),
            },
            MaryTxOut {
                address: vec![0x61; 29],
                amount: Value::Coin(3_000_000),
            },
        ],
        fee: 170_000,
        ttl: Some(200_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };
    let encoded = body.to_cbor_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&encoded).expect("decode MaryTxBody multi-asset out");
    assert_eq!(decoded, body);
}

#[test]
fn mary_tx_body_unknown_keys_skipped() {
    // Build a valid body then inject an extra key to test forward compat.
    let mut enc = Encoder::new();
    enc.map(4); // 3 required + 1 unknown

    // key 0: inputs (1 input)
    enc.unsigned(0).array(1);
    enc.array(2).bytes(&[0x44; 32]).unsigned(0);

    // key 1: outputs (1 output, pure coin)
    enc.unsigned(1).array(1);
    enc.array(2).bytes(&[0x61; 29]).unsigned(1_000_000);

    // key 2: fee
    enc.unsigned(2).unsigned(100_000);

    // key 99: unknown
    enc.unsigned(99).unsigned(12345);

    let bytes = enc.into_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&bytes).expect("decode with unknown keys");
    assert_eq!(decoded.fee, 100_000);
    assert!(decoded.mint.is_none());
}

#[test]
fn mary_tx_body_mint_signed_quantities_round_trip() {
    use std::collections::BTreeMap;
    // Burn scenario: all negative
    let mut burn_assets = BTreeMap::new();
    burn_assets.insert(b"TKN".to_vec(), -999_i64);
    let mut mint = BTreeMap::new();
    mint.insert([0xEE; 28], burn_assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x55; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
        }],
        fee: 160_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
    };
    let encoded = body.to_cbor_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&encoded).expect("decode burn mint");
    assert_eq!(decoded.mint.expect("mint present").get(&[0xEE; 28]).expect("policy")
        .get(&b"TKN".to_vec()).copied(), Some(-999));
}

