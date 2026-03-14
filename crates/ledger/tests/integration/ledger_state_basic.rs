use super::*;

pub(super) fn make_shelley_block_with_txs(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<ShelleyTxBody>,
) -> Block {
    let tx_list: Vec<yggdrasil_ledger::Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            yggdrasil_ledger::Tx {
                id: TxId(id_hash.0),
                body: raw,
            }
        })
        .collect();

    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: tx_list,
    }
}

#[test]
fn ledger_state_applies_block_with_utxo_transition() {
    let mut state = LedgerState::new(Era::Shelley);

    // Seed the UTxO with an initial entry.
    let genesis_txin = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    let genesis_txout = ShelleyTxOut {
        address: vec![0x01],
        amount: 1000,
    };
    state.utxo_mut().insert(genesis_txin, genesis_txout);
    assert_eq!(state.utxo().len(), 1);

    // Build a transaction that spends the genesis output.
    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![
            ShelleyTxOut {
                address: vec![0x02],
                amount: 700,
            },
            ShelleyTxOut {
                address: vec![0x03],
                amount: 200,
            },
        ],
        fee: 100,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block = make_shelley_block_with_txs(500, 1, 0xBB, vec![tx_body]);
    state.apply_block(&block).expect("apply block with tx");

    // UTxO should now have the 2 new outputs, genesis input removed.
    assert_eq!(state.utxo().len(), 2);
    assert!(state.utxo().get(&ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    }).is_none());
    assert_eq!(
        state.tip,
        Point::BlockPoint(SlotNo(500), HeaderHash([0xBB; 32]))
    );
}

#[test]
fn ledger_state_rejects_expired_transaction() {
    let mut state = LedgerState::new(Era::Shelley);

    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xCC; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 500,
        },
    );

    // TTL = 10, but block slot = 50 → expired
    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xCC; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x02],
            amount: 400,
        }],
        fee: 100,
        ttl: 10,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block = make_shelley_block_with_txs(50, 1, 0xDD, vec![tx_body]);
    let err = state
        .apply_block(&block)
        .expect_err("should reject expired tx");
    assert!(
        matches!(err, LedgerError::TxExpired { ttl: 10, slot: 50 }),
        "expected TxExpired, got {err:?}"
    );

    // UTxO should be unchanged (atomicity).
    assert_eq!(state.utxo().len(), 1);
    assert_eq!(state.tip, Point::Origin);
}

#[test]
fn ledger_state_rejects_missing_input() {
    let mut state = LedgerState::new(Era::Shelley);

    // No UTxO entries seeded — input doesn't exist.
    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xFF; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x01],
            amount: 100,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block = make_shelley_block_with_txs(100, 1, 0xEE, vec![tx_body]);
    let err = state
        .apply_block(&block)
        .expect_err("should reject missing input");
    assert!(
        matches!(err, LedgerError::InputNotInUtxo),
        "expected InputNotInUtxo, got {err:?}"
    );
    assert_eq!(state.utxo().len(), 0);
}

#[test]
fn ledger_state_atomicity_on_second_tx_failure() {
    let mut state = LedgerState::new(Era::Shelley);

    // Seed two inputs.
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x10],
            amount: 500,
        },
    );
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x20],
            amount: 300,
        },
    );

    // Tx1 is valid, Tx2 has value mismatch.
    let tx1 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x11],
            amount: 400,
        }],
        fee: 100,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let tx2 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x22],
            amount: 999, // intentional mismatch: consumed=300, produced=999+0 != 300
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block = make_shelley_block_with_txs(500, 1, 0xAA, vec![tx1, tx2]);
    state
        .apply_block(&block)
        .expect_err("second tx should fail");

    // Original UTxO should be unchanged despite tx1 being valid.
    assert_eq!(state.utxo().len(), 2);
    assert!(state.utxo().get(&ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    }).is_some());
    assert_eq!(state.tip, Point::Origin);
}

#[test]
fn ledger_state_empty_block_advances_tip() {
    let mut state = LedgerState::new(Era::Shelley);

    let block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([0xFF; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![],
    };

    state.apply_block(&block).expect("empty block");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(42), HeaderHash([0xFF; 32])));
    assert_eq!(state.utxo().len(), 0);
}

#[test]
fn ledger_state_checkpoint_restores_utxo_and_tip() {
    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_000,
        },
    );

    let checkpoint = state.checkpoint();

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x02],
            amount: 900,
        }],
        fee: 100,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block = make_shelley_block_with_txs(50, 1, 0xBC, vec![tx_body]);
    state.apply_block(&block).expect("apply block after checkpoint");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(50), HeaderHash([0xBC; 32])));
    assert_eq!(state.utxo().len(), 1);

    state.rollback_to_checkpoint(&checkpoint);
    assert_eq!(state.tip, Point::Origin);
    assert_eq!(state.utxo().len(), 1);
    assert!(state
        .utxo()
        .get(&ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        })
        .is_some());
}

#[test]
fn ledger_state_chained_transactions() {
    let mut state = LedgerState::new(Era::Shelley);

    // Seed genesis.
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x00; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1000,
        },
    );

    // Block 1: spend genesis, produce 2 outputs.
    let tx1 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x00; 32],
            index: 0,
        }],
        outputs: vec![
            ShelleyTxOut { address: vec![0x10], amount: 600 },
            ShelleyTxOut { address: vec![0x11], amount: 200 },
        ],
        fee: 200,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block1 = make_shelley_block_with_txs(100, 1, 0xA1, vec![tx1.clone()]);
    state.apply_block(&block1).expect("block 1");
    assert_eq!(state.utxo().len(), 2);

    // Block 2: spend the first output from block 1.
    // We need the real tx_id from block 1.
    let tx1_raw = tx1.to_cbor_bytes();
    let tx1_id = yggdrasil_crypto::hash_bytes_256(&tx1_raw).0;

    let tx2 = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: tx1_id,
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x20],
            amount: 500,
        }],
        fee: 100,
        ttl: 2000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let block2 = make_shelley_block_with_txs(200, 2, 0xA2, vec![tx2]);
    state.apply_block(&block2).expect("block 2");

    // Now: 1 output from tx1 (index 1) + 1 output from tx2 (index 0) = 2
    assert_eq!(state.utxo().len(), 2);
    assert_eq!(
        state.tip,
        Point::BlockPoint(SlotNo(200), HeaderHash([0xA2; 32]))
    );
}

// ---------------------------------------------------------------------------
