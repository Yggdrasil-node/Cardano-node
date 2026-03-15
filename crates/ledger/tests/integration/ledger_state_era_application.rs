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
    };

    state
        .apply_block(&block)
        .expect("byron should advance the tip without failing");
    assert_eq!(state.tip, Point::BlockPoint(SlotNo(1), HeaderHash([0xFF; 32])));
    assert_eq!(state.snapshot().query_balance(&Address::Byron(vec![0x01])), original_snapshot.query_balance(&Address::Byron(vec![0x01])));
}