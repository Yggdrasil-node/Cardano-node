use super::*;

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
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block(&block)
        .expect("matching era block should apply to ledger state");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(42), header_hash));
}

#[test]
fn byron_block_advances_tip_without_state_transition() {
    let mut state = LedgerState::new(Era::Shelley);
    assert_eq!(state.tip, Point::Origin);
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
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block(&block)
        .expect("byron blocks should advance the tip as a no-op transition");
    assert_eq!(
        state.tip,
        Point::BlockPoint(SlotNo(1), HeaderHash([0xBB; 32]))
    );
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
    for &v in &[
        0u64,
        1,
        23,
        24,
        255,
        256,
        65535,
        65536,
        u32::MAX as u64,
        u64::MAX,
    ] {
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
    let err = HeaderHash::from_cbor_bytes(enc.as_bytes()).expect_err("should reject short hash");
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
    assert_eq!(dec.negative().expect("n=0"), 0); // represents -1
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
    enc.unsigned(42)
        .text("skip me")
        .bytes(&[1, 2, 3])
        .bool(true);
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
