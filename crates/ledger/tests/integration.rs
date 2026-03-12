use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, CborDecode, CborEncode, Decoder, Encoder, Era, HeaderHash,
    LedgerError, LedgerState, Nonce, Point, ShelleyBlock, ShelleyHeader, ShelleyHeaderBody,
    ShelleyOpCert, ShelleyTx, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyUtxo,
    ShelleyVkeyWitness, ShelleyVrfCert, ShelleyWitnessSet, SlotNo, TxId,
};

#[test]
fn applies_block_for_matching_era() {
    let mut state = LedgerState::new(Era::Shelley);
    assert_eq!(state.tip, Point::Origin);

    let header_hash = HeaderHash([0xAA; 32]);
    let block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: header_hash,
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: Vec::new(),
    };

    state
        .apply_block(&block)
        .expect("matching era block should apply to ledger state");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(42), header_hash));
}

#[test]
fn rejects_block_for_mismatched_era() {
    let mut state = LedgerState::new(Era::Shelley);
    let block = Block {
        era: Era::Byron,
        header: BlockHeader {
            hash: HeaderHash([0xBB; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(1),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: Vec::new(),
    };

    let err = state
        .apply_block(&block)
        .expect_err("mismatched era should be rejected");
    assert_eq!(
        err,
        yggdrasil_ledger::LedgerError::UnsupportedEra(Era::Byron)
    );
    assert_eq!(state.tip, Point::Origin);
}

#[test]
fn point_accessors() {
    let origin = Point::Origin;
    assert!(origin.slot().is_none());
    assert!(origin.hash().is_none());

    let hash = HeaderHash([0xCC; 32]);
    let bp = Point::BlockPoint(SlotNo(100), hash);
    assert_eq!(bp.slot(), Some(SlotNo(100)));
    assert_eq!(bp.hash(), Some(hash));
}

// ═══════════════════════════════════════════════════════════════════════════
// CBOR round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cbor_slot_no_round_trip() {
    for &v in &[0u64, 1, 23, 24, 255, 256, 65535, 65536, u32::MAX as u64, u64::MAX] {
        let slot = SlotNo(v);
        let encoded = slot.to_cbor_bytes();
        let decoded = SlotNo::from_cbor_bytes(&encoded).expect("SlotNo CBOR round-trip");
        assert_eq!(slot, decoded, "SlotNo({v}) failed round-trip");
    }
}

#[test]
fn cbor_block_no_round_trip() {
    let b = BlockNo(42);
    let decoded = BlockNo::from_cbor_bytes(&b.to_cbor_bytes()).expect("BlockNo CBOR round-trip");
    assert_eq!(b, decoded);
}

#[test]
fn cbor_header_hash_round_trip() {
    let h = HeaderHash([0xAB; 32]);
    let decoded =
        HeaderHash::from_cbor_bytes(&h.to_cbor_bytes()).expect("HeaderHash CBOR round-trip");
    assert_eq!(h, decoded);
}

#[test]
fn cbor_tx_id_round_trip() {
    let t = TxId([0xCD; 32]);
    let decoded = TxId::from_cbor_bytes(&t.to_cbor_bytes()).expect("TxId CBOR round-trip");
    assert_eq!(t, decoded);
}

#[test]
fn cbor_point_origin_round_trip() {
    let p = Point::Origin;
    let decoded = Point::from_cbor_bytes(&p.to_cbor_bytes()).expect("Point::Origin CBOR");
    assert_eq!(p, decoded);
}

#[test]
fn cbor_point_block_round_trip() {
    let p = Point::BlockPoint(SlotNo(12345), HeaderHash([0xFF; 32]));
    let decoded = Point::from_cbor_bytes(&p.to_cbor_bytes()).expect("Point::BlockPoint CBOR");
    assert_eq!(p, decoded);
}

#[test]
fn cbor_nonce_neutral_round_trip() {
    let n = Nonce::Neutral;
    let decoded = Nonce::from_cbor_bytes(&n.to_cbor_bytes()).expect("Nonce::Neutral CBOR");
    assert_eq!(n, decoded);
}

#[test]
fn cbor_nonce_hash_round_trip() {
    let n = Nonce::Hash([0x42; 32]);
    let decoded = Nonce::from_cbor_bytes(&n.to_cbor_bytes()).expect("Nonce::Hash CBOR");
    assert_eq!(n, decoded);
}

#[test]
fn cbor_unsigned_encoding_is_canonical() {
    // Values under 24 encode in a single byte (major type 0 + value)
    let encoded = SlotNo(0).to_cbor_bytes();
    assert_eq!(encoded, vec![0x00]);

    let encoded = SlotNo(23).to_cbor_bytes();
    assert_eq!(encoded, vec![0x17]);

    // 24 requires additional byte
    let encoded = SlotNo(24).to_cbor_bytes();
    assert_eq!(encoded, vec![0x18, 0x18]);

    // 256 requires two additional bytes
    let encoded = SlotNo(256).to_cbor_bytes();
    assert_eq!(encoded, vec![0x19, 0x01, 0x00]);
}

#[test]
fn cbor_trailing_bytes_rejected() {
    let mut bytes = SlotNo(1).to_cbor_bytes();
    bytes.push(0xFF); // trailing junk
    let err = SlotNo::from_cbor_bytes(&bytes).expect_err("should reject trailing bytes");
    assert!(
        matches!(err, yggdrasil_ledger::LedgerError::CborTrailingBytes(1)),
        "expected CborTrailingBytes(1), got {err:?}"
    );
}

#[test]
fn cbor_type_mismatch_detected() {
    // Encode a byte string, try to decode as unsigned integer
    let hash_bytes = HeaderHash([0; 32]).to_cbor_bytes();
    let err = SlotNo::from_cbor_bytes(&hash_bytes).expect_err("should reject type mismatch");
    assert!(
        matches!(
            err,
            yggdrasil_ledger::LedgerError::CborTypeMismatch {
                expected: 0,
                actual: 2
            }
        ),
        "expected CborTypeMismatch, got {err:?}"
    );
}

#[test]
fn cbor_short_hash_rejected() {
    // Encode a 16-byte bstr, try to decode as HeaderHash (needs 32)
    let mut enc = yggdrasil_ledger::Encoder::new();
    enc.bytes(&[0xAA; 16]);
    let err = HeaderHash::from_cbor_bytes(enc.as_bytes())
        .expect_err("should reject short hash");
    assert!(
        matches!(
            err,
            yggdrasil_ledger::LedgerError::CborInvalidLength {
                expected: 32,
                actual: 16
            }
        ),
        "expected CborInvalidLength, got {err:?}"
    );
}

// ===========================================================================
// CBOR extensions: text, map, negative, bool decode, skip
// ===========================================================================

#[test]
fn cbor_text_round_trip() {
    let mut enc = Encoder::new();
    enc.text("hello, cardano!");
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let s = dec.text().expect("decode text");
    assert_eq!(s, "hello, cardano!");
    assert!(dec.is_empty());
}

#[test]
fn cbor_text_empty_string() {
    let mut enc = Encoder::new();
    enc.text("");
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let s = dec.text().expect("decode empty text");
    assert_eq!(s, "");
}

#[test]
fn cbor_map_round_trip() {
    let mut enc = Encoder::new();
    enc.map(2);
    enc.unsigned(14).text("version fourteen");
    enc.unsigned(15).text("version fifteen");
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let count = dec.map().expect("decode map");
    assert_eq!(count, 2);
    assert_eq!(dec.unsigned().expect("key1"), 14);
    assert_eq!(dec.text().expect("val1"), "version fourteen");
    assert_eq!(dec.unsigned().expect("key2"), 15);
    assert_eq!(dec.text().expect("val2"), "version fifteen");
    assert!(dec.is_empty());
}

#[test]
fn cbor_negative_round_trip() {
    let mut enc = Encoder::new();
    // Encode -1 (n=0), -100 (n=99), -256 (n=255)
    enc.negative(0).negative(99).negative(255);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.negative().expect("n=0"), 0);  // represents -1
    assert_eq!(dec.negative().expect("n=99"), 99); // represents -100
    assert_eq!(dec.negative().expect("n=255"), 255); // represents -256
    assert!(dec.is_empty());
}

#[test]
fn cbor_bool_decode() {
    let mut enc = Encoder::new();
    enc.bool(false).bool(true);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    assert!(!dec.bool().expect("decode false"));
    assert!(dec.bool().expect("decode true"));
    assert!(dec.is_empty());
}

#[test]
fn cbor_skip_primitives() {
    let mut enc = Encoder::new();
    enc.unsigned(42).text("skip me").bytes(&[1, 2, 3]).bool(true);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    // Skip all four items
    dec.skip().expect("skip unsigned");
    dec.skip().expect("skip text");
    dec.skip().expect("skip bytes");
    dec.skip().expect("skip bool");
    assert!(dec.is_empty());
}

#[test]
fn cbor_skip_nested_structures() {
    let mut enc = Encoder::new();
    // Array [1, [2, 3], "hello"]
    enc.array(3).unsigned(1);
    enc.array(2).unsigned(2).unsigned(3);
    enc.text("hello");

    // Map {0: "a", 1: "b"}
    enc.map(2);
    enc.unsigned(0).text("a");
    enc.unsigned(1).text("b");

    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    dec.skip().expect("skip nested array");
    dec.skip().expect("skip nested map");
    assert!(dec.is_empty());
}

#[test]
fn cbor_raw_passthrough() {
    // Encode a value, then insert it as raw bytes into another encoder
    let inner = SlotNo(999).to_cbor_bytes();

    let mut enc = Encoder::new();
    enc.array(2).raw(&inner).unsigned(1);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("array");
    assert_eq!(len, 2);
    let slot = SlotNo::decode_cbor(&mut dec).expect("decode slot");
    assert_eq!(slot, SlotNo(999));
    assert_eq!(dec.unsigned().expect("trailing uint"), 1);
}

// ===========================================================================
// CBOR tag encode/decode
// ===========================================================================

#[test]
fn cbor_tag_round_trip() {
    // Encode tag 258 wrapping an array of two uints (simulating a tagged set).
    let mut enc = Encoder::new();
    enc.tag(258).array(2).unsigned(10).unsigned(20);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let tag_num = dec.tag().expect("decode tag");
    assert_eq!(tag_num, 258);
    let count = dec.array().expect("decode array");
    assert_eq!(count, 2);
    assert_eq!(dec.unsigned().expect("first"), 10);
    assert_eq!(dec.unsigned().expect("second"), 20);
    assert!(dec.is_empty());
}

#[test]
fn cbor_tag_24_encoded_cbor() {
    // Tag 24 wraps an embedded CBOR byte string.
    let inner_cbor = {
        let mut e = Encoder::new();
        e.unsigned(42);
        e.into_bytes()
    };

    let mut enc = Encoder::new();
    enc.tag(24).bytes(&inner_cbor);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let tag = dec.tag().expect("tag");
    assert_eq!(tag, 24);
    let embedded = dec.bytes().expect("embedded bytes");
    let mut inner_dec = Decoder::new(embedded);
    assert_eq!(inner_dec.unsigned().expect("inner uint"), 42);
}

// ===========================================================================
// Shelley transaction types — CBOR round-trip tests
// ===========================================================================

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
        bootstrap_witnesses: vec![],
    };
    let bytes = wset.to_cbor_bytes();
    let decoded =
        ShelleyWitnessSet::from_cbor_bytes(&bytes).expect("ShelleyWitnessSet round-trip");
    assert_eq!(wset, decoded);
}

