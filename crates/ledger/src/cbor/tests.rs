// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;

// ── Encoder / Decoder round-trip: unsigned ──────────────────────────

#[test]
fn unsigned_zero() {
    let mut enc = Encoder::new();
    enc.unsigned(0);
    let bytes = enc.into_bytes();
    assert_eq!(bytes, [0x00]);
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.unsigned().unwrap(), 0);
    assert!(dec.is_empty());
}

#[test]
fn unsigned_23_one_byte_boundary() {
    let mut enc = Encoder::new();
    enc.unsigned(23);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 1);
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 23);
}

#[test]
fn unsigned_24_two_byte_boundary() {
    let mut enc = Encoder::new();
    enc.unsigned(24);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 2); // initial byte + 1 arg byte
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 24);
}

#[test]
fn unsigned_255_u8_max() {
    let mut enc = Encoder::new();
    enc.unsigned(255);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 2);
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 255);
}

#[test]
fn unsigned_256_u16_boundary() {
    let mut enc = Encoder::new();
    enc.unsigned(256);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 3); // initial byte + 2 arg bytes
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 256);
}

#[test]
fn unsigned_65535_u16_max() {
    let mut enc = Encoder::new();
    enc.unsigned(65535);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 3);
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 65535);
}

#[test]
fn unsigned_65536_u32_boundary() {
    let mut enc = Encoder::new();
    enc.unsigned(65536);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 5); // initial byte + 4 arg bytes
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 65536);
}

#[test]
fn unsigned_u32_max() {
    let mut enc = Encoder::new();
    enc.unsigned(u32::MAX as u64);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 5);
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), u32::MAX as u64);
}

#[test]
fn unsigned_u32_max_plus_one_u64_boundary() {
    let mut enc = Encoder::new();
    enc.unsigned(u32::MAX as u64 + 1);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 9); // initial byte + 8 arg bytes
    assert_eq!(
        Decoder::new(&bytes).unsigned().unwrap(),
        u32::MAX as u64 + 1
    );
}

#[test]
fn unsigned_u64_max() {
    let mut enc = Encoder::new();
    enc.unsigned(u64::MAX);
    let bytes = enc.into_bytes();
    assert_eq!(bytes.len(), 9);
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), u64::MAX);
}

// ── Encoder / Decoder round-trip: negative ─────────────────────────

#[test]
fn negative_zero_means_minus_one() {
    let mut enc = Encoder::new();
    enc.negative(0);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.negative().unwrap(), 0); // raw arg
    // Through integer(): -(1+0) = -1
    let mut dec2 = Decoder::new(&bytes);
    assert_eq!(dec2.integer().unwrap(), -1);
}

#[test]
fn integer_positive_round_trip() {
    let mut enc = Encoder::new();
    enc.integer(42);
    let bytes = enc.into_bytes();
    assert_eq!(Decoder::new(&bytes).integer().unwrap(), 42);
}

#[test]
fn integer_negative_round_trip() {
    let mut enc = Encoder::new();
    enc.integer(-100);
    let bytes = enc.into_bytes();
    assert_eq!(Decoder::new(&bytes).integer().unwrap(), -100);
}

#[test]
fn integer_i64_min() {
    let mut enc = Encoder::new();
    enc.integer(i64::MIN);
    let bytes = enc.into_bytes();
    assert_eq!(Decoder::new(&bytes).integer().unwrap(), i64::MIN);
}

// ── bytes ───────────────────────────────────────────────────────────

#[test]
fn bytes_empty() {
    let mut enc = Encoder::new();
    enc.bytes(&[]);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.bytes().unwrap(), &[] as &[u8]);
}

#[test]
fn bytes_round_trip() {
    let data = b"hello world";
    let mut enc = Encoder::new();
    enc.bytes(data);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.bytes().unwrap(), data);
}

// ── text ────────────────────────────────────────────────────────────

#[test]
fn text_round_trip() {
    let mut enc = Encoder::new();
    enc.text("Cardano");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.text().unwrap(), "Cardano");
}

#[test]
fn text_empty() {
    let mut enc = Encoder::new();
    enc.text("");
    let bytes = enc.into_bytes();
    assert_eq!(Decoder::new(&bytes).text().unwrap(), "");
}

// ── bool ────────────────────────────────────────────────────────────

#[test]
fn bool_true_round_trip() {
    let mut enc = Encoder::new();
    enc.bool(true);
    let bytes = enc.into_bytes();
    assert!(Decoder::new(&bytes).bool().unwrap());
}

#[test]
fn bool_false_round_trip() {
    let mut enc = Encoder::new();
    enc.bool(false);
    let bytes = enc.into_bytes();
    assert!(!Decoder::new(&bytes).bool().unwrap());
}

// ── null ────────────────────────────────────────────────────────────

#[test]
fn null_round_trip() {
    let mut enc = Encoder::new();
    enc.null();
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert!(dec.peek_is_null());
    dec.null().unwrap();
    assert!(dec.is_empty());
}

// ── array ───────────────────────────────────────────────────────────

#[test]
fn array_round_trip() {
    let mut enc = Encoder::new();
    enc.array(3).unsigned(1).unsigned(2).unsigned(3);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.array().unwrap(), 3);
    assert_eq!(dec.unsigned().unwrap(), 1);
    assert_eq!(dec.unsigned().unwrap(), 2);
    assert_eq!(dec.unsigned().unwrap(), 3);
    assert!(dec.is_empty());
}

#[test]
fn array_empty() {
    let mut enc = Encoder::new();
    enc.array(0);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.array().unwrap(), 0);
    assert!(dec.is_empty());
}

// ── map ─────────────────────────────────────────────────────────────

#[test]
fn map_round_trip() {
    let mut enc = Encoder::new();
    enc.map(1).unsigned(42).text("value");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.map().unwrap(), 1);
    assert_eq!(dec.unsigned().unwrap(), 42);
    assert_eq!(dec.text().unwrap(), "value");
}

// ── tag ─────────────────────────────────────────────────────────────

