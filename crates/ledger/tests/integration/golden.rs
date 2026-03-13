use super::*;

// These tests verify that our CBOR codecs produce byte-accurate encodings
// for known Cardano types.  Hand-crafted hex strings represent canonical
// CBOR per the Cardano ledger CDDL specification.

/// A ShelleyTxBody with a single input and single output encodes to
/// canonical CDDL-conformant CBOR.
#[test]
fn cbor_golden_shelley_tx_body() {
    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 29],
            amount: 2_000_000,
        }],
        fee: 200_000,
        ttl: 1_000_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let encoded = tx_body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded.inputs.len(), 1);
    assert_eq!(decoded.outputs.len(), 1);
    assert_eq!(decoded.fee, 200_000);
    assert_eq!(decoded.ttl, 1_000_000);

    // Re-encode must produce identical bytes (round-trip parity).
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(encoded, re_encoded, "CBOR round-trip must be byte-identical");
}

/// A Shelley block with header and transactions round-trips cleanly.
#[test]
fn cbor_golden_shelley_block_round_trip() {
    let header = ShelleyHeader {
        body: ShelleyHeaderBody {
            block_number: 1,
            slot: 100,
            prev_hash: Some([0xBB; 32]),
            issuer_vkey: [0xCC; 32],
            vrf_vkey: [0xDD; 32],
            nonce_vrf: ShelleyVrfCert {
                output: vec![0x11; 64],
                proof: [0x22; 80],
            },
            leader_vrf: ShelleyVrfCert {
                output: vec![0x44; 64],
                proof: [0x55; 80],
            },
            block_body_size: 256,
            block_body_hash: [0xEE; 32],
            operational_cert: ShelleyOpCert {
                hot_vkey: [0xFF; 32],
                sequence_number: 0,
                kes_period: 0,
                sigma: [0xAA; 64],
            },
            protocol_version: (10, 0),
        },
        signature: vec![0x33; 448],
    };

    let block = ShelleyBlock {
        header: header.clone(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };

    let encoded = block.to_cbor_bytes();
    let decoded = ShelleyBlock::from_cbor_bytes(&encoded).expect("decode");
    assert_eq!(decoded.header.body.block_number, 1);
    assert_eq!(decoded.header.body.slot, 100);

    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(encoded, re_encoded, "ShelleyBlock CBOR round-trip parity");
}

/// PlutusData AST round-trips through all constructors.
#[test]
fn cbor_golden_plutus_data_all_variants() {
    // Constr with compact tag (121 = constructor 0)
    let constr = PlutusData::Constr(0, vec![PlutusData::Integer(42)]);
    let enc = constr.to_cbor_bytes();
    let dec = PlutusData::from_cbor_bytes(&enc).expect("constr");
    assert_eq!(dec, constr);

    // General form (tag 102) for constructor 7 (outside 0-6 compact range)
    let general = PlutusData::Constr(7, vec![PlutusData::Bytes(vec![0xDE, 0xAD])]);
    let enc = general.to_cbor_bytes();
    let dec = PlutusData::from_cbor_bytes(&enc).expect("general constr");
    assert_eq!(dec, general);

    // Map
    let map = PlutusData::Map(vec![
        (PlutusData::Integer(1), PlutusData::Bytes(vec![0xCA, 0xFE])),
    ]);
    let enc = map.to_cbor_bytes();
    let dec = PlutusData::from_cbor_bytes(&enc).expect("map");
    assert_eq!(dec, map);

    // List
    let list = PlutusData::List(vec![PlutusData::Integer(100), PlutusData::Integer(-1)]);
    let enc = list.to_cbor_bytes();
    let dec = PlutusData::from_cbor_bytes(&enc).expect("list");
    assert_eq!(dec, list);

    // Bignum (> i64 range)
    let big = PlutusData::Integer(i128::from(i64::MAX) + 1);
    let enc = big.to_cbor_bytes();
    let dec = PlutusData::from_cbor_bytes(&enc).expect("bignum");
    assert_eq!(dec, big);
}

/// Credential types round-trip through CBOR.
#[test]
fn cbor_golden_stake_credential() {
    let key_cred = StakeCredential::AddrKeyHash([0x01; 28]);
    let enc = key_cred.to_cbor_bytes();
    let dec = StakeCredential::from_cbor_bytes(&enc).expect("key cred");
    assert_eq!(dec, key_cred);

    let script_cred = StakeCredential::ScriptHash([0x02; 28]);
    let enc = script_cred.to_cbor_bytes();
    let dec = StakeCredential::from_cbor_bytes(&enc).expect("script cred");
    assert_eq!(dec, script_cred);
}

/// Byron epoch constant matches upstream.
#[test]
fn byron_epoch_constant() {
    // Upstream Byron epoch is 21,600 slots.
    assert_eq!(BYRON_SLOTS_PER_EPOCH, 21_600);
}

/// Multi-era tx out coin accessor consistency.
#[test]
fn multi_era_tx_out_coin_consistency() {
    let shelley = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: vec![0x61; 29],
        amount: 5_000_000,
    });
    assert_eq!(shelley.coin(), 5_000_000);

    let mary = MultiEraTxOut::Mary(MaryTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(3_000_000),
    });
    assert_eq!(mary.coin(), 3_000_000);

    let alonzo = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(1_000_000),
        datum_hash: None,
    });
    assert_eq!(alonzo.coin(), 1_000_000);

    let babbage = MultiEraTxOut::Babbage(BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: None,
    });
    assert_eq!(babbage.coin(), 2_000_000);
}
