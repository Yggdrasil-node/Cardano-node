use super::*;

#[test]
fn shelley_txin_cbor_round_trip() {
    let txin = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 7,
    };
    let bytes = txin.to_cbor_bytes();
    let decoded = ShelleyTxIn::from_cbor_bytes(&bytes).expect("ShelleyTxIn round-trip");
    assert_eq!(txin, decoded);
}

#[test]
fn shelley_txin_encoding_structure() {
    // CDDL: transaction_input = [transaction_id, index]
    // Must encode as a 2-element array.
    let txin = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };
    let bytes = txin.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array header");
    assert_eq!(len, 2, "transaction_input must be a 2-element array");
    let _ = dec.bytes().expect("transaction_id bytes");
    let _ = dec.unsigned().expect("index uint");
    assert!(dec.is_empty());
}

#[test]
fn shelley_txout_cbor_round_trip() {
    let txout = ShelleyTxOut {
        address: vec![0x61, 0x00, 0x11, 0x22, 0x33],
        amount: 2_000_000,
    };
    let bytes = txout.to_cbor_bytes();
    let decoded = ShelleyTxOut::from_cbor_bytes(&bytes).expect("ShelleyTxOut round-trip");
    assert_eq!(txout, decoded);
}

#[test]
fn shelley_txout_encoding_structure() {
    // CDDL: transaction_output = [address, amount : coin]
    let txout = ShelleyTxOut {
        address: vec![0x00; 57],
        amount: 1_000_000,
    };
    let bytes = txout.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array header");
    assert_eq!(len, 2, "transaction_output must be a 2-element array");
    let addr = dec.bytes().expect("address bytes");
    assert_eq!(addr.len(), 57);
    let amount = dec.unsigned().expect("coin amount");
    assert_eq!(amount, 1_000_000);
}

#[test]
fn shelley_tx_body_cbor_round_trip_required_fields() {
    let body = ShelleyTxBody {
        inputs: vec![
            ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            },
            ShelleyTxIn {
                transaction_id: [0xBB; 32],
                index: 1,
            },
        ],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 5_000_000,
        }],
        fee: 180_000,
        ttl: 50_000_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("ShelleyTxBody round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_cbor_with_metadata_hash() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xCC; 32],
            index: 3,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 10_000_000,
        }],
        fee: 200_000,
        ttl: 100_000_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some([0xDD; 32]),
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("ShelleyTxBody with metadata hash");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_encoding_is_map() {
    // CDDL: transaction_body = { 0: ..., 1: ..., 2: ..., 3: ... }
    let body = ShelleyTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: 0,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("must encode as CBOR map");
    assert_eq!(count, 4, "4 required fields when no metadata hash");
}

#[test]
fn shelley_tx_body_map_has_5_entries_with_metadata_hash() {
    let body = ShelleyTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: 0,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some([0xFF; 32]),
    };
    let bytes = body.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("map header");
    assert_eq!(count, 5, "5 entries when metadata hash is present");
}

#[test]
fn shelley_vkey_witness_cbor_round_trip() {
    let witness = ShelleyVkeyWitness {
        vkey: [0x11; 32],
        signature: [0x22; 64],
    };
    let bytes = witness.to_cbor_bytes();
    let decoded =
        ShelleyVkeyWitness::from_cbor_bytes(&bytes).expect("ShelleyVkeyWitness round-trip");
    assert_eq!(witness, decoded);
}

#[test]
fn shelley_witness_set_cbor_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![
            ShelleyVkeyWitness {
                vkey: [0xAA; 32],
                signature: [0xBB; 64],
            },
            ShelleyVkeyWitness {
                vkey: [0xCC; 32],
                signature: [0xDD; 64],
            },
        ],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("ShelleyWitnessSet round-trip");
    assert_eq!(wset, decoded);
}

#[test]
fn shelley_witness_set_empty_cbor_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    // Empty witness set: map(0)
    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("map header");
    assert_eq!(count, 0, "empty witness set encodes as map(0)");

    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("empty ShelleyWitnessSet round-trip");
    assert_eq!(wset, decoded);
}