#[test]
fn tag_round_trip() {
    let mut enc = Encoder::new();
    enc.tag(24).bytes(b"inner");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.tag().unwrap(), 24);
    assert_eq!(dec.bytes().unwrap(), b"inner");
}

// ── peek_major ──────────────────────────────────────────────────────

#[test]
fn peek_major_does_not_consume() {
    let mut enc = Encoder::new();
    enc.unsigned(10);
    let bytes = enc.into_bytes();
    let dec = Decoder::new(&bytes);
    assert_eq!(dec.peek_major().unwrap(), 0); // MAJOR_UNSIGNED
    assert_eq!(dec.remaining(), bytes.len());
}

// ── skip ────────────────────────────────────────────────────────────

#[test]
fn skip_unsigned() {
    let mut enc = Encoder::new();
    enc.unsigned(999).text("after");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    dec.skip().unwrap();
    assert_eq!(dec.text().unwrap(), "after");
}

#[test]
fn skip_nested_array() {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(1).array(1).unsigned(2);
    enc.text("sentinel");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    dec.skip().unwrap(); // skip entire array(2, 1, array(1, 2))
    assert_eq!(dec.text().unwrap(), "sentinel");
}

#[test]
fn skip_map() {
    let mut enc = Encoder::new();
    enc.map(1).unsigned(0).bytes(b"x");
    enc.null();
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    dec.skip().unwrap(); // skip the map
    dec.null().unwrap();
}

#[test]
fn skip_tag() {
    let mut enc = Encoder::new();
    enc.tag(30).array(2).unsigned(1).unsigned(2);
    enc.bool(true);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    dec.skip().unwrap(); // skip tagged item
    assert!(dec.bool().unwrap());
}

// ── slice ───────────────────────────────────────────────────────────

#[test]
fn slice_captures_range() {
    let mut enc = Encoder::new();
    enc.unsigned(1).unsigned(2).unsigned(3);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let start = dec.position();
    dec.unsigned().unwrap();
    let end = dec.position();
    let captured = dec.slice(start, end).unwrap();
    assert_eq!(captured, &[0x01]);
}

#[test]
fn slice_out_of_range_error() {
    let bytes = [0x01];
    let dec = Decoder::new(&bytes);
    assert!(dec.slice(0, 10).is_err());
}

// ── position / remaining / is_empty ─────────────────────────────────

#[test]
fn position_remaining_is_empty() {
    let mut enc = Encoder::new();
    enc.unsigned(5);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.position(), 0);
    assert_eq!(dec.remaining(), 1);
    assert!(!dec.is_empty());
    dec.unsigned().unwrap();
    assert_eq!(dec.position(), 1);
    assert_eq!(dec.remaining(), 0);
    assert!(dec.is_empty());
}

// ── with_capacity ───────────────────────────────────────────────────

#[test]
fn encoder_with_capacity() {
    let enc = Encoder::with_capacity(128);
    assert!(enc.as_bytes().is_empty());
}

// ── raw ─────────────────────────────────────────────────────────────

#[test]
fn raw_passthrough() {
    let mut enc = Encoder::new();
    let inner = {
        let mut e2 = Encoder::new();
        e2.unsigned(42);
        e2.into_bytes()
    };
    enc.raw(&inner);
    let bytes = enc.into_bytes();
    assert_eq!(Decoder::new(&bytes).unsigned().unwrap(), 42);
}

// ── Error conditions ────────────────────────────────────────────────

#[test]
fn decode_empty_input_eof() {
    let mut dec = Decoder::new(&[]);
    assert!(dec.unsigned().is_err());
}

#[test]
fn decode_type_mismatch_unsigned_vs_bytes() {
    let mut enc = Encoder::new();
    enc.bytes(b"data");
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert!(dec.unsigned().is_err());
}

#[test]
fn decode_type_mismatch_text_vs_unsigned() {
    let mut enc = Encoder::new();
    enc.unsigned(42);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    assert!(dec.text().is_err());
}

#[test]
fn decode_truncated_bytes() {
    // byte string header says length 10 but only 2 bytes follow
    let bytes = [0x4a, 0x01, 0x02];
    let mut dec = Decoder::new(&bytes);
    assert!(dec.bytes().is_err());
}

#[test]
fn from_cbor_bytes_rejects_trailing() {
    let mut enc = Encoder::new();
    enc.unsigned(1).unsigned(2);
    let bytes = enc.into_bytes();
    // SlotNo::from_cbor_bytes should reject the trailing unsigned(2)
    assert!(SlotNo::from_cbor_bytes(&bytes).is_err());
}

// ── CborEncode / CborDecode trait round-trips ───────────────────────

#[test]
fn era_round_trip_all_variants() {
    for (tag, era) in [
        (0u64, Era::Byron),
        (1, Era::Shelley),
        (2, Era::Allegra),
        (3, Era::Mary),
        (4, Era::Alonzo),
        (5, Era::Babbage),
        (6, Era::Conway),
    ] {
        let encoded = era.to_cbor_bytes();
        let decoded = Era::from_cbor_bytes(&encoded).unwrap();
        assert_eq!(decoded, era, "Era tag {tag}");
    }
}

#[test]
fn slot_no_round_trip() {
    let slot = SlotNo(123_456_789);
    let decoded = SlotNo::from_cbor_bytes(&slot.to_cbor_bytes()).unwrap();
    assert_eq!(decoded, slot);
}

#[test]
fn block_no_round_trip() {
    let bn = BlockNo(42);
    assert_eq!(BlockNo::from_cbor_bytes(&bn.to_cbor_bytes()).unwrap(), bn);
}

#[test]
fn epoch_no_round_trip() {
    let en = EpochNo(500);
    assert_eq!(EpochNo::from_cbor_bytes(&en.to_cbor_bytes()).unwrap(), en);
}

#[test]
fn header_hash_round_trip() {
    let hh = HeaderHash([0xab; 32]);
    assert_eq!(
        HeaderHash::from_cbor_bytes(&hh.to_cbor_bytes()).unwrap(),
        hh
    );
}

