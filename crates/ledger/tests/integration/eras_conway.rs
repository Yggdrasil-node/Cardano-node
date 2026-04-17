use super::*;

#[test]
fn vote_cbor_round_trip() {
    for (vote, expected_byte) in [(Vote::No, 0u8), (Vote::Yes, 1), (Vote::Abstain, 2)] {
        let bytes = vote.to_cbor_bytes();
        assert_eq!(bytes, vec![expected_byte]);
        let decoded = Vote::from_cbor_bytes(&bytes).expect("decode");
        assert_eq!(vote, decoded);
    }
}

#[test]
fn voter_all_variants_cbor_round_trip() {
    let hash28 = [0xAB; 28];
    let voters = vec![
        Voter::CommitteeKeyHash(hash28),
        Voter::CommitteeScript(hash28),
        Voter::DRepKeyHash(hash28),
        Voter::DRepScript(hash28),
        Voter::StakePool(hash28),
    ];
    for voter in voters {
        let bytes = voter.to_cbor_bytes();
        let decoded = Voter::from_cbor_bytes(&bytes).expect("decode");
        assert_eq!(voter, decoded);
    }
}

#[test]
fn gov_action_id_cbor_round_trip() {
    let gaid = GovActionId {
        transaction_id: [0x42; 32],
        gov_action_index: 7,
    };
    let bytes = gaid.to_cbor_bytes();
    let decoded = GovActionId::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(gaid, decoded);
}

#[test]
fn anchor_cbor_round_trip() {
    let anchor = Anchor {
        url: "https://example.com/metadata.json".to_owned(),
        data_hash: [0xCC; 32],
    };
    let bytes = anchor.to_cbor_bytes();
    let decoded = Anchor::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(anchor, decoded);
}

#[test]
fn voting_procedure_with_anchor_cbor_round_trip() {
    let vp = VotingProcedure {
        vote: Vote::Yes,
        anchor: Some(Anchor {
            url: "https://drep.example/rationale".to_owned(),
            data_hash: [0xDD; 32],
        }),
    };
    let bytes = vp.to_cbor_bytes();
    let decoded = VotingProcedure::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(vp, decoded);
}

#[test]
fn voting_procedure_without_anchor_cbor_round_trip() {
    let vp = VotingProcedure {
        vote: Vote::No,
        anchor: None,
    };
    let bytes = vp.to_cbor_bytes();
    let decoded = VotingProcedure::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(vp, decoded);
    assert!(decoded.anchor.is_none());
}

#[test]
fn voting_procedures_nested_map_cbor_round_trip() {
    use std::collections::BTreeMap;

    let voter1 = Voter::DRepKeyHash([0x01; 28]);
    let voter2 = Voter::StakePool([0x02; 28]);
    let gaid1 = GovActionId {
        transaction_id: [0xAA; 32],
        gov_action_index: 0,
    };
    let gaid2 = GovActionId {
        transaction_id: [0xBB; 32],
        gov_action_index: 1,
    };

    let mut inner1 = BTreeMap::new();
    inner1.insert(
        gaid1.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    inner1.insert(
        gaid2,
        VotingProcedure {
            vote: Vote::Abstain,
            anchor: None,
        },
    );

    let mut inner2 = BTreeMap::new();
    inner2.insert(
        gaid1,
        VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );

    let mut procedures = BTreeMap::new();
    procedures.insert(voter1, inner1);
    procedures.insert(voter2, inner2);

    let vps = VotingProcedures { procedures };
    let bytes = vps.to_cbor_bytes();
    let decoded = VotingProcedures::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(vps, decoded);
}

#[test]
fn proposal_procedure_cbor_round_trip() {
    let prop = ProposalProcedure {
        deposit: 500_000_000,
        reward_account: vec![0xE0, 0x01, 0x02, 0x03],
        gov_action: GovAction::InfoAction,
        anchor: Anchor {
            url: "https://gov.example/proposal.json".to_owned(),
            data_hash: [0xEE; 32],
        },
    };
    let bytes = prop.to_cbor_bytes();
    let decoded = ProposalProcedure::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(prop, decoded);
    assert_eq!(decoded.gov_action, GovAction::InfoAction);
}

#[test]
fn conway_tx_body_required_fields_only() {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 28],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
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
    let bytes = body.to_cbor_bytes();
    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(body, decoded);
}

#[test]
fn conway_tx_body_with_governance_fields() {
    use std::collections::BTreeMap;

    let voter = Voter::DRepKeyHash([0xAA; 28]);
    let gaid = GovActionId {
        transaction_id: [0xBB; 32],
        gov_action_index: 0,
    };
    let mut inner = BTreeMap::new();
    inner.insert(
        gaid,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let mut procedures = BTreeMap::new();
    procedures.insert(voter, inner);

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 28],
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 300_000,
        ttl: None,
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
        voting_procedures: Some(VotingProcedures { procedures }),
        proposal_procedures: Some(vec![ProposalProcedure {
            deposit: 500_000_000,
            reward_account: vec![0xE0, 0x01],
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.com/proposal".to_owned(),
                data_hash: [0xCC; 32],
            },
        }]),
        current_treasury_value: Some(1_000_000_000),
        treasury_donation: Some(10_000_000),
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(body, decoded);
    assert!(decoded.voting_procedures.is_some());
    assert_eq!(decoded.proposal_procedures.as_ref().map(Vec::len), Some(1));
    assert_eq!(decoded.current_treasury_value, Some(1_000_000_000));
    assert_eq!(decoded.treasury_donation, Some(10_000_000));
}

#[test]
fn conway_tx_body_unknown_keys_skipped() {
    let mut enc = Encoder::new();
    enc.map(4);
    enc.unsigned(0)
        .array(1)
        .array(2)
        .bytes(&[0x11; 32])
        .unsigned(0);
    enc.unsigned(1).array(1);
    enc.map(2);
    enc.unsigned(0).bytes(&[0x01; 28]);
    enc.unsigned(1).unsigned(1_000_000);
    enc.unsigned(2).unsigned(200_000);
    enc.unsigned(99).unsigned(42);
    let bytes = enc.into_bytes();
    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.fee, 200_000);
    assert!(decoded.voting_procedures.is_none());
}

#[test]
fn conway_tx_body_treasury_only() {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 1,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02; 28],
            amount: Value::Coin(3_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 180_000,
        ttl: None,
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
        current_treasury_value: Some(2_000_000_000),
        treasury_donation: None,
    };
    let bytes = body.to_cbor_bytes();
    let decoded = ConwayTxBody::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(decoded.current_treasury_value, Some(2_000_000_000));
    assert!(decoded.treasury_donation.is_none());
    assert_eq!(body, decoded);
}

#[test]
fn voter_ordering_deterministic() {
    let v1 = Voter::CommitteeKeyHash([0x01; 28]);
    let v2 = Voter::DRepKeyHash([0x01; 28]);
    let v3 = Voter::StakePool([0x01; 28]);
    assert!(v1 < v2);
    assert!(v2 < v3);
}