#[test]
fn bootstrap_witness_cbor_round_trip() {
    let bw = BootstrapWitness {
        public_key: [0x11; 32],
        signature: [0x22; 64],
        chain_code: [0x33; 32],
        attributes: vec![0xA0], // empty CBOR map
    };
    let bytes = bw.to_cbor_bytes();
    let decoded = BootstrapWitness::from_cbor_bytes(&bytes).expect("BootstrapWitness round-trip");
    assert_eq!(bw, decoded);
}

#[test]
fn witness_set_with_native_scripts_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![
            NativeScript::ScriptPubkey([0xAA; 28]),
            NativeScript::InvalidBefore(100),
        ],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with native scripts");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_bootstrap_witnesses_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![BootstrapWitness {
            public_key: [0x01; 32],
            signature: [0x02; 64],
            chain_code: [0x03; 32],
            attributes: vec![0xA0],
        }],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with bootstrap witnesses");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_plutus_v1_scripts_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![vec![0x01; 20], vec![0x02; 30]],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with plutus v1 scripts");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_plutus_data_round_trip() {
    // A typed PlutusData value: integer 42
    let datum = PlutusData::Integer(42);

    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![datum],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with plutus data");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_redeemers_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with redeemers");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_plutus_v2_scripts_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![vec![0x10; 24], vec![0x20; 16]],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with plutus v2 scripts");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_with_plutus_v3_scripts_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![vec![0x30; 32]],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set with plutus v3 scripts");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_all_keys_round_trip() {
    // Plutus data: integer 99
    let datum = PlutusData::Integer(99);

    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![ShelleyVkeyWitness {
            vkey: [0xAA; 32],
            signature: [0xBB; 64],
        }],
        native_scripts: vec![NativeScript::ScriptPubkey([0xCC; 28])],
        bootstrap_witnesses: vec![BootstrapWitness {
            public_key: [0x01; 32],
            signature: [0x02; 64],
            chain_code: [0x03; 32],
            attributes: vec![0xA0],
        }],
        plutus_v1_scripts: vec![vec![0x11; 20]],
        plutus_data: vec![datum],
        redeemers: vec![Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(7),
            ex_units: ExUnits {
                mem: 500,
                steps: 1000,
            },
        }],
        plutus_v2_scripts: vec![vec![0x22; 16]],
        plutus_v3_scripts: vec![vec![0x33; 8]],
    };
    let bytes = wset.to_cbor_bytes();

    // Verify map count is 8 (all keys present)
    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("map header");
    assert_eq!(count, 8, "all 8 keys present in witness set");

    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("witness set all keys round-trip");
    assert_eq!(wset, decoded);
}

#[test]
fn witness_set_map_count_only_includes_nonempty_keys() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![ShelleyVkeyWitness {
            vkey: [0xAA; 32],
            signature: [0xBB; 64],
        }],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![vec![0x01; 20]],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("map header");
    assert_eq!(count, 2, "only keys 0 and 3 populated");
}

#[test]
fn witness_set_conway_map_redeemers_round_trip() {
    // Build Conway-style map-format redeemers: { [tag, index] => [data, ex_units] }

    // Redeemer data: integer 5
    let mut denc = Encoder::new();
    denc.unsigned(5);
    let rdata = denc.into_bytes();

    // ex_units: [300, 600]
    let mut eu_enc = Encoder::new();
    eu_enc.array(2).unsigned(300).unsigned(600);
    let eu_bytes = eu_enc.into_bytes();

    // Build the witness set with map-format redeemers via raw CBOR
    let mut ws_enc = Encoder::new();
    ws_enc.map(1); // one key in the witness set
    ws_enc.unsigned(5); // key 5 = redeemers
    // Conway map format: { [0, 0] => [data, [mem, steps]] }
    ws_enc.map(1);
    ws_enc.array(2).unsigned(0).unsigned(0); // key: [tag=0, index=0]
    ws_enc.array(2);
    ws_enc.raw(&rdata); // data
    ws_enc.raw(&eu_bytes); // ex_units

    let bytes = ws_enc.into_bytes();
    let decoded = ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("Conway map-format redeemers");
    assert_eq!(decoded.redeemers.len(), 1);
    assert_eq!(decoded.redeemers[0].tag, 0);
    assert_eq!(decoded.redeemers[0].index, 0);
    assert_eq!(decoded.redeemers[0].ex_units.mem, 300);
    assert_eq!(decoded.redeemers[0].ex_units.steps, 600);
}

