//! Conway-era certificate deposit / refund / withdrawal validation tests.
//!
//! Upstream references:
//! - `Cardano.Ledger.Conway.Rules.Deleg` — `IncorrectDepositDELEG`, `RefundIncorrectDELEG`
//! - `Cardano.Ledger.Conway.Rules.GovCert` — `ConwayDRepIncorrectDeposit`, `ConwayDRepIncorrectRefund`
//! - `Cardano.Ledger.Conway.Rules.Certs` — `WithdrawalsNotInRewardsCERTS`

use super::*;

/// Helper to build a Conway-era block.
fn make_conway_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<ConwayTxBody>) -> Block {
    let tx_list: Vec<Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            Tx {
                id: TxId(id_hash.0),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            }
        })
        .collect();

    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: tx_list,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

/// Build a Conway `LedgerState` with protocol params requiring specific
/// `key_deposit` and `drep_deposit` values, and fee enforcement disabled.
fn conway_state(key_deposit: u64, drep_deposit: u64) -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.key_deposit = key_deposit;
    pp.drep_deposit = Some(drep_deposit);
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    state.set_protocol_params(pp);
    state
}

/// Minimal Conway tx body with a single cert, fee = 0, and the given UTxO
/// input pointing at consumed.
fn conway_tx_with_cert(input_hash: [u8; 32], output_coin: u64, fee: u64, cert: DCert) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: input_hash, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(100),
        certificates: Some(vec![cert]),
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: None,
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }
}

