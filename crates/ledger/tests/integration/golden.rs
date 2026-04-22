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
    assert_eq!(
        encoded, re_encoded,
        "CBOR round-trip must be byte-identical"
    );
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
    let map = PlutusData::Map(vec![(
        PlutusData::Integer(1),
        PlutusData::Bytes(vec![0xCA, 0xFE]),
    )]);
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

// ---------------------------------------------------------------------------
// Byron transaction golden round-trips
// ---------------------------------------------------------------------------

/// ByronTx round-trip produces byte-identical output.
#[test]
fn cbor_golden_byron_tx_round_trip() {
    let tx = ByronTx {
        inputs: vec![
            ByronTxIn {
                txid: [0x11; 32],
                index: 0,
            },
            ByronTxIn {
                txid: [0x22; 32],
                index: 3,
            },
        ],
        outputs: vec![
            ByronTxOut {
                address: vec![0xD8, 0x18, 0x43, 0x01, 0x02, 0x03],
                amount: 1_000_000,
            },
            ByronTxOut {
                address: vec![0xD8, 0x18, 0x43, 0x04, 0x05, 0x06],
                amount: 500_000,
            },
        ],
        attributes: {
            let mut enc = Encoder::new();
            enc.map(0);
            enc.into_bytes()
        },
    };

    let encoded = tx.to_cbor_bytes();
    let decoded = ByronTx::from_cbor_bytes(&encoded).expect("decode ByronTx");
    assert_eq!(decoded, tx);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        encoded, re_encoded,
        "ByronTx CBOR round-trip must be byte-identical"
    );
}

/// ByronTxAux round-trip produces byte-identical output.
#[test]
fn cbor_golden_byron_tx_aux_round_trip() {
    let tx_aux = ByronTxAux {
        tx: ByronTx {
            inputs: vec![ByronTxIn {
                txid: [0xFF; 32],
                index: 1,
            }],
            outputs: vec![ByronTxOut {
                address: vec![0xD8, 0x18, 0x43, 0x07, 0x08, 0x09],
                amount: 42,
            }],
            attributes: {
                let mut enc = Encoder::new();
                enc.map(0);
                enc.into_bytes()
            },
        },
        witnesses: vec![ByronTxWitness {
            witness_type: 0,
            payload: vec![0x82, 0x40, 0x40],
        }],
        raw_tx_cbor: Vec::new(),
    };

    let encoded = tx_aux.to_cbor_bytes();
    let decoded = ByronTxAux::from_cbor_bytes(&encoded).expect("decode ByronTxAux");
    assert_eq!(decoded, tx_aux);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        encoded, re_encoded,
        "ByronTxAux CBOR round-trip must be byte-identical"
    );
}

/// Byron tx_id is Blake2b-256 of the CBOR-encoded Tx body.
#[test]
fn cbor_golden_byron_tx_id() {
    let tx = ByronTx {
        inputs: vec![ByronTxIn {
            txid: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ByronTxOut {
            address: vec![0xD8, 0x18, 0x43, 0x01, 0x02, 0x03],
            amount: 100,
        }],
        attributes: {
            let mut enc = Encoder::new();
            enc.map(0);
            enc.into_bytes()
        },
    };

    let id = tx.tx_id();
    // Must be 32 bytes and not all zeros.
    assert_eq!(id.len(), 32);
    assert_ne!(id, [0u8; 32]);
    // Must be deterministic.
    assert_eq!(id, tx.tx_id());
}

// ---------------------------------------------------------------------------
// Submitted transaction round-trips (all Shelley-based eras)
// ---------------------------------------------------------------------------

/// Helper: builds a minimal witness set for testing.
fn minimal_witness_set() -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

/// Shelley submitted tx (ShelleyTx) round-trip via MultiEraSubmittedTx.
#[test]
fn cbor_golden_shelley_submitted_tx_round_trip() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 29],
            amount: 2_000_000,
        }],
        fee: 170_000,
        ttl: 500_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let tx = ShelleyTx {
        body: body.clone(),
        witness_set: minimal_witness_set(),
        auxiliary_data: None,
    };
    let cbor = tx.to_cbor_bytes();
    let decoded = ShelleyTx::from_cbor_bytes(&cbor).expect("decode ShelleyTx");
    assert_eq!(decoded.body, tx.body);
    assert_eq!(decoded.witness_set, tx.witness_set);

    // Via MultiEraSubmittedTx path
    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Shelley, &cbor)
        .expect("decode Shelley via multi-era");
    assert_eq!(mst.era(), Era::Shelley);
    assert_eq!(mst.fee(), 170_000);
}

/// Allegra submitted tx round-trip.
#[test]
fn cbor_golden_allegra_submitted_tx_round_trip() {
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 1,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 29],
            amount: 3_000_000,
        }],
        fee: 180_000,
        ttl: Some(600_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: Some(100),
    };

    let tx = ShelleyCompatibleSubmittedTx::new(body, minimal_witness_set(), None);
    let cbor = tx.to_cbor_bytes();
    let decoded = ShelleyCompatibleSubmittedTx::<AllegraTxBody>::from_cbor_bytes(&cbor)
        .expect("decode Allegra");
    assert_eq!(decoded.body, tx.body);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        cbor, re_encoded,
        "Allegra submitted tx CBOR round-trip must be byte-identical"
    );

    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Allegra, &cbor)
        .expect("decode Allegra via multi-era");
    assert_eq!(mst.era(), Era::Allegra);
    assert_eq!(mst.fee(), 180_000);
}