#[test]
fn shelley_tx_cbor_round_trip_no_metadata() {
    let tx = ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x01; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 2_000_000,
            }],
            fee: 170_000,
            ttl: 30_000_000,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: ShelleyWitnessSet {
            vkey_witnesses: vec![ShelleyVkeyWitness {
                vkey: [0xAA; 32],
                signature: [0xBB; 64],
            }],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        auxiliary_data: None,
    };
    let bytes = tx.to_cbor_bytes();
    let decoded = ShelleyTx::from_cbor_bytes(&bytes).expect("ShelleyTx round-trip no metadata");
    assert_eq!(tx, decoded);
}

#[test]
fn shelley_tx_encoding_is_three_element_array() {
    // CDDL: transaction = [transaction_body, transaction_witness_set, metadata / nil]
    let tx = ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![],
            outputs: vec![],
            fee: 0,
            ttl: 0,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        auxiliary_data: None,
    };
    let bytes = tx.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("top-level array");
    assert_eq!(len, 3, "Shelley tx must be a 3-element array");
}

#[test]
fn shelley_submitted_tx_round_trip_preserves_id_and_raw_bytes() {
    let tx = ShelleyCompatibleSubmittedTx::new(
        ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x21; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 3_000_000,
            }],
            fee: 175_000,
            ttl: 31_000_000,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        Some(vec![0x81, 0x01]),
    );

    let bytes = tx.to_cbor_bytes();
    let decoded = ShelleyCompatibleSubmittedTx::<ShelleyTxBody>::from_cbor_bytes(&bytes)
        .expect("Shelley-compatible submitted tx round-trip");

    assert_eq!(decoded, tx);
    assert_eq!(decoded.raw_cbor, bytes);
    // Authoritative: tx_id is the hash of the on-wire body bytes.
    assert_eq!(decoded.tx_id(), compute_tx_id(&decoded.raw_body));
    // For canonically-encoded input, re-encoding produces the same bytes,
    // so the typed-fallback hash also matches.  See
    // `shelley_submitted_tx_id_uses_on_wire_bytes_not_re_encoded` for the
    // non-canonical case where these two diverge.
    assert_eq!(
        decoded.tx_id(),
        compute_tx_id(&decoded.body.to_cbor_bytes())
    );
}

#[test]
fn alonzo_submitted_tx_round_trip_preserves_id_and_raw_bytes() {
    let tx = AlonzoCompatibleSubmittedTx::new(
        AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x31; 32],
                index: 1,
            }],
            outputs: vec![AlonzoTxOut {
                address: vec![0x61; 28],
                amount: Value::Coin(2_500_000),
                datum_hash: None,
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
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        false,
        Some(vec![0x81, 0x02]),
    );

    let bytes = tx.to_cbor_bytes();
    let decoded = AlonzoCompatibleSubmittedTx::<AlonzoTxBody>::from_cbor_bytes(&bytes)
        .expect("Alonzo-compatible submitted tx round-trip");

    assert_eq!(decoded, tx);
    assert_eq!(decoded.raw_cbor, bytes);
    // Authoritative: tx_id is the hash of the on-wire body bytes.
    assert_eq!(decoded.tx_id(), compute_tx_id(&decoded.raw_body));
    // For canonically-encoded input both hashes agree; see the Shelley
    // non-canonical regression test for the divergent case.
    assert_eq!(
        decoded.tx_id(),
        compute_tx_id(&decoded.body.to_cbor_bytes())
    );
}