#[test]
fn shelley_witness_set_empty_cbor_round_trip() {
    let wset = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        bootstrap_witnesses: vec![],
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
            auxiliary_data_hash: None,
        },
        witness_set: ShelleyWitnessSet {
            vkey_witnesses: vec![ShelleyVkeyWitness {
                vkey: [0xAA; 32],
                signature: [0xBB; 64],
            }],
            bootstrap_witnesses: vec![],
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
            auxiliary_data_hash: None,
        },
        witness_set: ShelleyWitnessSet {
            vkey_witnesses: vec![],
            bootstrap_witnesses: vec![],
        },
        auxiliary_data: None,
    };
    let bytes = tx.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("top-level array");
    assert_eq!(len, 3, "Shelley tx must be a 3-element array");
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
    let decoded =
        ShelleyTxBody::from_cbor_bytes(&bytes).expect("should skip unknown map key");
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
        auxiliary_data_hash: None,
    };

    utxo.apply_tx(tx_id, &body, 500)
        .expect("valid tx should apply");

    // Original input consumed.
    assert!(utxo.get(&ShelleyTxIn { transaction_id: genesis_hash, index: 0 }).is_none());
    // Two new outputs produced.
    assert_eq!(utxo.len(), 2);
    assert_eq!(
        utxo.get(&ShelleyTxIn { transaction_id: tx_id, index: 0 })
            .expect("output 0")
            .amount,
        8_000_000,
    );
    assert_eq!(
        utxo.get(&ShelleyTxIn { transaction_id: tx_id, index: 1 })
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
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 4_800_000 }],
        fee: 200_000,
        ttl: 99,
        auxiliary_data_hash: None,
    };

    let err = utxo.apply_tx([0xBB; 32], &body, 100).expect_err("ttl = 99, slot = 100");
    assert_eq!(err, LedgerError::TxExpired { ttl: 99, slot: 100 });
    // UTxO unchanged.
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_rejects_missing_input() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0xFF; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        auxiliary_data_hash: None,
    };

    let err = utxo.apply_tx([0xCC; 32], &body, 500).expect_err("input not in utxo");
    assert_eq!(err, LedgerError::InputNotInUtxo);
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_rejects_value_not_preserved() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 10_000_000);

    // Output + fee > consumed (try to create value from thin air).
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 10_000_000 }],
        fee: 200_000,
        ttl: 1000,
        auxiliary_data_hash: None,
    };

    let err = utxo.apply_tx([0xDD; 32], &body, 500).expect_err("value not preserved");
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
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 0 }],
        fee: 0,
        ttl: 1000,
        auxiliary_data_hash: None,
    };
    let err = utxo.apply_tx([0xEE; 32], &body, 500).expect_err("no inputs");
    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn utxo_rejects_no_outputs() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![],
        fee: 5_000_000,
        ttl: 1000,
        auxiliary_data_hash: None,
    };
    let err = utxo.apply_tx([0xFF; 32], &body, 500).expect_err("no outputs");
    assert_eq!(err, LedgerError::NoOutputs);
    assert_eq!(utxo.len(), 1);
}