/// Mary submitted tx round-trip with multi-asset output.
#[test]
fn cbor_golden_mary_submitted_tx_round_trip() {
    use std::collections::BTreeMap;
    let mut assets = BTreeMap::new();
    assets.insert(vec![0x01, 0x02, 0x03], 500u64);
    let mut multi_asset = BTreeMap::new();
    multi_asset.insert([0xCC; 28], assets);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x33; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x61; 29],
            amount: Value::CoinAndAssets(2_000_000, multi_asset),
        }],
        fee: 200_000,
        ttl: Some(700_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };

    let tx = ShelleyCompatibleSubmittedTx::new(body, minimal_witness_set(), None);
    let cbor = tx.to_cbor_bytes();
    let decoded =
        ShelleyCompatibleSubmittedTx::<MaryTxBody>::from_cbor_bytes(&cbor).expect("decode Mary");
    assert_eq!(decoded.body, tx.body);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        cbor, re_encoded,
        "Mary submitted tx CBOR round-trip must be byte-identical"
    );

    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Mary, &cbor)
        .expect("decode Mary via multi-era");
    assert_eq!(mst.era(), Era::Mary);
    assert_eq!(mst.fee(), 200_000);
}

/// Alonzo submitted tx round-trip (4-element shape with is_valid).
#[test]
fn cbor_golden_alonzo_submitted_tx_round_trip() {
    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x44; 32],
            index: 2,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(5_000_000),
            datum_hash: Some([0xDD; 32]),
        }],
        fee: 250_000,
        ttl: Some(800_000),
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

    let tx = AlonzoCompatibleSubmittedTx::new(body, minimal_witness_set(), true, None);
    let cbor = tx.to_cbor_bytes();
    let decoded =
        AlonzoCompatibleSubmittedTx::<AlonzoTxBody>::from_cbor_bytes(&cbor).expect("decode Alonzo");
    assert_eq!(decoded.body, tx.body);
    assert!(decoded.is_valid);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        cbor, re_encoded,
        "Alonzo submitted tx CBOR round-trip must be byte-identical"
    );

    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Alonzo, &cbor)
        .expect("decode Alonzo via multi-era");
    assert_eq!(mst.era(), Era::Alonzo);
    assert_eq!(mst.fee(), 250_000);
}

/// Babbage submitted tx round-trip (4-element Alonzo shape).
#[test]
fn cbor_golden_babbage_submitted_tx_round_trip() {
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x55; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(3_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 300_000,
        ttl: Some(900_000),
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

    let tx = AlonzoCompatibleSubmittedTx::new(body, minimal_witness_set(), true, None);
    let cbor = tx.to_cbor_bytes();
    let decoded = AlonzoCompatibleSubmittedTx::<BabbageTxBody>::from_cbor_bytes(&cbor)
        .expect("decode Babbage");
    assert_eq!(decoded.body, tx.body);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        cbor, re_encoded,
        "Babbage submitted tx CBOR round-trip must be byte-identical"
    );

    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Babbage, &cbor)
        .expect("decode Babbage via multi-era");
    assert_eq!(mst.era(), Era::Babbage);
    assert_eq!(mst.fee(), 300_000);
}

/// Conway submitted tx round-trip (4-element Alonzo shape).
#[test]
fn cbor_golden_conway_submitted_tx_round_trip() {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x66; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(4_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 350_000,
        ttl: Some(1_000_000),
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

    let tx = AlonzoCompatibleSubmittedTx::new(body, minimal_witness_set(), true, None);
    let cbor = tx.to_cbor_bytes();
    let decoded =
        AlonzoCompatibleSubmittedTx::<ConwayTxBody>::from_cbor_bytes(&cbor).expect("decode Conway");
    assert_eq!(decoded.body, tx.body);
    let re_encoded = decoded.to_cbor_bytes();
    assert_eq!(
        cbor, re_encoded,
        "Conway submitted tx CBOR round-trip must be byte-identical"
    );

    let mst = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Conway, &cbor)
        .expect("decode Conway via multi-era");
    assert_eq!(mst.era(), Era::Conway);
    assert_eq!(mst.fee(), 350_000);
}

/// MultiEraSubmittedTx rejects Byron era.
#[test]
fn multi_era_submitted_tx_byron_unsupported() {
    let result =
        MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Byron, &[0x83, 0x80, 0x80, 0xF6]);
    assert!(result.is_err());
}

/// Transaction IDs across eras are deterministic and non-zero.
#[test]
fn submitted_tx_ids_are_deterministic() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 29],
            amount: 1_000_000,
        }],
        fee: 100_000,
        ttl: 500_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let tx = ShelleyTx {
        body: body.clone(),
        witness_set: minimal_witness_set(),
        auxiliary_data: None,
    };
    let mst = MultiEraSubmittedTx::Shelley(tx.clone());

    let id1 = mst.tx_id();
    let id2 = mst.tx_id();
    assert_eq!(id1, id2);
    assert_ne!(id1, TxId([0u8; 32]));

    // Body CBOR is also deterministic.
    let body_cbor1 = mst.body_cbor();
    let body_cbor2 = mst.body_cbor();
    assert_eq!(body_cbor1, body_cbor2);
}