/// Regression guard: a wallet that submits a transaction whose CBOR body
/// uses a non-canonical encoding (here: an over-long `uint` for `fee`)
/// must still get the **same** `TxId` that any other Cardano implementation
/// would compute for it — namely, `blake2b-256(on-wire body bytes)`.
///
/// If `MultiEraSubmittedTx::Shelley::tx_id()` ever regresses to hashing
/// `body.to_cbor_bytes()` (the typed re-encode), this test fails because
/// the typed encoder produces canonical bytes whose hash differs from
/// the on-wire hash.  The mempool-eviction divergence motivated by this
/// guard is captured in the `extract_tx_ids` doc comment in
/// `node/src/sync.rs`.
///
/// Reference: `Cardano.Ledger.Core.txIdTxBody` — hashes the original wire
/// bytes, not a re-serialisation.
#[test]
fn shelley_submitted_tx_id_uses_on_wire_bytes_not_re_encoded() {
    // Canonical baseline.
    let tx = ShelleyCompatibleSubmittedTx::new(
        ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x77; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 1_500_000,
            }],
            fee: 175_000, // canonical CBOR uint32: 0x1A 00 02 AB 18
            ttl: 5_000,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        Some(vec![0x81, 0x01]),
    );

    let canonical_body = tx.body.to_cbor_bytes();
    // Splice a longer-than-necessary `uint` encoding for the `fee` field.
    // CBOR map entry: key 0x02 (fee) followed by value.
    // 175_000 = 0x0002AB98, so canonical is 0x1A 00 02 AB 98 (uint32, 5 bytes
    // payload).  Non-canonical equivalent: 0x1B 00 00 00 00 00 02 AB 98
    // (uint64, 9 bytes payload).  Both decode to the same value.
    let canonical_fee = [0x02_u8, 0x1A, 0x00, 0x02, 0xAB, 0x98];
    let non_canonical_fee = [
        0x02_u8, 0x1B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xAB, 0x98,
    ];
    let fee_pos = canonical_body
        .windows(canonical_fee.len())
        .position(|w| w == canonical_fee)
        .expect("canonical fee bytes present in encoded body");
    let mut non_canonical_body = canonical_body[..fee_pos].to_vec();
    non_canonical_body.extend_from_slice(&non_canonical_fee);
    non_canonical_body.extend_from_slice(&canonical_body[fee_pos + canonical_fee.len()..]);
    assert_ne!(
        non_canonical_body, canonical_body,
        "splice must change the body bytes"
    );

    // Re-build the on-wire tx envelope manually:
    //   [body, witness_set, aux_data] = 0x83 || body || ws || aux
    let canonical_full = tx.to_cbor_bytes();
    // Strip the canonical tx envelope down to its head + post-body suffix
    // so we can swap the body in.  `raw_body` from a round-trip decode of
    // the canonical bytes is a reliable way to find the body span.
    let canonical_decoded =
        ShelleyCompatibleSubmittedTx::<ShelleyTxBody>::from_cbor_bytes(&canonical_full)
            .expect("canonical round-trip");
    let body_start = canonical_full
        .windows(canonical_decoded.raw_body.len())
        .position(|w| w == canonical_decoded.raw_body.as_slice())
        .expect("body span must appear in tx envelope");
    let mut non_canonical_full = canonical_full[..body_start].to_vec();
    non_canonical_full.extend_from_slice(&non_canonical_body);
    non_canonical_full
        .extend_from_slice(&canonical_full[body_start + canonical_decoded.raw_body.len()..]);

    // Decode the non-canonical wire form.
    let decoded =
        ShelleyCompatibleSubmittedTx::<ShelleyTxBody>::from_cbor_bytes(&non_canonical_full)
            .expect("non-canonical Shelley tx must still decode");
    assert_eq!(
        decoded.body.fee, 175_000,
        "value preserved across encodings"
    );

    // The captured raw_body is the non-canonical bytes, NOT a re-encode.
    assert_eq!(decoded.raw_body, non_canonical_body);
    assert_ne!(
        decoded.raw_body,
        decoded.body.to_cbor_bytes(),
        "raw_body must differ from typed re-encoding for non-canonical input"
    );

    // tx_id hashes the on-wire body bytes (authoritative).
    assert_eq!(decoded.tx_id(), compute_tx_id(&non_canonical_body));
    // …and is NOT equal to the hash of the typed re-encoding.
    assert_ne!(
        decoded.tx_id(),
        compute_tx_id(&decoded.body.to_cbor_bytes()),
        "tx_id must use on-wire bytes; re-encoded hash would diverge \
         from every other Cardano implementation"
    );

    // The same property when accessed via MultiEraSubmittedTx.
    let multi = MultiEraSubmittedTx::Shelley(decoded.clone());
    assert_eq!(multi.tx_id(), compute_tx_id(&non_canonical_body));
}