#[test]
fn utxo_ttl_equal_to_slot_is_valid() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x01; 32], 0, 1_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 800_000 }],
        fee: 200_000,
        ttl: 500,
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
            ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
            ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        ],
        outputs: vec![
            ShelleyTxOut { address: vec![0x00; 57], amount: 4_000_000 },
            ShelleyTxOut { address: vec![0x01; 57], amount: 3_000_000 },
            ShelleyTxOut { address: vec![0x02; 57], amount: 2_500_000 },
        ],
        fee: 500_000,
        ttl: 10_000,
        auxiliary_data_hash: None,
    };

    utxo.apply_tx(tx_id, &body, 100).expect("multi-input/output tx");
    // Both original inputs consumed, 3 new outputs created.
    assert_eq!(utxo.len(), 3);
    assert!(utxo.get(&ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }).is_none());
    assert!(utxo.get(&ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }).is_none());
}

#[test]
fn utxo_chained_transactions() {
    let mut utxo = ShelleyUtxo::new();
    seed_utxo(&mut utxo, [0x00; 32], 0, 50_000_000);

    // Tx 1: spend genesis, produce two outputs.
    let tx1_id = [0x11; 32];
    let body1 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x00; 32], index: 0 }],
        outputs: vec![
            ShelleyTxOut { address: vec![0xA0; 57], amount: 30_000_000 },
            ShelleyTxOut { address: vec![0xB0; 57], amount: 19_800_000 },
        ],
        fee: 200_000,
        ttl: 10_000,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx(tx1_id, &body1, 100).expect("tx1");
    assert_eq!(utxo.len(), 2);

    // Tx 2: spend first output of tx1.
    let tx2_id = [0x22; 32];
    let body2 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: tx1_id, index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0xC0; 57], amount: 29_700_000 }],
        fee: 300_000,
        ttl: 10_000,
        auxiliary_data_hash: None,
    };
    utxo.apply_tx(tx2_id, &body2, 200).expect("tx2");
    // tx1 output 0 consumed, tx1 output 1 still there, tx2 output 0 added.
    assert_eq!(utxo.len(), 2);
    assert!(utxo.get(&ShelleyTxIn { transaction_id: tx1_id, index: 0 }).is_none());
    assert!(utxo.get(&ShelleyTxIn { transaction_id: tx1_id, index: 1 }).is_some());
    assert!(utxo.get(&ShelleyTxIn { transaction_id: tx2_id, index: 0 }).is_some());
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
        body_size: 1024,
        body_hash: [0x55; 32],
        opcert: sample_opcert(0x60),
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
        witness_sets: vec![],
        transaction_metadata: std::collections::HashMap::new(),
    };
    let bytes = block.to_cbor_bytes();
    let decoded = ShelleyBlock::from_cbor_bytes(&bytes).expect("Block no-txs round-trip");
    assert_eq!(block, decoded);
}

#[test]
fn shelley_block_cbor_round_trip_with_txs() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x00; 57], amount: 1_000_000 }],
        fee: 200_000,
        ttl: 1000,
        auxiliary_data_hash: None,
    };
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![ShelleyVkeyWitness {
            vkey: [0xAA; 32],
            signature: [0xBB; 64],
        }],
        bootstrap_witnesses: vec![],
    };
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xCC; 448],
        },
        transaction_bodies: vec![body],
        witness_sets: vec![ws],
        transaction_metadata: std::collections::HashMap::new(),
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
        witness_sets: vec![],
        transaction_metadata: std::collections::HashMap::new(),
    };
    let bytes = block.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let len = dec.array().expect("block array header");
    assert_eq!(len, 4, "Shelley block must be 4-element array");
}