// -----------------------------------------------------------------------
// IncorrectDepositDELEG — AccountRegistrationDeposit
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_key_deposit_on_registration() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let consumed = 1_000_000 + 3_000_000; // wrong deposit amount (3M ≠ 2M)
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xA0; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let cred = StakeCredential::AddrKeyHash([0xA1; 28]);
    let block = make_conway_block(10, 1, 0xA2, vec![conway_tx_with_cert(
        [0xA0; 32],
        1_000_000,
        0,
        DCert::AccountRegistrationDeposit(cred, 3_000_000), // wrong deposit
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::IncorrectDepositDELEG { supplied: 3_000_000, expected: key_deposit }
    );
}

#[test]
fn conway_accepts_correct_key_deposit_on_registration() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let consumed = 1_000_000 + key_deposit;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xA3; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let cred = StakeCredential::AddrKeyHash([0xA4; 28]);
    let block = make_conway_block(10, 1, 0xA5, vec![conway_tx_with_cert(
        [0xA3; 32],
        1_000_000,
        0,
        DCert::AccountRegistrationDeposit(cred, key_deposit),
    )]);

    state.apply_block(&block).expect("correct deposit should be accepted");
    assert!(state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// IncorrectDepositDELEG — AccountRegistrationDelegationToStakePool
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_deposit_on_reg_delegation_to_pool() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let consumed = 1_000_000 + 999_999;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xB0; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    // Register a pool for delegation target
    state.pool_state_mut().register(PoolParams {
        operator: [0xBB; 28],
        vrf_keyhash: [0xBB; 32],
        pledge: 0,
        cost: 340_000_000,
        margin: UnitInterval { numerator: 0, denominator: 1 },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xBB; 28]),
        },
        pool_owners: vec![[0xBB; 28]],
        relays: vec![],
        pool_metadata: None,
    });

    let cred = StakeCredential::AddrKeyHash([0xB1; 28]);
    let block = make_conway_block(10, 1, 0xB2, vec![conway_tx_with_cert(
        [0xB0; 32],
        1_000_000,
        0,
        DCert::AccountRegistrationDelegationToStakePool(cred, [0xBB; 28], 999_999), // wrong
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::IncorrectDepositDELEG { supplied: 999_999, expected: key_deposit }
    );
}

// -----------------------------------------------------------------------
// IncorrectDepositDELEG — AccountRegistrationDelegationToDrep
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_deposit_on_reg_delegation_to_drep() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let consumed = 1_000_000 + 1_000_000;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xC0; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    let block = make_conway_block(10, 1, 0xC2, vec![conway_tx_with_cert(
        [0xC0; 32],
        1_000_000,
        0,
        DCert::AccountRegistrationDelegationToDrep(cred, DRep::AlwaysAbstain, 1_000_000), // wrong
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::IncorrectDepositDELEG { supplied: 1_000_000, expected: key_deposit }
    );
}

// -----------------------------------------------------------------------
// IncorrectDepositDELEG — AccountRegistrationDelegationToStakePoolAndDrep
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_deposit_on_reg_delegation_to_pool_and_drep() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let consumed = 1_000_000 + 5_000_000;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xD0; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    state.pool_state_mut().register(PoolParams {
        operator: [0xDD; 28],
        vrf_keyhash: [0xDD; 32],
        pledge: 0,
        cost: 340_000_000,
        margin: UnitInterval { numerator: 0, denominator: 1 },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xDD; 28]),
        },
        pool_owners: vec![[0xDD; 28]],
        relays: vec![],
        pool_metadata: None,
    });

    let cred = StakeCredential::AddrKeyHash([0xD1; 28]);
    let block = make_conway_block(10, 1, 0xD2, vec![conway_tx_with_cert(
        [0xD0; 32],
        1_000_000,
        0,
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(
            cred,
            [0xDD; 28],
            DRep::AlwaysAbstain,
            5_000_000, // wrong
        ),
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::IncorrectDepositDELEG { supplied: 5_000_000, expected: key_deposit }
    );
}

// -----------------------------------------------------------------------
// IncorrectKeyDepositRefund — AccountUnregistrationDeposit
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_key_refund_on_unregistration() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let cred = StakeCredential::AddrKeyHash([0xE0; 28]);
    state.stake_credentials_mut().register(cred);
    state.deposit_pot_mut().key_deposits += key_deposit;

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xE1; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: 200_000 },
    );

    let block = make_conway_block(10, 1, 0xE2, vec![conway_tx_with_cert(
        [0xE1; 32],
        200_000,
        0,
        DCert::AccountUnregistrationDeposit(cred, 1_000_000), // wrong refund
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::IncorrectKeyDepositRefund { supplied: 1_000_000, expected: key_deposit }
    );
}

#[test]
fn conway_accepts_correct_key_refund_on_unregistration() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state(key_deposit, 500_000);

    let cred = StakeCredential::AddrKeyHash([0xE3; 28]);
    state.stake_credentials_mut().register(cred);
    state.deposit_pot_mut().key_deposits += key_deposit;

    // consumed + refund = output + fee → output = refund, consumed = 0
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xE4; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: 0 },
    );

    let block = make_conway_block(10, 1, 0xE5, vec![conway_tx_with_cert(
        [0xE4; 32],
        key_deposit,
        0,
        DCert::AccountUnregistrationDeposit(cred, key_deposit),
    )]);

    state.apply_block(&block).expect("correct refund should be accepted");
    assert!(!state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// DrepIncorrectDeposit — DrepRegistration
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_drep_deposit() {
    let drep_deposit = 500_000u64;
    let mut state = conway_state(2_000_000, drep_deposit);

    let consumed = 1_000_000 + 999_999;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xF0; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let cred = StakeCredential::AddrKeyHash([0xF1; 28]);
    let block = make_conway_block(10, 1, 0xF2, vec![conway_tx_with_cert(
        [0xF0; 32],
        1_000_000,
        0,
        DCert::DrepRegistration(cred, 999_999, None), // wrong deposit
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::DrepIncorrectDeposit { supplied: 999_999, expected: drep_deposit }
    );
}

#[test]
fn conway_accepts_correct_drep_deposit() {
    let drep_deposit = 500_000u64;
    let mut state = conway_state(2_000_000, drep_deposit);

    let consumed = 1_000_000 + drep_deposit;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xF3; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let cred = StakeCredential::AddrKeyHash([0xF4; 28]);
    let block = make_conway_block(10, 1, 0xF5, vec![conway_tx_with_cert(
        [0xF3; 32],
        1_000_000,
        0,
        DCert::DrepRegistration(cred, drep_deposit, None),
    )]);

    state.apply_block(&block).expect("correct drep deposit should be accepted");
}

// -----------------------------------------------------------------------
// DrepIncorrectRefund — DrepUnregistration
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_incorrect_drep_refund() {
    let drep_deposit = 500_000u64;
    let mut state = conway_state(2_000_000, drep_deposit);

    let cred = StakeCredential::AddrKeyHash([0xF6; 28]);
    let drep = DRep::KeyHash([0xF6; 28]);
    state.drep_state_mut().register(drep, RegisteredDrep::new(drep_deposit, None));
    state.deposit_pot_mut().drep_deposits += drep_deposit;

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xF7; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: 200_000 },
    );

    let block = make_conway_block(10, 1, 0xF8, vec![conway_tx_with_cert(
        [0xF7; 32],
        200_000,
        0,
        DCert::DrepUnregistration(cred, 999_999), // wrong refund
    )]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::DrepIncorrectRefund { supplied: 999_999, expected: drep_deposit }
    );
}

#[test]
fn conway_accepts_correct_drep_refund() {
    let drep_deposit = 500_000u64;
    let mut state = conway_state(2_000_000, drep_deposit);

    let cred = StakeCredential::AddrKeyHash([0xF9; 28]);
    let drep = DRep::KeyHash([0xF9; 28]);
    state.drep_state_mut().register(drep, RegisteredDrep::new(drep_deposit, None));
    state.deposit_pot_mut().drep_deposits += drep_deposit;

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xFA; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: 0 },
    );

    let block = make_conway_block(10, 1, 0xFB, vec![conway_tx_with_cert(
        [0xFA; 32],
        drep_deposit,
        0,
        DCert::DrepUnregistration(cred, drep_deposit),
    )]);

    state.apply_block(&block).expect("correct drep refund should be accepted");
    assert!(!state.drep_state().is_registered(&drep));
}