#[test]
fn multi_era_submitted_tx_decodes_shelley_and_alonzo_shapes() {
    let shelley = ShelleyCompatibleSubmittedTx::new(
        ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x41; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 4_000_000,
            }],
            fee: 180_000,
            ttl: 32_000_000,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        None,
    )
    .to_cbor_bytes();

    let alonzo = AlonzoCompatibleSubmittedTx::new(
        AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x42; 32],
                index: 0,
            }],
            outputs: vec![AlonzoTxOut {
                address: vec![0x61; 28],
                amount: Value::Coin(5_000_000),
                datum_hash: None,
            }],
            fee: 260_000,
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
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        true,
        None,
    )
    .to_cbor_bytes();

    let shelley_decoded = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Shelley, &shelley)
        .expect("decode Shelley submitted tx");
    let alonzo_decoded = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Alonzo, &alonzo)
        .expect("decode Alonzo submitted tx");

    assert_eq!(shelley_decoded.era(), Era::Shelley);
    assert_eq!(alonzo_decoded.era(), Era::Alonzo);
    assert_eq!(
        shelley_decoded.tx_id(),
        compute_tx_id(&match &shelley_decoded {
            MultiEraSubmittedTx::Shelley(tx) => tx.body.to_cbor_bytes(),
            _ => unreachable!("decoded Shelley tx should stay Shelley"),
        })
    );
    assert_eq!(
        alonzo_decoded.tx_id(),
        compute_tx_id(&match &alonzo_decoded {
            MultiEraSubmittedTx::Alonzo(tx) => tx.body.to_cbor_bytes(),
            _ => unreachable!("decoded Alonzo tx should stay Alonzo"),
        })
    );
    assert_eq!(shelley_decoded.raw_cbor(), shelley);
    assert_eq!(alonzo_decoded.raw_cbor(), alonzo);
}

#[test]
fn shelley_tx_body_unknown_keys_skipped() {
    // Encode a body with an extra key 99 carrying a text value.
    // Decoder should skip unknown keys gracefully.
    let mut enc = Encoder::new();
    enc.map(5);
    enc.unsigned(0).array(0); // inputs (empty)
    enc.unsigned(1).array(0); // outputs (empty)
    enc.unsigned(2).unsigned(100); // fee
    enc.unsigned(3).unsigned(200); // ttl
    enc.unsigned(99).text("future extension"); // unknown key

    let bytes = enc.into_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("should skip unknown map key");
    assert_eq!(decoded.fee, 100);
    assert_eq!(decoded.ttl, 200);
    assert!(decoded.inputs.is_empty());
    assert!(decoded.outputs.is_empty());
    assert!(decoded.auxiliary_data_hash.is_none());
}

// ===========================================================================
// Shelley UTxO state transition tests
// ===========================================================================

/// Helper: seed a UTxO set with a single entry and return the matching TxIn.
fn seed_utxo(utxo: &mut ShelleyUtxo, tx_hash: [u8; 32], index: u16, amount: u64) -> ShelleyTxIn {
    let txin = ShelleyTxIn {
        transaction_id: tx_hash,
        index,
    };
    utxo.insert(
        txin.clone(),
        ShelleyTxOut {
            address: vec![0x61; 29],
            amount,
        },
    );
    txin
}

#[test]
fn utxo_apply_tx_happy_path() {
    let mut utxo = ShelleyUtxo::new();
    let genesis_hash = [0x01; 32];
    let _inp = seed_utxo(&mut utxo, genesis_hash, 0, 10_000_000);
    assert_eq!(utxo.len(), 1);

    let tx_id = [0xAA; 32];
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: genesis_hash,
            index: 0,
        }],
        outputs: vec![
            ShelleyTxOut {
                address: vec![0x00; 57],
                amount: 8_000_000,
            },
            ShelleyTxOut {
                address: vec![0x01; 57],
                amount: 1_800_000,
            },
        ],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    utxo.apply_tx(tx_id, &body, 500)
        .expect("valid tx should apply");

    // Original input consumed.
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: genesis_hash,
            index: 0
        })
        .is_none()
    );
    // Two new outputs produced.
    assert_eq!(utxo.len(), 2);
    assert_eq!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx_id,
            index: 0
        })
        .expect("output 0")
        .amount,
        8_000_000,
    );
    assert_eq!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx_id,
            index: 1
        })
        .expect("output 1")
        .amount,
        1_800_000,
    );
}

