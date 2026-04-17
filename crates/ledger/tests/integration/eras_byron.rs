use super::*;

/// Build a synthetic Byron EBB as CBOR bytes.
fn build_byron_ebb(epoch: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(prev_hash);
    enc.bytes(&[0xAA; 32]);
    enc.array(2);
    enc.unsigned(epoch);
    enc.array(1).unsigned(0);
    enc.array(0);
    enc.array(0);
    enc.array(0);
    enc.into_bytes()
}

/// Build a synthetic Byron main block as CBOR bytes.
fn build_byron_main(epoch: u64, slot_in_epoch: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    // Outer: [header, body, extra]
    enc.array(3);
    // Header: [protocol_magic, prev_hash, body_proof, consensus_data, extra_data]
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(prev_hash);
    enc.bytes(&[0xBB; 32]);
    // consensus_data: [slot_id, pubkey, difficulty, signature]
    enc.array(4);
    enc.array(2);
    enc.unsigned(epoch);
    enc.unsigned(slot_in_epoch);
    enc.bytes(&[0xCC; 64]);
    enc.array(1).unsigned(1);
    enc.bytes(&[0xDD; 64]);
    // extra_data
    enc.array(0);
    // Body: [tx_payload, ssc_payload, dlg_payload, upd_payload]
    enc.array(4);
    // tx_payload: [[TxAux...]] — 1-element wrapper around empty list
    enc.array(1);
    enc.array(0);
    // ssc_payload, dlg_payload, upd_payload
    enc.array(0);
    enc.array(0);
    enc.array(0);
    // extra (block-level)
    enc.array(0);
    enc.into_bytes()
}

#[test]
fn byron_ebb_decode() {
    let prev_hash = [0x11; 32];
    let raw = build_byron_ebb(5, &prev_hash);
    let block = ByronBlock::decode_ebb(&raw).expect("decode EBB");
    assert_eq!(block.epoch(), 5);
    assert_eq!(*block.prev_hash(), prev_hash);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 5 * 21600);
}

#[test]
fn byron_main_block_decode() {
    let prev_hash = [0x22; 32];
    let raw = build_byron_main(10, 500, &prev_hash);
    let block = ByronBlock::decode_main(&raw).expect("decode main block");
    assert_eq!(block.epoch(), 10);
    assert_eq!(*block.prev_hash(), prev_hash);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 10 * 21600 + 500);
}

#[test]
fn byron_ebb_epoch_zero() {
    let raw = build_byron_ebb(0, &[0x00; 32]);
    let block = ByronBlock::decode_ebb(&raw).expect("decode EBB epoch 0");
    assert_eq!(block.epoch(), 0);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 0);
}

#[test]
fn byron_main_block_first_slot() {
    let raw = build_byron_main(3, 0, &[0x33; 32]);
    let block = ByronBlock::decode_main(&raw).expect("decode first slot");
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 3 * 21600);
}

#[test]
fn byron_main_block_last_slot() {
    let raw = build_byron_main(7, 21599, &[0x44; 32]);
    let block = ByronBlock::decode_main(&raw).expect("decode last slot");
    assert_eq!(
        block.absolute_slot(BYRON_SLOTS_PER_EPOCH),
        7 * 21600 + 21599
    );
}

#[test]
fn byron_block_variant_accessors() {
    let ebb = ByronBlock::EpochBoundary {
        epoch: 2,
        prev_hash: [0x55; 32],
        chain_difficulty: 0,
        raw_header: vec![],
    };
    assert_eq!(ebb.epoch(), 2);
    assert_eq!(*ebb.prev_hash(), [0x55; 32]);

    let main = ByronBlock::MainBlock {
        epoch: 3,
        slot_in_epoch: 100,
        prev_hash: [0x66; 32],
        chain_difficulty: 1,
        issuer_vkey: [0u8; 32],
        raw_header: vec![],
        transactions: vec![],
    };
    assert_eq!(main.epoch(), 3);
    assert_eq!(*main.prev_hash(), [0x66; 32]);
}

// ---------------------------------------------------------------------------
// Byron transaction type round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn byron_txin_cbor_round_trip() {
    let txin = ByronTxIn {
        txid: [0xAA; 32],
        index: 7,
    };
    let encoded = txin.to_cbor_bytes();
    let decoded = ByronTxIn::from_cbor_bytes(&encoded).expect("decode ByronTxIn");
    assert_eq!(txin, decoded);
}

#[test]
fn byron_txout_cbor_round_trip() {
    // Build a simple Byron-style address: tag 24 wrapping opaque bytes.
    let mut addr_enc = Encoder::new();
    addr_enc.tag(24).bytes(&[0x01, 0x02, 0x03]);
    let address = addr_enc.into_bytes();

    let txout = ByronTxOut {
        address,
        amount: 1_000_000,
    };
    let encoded = txout.to_cbor_bytes();
    let decoded = ByronTxOut::from_cbor_bytes(&encoded).expect("decode ByronTxOut");
    assert_eq!(txout, decoded);
}

#[test]
fn byron_tx_cbor_round_trip() {
    let tx = ByronTx {
        inputs: vec![
            ByronTxIn {
                txid: [0x11; 32],
                index: 0,
            },
            ByronTxIn {
                txid: [0x22; 32],
                index: 1,
            },
        ],
        outputs: vec![ByronTxOut {
            address: vec![0xD8, 0x18, 0x43, 0x01, 0x02, 0x03], // tag 24 + 3 bytes
            amount: 500_000,
        }],
        attributes: {
            let mut enc = Encoder::new();
            enc.map(0);
            enc.into_bytes()
        },
    };
    let encoded = tx.to_cbor_bytes();
    let decoded = ByronTx::from_cbor_bytes(&encoded).expect("decode ByronTx");
    assert_eq!(tx, decoded);
}

