use super::*;
use super::types_and_certs::sample_hash32;

pub(super) fn sample_reward_account() -> RewardAccount {
    // 0xE0 header = reward account keyhash on mainnet
    let mut raw = [0u8; 29];
    raw[0] = 0xE0;
    raw[1..].copy_from_slice(&[0x11; 28]);
    RewardAccount::from_bytes(&raw).expect("valid reward account")
}

#[test]
fn shelley_tx_body_with_certificates_round_trip() {
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 1_000_000,
        }],
        fee: 200_000,
        ttl: 500_000,
        certificates: Some(vec![
            DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x01; 28])),
            DCert::AccountUnregistration(StakeCredential::ScriptHash([0x02; 28])),
        ]),
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_with_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 5_000_000);

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 1_000_000,
        }],
        fee: 200_000,
        ttl: 500_000,
        certificates: None,
        withdrawals: Some(wdrl),
        update: None,
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_with_update_round_trip() {
    use std::collections::BTreeMap;
    let mut proposed = BTreeMap::new();
    let param_update = ProtocolParameterUpdate::default();
    proposed.insert([0x01; 28], param_update);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 100,
    };

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 1_000_000,
        }],
        fee: 200_000,
        ttl: 500_000,
        certificates: None,
        withdrawals: None,
        update: Some(update),
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_with_all_keys_4_6_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 1_000_000);

    let mut proposed = BTreeMap::new();
    let param_update = ProtocolParameterUpdate::default();
    proposed.insert([0x02; 28], param_update);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 42,
    };

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 2_000_000,
        }],
        fee: 200_000,
        ttl: 500_000,
        certificates: Some(vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x01; 28]))]),
        withdrawals: Some(wdrl),
        update: Some(update),
        auxiliary_data_hash: Some([0xFF; 32]),
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ShelleyTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn allegra_tx_body_with_certs_and_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 3_000_000);

    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xCC; 32],
            index: 1,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 1_000_000,
        }],
        fee: 180_000,
        ttl: Some(600_000),
        certificates: Some(vec![DCert::DelegationToStakePool(
            StakeCredential::AddrKeyHash([0x01; 28]),
            [0x02; 28],
        )]),
        withdrawals: Some(wdrl),
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: Some(100),
    };
    let bytes = body.to_cbor_bytes();
    let decoded = AllegraTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn mary_tx_body_with_certs_and_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 2_000_000);

    let body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xDD; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x61; 28],
            amount: Value::Coin(1_000_000),
        }],
        fee: 190_000,
        ttl: Some(700_000),
        certificates: Some(vec![DCert::AccountRegistration(StakeCredential::ScriptHash([0x03; 28]))]),
        withdrawals: Some(wdrl),
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = MaryTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn alonzo_tx_body_with_certs_and_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 4_000_000);

    let mut proposed = BTreeMap::new();
    let param_update = ProtocolParameterUpdate::default();
    proposed.insert([0x03; 28], param_update);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 200,
    };

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xEE; 32],
            index: 0,
        }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61; 28],
            amount: Value::Coin(1_000_000),
            datum_hash: None,
        }],
        fee: 250_000,
        ttl: Some(800_000),
        certificates: Some(vec![DCert::AccountUnregistration(StakeCredential::AddrKeyHash([0x04; 28]))]),
        withdrawals: Some(wdrl),
        update: Some(update),
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = AlonzoTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn babbage_tx_body_with_certs_and_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 6_000_000);

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xFF; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61; 28],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 300_000,
        ttl: None,
        certificates: Some(vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x05; 28]))]),
        withdrawals: Some(wdrl),
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
    let bytes = body.to_cbor_bytes();
    let decoded = BabbageTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn conway_tx_body_with_certs_and_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 7_000_000);

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61; 28],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 350_000,
        ttl: None,
        certificates: Some(vec![
            DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x06; 28])),
            DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash([0x07; 28]), 2_000_000),
        ]),
        withdrawals: Some(wdrl),
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
    let bytes = body.to_cbor_bytes();
    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn shelley_tx_body_map_count_includes_keys_4_5_6() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 100);

    let mut proposed = BTreeMap::new();
    let param_update = ProtocolParameterUpdate::default();
    proposed.insert([0x04; 28], param_update);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 0,
    };

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x61; 28],
            amount: 1_000_000,
        }],
        fee: 200_000,
        ttl: 500_000,
        certificates: Some(vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x01; 28]))]),
        withdrawals: Some(wdrl),
        update: Some(update),
        auxiliary_data_hash: None,
    };
    let bytes = body.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let map_len = dec.map().expect("map header");
    assert_eq!(map_len, 7);
}

#[test]
fn conway_tx_body_no_update_key_round_trip() {
    use std::collections::BTreeMap;
    let mut wdrl = BTreeMap::new();
    wdrl.insert(sample_reward_account(), 100);

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61; 28],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: Some(vec![DCert::AccountRegistration(StakeCredential::AddrKeyHash([0x01; 28]))]),
        withdrawals: Some(wdrl),
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
    let bytes = body.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let map_len = dec.map().expect("map header");
    assert_eq!(map_len, 5);

    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(body, decoded);
}

#[test]
fn anchor_cbor_round_trip_types_module() {
    let anchor = Anchor {
        url: "https://example.com/metadata.json".to_string(),
        data_hash: sample_hash32(),
    };
    let bytes = anchor.to_cbor_bytes();
    let decoded = Anchor::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(anchor, decoded);
}