// -----------------------------------------------------------------------
// WithdrawalNotFullDrain — Conway exact-drain semantics
// -----------------------------------------------------------------------

#[test]
fn conway_rejects_partial_withdrawal() {
    let mut state = conway_state(2_000_000, 500_000);

    let cred = StakeCredential::AddrKeyHash([0xFC; 28]);
    state.stake_credentials_mut().register(cred);
    let ra = RewardAccount { network: 1, credential: cred };
    state.reward_accounts_mut().insert(ra, RewardAccountState::new(1_000_000, None));

    // Supply enough for output + fee + withdrawal = balance
    let consumed = 1_500_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0xFD; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra, 500_000); // partial: only 500k of 1M balance

    let block = make_conway_block(10, 1, 0xFE, vec![ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0xFD; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: Some(withdrawals),
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }]);

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(
        err,
        LedgerError::WithdrawalNotFullDrain {
            account: ra,
            requested: 500_000,
            balance: 1_000_000,
        }
    );
}

#[test]
fn conway_accepts_full_drain_withdrawal() {
    let mut state = conway_state(2_000_000, 500_000);

    let cred = StakeCredential::AddrKeyHash([0x01; 28]);
    state.stake_credentials_mut().register(cred);
    let ra = RewardAccount { network: 1, credential: cred };
    state.reward_accounts_mut().insert(ra, RewardAccountState::new(1_000_000, None));

    let consumed = 1_000_000u64; // consumed + withdrawal(1M) = output(2M) + fee(0)
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra, 1_000_000); // full drain

    let block = make_conway_block(10, 1, 0x03, vec![ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: Some(withdrawals),
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }]);

    state.apply_block(&block).expect("full drain withdrawal should be accepted");
    assert_eq!(state.reward_accounts().get(&ra).unwrap().balance(), 0);
}

#[test]
fn shelley_allows_partial_withdrawal() {
    // Shelley does NOT enforce exact-drain semantics.
    let mut state = LedgerState::new(Era::Shelley);
    let mut pp = ProtocolParameters::default();
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    state.set_protocol_params(pp);

    let cred = StakeCredential::AddrKeyHash([0x04; 28]);
    state.stake_credentials_mut().register(cred);
    let ra = RewardAccount { network: 1, credential: cred };
    state.reward_accounts_mut().insert(ra, RewardAccountState::new(1_000_000, None));

    let consumed = 1_000_000u64;
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x05; 32], index: 0 },
        ShelleyTxOut { address: vec![0x01], amount: consumed },
    );

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra, 500_000); // partial — allowed in Shelley

    let block = super::ledger_state_basic::make_shelley_block_with_txs(5, 1, 0x06, vec![ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x05; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: vec![0x02], amount: 1_500_000 }],
        fee: 0,
        ttl: 100,
        certificates: None,
        withdrawals: Some(withdrawals),
        update: None,
        auxiliary_data_hash: None,
    }]);

    state.apply_block(&block).expect("Shelley allows partial withdrawal");
    assert_eq!(state.reward_accounts().get(&ra).unwrap().balance(), 500_000);
}
