use super::*;
use super::txbody_keys::sample_reward_account;

#[test]
fn gov_action_info_action_round_trip() {
    let ga = GovAction::InfoAction;
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_no_confidence_round_trip() {
    let ga = GovAction::NoConfidence {
        prev_action_id: Some(GovActionId {
            transaction_id: [0x11; 32],
            gov_action_index: 0,
        }),
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_no_confidence_null_prev_round_trip() {
    let ga = GovAction::NoConfidence { prev_action_id: None };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_hard_fork_round_trip() {
    let ga = GovAction::HardForkInitiation {
        prev_action_id: Some(GovActionId {
            transaction_id: [0xAA; 32],
            gov_action_index: 3,
        }),
        protocol_version: (10, 0),
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_parameter_change_round_trip() {
    let ga = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: ProtocolParameterUpdate::default(),
        guardrails_script_hash: Some([0xFF; 28]),
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_parameter_change_no_guardrails_round_trip() {
    let ga = GovAction::ParameterChange {
        prev_action_id: Some(GovActionId {
            transaction_id: [0xBB; 32],
            gov_action_index: 1,
        }),
        protocol_param_update: ProtocolParameterUpdate {
            min_fee_a: Some(500),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_treasury_withdrawals_round_trip() {
    use std::collections::BTreeMap;
    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(sample_reward_account(), 5_000_000);
    let ga = GovAction::TreasuryWithdrawals {
        withdrawals,
        guardrails_script_hash: Some([0xCC; 28]),
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_treasury_withdrawals_no_guardrails_round_trip() {
    use std::collections::BTreeMap;
    let ga = GovAction::TreasuryWithdrawals {
        withdrawals: BTreeMap::new(),
        guardrails_script_hash: None,
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_update_committee_round_trip() {
    use std::collections::BTreeMap;
    let to_remove = vec![StakeCredential::AddrKeyHash([0x01; 28])];
    let mut to_add = BTreeMap::new();
    to_add.insert(StakeCredential::ScriptHash([0x02; 28]), 300u64);
    let ga = GovAction::UpdateCommittee {
        prev_action_id: None,
        members_to_remove: to_remove,
        members_to_add: to_add,
        quorum: UnitInterval {
            numerator: 2,
            denominator: 3,
        },
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_new_constitution_round_trip() {
    let ga = GovAction::NewConstitution {
        prev_action_id: Some(GovActionId {
            transaction_id: [0xDD; 32],
            gov_action_index: 0,
        }),
        constitution: Constitution {
            anchor: Anchor {
                url: "https://constitution.example".to_owned(),
                data_hash: [0xEE; 32],
            },
            guardrails_script_hash: Some([0xFF; 28]),
        },
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn gov_action_new_constitution_no_guardrails_round_trip() {
    let ga = GovAction::NewConstitution {
        prev_action_id: None,
        constitution: Constitution {
            anchor: Anchor {
                url: "https://example.com/constitution".to_owned(),
                data_hash: [0xAA; 32],
            },
            guardrails_script_hash: None,
        },
    };
    let bytes = ga.to_cbor_bytes();
    let decoded = GovAction::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(ga, decoded);
}

#[test]
fn constitution_round_trip() {
    let c = Constitution {
        anchor: Anchor {
            url: "https://constitution.cardano".to_owned(),
            data_hash: [0x11; 32],
        },
        guardrails_script_hash: Some([0x22; 28]),
    };
    let bytes = c.to_cbor_bytes();
    let decoded = Constitution::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(c, decoded);
}

#[test]
fn constitution_null_guardrails_round_trip() {
    let c = Constitution {
        anchor: Anchor {
            url: "https://example.com".to_owned(),
            data_hash: [0x33; 32],
        },
        guardrails_script_hash: None,
    };
    let bytes = c.to_cbor_bytes();
    let decoded = Constitution::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(c, decoded);
}

#[test]
fn shelley_update_round_trip() {
    use std::collections::BTreeMap;
    let mut proposed = BTreeMap::new();
    let param_update = ProtocolParameterUpdate {
        min_fee_a: Some(1000),
        ..Default::default()
    };
    proposed.insert([0x01; 28], param_update);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 50,
    };
    let bytes = update.to_cbor_bytes();
    let decoded = ShelleyUpdate::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(update, decoded);
}

#[test]
fn shelley_update_multiple_delegates_round_trip() {
    use std::collections::BTreeMap;
    let mut proposed = BTreeMap::new();
    let p1 = ProtocolParameterUpdate::default();
    let p2 = ProtocolParameterUpdate {
        min_fee_b: Some(500_000),
        ..Default::default()
    };
    proposed.insert([0x01; 28], p1);
    proposed.insert([0x02; 28], p2);
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch: 200,
    };
    let bytes = update.to_cbor_bytes();
    let decoded = ShelleyUpdate::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(update, decoded);
}

#[test]
fn shelley_update_empty_proposals_round_trip() {
    use std::collections::BTreeMap;
    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: BTreeMap::new(),
        epoch: 0,
    };
    let bytes = update.to_cbor_bytes();
    let decoded = ShelleyUpdate::from_cbor_bytes(&bytes).expect("decode");
    assert_eq!(update, decoded);
}

#[test]
fn proposal_procedure_with_typed_gov_action_all_variants() {
    for gov_action in [
        GovAction::InfoAction,
        GovAction::NoConfidence { prev_action_id: None },
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (9, 0),
        },
    ] {
        let prop = ProposalProcedure {
            deposit: 1_000_000,
            reward_account: vec![0xE0, 0x01],
            gov_action,
            anchor: Anchor {
                url: "https://example.com".to_owned(),
                data_hash: [0xAA; 32],
            },
        };
        let bytes = prop.to_cbor_bytes();
        let decoded = ProposalProcedure::from_cbor_bytes(&bytes).expect("decode");
        assert_eq!(prop, decoded);
    }
}