//! Tests for deposit/refund inclusion in the value preservation equation.
//!
//! Upstream reference: `Cardano.Ledger.Shelley.Rules.Utxo`
//! ```text
//! consumed = balance(txins ◁ utxo) + refunds + withdrawals
//! produced = balance(outs) + fee + deposits [+ donation]
//! ```

use super::ledger_state_basic::make_shelley_block_with_txs;
use super::*;

/// Helper to build a Conway-era block with a single CBOR-encoded transaction.
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

// -----------------------------------------------------------------------
// Shelley-era: key registration deposit
// -----------------------------------------------------------------------

#[test]
fn shelley_key_registration_deposit_balances_value_preservation() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(ProtocolParameters::default());
    let key_deposit = state.protocol_params().map(|pp| pp.key_deposit).unwrap();

    // consumed = output + fee + deposit
    let fee = 200_000;
    let consumed = 1_000_000 + fee + key_deposit;
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0x10; 28]);
    let block = make_shelley_block_with_txs(
        5,
        1,
        0x02,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x01; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee,
            ttl: 100,
            certificates: Some(vec![DCert::AccountRegistration(cred)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("block with key registration should balance");
    assert!(state.stake_credentials().is_registered(&cred));
}

#[test]
fn shelley_key_registration_without_deposit_fails() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(ProtocolParameters::default());

    // consumed = output + fee only (no room for deposit) — should fail
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x03; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_200_000,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0x11; 28]);
    let block = make_shelley_block_with_txs(
        5,
        1,
        0x04,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x03; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 200_000,
            ttl: 100,
            certificates: Some(vec![DCert::AccountRegistration(cred)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    let err = state.apply_block(&block).unwrap_err();
    assert!(matches!(err, LedgerError::ValueNotPreserved { .. }));
}

// -----------------------------------------------------------------------
// Shelley-era: key deregistration refund
// -----------------------------------------------------------------------

#[test]
fn shelley_key_deregistration_refund_balances_value_preservation() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(ProtocolParameters::default());
    let key_deposit = state.protocol_params().map(|pp| pp.key_deposit).unwrap();

    let cred = StakeCredential::AddrKeyHash([0x20; 28]);
    state.stake_credentials_mut().register(cred);
    state.deposit_pot_mut().key_deposits += key_deposit;

    // consumed + refund = output + fee
    // consumed = 200_000, refund = key_deposit, output = key_deposit, fee = 200_000
    // check: 200_000 + key_deposit = key_deposit + 200_000 ✓
    let fee = 200_000;
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x05; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: fee,
        },
    );

    let block = make_shelley_block_with_txs(
        5,
        1,
        0x06,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x05; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: key_deposit,
            }],
            fee,
            ttl: 100,
            certificates: Some(vec![DCert::AccountUnregistration(cred)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("block with key deregistration refund should balance");
    assert!(!state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// Conway-era: explicit deposit registration + deregistration
// -----------------------------------------------------------------------

#[test]
fn conway_explicit_deposit_registration_balances() {
    let mut state = LedgerState::new(Era::Conway);
    let deposit = 3_000_000u64;
    let mut pp = ProtocolParameters::default();
    pp.key_deposit = deposit;
    state.set_protocol_params(pp);

    // consumed = output + fee + deposit
    let consumed = 1_000_000 + 200_000 + deposit;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x07; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0x30; 28]);
    let block = make_conway_block(
        10,
        1,
        0x08,
        vec![ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x07; 32],
                index: 0,
            }],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            fee: 200_000,
            ttl: Some(100),
            certificates: Some(vec![DCert::AccountRegistrationDeposit(cred, deposit)]),
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
        }],
    );

    state
        .apply_block(&block)
        .expect("Conway explicit deposit registration should balance");
    assert!(state.stake_credentials().is_registered(&cred));
}

#[test]
fn conway_explicit_deposit_deregistration_refund_balances() {
    let mut state = LedgerState::new(Era::Conway);
    let deposit = 3_000_000u64;
    let mut pp = ProtocolParameters::default();
    pp.key_deposit = deposit;
    state.set_protocol_params(pp);

    let cred = StakeCredential::AddrKeyHash([0x31; 28]);
    state.stake_credentials_mut().register(cred);
    state.deposit_pot_mut().key_deposits += deposit;

    // consumed + refund = output + fee → consumed = 200_000, refund = deposit, output = deposit, fee = 200_000
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x09; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 200_000,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x0A,
        vec![ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x09; 32],
                index: 0,
            }],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(deposit),
                datum_option: None,
                script_ref: None,
            }],
            fee: 200_000,
            ttl: Some(100),
            certificates: Some(vec![DCert::AccountUnregistrationDeposit(cred, deposit)]),
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
        }],
    );

    state
        .apply_block(&block)
        .expect("Conway explicit deposit deregistration refund should balance");
    assert!(!state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// Pool registration deposit
// -----------------------------------------------------------------------

#[test]
fn shelley_pool_registration_deposit_balances() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(ProtocolParameters::default());
    let pool_deposit = state.protocol_params().map(|pp| pp.pool_deposit).unwrap();

    let fee = 200_000;
    let consumed = 1_000_000 + fee + pool_deposit;
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x0B; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    // Register pool owner as stake credential (not required by upstream
    // POOL rule, but useful for reward claiming).
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0x40; 28]));

    let params = PoolParams {
        operator: [0x40; 28],
        vrf_keyhash: [0x40; 32],
        pledge: 0,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x40; 28]),
        },
        pool_owners: vec![[0x40; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let block = make_shelley_block_with_txs(
        5,
        1,
        0x0C,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x0B; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 200_000,
            ttl: 100,
            certificates: Some(vec![DCert::PoolRegistration(params)]),
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("pool registration deposit should balance");
    assert!(state.pool_state().is_registered(&[0x40; 28]));
}

// -----------------------------------------------------------------------
// Combined: deposit + withdrawal in same tx
// -----------------------------------------------------------------------

#[test]
fn shelley_registration_deposit_plus_withdrawal_balances() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(ProtocolParameters::default());
    let key_deposit = state.protocol_params().map(|pp| pp.key_deposit).unwrap();

    // Seed a reward account with some balance for withdrawal.
    let cred_existing = StakeCredential::AddrKeyHash([0x50; 28]);
    state.stake_credentials_mut().register(cred_existing);
    let ra = RewardAccount {
        network: 1,
        credential: cred_existing,
    };
    state
        .reward_accounts_mut()
        .insert(ra, RewardAccountState::new(500_000, None));

    // consumed + withdrawal + refunds = output + fee + deposits
    // consumed + 500_000 + 0 = output + fee + key_deposit
    let consumed = 1_000_000 + 200_000 + key_deposit - 500_000;
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x0D; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred_new = StakeCredential::AddrKeyHash([0x51; 28]);
    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra, 500_000);

    let block = make_shelley_block_with_txs(
        5,
        1,
        0x0E,
        vec![ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x0D; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x02],
                amount: 1_000_000,
            }],
            fee: 200_000,
            ttl: 100,
            certificates: Some(vec![DCert::AccountRegistration(cred_new)]),
            withdrawals: Some(withdrawals),
            update: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("deposit + withdrawal should balance");
    assert!(state.stake_credentials().is_registered(&cred_new));
}