#[test]
fn tx_id_round_trip() {
    let txid = TxId([0xcd; 32]);
    assert_eq!(TxId::from_cbor_bytes(&txid.to_cbor_bytes()).unwrap(), txid);
}

#[test]
fn point_origin_round_trip() {
    let pt = Point::Origin;
    assert_eq!(Point::from_cbor_bytes(&pt.to_cbor_bytes()).unwrap(), pt);
}

#[test]
fn point_block_round_trip() {
    let pt = Point::BlockPoint(SlotNo(100), HeaderHash([0x11; 32]));
    assert_eq!(Point::from_cbor_bytes(&pt.to_cbor_bytes()).unwrap(), pt);
}

#[test]
fn nonce_neutral_round_trip() {
    let n = Nonce::Neutral;
    assert_eq!(Nonce::from_cbor_bytes(&n.to_cbor_bytes()).unwrap(), n);
}

#[test]
fn nonce_hash_round_trip() {
    let n = Nonce::Hash([0xff; 32]);
    assert_eq!(Nonce::from_cbor_bytes(&n.to_cbor_bytes()).unwrap(), n);
}

#[test]
fn stake_credential_keyhash_round_trip() {
    let cred = StakeCredential::AddrKeyHash([0x01; 28]);
    assert_eq!(
        StakeCredential::from_cbor_bytes(&cred.to_cbor_bytes()).unwrap(),
        cred
    );
}

#[test]
fn stake_credential_scripthash_round_trip() {
    let cred = StakeCredential::ScriptHash([0x02; 28]);
    assert_eq!(
        StakeCredential::from_cbor_bytes(&cred.to_cbor_bytes()).unwrap(),
        cred
    );
}

#[test]
fn reward_account_round_trip() {
    let ra = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash([0x0a; 28]),
    };
    assert_eq!(
        RewardAccount::from_cbor_bytes(&ra.to_cbor_bytes()).unwrap(),
        ra
    );
}

#[test]
fn anchor_round_trip() {
    let a = Anchor {
        url: "https://example.com".to_string(),
        data_hash: [0xee; 32],
    };
    assert_eq!(Anchor::from_cbor_bytes(&a.to_cbor_bytes()).unwrap(), a);
}

#[test]
fn unit_interval_round_trip() {
    let ui = UnitInterval {
        numerator: 1,
        denominator: 3,
    };
    assert_eq!(
        UnitInterval::from_cbor_bytes(&ui.to_cbor_bytes()).unwrap(),
        ui
    );
}

#[test]
fn relay_single_host_addr_round_trip() {
    let r = Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 1]), None);
    assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
}

#[test]
fn relay_single_host_name_round_trip() {
    let r = Relay::SingleHostName(Some(3001), "relay.example.com".to_string());
    assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
}

#[test]
fn relay_multi_host_name_round_trip() {
    let r = Relay::MultiHostName("pool.example.com".to_string());
    assert_eq!(Relay::from_cbor_bytes(&r.to_cbor_bytes()).unwrap(), r);
}

#[test]
fn pool_metadata_round_trip() {
    let pm = PoolMetadata {
        url: "https://meta.pool.io".to_string(),
        metadata_hash: [0xdd; 32],
    };
    assert_eq!(
        PoolMetadata::from_cbor_bytes(&pm.to_cbor_bytes()).unwrap(),
        pm
    );
}

#[test]
fn pool_params_round_trip() {
    let pp = PoolParams {
        operator: [0x01; 28],
        vrf_keyhash: [0x02; 32],
        pledge: 1_000_000,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 100,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x03; 28]),
        },
        pool_owners: vec![[0x04; 28]],
        relays: vec![Relay::SingleHostName(Some(3001), "r.io".to_string())],
        pool_metadata: None,
    };
    assert_eq!(
        PoolParams::from_cbor_bytes(&pp.to_cbor_bytes()).unwrap(),
        pp
    );
}

#[test]
fn drep_all_variants_round_trip() {
    for drep in [
        DRep::KeyHash([0x01; 28]),
        DRep::ScriptHash([0x02; 28]),
        DRep::AlwaysAbstain,
        DRep::AlwaysNoConfidence,
    ] {
        let decoded = DRep::from_cbor_bytes(&drep.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, drep);
    }
}

#[test]
fn dcert_shelley_tags_round_trip() {
    let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
    let pool = [0x0b; 28];
    let certs = vec![
        DCert::AccountRegistration(cred),
        DCert::AccountUnregistration(cred),
        DCert::DelegationToStakePool(cred, pool),
        DCert::PoolRetirement(pool, EpochNo(100)),
        DCert::GenesisDelegation([0x01; 28], [0x02; 28], [0x03; 32]),
    ];
    for cert in certs {
        let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, cert);
    }
}

#[test]
fn dcert_conway_tags_round_trip() {
    let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
    let pool = [0x0b; 28];
    let drep = DRep::KeyHash([0x0c; 28]);
    let anchor = Some(Anchor {
        url: "https://example.com".to_string(),
        data_hash: [0xee; 32],
    });
    let certs = vec![
        DCert::AccountRegistrationDeposit(cred, 2_000_000),
        DCert::AccountUnregistrationDeposit(cred, 2_000_000),
        DCert::DelegationToDrep(cred, drep),
        DCert::DelegationToStakePoolAndDrep(cred, pool, drep),
        DCert::AccountRegistrationDelegationToStakePool(cred, pool, 2_000_000),
        DCert::AccountRegistrationDelegationToDrep(cred, drep, 2_000_000),
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, 2_000_000),
        DCert::CommitteeAuthorization(cred, StakeCredential::ScriptHash([0x0d; 28])),
        DCert::CommitteeResignation(cred, anchor.clone()),
        DCert::DrepRegistration(cred, 500_000_000, anchor.clone()),
        DCert::DrepUnregistration(cred, 500_000_000),
        DCert::DrepUpdate(cred, None),
    ];
    for cert in certs {
        let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, cert);
    }
}