#[test]
fn utxo_rejects_expired_tx() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 4_800_000,
        }],
        fee: 200_000,
        ttl: 99,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let err = utxo
        .apply_tx([0xBB; 32], &body, 100)
        .expect_err("ttl = 99, slot = 100");
    assert_eq!(err, LedgerError::TxExpired { ttl: 99, slot: 100 });
    // UTxO unchanged.
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_rejects_missing_input() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xFF; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 4_800_000,
        }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let err = utxo
        .apply_tx([0xCC; 32], &body, 500)
        .expect_err("input not in utxo");
    assert_eq!(err, LedgerError::InputNotInUtxo);
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_rejects_value_not_preserved() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 10_000_000);

    // Output + fee > consumed (try to create value from thin air).
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 10_000_000,
        }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let err = utxo
        .apply_tx([0xDD; 32], &body, 500)
        .expect_err("value not preserved");
    assert_eq!(
        err,
        LedgerError::ValueNotPreserved {
            consumed: 10_000_000,
            produced: 10_000_000,
            fee: 200_000,
        }
    );
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_rejects_no_inputs() {
    let mut utxo = ShelleyUtxo::new();
    let body = ShelleyTxBody {
        inputs: vec![],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 0,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let err = utxo
        .apply_tx([0xEE; 32], &body, 500)
        .expect_err("no inputs");
    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn utxo_accepts_empty_outputs() {
    // Upstream has no `OutputSetEmptyUTxO` — CDDL allows `[* transaction_output]`.
    // A tx with inputs and zero outputs should be accepted (fee consumes all value).
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![],
        fee: 5_000_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx([0xFF; 32], &body, 500)
        .expect("empty outputs should be accepted");
    assert_eq!(utxo.len(), 0);
}

#[test]
fn utxo_ttl_equal_to_slot_is_valid() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 1_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 800_000,
        }],
        fee: 200_000,
        ttl: 500,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx([0xAA; 32], &body, 500)
        .expect("ttl == slot should be valid");
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_multi_input_multi_output() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 3_000_000);
    seed_utxo(&mut utxo, [0x02; 32], 0, 7_000_000);
    assert_eq!(utxo.len(), 2);

    let tx_id = [0xBB; 32];
    let body = ShelleyTxBody {
        inputs: vec![
            ShelleyTxIn {
                transaction_id: [0x01; 32],
                index: 0,
            },
            ShelleyTxIn {
                transaction_id: [0x02; 32],
                index: 0,
            },
        ],
        outputs: vec![
            ShelleyTxOut {
                address: vec![0x00; 57],
                amount: 4_000_000,
            },
            ShelleyTxOut {
                address: vec![0x01; 57],
                amount: 3_000_000,
            },
            ShelleyTxOut {
                address: vec![0x02; 57],
                amount: 2_500_000,
            },
        ],
        fee: 500_000,
        ttl: 10_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    utxo.apply_tx(tx_id, &body, 100)
        .expect("multi-input/output tx");
    // Both original inputs consumed, 3 new outputs created.
    assert_eq!(utxo.len(), 3);
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0
        })
        .is_none()
    );
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0
        })
        .is_none()
    );
}

#[test]
fn utxo_chained_transactions() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x00; 32], 0, 50_000_000);

    // Tx 1: spend genesis, produce two outputs.
    let tx1_id = [0x11; 32];
    let body1 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x00; 32],
            index: 0,
        }],
        outputs: vec![
            ShelleyTxOut {
                address: vec![0xA0; 57],
                amount: 30_000_000,
            },
            ShelleyTxOut {
                address: vec![0xB0; 57],
                amount: 19_800_000,
            },
        ],
        fee: 200_000,
        ttl: 10_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx(tx1_id, &body1, 100).expect("tx1");
    assert_eq!(utxo.len(), 2);

    // Tx 2: spend first output of tx1.
    let tx2_id = [0x22; 32];
    let body2 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: tx1_id,
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0xC0; 57],
            amount: 29_700_000,
        }],
        fee: 300_000,
        ttl: 10_000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx(tx2_id, &body2, 200).expect("tx2");
    // tx1 output 0 consumed, tx1 output 1 still there, tx2 output 0 added.
    assert_eq!(utxo.len(), 2);
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx1_id,
            index: 0
        })
        .is_none()
    );
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx1_id,
            index: 1
        })
        .is_some()
    );
    assert!(
        utxo.get(&ShelleyTxIn {
            transaction_id: tx2_id,
            index: 0
        })
        .is_some()
    );
}

