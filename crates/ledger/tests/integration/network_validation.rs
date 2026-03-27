//! Integration tests for network address validation wired into
//! `apply_block()` and `apply_submitted_tx()` pipelines.
//!
//! Covers the upstream Shelley UTXO rules:
//! - `WrongNetwork` — output address network ID must match expected
//! - `WrongNetworkWithdrawal` — withdrawal account network ID must match
//! - `WrongNetworkInTxBody` — Alonzo+ tx body network_id must match
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.Utxo`,
//!            `Cardano.Ledger.Alonzo.Rules.Utxo`.

use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a 29-byte enterprise key-hash address for the given network.
fn enterprise_addr(network: u8, keyhash: &[u8; 28]) -> Vec<u8> {
    // Enterprise keyhash: type nibble 0x6, network in lower nibble
    let mut addr = vec![0x60 | (network & 0x0f)];
    addr.extend_from_slice(keyhash);
    addr
}

/// Empty witness set for submitted-tx construction.
fn empty_witness_set() -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

fn make_shelley_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

fn make_alonzo_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

fn make_babbage_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

fn mainnet_params() -> ProtocolParameters {
    let mut p = ProtocolParameters::default();
    p.min_fee_a = 0;
    p.min_fee_b = 0;
    p
}

// ===========================================================================
// WrongNetwork — output address tests
// ===========================================================================

#[test]
fn shelley_block_rejects_output_to_wrong_network() {
    // Mainnet ledger (expected_network_id = 1), testnet output (network=0)
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);
    let testnet_addr = enterprise_addr(0, &keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: mainnet_addr, amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: testnet_addr, amount: 5_000_000 }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_shelley_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::WrongNetwork { expected: 1, found: 0 })),
        "expected WrongNetwork error, got: {:?}",
        result,
    );
}

#[test]
fn shelley_block_accepts_output_to_correct_network() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: mainnet_addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: mainnet_addr, amount: 5_000_000 }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_shelley_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

#[test]
fn network_validation_skipped_when_expected_network_not_set() {
    // Without expected_network_id, testnet outputs should pass on any ledger
    let keyhash = [0xAA; 28];
    let testnet_addr = enterprise_addr(0, &keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    // Don't set expected_network_id — validation should be skipped
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: testnet_addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: testnet_addr, amount: 5_000_000 }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_shelley_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "expected Ok when network not set, got: {:?}", result);
}

// ===========================================================================
// WrongNetworkWithdrawal — withdrawal account tests
// ===========================================================================

#[test]
fn shelley_block_rejects_withdrawal_from_wrong_network() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: mainnet_addr.clone(), amount: 5_000_000 },
    );

    // Withdrawal from testnet reward account (network=0) on mainnet ledger
    let wrong_network_acct = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0xBB; 28]),
    };

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: mainnet_addr, amount: 5_000_000 }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: Some(std::collections::BTreeMap::from([
            (wrong_network_acct, 0),
        ])),
        update: None,
        auxiliary_data_hash: None,
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_shelley_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::WrongNetworkWithdrawal { expected: 1, found: 0 })),
        "expected WrongNetworkWithdrawal error, got: {:?}",
        result,
    );
}

// ===========================================================================
// WrongNetworkInTxBody — Alonzo+ network_id field tests
// ===========================================================================

#[test]
fn alonzo_block_rejects_wrong_network_in_tx_body() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Alonzo(yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let tx_body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: Some(0), // WRONG: testnet on a mainnet ledger
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_alonzo_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::WrongNetworkInTxBody { expected: 1, found: 0 })),
        "expected WrongNetworkInTxBody error, got: {:?}",
        result,
    );
}

#[test]
fn alonzo_block_accepts_correct_network_in_tx_body() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Alonzo(yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let tx_body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: Some(1), // correct
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_alonzo_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

#[test]
fn alonzo_block_accepts_absent_network_in_tx_body() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Alonzo(yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let tx_body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None, // absent — always OK per upstream
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_alonzo_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// ===========================================================================
// Babbage-era network validation
// ===========================================================================

#[test]
fn babbage_block_rejects_output_to_wrong_network() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);
    let testnet_addr = enterprise_addr(0, &keyhash);

    let mut state = LedgerState::new(Era::Babbage);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let tx_body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: testnet_addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: Some(1),
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };
    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_babbage_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::WrongNetwork { expected: 1, found: 0 })),
        "expected WrongNetwork error in Babbage, got: {:?}",
        result,
    );
}

// ===========================================================================
// apply_submitted_tx — network validation on mempool admission path
// ===========================================================================

#[test]
fn submitted_shelley_tx_rejects_wrong_network_output() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);
    let testnet_addr = enterprise_addr(0, &keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: mainnet_addr, amount: 5_000_000 },
    );

    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: testnet_addr, amount: 5_000_000 }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Shelley(yggdrasil_ledger::ShelleyTx {
        body,
        witness_set: ws,
        auxiliary_data: None,
    });

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::WrongNetwork { expected: 1, found: 0 })),
        "expected WrongNetwork on submitted Shelley tx, got: {:?}",
        result,
    );
}

#[test]
fn submitted_alonzo_tx_rejects_wrong_network_in_tx_body() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Alonzo(yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![yggdrasil_ledger::AlonzoTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: Some(0), // WRONG
    };
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Alonzo(
        yggdrasil_ledger::AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::WrongNetworkInTxBody { expected: 1, found: 0 })),
        "expected WrongNetworkInTxBody on submitted Alonzo tx, got: {:?}",
        result,
    );
}

#[test]
fn submitted_conway_tx_rejects_wrong_withdrawal_network() {
    use yggdrasil_ledger::ConwayTxBody;

    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_expected_network_id(1);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let wrong_network_acct = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0xBB; 28]),
    };

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: Some(std::collections::BTreeMap::from([
            (wrong_network_acct, 0),
        ])),
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: Some(1),
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        yggdrasil_ledger::AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::WrongNetworkWithdrawal { expected: 1, found: 0 })),
        "expected WrongNetworkWithdrawal on submitted Conway tx, got: {:?}",
        result,
    );
}

#[test]
fn submitted_babbage_tx_rejects_missing_reference_input() {
    let keyhash = [0xAA; 28];
    let mainnet_addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: mainnet_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: mainnet_addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
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
        reference_inputs: Some(vec![
            ShelleyTxIn { transaction_id: [0xFF; 32], index: 99 }, // not in UTxO
        ]),
    };
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Babbage(
        yggdrasil_ledger::AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::ReferenceInputNotInUtxo)),
        "expected ReferenceInputNotInUtxo on submitted Babbage tx, got: {:?}",
        result,
    );
}