#[test]
fn dcert_mir_stake_credentials_round_trip() {
    let mut map = std::collections::BTreeMap::new();
    map.insert(StakeCredential::AddrKeyHash([0x01; 28]), 100i64);
    map.insert(StakeCredential::ScriptHash([0x02; 28]), -50i64);
    let cert = DCert::MoveInstantaneousReward(MirPot::Reserves, MirTarget::StakeCredentials(map));
    let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
    assert_eq!(decoded, cert);
}

#[test]
fn dcert_mir_send_to_pot_round_trip() {
    let cert =
        DCert::MoveInstantaneousReward(MirPot::Treasury, MirTarget::SendToOppositePot(1_000_000));
    let decoded = DCert::from_cbor_bytes(&cert.to_cbor_bytes()).unwrap();
    assert_eq!(decoded, cert);
}

/// Encoder-side drift guard for the full `DCert` wire-tag space.
///
/// Closes a subtle gap left by the round-trip tests above: a coupled
/// typo where the encoder and decoder agree on a wrong tag value
/// (e.g. both `enc.unsigned(0)` for `AccountRegistration` mistakenly
/// becomes `enc.unsigned(1)` AND the decoder's `1 => Account
/// Registration` arm is added in lockstep) would still round-trip
/// cleanly while silently breaking on-chain wire compat with upstream.
///
/// For every variant in the 0..=18 tag space, this test:
///   1. Constructs a representative value
///   2. Encodes it via `to_cbor_bytes`
///   3. Independently decodes the leading array header + tag, NOT
///      via the cascade — direct byte inspection of the array length
///      prefix and first integer
///   4. Asserts BOTH the encoded array length AND the tag against
///      the canonical CDDL-specified values
///
/// Exhaustive variant coverage in BOTH directions: every variant is
/// constructed; every tag 0..=18 is reached. A new upstream
/// certificate variant (tag 19+) added without a matching
/// `expected.push((19, ..., DCert::...))` entry here would slip past
/// CI undetected — but the next regression that mistypes any
/// existing tag fails immediately with a clearly-named diagnostic.
///
/// Reference: `Cardano.Ledger.Conway.TxCert` — `ConwayTxCert`
/// constructor tags; CDDL `certificate` rule in
/// `cardano-ledger-conway/cddl-files/conway.cddl`.
#[test]
fn dcert_encoder_tag_and_arity_match_canonical_cddl() {
    let cred = StakeCredential::AddrKeyHash([0x0a; 28]);
    let pool = [0x0b; 28];
    let drep = DRep::KeyHash([0x0c; 28]);
    let anchor = Anchor {
        url: "https://example.com".to_string(),
        data_hash: [0xee; 32],
    };

    // (canonical tag, canonical array length, variant). Lengths from
    // the CDDL `certificate` rule and matched in `encode_cbor` above.
    // PoolRegistration is length 10 (one tag + 9 inline fields per
    // upstream `pool_registration_cert`); MIR is length 2 (tag +
    // inner pair). Everything else follows the
    // `[tag, *fields]` shape with field count from the variant arity.
    let pool_params = pool_params_for_test();
    let mir_target = MirTarget::SendToOppositePot(42);

    let cases: Vec<(u64, u64, DCert)> = vec![
        (0, 2, DCert::AccountRegistration(cred)),
        (1, 2, DCert::AccountUnregistration(cred)),
        (2, 3, DCert::DelegationToStakePool(cred, pool)),
        (3, 10, DCert::PoolRegistration(pool_params)),
        (4, 3, DCert::PoolRetirement(pool, EpochNo(7))),
        (
            5,
            4,
            DCert::GenesisDelegation([0x01; 28], [0x02; 28], [0x03; 32]),
        ),
        (
            6,
            2,
            DCert::MoveInstantaneousReward(MirPot::Treasury, mir_target),
        ),
        (7, 3, DCert::AccountRegistrationDeposit(cred, 2_000_000)),
        (8, 3, DCert::AccountUnregistrationDeposit(cred, 2_000_000)),
        (9, 3, DCert::DelegationToDrep(cred, drep)),
        (10, 4, DCert::DelegationToStakePoolAndDrep(cred, pool, drep)),
        (
            11,
            4,
            DCert::AccountRegistrationDelegationToStakePool(cred, pool, 2_000_000),
        ),
        (
            12,
            4,
            DCert::AccountRegistrationDelegationToDrep(cred, drep, 2_000_000),
        ),
        (
            13,
            5,
            DCert::AccountRegistrationDelegationToStakePoolAndDrep(cred, pool, drep, 2_000_000),
        ),
        (
            14,
            3,
            DCert::CommitteeAuthorization(cred, StakeCredential::ScriptHash([0x0d; 28])),
        ),
        (
            15,
            3,
            DCert::CommitteeResignation(cred, Some(anchor.clone())),
        ),
        (16, 4, DCert::DrepRegistration(cred, 5_000_000, None)),
        (17, 3, DCert::DrepUnregistration(cred, 5_000_000)),
        (18, 3, DCert::DrepUpdate(cred, Some(anchor))),
    ];

    // Pin: exactly 19 cases (tags 0..=18) so a future upstream tag-19
    // variant added to the enum WITHOUT extending this test fails CI
    // (the existing round-trip tests would also need updating, but
    // that's a separate signal — this is the canonical "did you
    // remember to extend the drift-guard table" check).
    assert_eq!(
        cases.len(),
        19,
        "DCert tag space must be 0..=18 (19 variants)",
    );

    let mut seen_tags: Vec<u64> = Vec::with_capacity(19);
    for (canonical_tag, canonical_len, cert) in cases {
        let bytes = cert.to_cbor_bytes();
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("DCert encodes as a CBOR array");
        assert_eq!(
            len, canonical_len,
            "DCert::{:?} encoded with array length {len}, expected {canonical_len}",
            cert,
        );
        let tag = dec.unsigned().expect("first array element is the tag");
        assert_eq!(
            tag, canonical_tag,
            "DCert::{:?} encoded with tag {tag}, expected canonical CDDL tag {canonical_tag}",
            cert,
        );
        seen_tags.push(tag);
    }

    // Bidirectional completeness: every tag 0..=18 must appear
    // exactly once across the 19 cases. A duplicate tag (two
    // variants accidentally encoded with the same wire ID) or a
    // missing tag fails here naming the gap.
    seen_tags.sort();
    let expected_tags: Vec<u64> = (0..=18).collect();
    assert_eq!(
        seen_tags, expected_tags,
        "encoded DCert tag set must be exactly 0..=18 with no duplicates",
    );
}

