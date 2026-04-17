use super::ledger_state_basic::make_shelley_block_with_txs;
use super::txbody_keys::sample_reward_account;
use super::types_and_certs::sample_pool_params;
use super::*;

#[test]
fn ledger_state_pool_state_tracks_registration_and_retirement() {
    let mut state = LedgerState::new(Era::Shelley);
    let params = sample_pool_params();
    let operator = params.operator;

    state.pool_state_mut().register(params.clone());
    assert!(state.pool_state().is_registered(&operator));
    assert!(state.pool_state_mut().retire(operator, EpochNo(240)));

    let pool = state
        .registered_pool(&operator)
        .expect("registered pool after retirement");
    assert_eq!(pool.params(), &params);
    assert_eq!(pool.retiring_epoch(), Some(EpochNo(240)));
}

#[test]
fn ledger_state_query_reward_balance_reads_reward_accounts() {
    let mut state = LedgerState::new(Era::Allegra);
    let reward_account = sample_reward_account();

    assert_eq!(state.query_reward_balance(&reward_account), 0);

    state.reward_accounts_mut().insert(
        reward_account,
        RewardAccountState::new(4_200_000, Some(sample_pool_params().operator)),
    );

    assert_eq!(state.query_reward_balance(&reward_account), 4_200_000);
}

#[test]
fn ledger_state_applies_pool_registration_certificate() {
    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x90; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_100_000,
        },
    );

    let params = sample_pool_params();
    let operator = params.operator;
    // Register pool owners as stake credentials (not required by upstream
    // POOL rule, but useful for reward claiming / query tests).
    for owner in &params.pool_owners {
        state
            .stake_credentials_mut()
            .register(StakeCredential::AddrKeyHash(*owner));
    }
    let block = make_shelley_block_with_txs(
        12,
        1,
        0x91,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x90; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::PoolRegistration(params.clone())]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("pool registration block");

    let registered = state
        .registered_pool(&operator)
        .expect("pool should be registered");
    assert_eq!(registered.params(), &params);
    assert_eq!(registered.retiring_epoch(), None);
}

#[test]
fn ledger_state_pool_retirement_requires_registered_pool() {
    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x92; 32],
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
        0x93,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x92; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: Some(vec![DCert::PoolRetirement([0xA4; 28], EpochNo(90))]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state
        .apply_block(&block)
        .expect_err("missing pool should fail");
    assert_eq!(err, LedgerError::PoolNotRegistered([0xA4; 28]));
    assert_eq!(state.tip, Point::Origin);
}

#[test]
fn ledger_state_applies_withdrawal_and_debits_reward_balance() {
    use std::collections::BTreeMap;

    let mut state = LedgerState::new(Era::Shelley);
    let reward_account = sample_reward_account();
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(500_000, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x94; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_500_000,
        },
    );

    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(reward_account, 500_000);
    let block = make_shelley_block_with_txs(
        12,
        1,
        0x95,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x94; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_900_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: None,
            withdrawals: Some(withdrawals),
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state.apply_block(&block).expect("withdrawal block");
    assert_eq!(state.query_reward_balance(&reward_account), 0);
    assert_eq!(state.utxo().len(), 1);
}

#[test]
fn ledger_state_rejects_withdrawal_above_balance() {
    use std::collections::BTreeMap;

    let mut state = LedgerState::new(Era::Shelley);
    let reward_account = sample_reward_account();
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(400_000, None));
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x96; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_500_000,
        },
    );

    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(reward_account, 500_000);
    let block = make_shelley_block_with_txs(
        12,
        1,
        0x97,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x96; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_900_000,
            }],
            fee: 100_000,
            ttl: 20,
            certificates: None,
            withdrawals: Some(withdrawals),
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state
        .apply_block(&block)
        .expect_err("over-withdrawal should fail");
    assert_eq!(
        err,
        LedgerError::WithdrawalExceedsBalance {
            account: reward_account,
            requested: 500_000,
            available: 400_000,
        }
    );
    assert_eq!(state.query_reward_balance(&reward_account), 400_000);
    assert_eq!(state.tip, Point::Origin);
}

#[test]
fn ledger_state_query_utxos_by_address_deduplicates_dual_views() {
    let mut state = LedgerState::new(Era::Mary);
    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash([0x11; 28]),
    });
    let address_bytes = address.to_bytes();
    let txin = ShelleyTxIn {
        transaction_id: [0x22; 32],
        index: 0,
    };

    state.utxo_mut().insert(
        txin.clone(),
        ShelleyTxOut {
            address: address_bytes.clone(),
            amount: 3_000_000,
        },
    );
    state.multi_era_utxo_mut().insert_shelley(
        txin.clone(),
        ShelleyTxOut {
            address: address_bytes.clone(),
            amount: 3_000_000,
        },
    );
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x33; 32],
            index: 1,
        },
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: address_bytes,
            amount: 4_000_000,
        }),
    );

    let entries = state.query_utxos_by_address(&address);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, txin);
    assert_eq!(entries[1].0.index, 1);
}

#[test]
fn ledger_state_query_balance_aggregates_coin_and_assets() {
    use std::collections::BTreeMap;

    let mut state = LedgerState::new(Era::Mary);
    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash([0x44; 28]),
    });
    let address_bytes = address.to_bytes();

    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x55; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: address_bytes.clone(),
            amount: 2_000_000,
        },
    );

    let policy = [0x66; 28];
    let asset_name = b"oak".to_vec();
    let mut assets = BTreeMap::new();
    assets.insert(asset_name.clone(), 7u64);
    let mut multi_asset = BTreeMap::new();
    multi_asset.insert(policy, assets);

    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x77; 32],
            index: 1,
        },
        MultiEraTxOut::Mary(MaryTxOut {
            address: address_bytes,
            amount: Value::CoinAndAssets(5_000_000, multi_asset),
        }),
    );

    let balance = state.query_balance(&address);
    match balance {
        Value::CoinAndAssets(coin, assets) => {
            assert_eq!(coin, 7_000_000);
            assert_eq!(
                assets
                    .get(&policy)
                    .and_then(|m| m.get(&asset_name))
                    .copied(),
                Some(7)
            );
        }
        other => panic!("expected coin and assets, got {other:?}"),
    }
}
