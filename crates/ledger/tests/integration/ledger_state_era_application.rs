use super::*;

fn make_allegra_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<AllegraTxBody>) -> Block {
    let tx_list: Vec<yggdrasil_ledger::Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            yggdrasil_ledger::Tx {
                id: TxId(id_hash.0),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
            }
        })
        .collect();

    Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: tx_list,
        raw_cbor: None,
    }
}

fn make_mary_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<MaryTxBody>) -> Block {
    let tx_list: Vec<yggdrasil_ledger::Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            yggdrasil_ledger::Tx {
                id: TxId(id_hash.0),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
            }
        })
        .collect();

    Block {
        era: Era::Mary,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: tx_list,
        raw_cbor: None,
    }
}

#[test]
fn ledger_state_applies_allegra_block() {
    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: vec![0x02],
            amount: 4_800_000,
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let block = make_allegra_block(500, 1, 0xAB, vec![tx_body]);
    state.apply_block(&block).expect("allegra block");
    assert_eq!(state.multi_era_utxo().len(), 1);
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(500), HeaderHash([0xAB; 32])));
}

#[test]
fn ledger_state_applies_mary_block_with_mint() {
    use std::collections::BTreeMap;

    let mut state = LedgerState::new(Era::Mary);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let policy = [0xAA; 28];
    let asset_name = b"Coin".to_vec();

    let mut output_assets = BTreeMap::new();
    output_assets.insert(asset_name.clone(), 50u64);
    let mut output_ma = BTreeMap::new();
    output_ma.insert(policy, output_assets);

    let mut mint_assets: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    mint_assets.insert(asset_name.clone(), 50);
    let mut mint: BTreeMap<[u8; 28], BTreeMap<Vec<u8>, i64>> = BTreeMap::new();
    mint.insert(policy, mint_assets);

    let tx_body = MaryTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![MaryTxOut {
            address: vec![0x02],
            amount: Value::CoinAndAssets(9_800_000, output_ma),
        }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
    };

    let block = make_mary_block(500, 1, 0xCD, vec![tx_body]);
    state.apply_block(&block).expect("mary block with mint");
    assert_eq!(state.multi_era_utxo().len(), 1);
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(500), HeaderHash([0xCD; 32])));
}

#[test]
fn ledger_state_empty_allegra_block_advances_tip() {
    let mut state = LedgerState::new(Era::Allegra);

    let block = Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([0xFF; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![],
        raw_cbor: None,
    };

    state.apply_block(&block).expect("empty allegra block");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(42), HeaderHash([0xFF; 32])));
}

#[test]
fn ledger_state_snapshot_exposes_tip_and_era() {
    let mut state = LedgerState::new(Era::Babbage);
    state.tip = Point::BlockPoint(SlotNo(77), HeaderHash([0xAB; 32]));

    let snapshot = state.snapshot();
    assert_eq!(snapshot.current_era(), Era::Babbage);
    assert_eq!(snapshot.tip(), &Point::BlockPoint(SlotNo(77), HeaderHash([0xAB; 32])));
}