// -----------------------------------------------------------------------
// Conway: deposit + donation in same tx
// -----------------------------------------------------------------------

#[test]
fn conway_deposit_plus_donation_balances() {
    let mut state = LedgerState::new(Era::Conway);
    let deposit = 2_000_000u64;
    state.set_protocol_params(ProtocolParameters::default());
    let donation = 1_000_000u64;

    // consumed = output + fee + deposit + donation
    let consumed = 1_000_000 + 200_000 + deposit + donation;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x0F; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0x60; 28]);
    let block = make_conway_block(
        10,
        1,
        0x10,
        vec![ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x0F; 32],
                index: 0,
            }],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            fee: 200_000,
            ttl: Some(100),
            certificates: Some(vec![DCert::AccountRegistrationDeposit(cred, deposit)]),
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
            treasury_donation: Some(donation),
            current_treasury_value: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("Conway deposit + donation should balance");
    assert!(state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// DRep deposit
// -----------------------------------------------------------------------

#[test]
fn conway_drep_registration_deposit_balances() {
    let mut state = LedgerState::new(Era::Conway);
    let drep_deposit = 500_000u64;

    let consumed = 1_000_000 + 200_000 + drep_deposit;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0x70; 28]);
    let block = make_conway_block(
        10,
        1,
        0x12,
        vec![ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x11; 32],
                index: 0,
            }],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            fee: 200_000,
            ttl: Some(100),
            certificates: Some(vec![DCert::DrepRegistration(cred, drep_deposit, None)]),
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
        }],
    );

    state
        .apply_block(&block)
        .expect("DRep registration deposit should balance");
    let drep = DRep::KeyHash([0x70; 28]);
    assert!(state.drep_state().is_registered(&drep));
}

#[test]
fn conway_drep_deregistration_refund_balances() {
    let mut state = LedgerState::new(Era::Conway);
    let drep_deposit = 500_000u64;

    let cred = StakeCredential::AddrKeyHash([0x71; 28]);
    let drep = DRep::KeyHash([0x71; 28]);
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new(drep_deposit, None));
    state.deposit_pot_mut().drep_deposits += drep_deposit;

    // consumed + refund = output + fee → consumed = 200_000
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x13; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 200_000,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x14,
        vec![ConwayTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [0x13; 32],
                index: 0,
            }],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(drep_deposit),
                datum_option: None,
                script_ref: None,
            }],
            fee: 200_000,
            ttl: Some(100),
            certificates: Some(vec![DCert::DrepUnregistration(cred, drep_deposit)]),
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
        }],
    );

    state
        .apply_block(&block)
        .expect("DRep deregistration refund should balance");
    assert!(!state.drep_state().is_registered(&drep));
}

// -----------------------------------------------------------------------
// Conway submitted-tx treasury donation accumulates into utxos_donation
// -----------------------------------------------------------------------

/// Upstream: `Cardano.Ledger.Conway.Rules.Utxos` — `utxosDonationL`
/// Treasury donations in submitted-tx path must accumulate into
/// `LedgerState.utxos_donation` just like the block-apply path.
#[test]
fn conway_submitted_tx_accumulates_treasury_donation() {
    use yggdrasil_ledger::tx::AlonzoCompatibleSubmittedTx;
    use yggdrasil_ledger::tx::MultiEraSubmittedTx;

    let signer = TestSigner::new([0xD0; 32]);
    let mut state = LedgerState::new(Era::Conway);
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    state.set_protocol_params(params);

    let donation = 500_000u64;
    let consumed = 1_000_000 + donation; // output + donation (fee = 0)
    let input = ShelleyTxIn {
        transaction_id: [0xDD; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(consumed),
            datum_option: None,
            script_ref: None,
        }),
    );

    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
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
        treasury_donation: Some(donation),
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };

    assert_eq!(state.utxos_donation(), 0);

    let submitted =
        MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("Conway submitted tx with donation should succeed");

    assert_eq!(
        state.utxos_donation(),
        donation,
        "treasury donation should accumulate in utxos_donation",
    );
}
