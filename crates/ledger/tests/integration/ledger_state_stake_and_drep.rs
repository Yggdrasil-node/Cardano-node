use super::*;
use super::ledger_state_basic::make_shelley_block_with_txs;
use super::txbody_keys::sample_reward_account;
use super::types_and_certs::{sample_hash28, sample_pool_params};

#[test]
fn ledger_state_snapshot_exposes_pool_and_reward_state() {
    let mut state = LedgerState::new(Era::Conway);
    let params = sample_pool_params();
    let operator = params.operator;
    let reward_account = sample_reward_account();

    state.pool_state_mut().register(params.clone());
    state.reward_accounts_mut().insert(
        reward_account,
        RewardAccountState::new(9_000_000, Some(operator)),
    );

    let snapshot = state.snapshot();
    let pool = snapshot
        .registered_pool(&operator)
        .expect("registered pool in snapshot");
    let account = snapshot
        .reward_account_state(&reward_account)
        .expect("reward account in snapshot");

    assert_eq!(pool.params(), &params);
    assert_eq!(pool.retiring_epoch(), None);
    assert_eq!(account.balance(), 9_000_000);
    assert_eq!(account.delegated_pool(), Some(operator));
}

#[test]
fn ledger_state_checkpoint_restores_stake_pool_and_rewards_state() {
    let mut state = LedgerState::new(Era::Conway);
    let params = sample_pool_params();
    let operator = params.operator;
    let reward_account = sample_reward_account();

    state.pool_state_mut().register(params.clone());
    state.reward_accounts_mut().insert(
        reward_account,
        RewardAccountState::new(9_000_000, Some(operator)),
    );

    let checkpoint = state.checkpoint();

    state.pool_state_mut().retire(operator, EpochNo(99));
    state.reward_accounts_mut().insert(
        reward_account,
        RewardAccountState::new(1_000_000, None),
    );

    state.rollback_to_checkpoint(&checkpoint);

    let restored_pool = state
        .registered_pool(&operator)
        .expect("restored pool after rollback");
    let restored_account = state
        .reward_account_state(&reward_account)
        .expect("restored reward account after rollback");

    assert_eq!(restored_pool.params(), &params);
    assert_eq!(restored_pool.retiring_epoch(), None);
    assert_eq!(restored_account.balance(), 9_000_000);
    assert_eq!(restored_account.delegated_pool(), Some(operator));
}

#[test]
fn ledger_state_checkpoint_cbor_round_trip_preserves_state() {
    let mut state = LedgerState::new(Era::Conway);
    let params = sample_pool_params();
    let operator = params.operator;
    let reward_account = sample_reward_account();
    let credential = reward_account.credential;
    let drep = DRep::KeyHash([0x77; 28]);

    state.current_era = Era::Conway;
    state.tip = Point::BlockPoint(SlotNo(44), HeaderHash([0x44; 32]));
    state.set_current_epoch(EpochNo(7));
    state.pool_state_mut().register(params.clone());
    state.stake_credentials_mut().register(credential);
    state.reward_accounts_mut().insert(
        reward_account,
        RewardAccountState::new(9_000_000, Some(operator)),
    );
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new(2_000_000, Some(Anchor {
            url: "https://example.com/drep".to_owned(),
            data_hash: [0x55; 32],
        })));
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        },
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0x01, 0x02],
            amount: 123,
        }),
    );
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xBB; 32],
            index: 1,
        },
        ShelleyTxOut {
            address: vec![0x03, 0x04],
            amount: 456,
        },
    );

    let checkpoint = state.checkpoint();
    let encoded = checkpoint.to_cbor_bytes();
    let decoded = LedgerStateCheckpoint::from_cbor_bytes(&encoded).expect("decode checkpoint");

    assert_eq!(decoded, checkpoint);
    assert_eq!(decoded.restore(), state);
}

#[test]
fn ledger_state_snapshot_exposes_current_epoch() {
    let mut state = LedgerState::new(Era::Conway);
    state.set_current_epoch(EpochNo(4));

    let snapshot = state.snapshot();

    assert_eq!(snapshot.current_epoch(), EpochNo(4));
}

#[test]
fn ledger_state_registers_stake_credential_via_certificate() {
    let mut state = LedgerState::new(Era::Shelley);
    let credential = StakeCredential::AddrKeyHash(sample_hash28());
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x98; 32],
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
        0x99,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x98; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::AccountRegistration(credential)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("registration block");
    assert!(state.stake_credentials().is_registered(&credential));
    assert_eq!(
        state
            .stake_credential_state(&credential)
            .expect("registered credential")
            .delegated_pool(),
        None
    );
}

#[test]
fn ledger_state_delegates_registered_stake_credential_to_pool() {
    let mut state = LedgerState::new(Era::Shelley);
    let credential = StakeCredential::AddrKeyHash([0x21; 28]);
    let reward_account = RewardAccount {
        network: 1,
        credential,
    };
    let params = sample_pool_params();
    let operator = params.operator;

    state.pool_state_mut().register(params);
    state.stake_credentials_mut().register(credential);
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(0, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x9A; 32],
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
        0x9B,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x9A; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::DelegationToStakePool(credential, operator)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("delegation block");
    assert_eq!(
        state
            .stake_credential_state(&credential)
            .expect("delegated credential")
            .delegated_pool(),
        Some(operator)
    );
    assert_eq!(
        state
            .reward_account_state(&reward_account)
            .expect("reward account synced")
            .delegated_pool(),
        Some(operator)
    );
}

