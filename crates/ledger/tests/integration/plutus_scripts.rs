use super::*;

#[test]
fn plutus_data_integer_small_round_trip() {
    let pd = PlutusData::Integer(42);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_negative_integer_round_trip() {
    let pd = PlutusData::Integer(-100);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_big_uint_round_trip() {
    let big = i128::from(u64::MAX) + 1;
    let pd = PlutusData::Integer(big);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_big_nint_round_trip() {
    let big_neg = -(i128::from(u64::MAX)) - 2;
    let pd = PlutusData::Integer(big_neg);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_bytes_round_trip() {
    let pd = PlutusData::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_empty_bytes_round_trip() {
    let pd = PlutusData::Bytes(vec![]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_list_round_trip() {
    let pd = PlutusData::List(vec![
        PlutusData::Integer(1),
        PlutusData::Integer(2),
        PlutusData::Bytes(vec![0xFF]),
    ]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_empty_list_round_trip() {
    let pd = PlutusData::List(vec![]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_map_round_trip() {
    let pd = PlutusData::Map(vec![
        (PlutusData::Bytes(b"key1".to_vec()), PlutusData::Integer(10)),
        (PlutusData::Integer(0), PlutusData::Bytes(b"val".to_vec())),
    ]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_empty_map_round_trip() {
    let pd = PlutusData::Map(vec![]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_constr_compact_round_trip() {
    for alt in 0..=6u64 {
        let pd = PlutusData::Constr(alt, vec![PlutusData::Integer(alt as i128)]);
        let bytes = pd.to_cbor_bytes();
        let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
        assert_eq!(pd, decoded, "failed for alternative {alt}");
    }
}

#[test]
fn plutus_data_constr_general_form_round_trip() {
    let pd = PlutusData::Constr(7, vec![PlutusData::Bytes(vec![0x01]), PlutusData::Integer(99)]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_constr_large_alternative_round_trip() {
    let pd = PlutusData::Constr(1000, vec![]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_constr_empty_fields_round_trip() {
    let pd = PlutusData::Constr(0, vec![]);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_nested_complex_round_trip() {
    let pd = PlutusData::Constr(
        2,
        vec![
            PlutusData::Map(vec![
                (
                    PlutusData::Bytes(b"addr".to_vec()),
                    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![0xAA; 28])]),
                ),
                (
                    PlutusData::Integer(-1),
                    PlutusData::List(vec![PlutusData::Integer(100), PlutusData::Integer(200)]),
                ),
            ]),
            PlutusData::List(vec![]),
            PlutusData::Integer(0),
        ],
    );
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_zero_round_trip() {
    let pd = PlutusData::Integer(0);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn plutus_data_negative_one_round_trip() {
    let pd = PlutusData::Integer(-1);
    let bytes = pd.to_cbor_bytes();
    let decoded = PlutusData::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(pd, decoded);
}

#[test]
fn script_native_round_trip() {
    let s = Script::Native(NativeScript::ScriptPubkey([0xAB; 28]));
    let bytes = s.to_cbor_bytes();
    let decoded = Script::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(s, decoded);
}

#[test]
fn script_plutus_v1_round_trip() {
    let s = Script::PlutusV1(vec![0x01, 0x02, 0x03]);
    let bytes = s.to_cbor_bytes();
    let decoded = Script::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(s, decoded);
}

#[test]
fn script_plutus_v2_round_trip() {
    let s = Script::PlutusV2(vec![0x04, 0x05]);
    let bytes = s.to_cbor_bytes();
    let decoded = Script::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(s, decoded);
}

#[test]
fn script_plutus_v3_round_trip() {
    let s = Script::PlutusV3(vec![0x06, 0x07, 0x08, 0x09]);
    let bytes = s.to_cbor_bytes();
    let decoded = Script::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(s, decoded);
}

#[test]
fn script_ref_native_round_trip() {
    let sref = ScriptRef(Script::Native(NativeScript::InvalidBefore(1000)));
    let bytes = sref.to_cbor_bytes();
    let decoded = ScriptRef::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(sref, decoded);
}

#[test]
fn script_ref_plutus_v2_round_trip() {
    let sref = ScriptRef(Script::PlutusV2(vec![0xCA, 0xFE]));
    let bytes = sref.to_cbor_bytes();
    let decoded = ScriptRef::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(sref, decoded);
}

#[test]
fn babbage_txout_with_typed_script_ref_round_trip() {
    let txout = BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(5_000_000),
        datum_option: None,
        script_ref: Some(ScriptRef(Script::PlutusV1(vec![0x01, 0x02]))),
    };
    let bytes = txout.to_cbor_bytes();
    let decoded = BabbageTxOut::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(txout, decoded);
}

#[test]
fn babbage_txout_with_datum_and_typed_script_ref_round_trip() {
    let txout = BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(2_000_000),
        datum_option: Some(DatumOption::Hash([0xBB; 32])),
        script_ref: Some(ScriptRef(Script::PlutusV3(vec![0xAA]))),
    };
    let bytes = txout.to_cbor_bytes();
    let decoded = BabbageTxOut::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(txout, decoded);
}