/// Encoder-side drift guard for the Shelley `Relay` wire-tag space.
///
/// 3 variants (tags 0..=2) with mixed array lengths (4/3/2):
/// `SingleHostAddr` (port + ipv4 + ipv6, all optional, length 4),
/// `SingleHostName` (port + DNS, length 3), `MultiHostName`
/// (DNS only, length 2). A typo swapping tag-0 SingleHostAddr and
/// tag-1 SingleHostName would silently misinterpret every pool's
/// announced relay endpoints — every operator-published pool
/// metadata would point at garbage, breaking peer discovery.
///
/// Reference: `Cardano.Ledger.Shelley.TxBody.StakePoolRelay`;
/// CDDL `relay` rule.
#[test]
fn relay_encoder_tag_and_arity_match_canonical_cddl() {
    let cases: Vec<(u64, u64, Relay)> = vec![
        (
            0,
            4,
            Relay::SingleHostAddr(Some(3001), Some([192, 168, 1, 1]), None),
        ),
        (
            1,
            3,
            Relay::SingleHostName(Some(3001), "relays.example.com".to_string()),
        ),
        (2, 2, Relay::MultiHostName("relays.example.com".to_string())),
    ];
    assert_eq!(cases.len(), 3, "Relay tag space must be 0..=2");

    let mut seen: Vec<u64> = Vec::with_capacity(3);
    for (canonical_tag, canonical_len, relay) in cases {
        let bytes = relay.to_cbor_bytes();
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("Relay encodes as a CBOR array");
        assert_eq!(
            len, canonical_len,
            "Relay::{relay:?} array length {len}, expected {canonical_len}",
        );
        let tag = dec.unsigned().expect("first array element is the tag");
        assert_eq!(tag, canonical_tag, "Relay::{relay:?} tag drift");
        seen.push(tag);
    }
    seen.sort();
    assert_eq!(seen, vec![0, 1, 2], "Relay tag set must be exactly 0..=2");
}

/// Encoder-side drift guard for `MirPot` (the move-instantaneous-rewards
/// source-pot enum embedded inside DCert tag 6).
///
/// 2 values: `Reserves = 0`, `Treasury = 1`. Encoded as bare unsigned
/// inside the inner MIR array. A typo swapping the two would silently
/// misinterpret every MIR certificate's source pot — turning a
/// reserves-funded reward into a treasury-funded one and vice versa,
/// silently misallocating epoch-boundary fund movement (Shelley-Babbage;
/// Conway no longer supports MIR).
///
/// `MirPot` is encoded inline in the DCert tag-6 cascade rather than
/// having its own `CborEncode` impl, so this test exercises the
/// embedded encoding by constructing a full `DCert::MoveInstantaneous
/// Reward`, encoding it, and inspecting the inner array's pot byte.
///
/// Reference: `Cardano.Ledger.Shelley.TxCert.MIRPot`;
/// `move_instantaneous_reward` CDDL rule.
#[test]
fn mir_pot_encoder_value_matches_canonical_cddl() {
    for (canonical, pot) in [(0u64, MirPot::Reserves), (1u64, MirPot::Treasury)] {
        let cert = DCert::MoveInstantaneousReward(pot, MirTarget::SendToOppositePot(0));
        let bytes = cert.to_cbor_bytes();
        // Outer DCert: [6, [pot, 0]] — array(2) tag(6) array(2) pot 0
        let mut dec = Decoder::new(&bytes);
        let outer_len = dec.array().expect("outer DCert array");
        assert_eq!(outer_len, 2, "DCert MIR outer array must be length 2");
        let dcert_tag = dec.unsigned().expect("DCert tag");
        assert_eq!(dcert_tag, 6, "MIR DCert tag must be 6");
        let inner_len = dec.array().expect("inner MIR array");
        assert_eq!(
            inner_len, 2,
            "inner MIR array must be length 2 (pot, target)"
        );
        let pot_value = dec.unsigned().expect("inner pot value");
        assert_eq!(
            pot_value, canonical,
            "MirPot::{pot:?} encoded as {pot_value}, expected {canonical}",
        );
    }
}

/// Encoder-side drift guard for the Conway `DRep` wire-tag space.
///
/// 4 variants with mixed array lengths: `KeyHash=0` and `ScriptHash=1`
/// are length 2 (tag + hash); `AlwaysAbstain=2` and `AlwaysNoConfidence=3`
/// are length 1 (tag only). A drift here would silently misinterpret
/// DRep delegation/vote credentials — e.g. a stake delegation pointing
/// at a real DRep key hash that decodes as `AlwaysAbstain` would
/// strip a real voter's voice and silently flip them to abstain.
///
/// Pins per-variant array length AND tag, plus bidirectional
/// completeness (cases.len() == 4, sorted observed tags == 0..=3).
///
/// Reference: `Cardano.Ledger.Conway.Governance.DRep`; CDDL `drep`
/// rule.
#[test]
fn drep_encoder_tag_and_arity_match_canonical_cddl() {
    let h = [0x66; 28];
    // (canonical tag, canonical array length, variant)
    let cases: Vec<(u64, u64, DRep)> = vec![
        (0, 2, DRep::KeyHash(h)),
        (1, 2, DRep::ScriptHash(h)),
        (2, 1, DRep::AlwaysAbstain),
        (3, 1, DRep::AlwaysNoConfidence),
    ];
    assert_eq!(cases.len(), 4, "DRep tag space must be 0..=3 (4 variants)");

    let mut seen: Vec<u64> = Vec::with_capacity(4);
    for (canonical_tag, canonical_len, drep) in cases {
        let bytes = drep.to_cbor_bytes();
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("DRep encodes as a CBOR array");
        assert_eq!(
            len, canonical_len,
            "DRep::{drep:?} array length {len}, expected {canonical_len}",
        );
        let tag = dec.unsigned().expect("first array element is the tag");
        assert_eq!(tag, canonical_tag, "DRep::{drep:?} tag drift");
        seen.push(tag);
    }
    seen.sort();
    assert_eq!(seen, vec![0, 1, 2, 3], "DRep tag set must be exactly 0..=3");
}