#[test]
fn ledger_state_rejects_delegation_for_unregistered_stake_credential() {
    let mut state = LedgerState::new(Era::Shelley);
    let credential = StakeCredential::AddrKeyHash([0x31; 28]);
    let params = sample_pool_params();
    let operator = params.operator;

    state.pool_state_mut().register(params);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x9C; 32],
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
        0x9D,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x9C; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::DelegationToStakePool(credential, operator)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state.apply_block(&block).expect_err("unregistered delegation should fail");
    assert_eq!(err, LedgerError::StakeCredentialNotRegistered(credential));
}

#[test]
fn ledger_state_unregisters_stake_credential_with_zero_rewards() {
    let mut state = LedgerState::new(Era::Shelley);
    let credential = StakeCredential::AddrKeyHash([0x41; 28]);
    let reward_account = RewardAccount {
        network: 1,
        credential,
    };
    state.stake_credentials_mut().register(credential);
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(0, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x9E; 32],
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
        0x9F,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x9E; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::AccountUnregistration(credential)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("unregistration block");
    assert!(!state.stake_credentials().is_registered(&credential));
    assert!(state.reward_account_state(&reward_account).is_none());
}

#[test]
fn ledger_state_rejects_unregistration_with_nonzero_rewards() {
    let mut state = LedgerState::new(Era::Shelley);
    let credential = StakeCredential::AddrKeyHash([0x51; 28]);
    let reward_account = RewardAccount {
        network: 1,
        credential,
    };
    state.stake_credentials_mut().register(credential);
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(123_456, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xA0; 32],
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
        0xA1,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xA0; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::AccountUnregistration(credential)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state.apply_block(&block).expect_err("nonzero rewards should fail");
    assert_eq!(
        err,
        LedgerError::StakeCredentialHasRewards {
            credential,
            balance: 123_456,
        }
    );
    assert!(state.stake_credentials().is_registered(&credential));
}

#[test]
fn ledger_state_registers_and_updates_drep() {
    let mut state = LedgerState::new(Era::Conway);
    let credential = StakeCredential::AddrKeyHash([0x61; 28]);
    let drep = DRep::KeyHash([0x61; 28]);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xA2; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 3_100_000,
        },
    );

    let anchor = Anchor {
        url: "https://example.com/drep.json".to_string(),
        data_hash: [0x62; 32],
    };
    let block = make_shelley_block_with_txs(
        12,
        1,
        0xA3,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xA2; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![
                DCert::DrepRegistration(credential, 2_000_000, Some(anchor.clone())),
                DCert::DrepUpdate(credential, None),
            ]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("drep registration block");
    let registered = state.registered_drep(&drep).expect("registered drep");
    assert_eq!(registered.deposit(), 2_000_000);
    assert_eq!(registered.anchor(), None);
}

#[test]
fn ledger_state_delegates_registered_stake_credential_to_drep() {
    let mut state = LedgerState::new(Era::Conway);
    let credential = StakeCredential::AddrKeyHash([0x63; 28]);
    let drep_credential = StakeCredential::AddrKeyHash([0x64; 28]);
    let drep = DRep::KeyHash([0x64; 28]);
    state.stake_credentials_mut().register(credential);
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new(3_000_000, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xA4; 32],
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
        0xA5,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xA4; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::DelegationToDrep(credential, drep)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("drep delegation block");
    assert_eq!(
        state
            .stake_credential_state(&credential)
            .expect("delegated credential")
            .delegated_drep(),
        Some(DRep::KeyHash(drep_credential.hash().to_owned()))
    );
}

#[test]
fn ledger_state_allows_builtin_drep_delegation_without_registration() {
    let mut state = LedgerState::new(Era::Conway);
    let credential = StakeCredential::AddrKeyHash([0x65; 28]);
    state.stake_credentials_mut().register(credential);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xA6; 32],
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
        0xA7,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xA6; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::DelegationToDrep(credential, DRep::AlwaysAbstain)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("builtin drep delegation");
    assert_eq!(
        state
            .stake_credential_state(&credential)
            .expect("credential state")
            .delegated_drep(),
        Some(DRep::AlwaysAbstain)
    );
}

#[test]
fn ledger_state_rejects_unregistered_drep_delegation() {
    let mut state = LedgerState::new(Era::Conway);
    let credential = StakeCredential::AddrKeyHash([0x66; 28]);
    let drep = DRep::KeyHash([0x67; 28]);
    state.stake_credentials_mut().register(credential);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xA8; 32],
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
        0xA9,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0xA8; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::DelegationToDrep(credential, drep)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state.apply_block(&block).expect_err("unregistered drep should fail");
    assert_eq!(err, LedgerError::DelegateeDRepNotRegistered(drep));
}