#[test]
fn ledger_state_accepts_byron_block_as_tip_only_transition() {
    let mut state = LedgerState::new(Era::Byron);
    let original_snapshot = state.snapshot();

    let block = Block {
        era: Era::Byron,
        header: BlockHeader {
            hash: HeaderHash([0xFF; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(1),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![],
        raw_cbor: None,
    };

    state
        .apply_block(&block)
        .expect("byron should advance the tip without failing");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(1), HeaderHash([0xFF; 32])));
    assert_eq!(state.snapshot().query_balance(&Address::Byron(vec![0x01])), original_snapshot.query_balance(&Address::Byron(vec![0x01])));
}

#[test]
fn pending_shelley_genesis_stake_activates_on_first_shelley_block() {
    let mut state = LedgerState::new(Era::Byron);
    let credential = yggdrasil_ledger::StakeCredential::AddrKeyHash([0x44; 28]);
    let pool = [0x55; 28];
    state.configure_pending_shelley_genesis_stake(vec![(credential, pool)]);

    let byron_block = Block {
        era: Era::Byron,
        header: BlockHeader {
            hash: HeaderHash([0x10; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(1),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![],
        raw_cbor: None,
    };
    state.apply_block(&byron_block).expect("byron block");
    assert!(state.stake_credential_state(&credential).is_none());

    let shelley_block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([0x20; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(2),
            block_no: BlockNo(2),
            issuer_vkey: [0; 32],
        },
        transactions: vec![],
        raw_cbor: None,
    };
    state.apply_block(&shelley_block).expect("shelley block");

    let registered = state
        .stake_credential_state(&credential)
        .expect("genesis stake credential should activate");
    assert_eq!(registered.delegated_pool(), Some(pool));
}

// ---------------------------------------------------------------------------
// Byron UTxO transition tests
// ---------------------------------------------------------------------------

/// Helper: build a Byron block from a list of `ByronTx` values.
fn make_byron_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<ByronTx>) -> Block {
    let tx_list: Vec<yggdrasil_ledger::Tx> = txs
        .iter()
        .map(|tx| {
            let raw = tx.to_cbor_bytes();
            yggdrasil_ledger::Tx {
                id: compute_tx_id(&raw),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
            }
        })
        .collect();

    Block {
        era: Era::Byron,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0; 32],
        },
        transactions: tx_list,
        raw_cbor: None,
    }
}

#[test]
fn byron_block_applies_utxo_transitions() {
    let mut state = LedgerState::new(Era::Byron);

    // Seed the UTxO set with a genesis output that a Byron tx can spend.
    let genesis_txid = [0xAA; 32];
    let genesis_txin = ShelleyTxIn { transaction_id: genesis_txid, index: 0 };
    let genesis_txout = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: vec![0x82, 0x00, 0x01],  // opaque Byron address bytes
        amount: 1_000_000,
    });
    state.multi_era_utxo_mut().insert(genesis_txin.clone(), genesis_txout);
    assert_eq!(state.multi_era_utxo().len(), 1);

    // Build a Byron transaction that spends the genesis output and produces two new outputs.
    let byron_tx = ByronTx {
        inputs: vec![ByronTxIn { txid: genesis_txid, index: 0 }],
        outputs: vec![
            ByronTxOut { address: vec![0x82, 0x00, 0x02], amount: 600_000 },
            ByronTxOut { address: vec![0x82, 0x00, 0x03], amount: 300_000 },
        ],
        attributes: vec![0xA0],  // empty CBOR map
    };

    let block = make_byron_block(1, 1, 0x01, vec![byron_tx.clone()]);
    state.apply_block(&block).expect("byron block should apply");

    // The genesis output should be consumed; two new outputs should exist.
    assert_eq!(state.multi_era_utxo().len(), 2);
    assert!(state.multi_era_utxo().get(&genesis_txin).is_none());

    // Check produced outputs are keyed by the Byron tx id.
    let tx_id = byron_tx.tx_id();
    let out0 = state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: tx_id, index: 0 });
    assert!(out0.is_some());
    assert_eq!(out0.unwrap().coin(), 600_000);

    let out1 = state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: tx_id, index: 1 });
    assert!(out1.is_some());
    assert_eq!(out1.unwrap().coin(), 300_000);

    // Tip should be updated.
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(1), HeaderHash([0x01; 32])));
}

#[test]
fn byron_block_rejects_missing_input() {
    let mut state = LedgerState::new(Era::Byron);

    // Build a Byron tx that references a non-existent input.
    let byron_tx = ByronTx {
        inputs: vec![ByronTxIn { txid: [0xBB; 32], index: 0 }],
        outputs: vec![
            ByronTxOut { address: vec![0x82, 0x00, 0x01], amount: 100 },
        ],
        attributes: vec![0xA0],
    };

    let block = make_byron_block(1, 1, 0x02, vec![byron_tx]);
    let result = state.apply_block(&block);
    assert!(result.is_err());
    match result.unwrap_err() {
        LedgerError::InputNotInUtxo => {}
        other => panic!("expected InputNotInUtxo, got {:?}", other),
    }
}