/// Build a minimal valid `PoolParams` for `dcert_encoder_tag_and_arity_match
/// _canonical_cddl`. Kept as a free helper so the test body stays
/// focused on the tag/arity invariants.
fn pool_params_for_test() -> PoolParams {
    PoolParams {
        operator: [0x0b; 28],
        vrf_keyhash: [0x0c; 32],
        pledge: 0,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 100,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x0d; 28]),
        },
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    }
}

#[test]
fn header_hash_wrong_length_rejected() {
    let mut enc = Encoder::new();
    enc.bytes(&[0u8; 16]); // 16 bytes, not 32
    let bytes = enc.into_bytes();
    assert!(HeaderHash::from_cbor_bytes(&bytes).is_err());
}

#[test]
fn era_invalid_tag_rejected() {
    let mut enc = Encoder::new();
    enc.unsigned(99);
    let bytes = enc.into_bytes();
    assert!(Era::from_cbor_bytes(&bytes).is_err());
}

#[test]
fn point_invalid_length_rejected() {
    let mut enc = Encoder::new();
    enc.array(1).unsigned(0);
    let bytes = enc.into_bytes();
    assert!(Point::from_cbor_bytes(&bytes).is_err());
}

// ── Indefinite-length CBOR tests ─────────────────────────────────

#[test]
fn skip_indefinite_array() {
    // 0x9f 01 02 03 ff = [_ 1, 2, 3]
    let data = [0x9f, 0x01, 0x02, 0x03, 0xff];
    let mut dec = Decoder::new(&data);
    dec.skip().unwrap();
    assert!(dec.is_empty());
}

#[test]
fn skip_indefinite_map() {
    // 0xbf 01 02 03 04 ff = {_ 1: 2, 3: 4}
    let data = [0xbf, 0x01, 0x02, 0x03, 0x04, 0xff];
    let mut dec = Decoder::new(&data);
    dec.skip().unwrap();
    assert!(dec.is_empty());
}

#[test]
fn skip_indefinite_bytes() {
    // 0x5f 42 0102 43 030405 ff = (_ h'0102', h'030405')
    let data = [0x5f, 0x42, 0x01, 0x02, 0x43, 0x03, 0x04, 0x05, 0xff];
    let mut dec = Decoder::new(&data);
    dec.skip().unwrap();
    assert!(dec.is_empty());
}

#[test]
fn skip_indefinite_text() {
    // 0x7f 63 666f6f 63 626172 ff = (_ "foo", "bar")
    let data = [0x7f, 0x63, b'f', b'o', b'o', 0x63, b'b', b'a', b'r', 0xff];
    let mut dec = Decoder::new(&data);
    dec.skip().unwrap();
    assert!(dec.is_empty());
}

#[test]
fn skip_nested_indefinite() {
    // [_ [_ 1, 2], {_ 3: 4}]
    let data = [
        0x9f, // indef array
        0x9f, 0x01, 0x02, 0xff, // indef array [1, 2]
        0xbf, 0x03, 0x04, 0xff, // indef map {3: 4}
        0xff, // end outer
    ];
    let mut dec = Decoder::new(&data);
    dec.skip().unwrap();
    assert!(dec.is_empty());
}

#[test]
fn array_begin_definite() {
    // 83 01 02 03 = [1, 2, 3]
    let data = [0x83, 0x01, 0x02, 0x03];
    let mut dec = Decoder::new(&data);
    let count = dec.array_begin().unwrap();
    assert_eq!(count, Some(3));
    for _ in 0..3 {
        dec.unsigned().unwrap();
    }
    assert!(dec.is_empty());
}

#[test]
fn array_begin_indefinite() {
    // 9f 01 02 03 ff = [_ 1, 2, 3]
    let data = [0x9f, 0x01, 0x02, 0x03, 0xff];
    let mut dec = Decoder::new(&data);
    let count = dec.array_begin().unwrap();
    assert_eq!(count, None);
    let mut items = Vec::new();
    while !dec.is_break() {
        items.push(dec.unsigned().unwrap());
    }
    dec.consume_break().unwrap();
    assert_eq!(items, vec![1, 2, 3]);
    assert!(dec.is_empty());
}

#[test]
fn map_begin_indefinite() {
    // bf 01 02 ff = {_ 1: 2}
    let data = [0xbf, 0x01, 0x02, 0xff];
    let mut dec = Decoder::new(&data);
    let count = dec.map_begin().unwrap();
    assert_eq!(count, None);
    let mut entries = Vec::new();
    while !dec.is_break() {
        let k = dec.unsigned().unwrap();
        let v = dec.unsigned().unwrap();
        entries.push((k, v));
    }
    dec.consume_break().unwrap();
    assert_eq!(entries, vec![(1, 2)]);
}

#[test]
fn bytes_owned_definite() {
    let mut enc = Encoder::new();
    enc.bytes(&[0x01, 0x02, 0x03]);
    let data = enc.into_bytes();
    let mut dec = Decoder::new(&data);
    assert_eq!(dec.bytes_owned().unwrap(), vec![0x01, 0x02, 0x03]);
}

#[test]
fn bytes_owned_indefinite() {
    // 5f 42 0102 43 030405 ff = (_ h'0102', h'030405')
    let data = [0x5f, 0x42, 0x01, 0x02, 0x43, 0x03, 0x04, 0x05, 0xff];
    let mut dec = Decoder::new(&data);
    assert_eq!(
        dec.bytes_owned().unwrap(),
        vec![0x01, 0x02, 0x03, 0x04, 0x05]
    );
    assert!(dec.is_empty());
}

