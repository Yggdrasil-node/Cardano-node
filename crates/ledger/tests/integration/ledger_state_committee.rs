use super::*;
use super::ledger_state_basic::make_shelley_block_with_txs;

#[test]
fn ledger_state_authorizes_known_committee_member_hot_key() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x68; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x69; 28]);
    state.committee_state_mut().register(cold_credential);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let block = make_shelley_block_with_txs(
        12,
        1,
        0xAB,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xAA; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("committee authorization block");
    assert_eq!(
        state
            .committee_member_state(&cold_credential)
            .expect("committee member")
            .authorization(),
        Some(&CommitteeAuthorization::CommitteeHotCredential(hot_credential))
    );
}

#[test]
fn ledger_state_resigns_known_committee_member() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x6A; 28]);
    let hot_credential = StakeCredential::AddrKeyHash([0x6B; 28]);
    let anchor = Anchor {
        url: "https://example.com/committee-resignation.json".to_string(),
        data_hash: [0x6C; 32],
    };
    state.committee_state_mut().register(cold_credential);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAC; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAD; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let authorization_block = make_shelley_block_with_txs(
        12,
        1,
        0xAE,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xAC; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&authorization_block)
        .expect("committee authorization block");

    let block = make_shelley_block_with_txs(
        12,
        1,
        0xAF,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xAD; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeResignation(
                cold_credential,
                Some(anchor.clone()),
            )]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("committee resignation block");
    assert_eq!(
        state
            .committee_member_state(&cold_credential)
            .expect("committee member")
            .authorization(),
        Some(&CommitteeAuthorization::CommitteeMemberResigned(Some(anchor)))
    );
}

#[test]
fn ledger_state_rejects_unknown_committee_member_authorization() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x6D; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x6E; 28]);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAE; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let block = make_shelley_block_with_txs(
        12,
        1,
        0xAF,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xAE; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state.apply_block(&block).expect_err("unknown committee member");
    assert_eq!(err, LedgerError::CommitteeIsUnknown(cold_credential));
}

#[test]
fn ledger_state_rejects_reauthorizing_resigned_committee_member() {
    let mut state = LedgerState::new(Era::Conway);
    let cold_credential = StakeCredential::AddrKeyHash([0x70; 28]);
    let hot_credential = StakeCredential::ScriptHash([0x71; 28]);
    state.committee_state_mut().register(cold_credential);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xB0; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xB1; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let resignation_block = make_shelley_block_with_txs(
        12,
        1,
        0xB2,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xB0; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeResignation(cold_credential, None)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&resignation_block)
        .expect("committee resignation block");

    let block = make_shelley_block_with_txs(
        12,
        1,
        0xB3,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xB1; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::CommitteeAuthorization(
                cold_credential,
                hot_credential,
            )]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state
        .apply_block(&block)
        .expect_err("resigned committee member cannot reauthorize");
    assert_eq!(
        err,
        LedgerError::CommitteeHasPreviouslyResigned(cold_credential)
    );
}