#[test]
fn byron_tx_id_deterministic() {
    let tx = ByronTx {
        inputs: vec![ByronTxIn {
            txid: [0xFF; 32],
            index: 0,
        }],
        outputs: vec![ByronTxOut {
            address: vec![0xD8, 0x18, 0x43, 0x01, 0x02, 0x03],
            amount: 42,
        }],
        attributes: {
            let mut enc = Encoder::new();
            enc.map(0);
            enc.into_bytes()
        },
    };

    let id1 = tx.tx_id();
    let id2 = tx.tx_id();
    assert_eq!(id1, id2);
    // Not all zeros — actually hashed
    assert_ne!(id1, [0u8; 32]);
}

#[test]
fn byron_tx_witness_cbor_round_trip() {
    // PkWitness: type 0, payload is CBOR-encoded [pubkey, signature]
    let mut inner_enc = Encoder::new();
    inner_enc.array(2).bytes(&[0xAA; 64]).bytes(&[0xBB; 64]);
    let payload = inner_enc.into_bytes();

    let witness = ByronTxWitness {
        witness_type: 0,
        payload,
    };
    let encoded = witness.to_cbor_bytes();
    let decoded = ByronTxWitness::from_cbor_bytes(&encoded).expect("decode ByronTxWitness");
    assert_eq!(witness, decoded);
}

#[test]
fn byron_tx_aux_cbor_round_trip() {
    let tx_aux = ByronTxAux {
        tx: ByronTx {
            inputs: vec![ByronTxIn {
                txid: [0x33; 32],
                index: 2,
            }],
            outputs: vec![ByronTxOut {
                address: vec![0xD8, 0x18, 0x43, 0x04, 0x05, 0x06],
                amount: 999_999,
            }],
            attributes: {
                let mut enc = Encoder::new();
                enc.map(0);
                enc.into_bytes()
            },
        },
        witnesses: vec![ByronTxWitness {
            witness_type: 0,
            payload: vec![0x82, 0x40, 0x40], // [bytes"", bytes""]
        }],
    };
    let encoded = tx_aux.to_cbor_bytes();
    let decoded = ByronTxAux::from_cbor_bytes(&encoded).expect("decode ByronTxAux");
    assert_eq!(tx_aux, decoded);
}

/// Build a Byron main block with transactions and verify they decode.
#[test]
fn byron_main_block_with_transactions() {
    // Build a ByronTxAux as CBOR
    let mut tx_enc = Encoder::new();
    // TxAux: [Tx, [witnesses]]
    tx_enc.array(2);
    // Tx: [inputs, outputs, attributes]
    tx_enc.array(3);
    // inputs: [TxIn]
    tx_enc.array(1);
    // TxIn: [0, #6.24(bytes .cbor [txid, index])]
    tx_enc.array(2).unsigned(0).tag(24);
    let mut inner_txin = Encoder::new();
    inner_txin.array(2).bytes(&[0x77; 32]).unsigned(0);
    tx_enc.bytes(&inner_txin.into_bytes());
    // outputs: [TxOut]
    tx_enc.array(1);
    // TxOut: [address, coin]
    tx_enc.array(2);
    tx_enc.tag(24).bytes(&[0x01, 0x02, 0x03]);
    tx_enc.unsigned(2_000_000);
    // attributes: {}
    tx_enc.map(0);
    // witnesses: [PkWitness]
    tx_enc.array(1);
    tx_enc
        .array(2)
        .unsigned(0)
        .tag(24)
        .bytes(&[0x82, 0x40, 0x40]);
    let tx_aux_bytes = tx_enc.into_bytes();

    // Build main block with tx_payload containing 1 TxAux
    let mut enc = Encoder::new();
    // Outer: [header, body, extra]
    enc.array(3);
    // Header
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(&[0x00; 32]);
    enc.bytes(&[0xBB; 32]);
    enc.array(4);
    enc.array(2).unsigned(1).unsigned(5);
    enc.bytes(&[0xCC; 64]);
    enc.array(1).unsigned(42);
    enc.bytes(&[0xDD; 64]);
    enc.array(0);
    // Body: [tx_payload, ssc, dlg, upd]
    enc.array(4);
    // tx_payload: [[TxAux]]
    enc.array(1);
    enc.array(1);
    enc.raw(&tx_aux_bytes);
    enc.array(0); // ssc
    enc.array(0); // dlg
    enc.array(0); // upd
    // extra
    enc.array(0);

    let raw = enc.into_bytes();
    let block = ByronBlock::decode_main(&raw).expect("decode main block with txs");
    let txs = block.transactions();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0].tx.inputs.len(), 1);
    assert_eq!(txs[0].tx.inputs[0].txid, [0x77; 32]);
    assert_eq!(txs[0].tx.inputs[0].index, 0);
    assert_eq!(txs[0].tx.outputs.len(), 1);
    assert_eq!(txs[0].tx.outputs[0].amount, 2_000_000);
    assert_eq!(txs[0].witnesses.len(), 1);
    assert_eq!(txs[0].witnesses[0].witness_type, 0);
}

#[test]
fn byron_main_block_empty_transactions() {
    let raw = build_byron_main(0, 0, &[0x00; 32]);
    let block = ByronBlock::decode_main(&raw).expect("decode empty tx block");
    assert!(block.transactions().is_empty());
}

#[test]
fn byron_ebb_has_no_transactions() {
    let raw = build_byron_ebb(5, &[0x11; 32]);
    let block = ByronBlock::decode_ebb(&raw).expect("decode EBB");
    assert!(block.transactions().is_empty());
}