#[test]
fn byron_block_rejects_negative_fee() {
    let mut state = LedgerState::new(Era::Byron);

    // Seed with 100 lovelace.
    let txid = [0xCC; 32];
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: txid, index: 0 },
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0x82, 0x00, 0x01],
            amount: 100,
        }),
    );

    // Try to produce more than consumed (fee would be negative).
    let byron_tx = ByronTx {
        inputs: vec![ByronTxIn { txid, index: 0 }],
        outputs: vec![
            ByronTxOut { address: vec![0x82, 0x00, 0x02], amount: 200 },
        ],
        attributes: vec![0xA0],
    };

    let block = make_byron_block(1, 1, 0x03, vec![byron_tx]);
    let result = state.apply_block(&block);
    assert!(result.is_err());
    match result.unwrap_err() {
        LedgerError::ValueNotPreserved { consumed, produced, .. } => {
            assert_eq!(consumed, 100);
            assert_eq!(produced, 200);
        }
        other => panic!("expected ValueNotPreserved, got {:?}", other),
    }
}

#[test]
fn byron_block_multiple_txs_applied_atomically() {
    let mut state = LedgerState::new(Era::Byron);

    // Seed with two outputs.
    let txid_a = [0xDD; 32];
    let txid_b = [0xEE; 32];
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: txid_a, index: 0 },
        MultiEraTxOut::Shelley(ShelleyTxOut { address: vec![0x01], amount: 500 }),
    );
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: txid_b, index: 0 },
        MultiEraTxOut::Shelley(ShelleyTxOut { address: vec![0x02], amount: 500 }),
    );

    // First tx spends txid_a, second tx references non-existent input.
    let tx1 = ByronTx {
        inputs: vec![ByronTxIn { txid: txid_a, index: 0 }],
        outputs: vec![ByronTxOut { address: vec![0x03], amount: 400 }],
        attributes: vec![0xA0],
    };
    let tx2 = ByronTx {
        inputs: vec![ByronTxIn { txid: [0xFF; 32], index: 0 }],
        outputs: vec![ByronTxOut { address: vec![0x04], amount: 100 }],
        attributes: vec![0xA0],
    };

    let block = make_byron_block(2, 2, 0x04, vec![tx1, tx2]);
    let result = state.apply_block(&block);
    assert!(result.is_err());

    // Atomicity: both original outputs should still be present.
    assert_eq!(state.multi_era_utxo().len(), 2);
    assert!(state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: txid_a, index: 0 }).is_some());
    assert!(state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: txid_b, index: 0 }).is_some());
}

#[test]
fn byron_block_chain_spending() {
    let mut state = LedgerState::new(Era::Byron);

    // Seed with a genesis output.
    let genesis_txid = [0x11; 32];
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: genesis_txid, index: 0 },
        MultiEraTxOut::Shelley(ShelleyTxOut { address: vec![0x01], amount: 10_000 }),
    );

    // Block 1: spend genesis.
    let tx1 = ByronTx {
        inputs: vec![ByronTxIn { txid: genesis_txid, index: 0 }],
        outputs: vec![
            ByronTxOut { address: vec![0x02], amount: 8_000 },
            ByronTxOut { address: vec![0x03], amount: 1_000 },
        ],
        attributes: vec![0xA0],
    };
    let block1 = make_byron_block(1, 1, 0x10, vec![tx1.clone()]);
    state.apply_block(&block1).expect("block 1");

    // Block 2: spend output from block 1.
    let tx1_id = tx1.tx_id();
    let tx2 = ByronTx {
        inputs: vec![ByronTxIn { txid: tx1_id, index: 0 }],
        outputs: vec![
            ByronTxOut { address: vec![0x04], amount: 7_000 },
        ],
        attributes: vec![0xA0],
    };
    let block2 = make_byron_block(2, 2, 0x20, vec![tx2.clone()]);
    state.apply_block(&block2).expect("block 2");

    // Should have 2 outputs: tx1 output[1] + tx2 output[0].
    assert_eq!(state.multi_era_utxo().len(), 2);
    let tx2_id = tx2.tx_id();
    assert_eq!(state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: tx2_id, index: 0 }).unwrap().coin(), 7_000);
    assert_eq!(state.multi_era_utxo().get(&ShelleyTxIn { transaction_id: tx1_id, index: 1 }).unwrap().coin(), 1_000);
}