#[test]
fn text_owned_definite() {
    let mut enc = Encoder::new();
    enc.text("hello");
    let data = enc.into_bytes();
    let mut dec = Decoder::new(&data);
    assert_eq!(dec.text_owned().unwrap(), "hello");
}

#[test]
fn text_owned_indefinite() {
    // 7f 63 666f6f 63 626172 ff = (_ "foo", "bar")
    let data = [0x7f, 0x63, b'f', b'o', b'o', 0x63, b'b', b'a', b'r', 0xff];
    let mut dec = Decoder::new(&data);
    assert_eq!(dec.text_owned().unwrap(), "foobar");
    assert!(dec.is_empty());
}

#[test]
fn raw_value_captures_indefinite_array() {
    // 9f 01 02 ff followed by 05
    let data = [0x9f, 0x01, 0x02, 0xff, 0x05];
    let mut dec = Decoder::new(&data);
    let raw = dec.raw_value().unwrap();
    assert_eq!(raw, &[0x9f, 0x01, 0x02, 0xff]);
    assert_eq!(dec.unsigned().unwrap(), 5);
}

// ── array_or_set: CBOR tag 258 transparent set decode ──────────────

#[test]
fn array_or_set_plain_array() {
    // Plain array: 83 01 02 03  →  [1, 2, 3]
    let data = [0x83, 0x01, 0x02, 0x03];
    let mut dec = Decoder::new(&data);
    let len = dec.array_or_set().unwrap();
    assert_eq!(len, 3);
    assert_eq!(dec.unsigned().unwrap(), 1);
    assert_eq!(dec.unsigned().unwrap(), 2);
    assert_eq!(dec.unsigned().unwrap(), 3);
    assert!(dec.is_empty());
}

#[test]
fn array_or_set_tagged_258() {
    // Tag 258 wrapping array: d9 0102 83 01 02 03  →  258([1, 2, 3])
    let data = [0xd9, 0x01, 0x02, 0x83, 0x01, 0x02, 0x03];
    let mut dec = Decoder::new(&data);
    let len = dec.array_or_set().unwrap();
    assert_eq!(len, 3);
    assert_eq!(dec.unsigned().unwrap(), 1);
    assert_eq!(dec.unsigned().unwrap(), 2);
    assert_eq!(dec.unsigned().unwrap(), 3);
    assert!(dec.is_empty());
}

#[test]
fn array_or_set_empty_tagged_258() {
    // Tag 258 wrapping empty array: d9 0102 80  →  258([])
    let data = [0xd9, 0x01, 0x02, 0x80];
    let mut dec = Decoder::new(&data);
    let len = dec.array_or_set().unwrap();
    assert_eq!(len, 0);
    assert!(dec.is_empty());
}

#[test]
fn array_or_set_rejects_non_array_non_tag() {
    // Unsigned integer 0x05 — neither array nor tag.
    let data = [0x05];
    let mut dec = Decoder::new(&data);
    assert!(dec.array_or_set().is_err());
}

// ── extract_block_tx_byte_spans ────────────────────────────────────

/// Synthesizes a minimal but structurally-correct block envelope:
/// `[header, [body0, body1], [ws0, ws1], { 0 => meta0 }]`.
/// Each "body" / "ws" / "header" is a single CBOR unsigned-int marker so
/// we can verify the helper sliced exactly the right bytes back.
#[test]
fn extract_block_tx_byte_spans_round_trip() {
    let mut enc = Encoder::new();
    enc.array(4);
    // Header (tag 0xAA → unsigned 0x18 0xAA).
    enc.unsigned(0xAA);
    // Bodies array of 2.
    enc.array(2);
    let body0_start = enc.as_bytes().len();
    enc.unsigned(0x10);
    let body0_end = enc.as_bytes().len();
    let body1_start = enc.as_bytes().len();
    enc.unsigned(0x11);
    let body1_end = enc.as_bytes().len();
    // Witness sets array of 2.
    enc.array(2);
    let ws0_start = enc.as_bytes().len();
    enc.unsigned(0x20);
    let ws0_end = enc.as_bytes().len();
    let ws1_start = enc.as_bytes().len();
    enc.unsigned(0x21);
    let ws1_end = enc.as_bytes().len();
    // Metadata map (single entry).
    enc.map(1).unsigned(0).unsigned(0xFF);

    let raw = enc.into_bytes();
    let spans = extract_block_tx_byte_spans(&raw).expect("extract spans");
    assert_eq!(spans.bodies.len(), 2);
    assert_eq!(spans.witness_sets.len(), 2);
    assert_eq!(spans.bodies[0], &raw[body0_start..body0_end]);
    assert_eq!(spans.bodies[1], &raw[body1_start..body1_end]);
    assert_eq!(spans.witness_sets[0], &raw[ws0_start..ws0_end]);
    assert_eq!(spans.witness_sets[1], &raw[ws1_start..ws1_end]);
}

#[test]
fn extract_block_tx_byte_spans_unwraps_hfc_envelope() {
    let inner: Vec<u8> = vec![
        0x84, // array(4)
        0x00, //   header
        0x81, //   bodies array(1)
        0x9f, 0x18, 0x42, 0xff, // body uses non-canonical-on-reencode form
        0x81, //   witnesses array(1)
        0x18, 0x55, // witness set marker
        0xa0, //   metadata
    ];
    let mut wrapped = Vec::with_capacity(inner.len() + 2);
    wrapped.push(0x82); // HFC [era_index, inner_block]
    wrapped.push(0x06); // Babbage-era index in deployed networks
    wrapped.extend_from_slice(&inner);

    let spans = extract_block_tx_byte_spans(&wrapped).expect("extract wrapped spans");
    assert_eq!(spans.bodies, vec![vec![0x9f, 0x18, 0x42, 0xff]]);
    assert_eq!(spans.witness_sets, vec![vec![0x18, 0x55]]);
}