// ===========================================================================
// Shelley header and block — CBOR round-trip tests
// ===========================================================================

/// Helper: build a sample VRF cert with deterministic bytes.
fn sample_vrf_cert(seed: u8) -> ShelleyVrfCert {
    ShelleyVrfCert {
        output: vec![seed; 32],
        proof: [seed.wrapping_add(1); 80],
    }
}

/// Helper: build a sample opcert.
fn sample_opcert(seed: u8) -> ShelleyOpCert {
    ShelleyOpCert {
        hot_vkey: [seed; 32],
        sequence_number: 42,
        kes_period: 100,
        sigma: [seed.wrapping_add(2); 64],
    }
}

/// Helper: build a full sample header body.
fn sample_header_body() -> ShelleyHeaderBody {
    ShelleyHeaderBody {
        block_number: 1,
        slot: 500,
        prev_hash: Some([0xAA; 32]),
        issuer_vkey: [0x11; 32],
        vrf_vkey: [0x22; 32],
        nonce_vrf: sample_vrf_cert(0x30),
        leader_vrf: sample_vrf_cert(0x40),
        block_body_size: 1024,
        block_body_hash: [0x55; 32],
        operational_cert: sample_opcert(0x60),
        protocol_version: (2, 0),
    }
}

#[test]
fn shelley_vrf_cert_cbor_round_trip() {
    let cert = sample_vrf_cert(0xAA);
    let bytes = cert.to_cbor_bytes();
    let decoded = ShelleyVrfCert::from_cbor_bytes(&bytes).expect("VrfCert round-trip");
    assert_eq!(cert, decoded);
}

#[test]
fn shelley_opcert_cbor_round_trip() {
    // OpCert is a group, so we encode/decode inside an array wrapper.
    let oc = sample_opcert(0xBB);
    let mut enc = Encoder::new();
    enc.array(4);
    oc.encode_fields(&mut enc);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array header");
    assert_eq!(len, 4);
    let decoded = ShelleyOpCert::decode_fields(&mut dec).expect("OpCert decode");
    assert!(dec.is_empty());
    assert_eq!(oc, decoded);
}

#[test]
fn shelley_header_body_cbor_round_trip() {
    let hb = sample_header_body();
    let bytes = hb.to_cbor_bytes();
    let decoded = ShelleyHeaderBody::from_cbor_bytes(&bytes).expect("HeaderBody round-trip");
    assert_eq!(hb, decoded);
}

#[test]
fn shelley_header_body_is_15_element_array() {
    let hb = sample_header_body();
    let bytes = hb.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array header");
    assert_eq!(len, 15, "Shelley header_body must be 15-element array");
}

#[test]
fn shelley_header_body_genesis_prev_hash() {
    let mut hb = sample_header_body();
    hb.prev_hash = None;
    let bytes = hb.to_cbor_bytes();
    let decoded = ShelleyHeaderBody::from_cbor_bytes(&bytes).expect("genesis prev_hash");
    assert_eq!(decoded.prev_hash, None);
}

#[test]
fn shelley_header_cbor_round_trip() {
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xEE; 448],
    };
    let bytes = header.to_cbor_bytes();
    let decoded = ShelleyHeader::from_cbor_bytes(&bytes).expect("Header round-trip");
    assert_eq!(header, decoded);
}

#[test]
fn shelley_block_cbor_round_trip_no_txs() {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let bytes = block.to_cbor_bytes();
    let decoded = ShelleyBlock::from_cbor_bytes(&bytes).expect("Block no-txs round-trip");
    assert_eq!(block, decoded);
}

#[test]
fn shelley_block_cbor_round_trip_with_txs() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x00; 57],
            amount: 1_000_000,
        }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![ShelleyVkeyWitness {
            vkey: [0xAA; 32],
            signature: [0xBB; 64],
        }],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xCC; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ws],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let bytes = block.to_cbor_bytes();
    let decoded = ShelleyBlock::from_cbor_bytes(&bytes).expect("Block with-txs round-trip");
    assert_eq!(block, decoded);
}

#[test]
fn shelley_block_is_four_element_array() {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let bytes = block.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("block array header");
    assert_eq!(len, 4, "Shelley block must be 4-element array");
}