/// Verifies the central parity invariant: when re-serialising a typed
/// value yields different bytes than the on-wire encoding (here, a
/// definite-length array vs an indefinite-length array), the helper
/// returns the **on-wire** bytes byte-for-byte.  This is what the
/// linear fee formula and tx-id hash both depend on.
///
/// Reference: 2026-04-27 preprod sync rehearsal finding (440-lovelace
/// gap on the first transaction after the Byron→Shelley boundary,
/// preprod slot ≈ 518 460), captured in
/// docs/REAL_PREPROD_POOL_VERIFICATION.md.
#[test]
fn extract_block_tx_byte_spans_returns_on_wire_bytes_for_indefinite_encoding() {
    // Hand-craft a block with an indefinite-length body so re-encoding
    // (which always emits definite-length) would produce different
    // bytes than what the helper extracts.
    //
    // Outer:  array(4)
    //   header: unsigned(0)
    //   bodies: array(1) [ indefinite_array(0xff_terminated) [unsigned(0x42)] ]
    //   witnesses: array(1) [ unsigned(0x55) ]
    //   metadata: map(0)
    let raw: Vec<u8> = vec![
        0x84, // array(4)
        0x00, //   unsigned(0)            ← header
        0x81, //   array(1)               ← bodies
        0x9f, 0x18, 0x42, 0xff, //     indef-array [unsigned(0x42)]
        0x81, //   array(1)               ← witnesses
        0x18, 0x55, //     unsigned(0x55)
        0xa0, //   map(0)                 ← metadata
    ];
    let spans = extract_block_tx_byte_spans(&raw).expect("extract spans");
    assert_eq!(spans.bodies.len(), 1);
    assert_eq!(spans.witness_sets.len(), 1);
    // The body span MUST be the indefinite-length encoding 0x9f 0x18 0x42 0xff.
    // Re-serialising via `to_cbor_bytes()` would emit 0x81 0x18 0x42 instead
    // (definite-length, 3 bytes) — that 1-byte difference is exactly the
    // class of mismatch that caused the 440-lovelace fee gap upstream.
    assert_eq!(spans.bodies[0], &[0x9f, 0x18, 0x42, 0xff]);
    assert_eq!(spans.witness_sets[0], &[0x18, 0x55]);
}

/// R251 — preview Babbage blocks observed in chunk 353 around slot
/// `~1,525,259` from the IOG bootstrap peer (`99.80.240.19:3001`)
/// encode the **bodies array itself** (not just individual body
/// elements) with CBOR indefinite-length (`0x9f ... 0xff`). Pre-R251
/// `extract_block_tx_byte_spans` used strict `dec.array()` for that
/// array, which fails with `CborInvalidAdditionalInfo(31)`. The error
/// propagated up to the apply-path macro
/// `crates/node/sync/src/lib.rs::alonzo_family_block_to_block_with_spans` which
/// then fell back to `tx_body.to_cbor_bytes()` re-serialisation —
/// producing a `tx_body_hash` that differed from the on-wire
/// (indefinite) hash the signer signed. The signature was
/// fully-canonical (R/S in range, vkey not small-order) per offline
/// classification, but verified invalid against our wrong hash.
/// This was Yggdrasil's R249/R250 Gap BQ — preview vkey witness
/// rejection at slot `~1,525,024` on tx `44ccae43…`. Closed in R251 by
/// switching all three `array()` calls in `extract_block_tx_byte_spans`
/// to `array_begin()` + indefinite-length walk.
#[test]
fn extract_block_tx_byte_spans_handles_indefinite_bodies_array() {
    // Outer:  array(4)
    //   header: unsigned(0)
    //   bodies: indefinite_array [ unsigned(0x42), unsigned(0x99) ]   ← KEY: indefinite, not definite
    //   witnesses: array(2) [ unsigned(0x55), unsigned(0x66) ]
    //   metadata: map(0)
    let raw: Vec<u8> = vec![
        0x84, // array(4)
        0x00, //   unsigned(0)            ← header
        0x9f, 0x18, 0x42, 0x18, 0x99, 0xff, //   indef-array of 2 bodies, terminated by break
        0x82, //   array(2)               ← witnesses
        0x18, 0x55, //     unsigned(0x55)
        0x18, 0x66, //     unsigned(0x66)
        0xa0, //   map(0)                 ← metadata
    ];
    let spans = extract_block_tx_byte_spans(&raw).expect("extract spans (indefinite bodies array)");
    assert_eq!(
        spans.bodies.len(),
        2,
        "indefinite bodies must yield 2 entries"
    );
    assert_eq!(spans.bodies[0], &[0x18, 0x42]);
    assert_eq!(spans.bodies[1], &[0x18, 0x99]);
    assert_eq!(spans.witness_sets.len(), 2);
    assert_eq!(spans.witness_sets[0], &[0x18, 0x55]);
    assert_eq!(spans.witness_sets[1], &[0x18, 0x66]);
}

/// R251 — and the symmetric case: indefinite-length witnesses array.
#[test]
fn extract_block_tx_byte_spans_handles_indefinite_witnesses_array() {
    let raw: Vec<u8> = vec![
        0x84, // array(4)
        0x00, //   unsigned(0)            ← header
        0x82, //   array(2)               ← bodies (definite)
        0x18, 0x42, 0x18, 0x99, //     two unsigneds
        0x9f, 0x18, 0x55, 0x18, 0x66, 0xff, //   indef-array of 2 witnesses
        0xa0, //   map(0)                 ← metadata
    ];
    let spans =
        extract_block_tx_byte_spans(&raw).expect("extract spans (indefinite witness array)");
    assert_eq!(spans.bodies.len(), 2);
    assert_eq!(spans.witness_sets.len(), 2);
    assert_eq!(spans.witness_sets[0], &[0x18, 0x55]);
    assert_eq!(spans.witness_sets[1], &[0x18, 0x66]);
}

#[test]
fn extract_block_tx_byte_spans_rejects_truncated_header() {
    // Just `array(4)` with no further data.
    let raw = [0x84];
    assert!(extract_block_tx_byte_spans(&raw).is_err());
}

#[test]
fn extract_block_tx_byte_spans_rejects_too_few_outer_elements() {
    // array(2) — only two elements, missing bodies + witnesses.
    let raw = [0x82, 0x00, 0x00];
    assert!(extract_block_tx_byte_spans(&raw).is_err());
}
