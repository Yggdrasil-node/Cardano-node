//! Integration tests for BBODY max block body size validation.
//!
//! Upstream reference: `Cardano.Ledger.Shelley.Rules.Bbody` —
//! `validateMaxBlockBodySize` should account for full serialized
//! transaction bytes in the block body, not only tx body bytes.

use super::*;

fn make_tx_with_sizes(body_len: usize, witness_len: usize, aux_len: Option<usize>, is_valid: bool) -> Tx {
    let body = vec![0u8; body_len];
    let witnesses = Some(vec![0u8; witness_len]);
    let auxiliary_data = aux_len.map(|len| vec![0u8; len]);
    Tx {
        id: compute_tx_id(&body),
        body,
        witnesses,
        auxiliary_data,
        is_valid: Some(is_valid),
    }
}

fn make_block(slot: u64, block_no: u64, txs: Vec<Tx>) -> Block {
    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([0xAB; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

#[test]
fn block_body_size_counts_witnesses_and_aux_data() {
    let mut state = LedgerState::new(Era::Conway);

    let tx = make_tx_with_sizes(8, 96, Some(32), true);
    let full_size = tx.serialized_size();
    assert!(full_size > tx.body.len());

    let mut params = ProtocolParameters::alonzo_defaults();
    // Choose a limit that old body-only accounting would pass but full-size
    // accounting must reject.
    params.max_block_body_size = (tx.body.len() + 4) as u32;
    state.set_protocol_params(params);

    let block = make_block(10, 1, vec![tx]);
    let err = state.apply_block(&block).expect_err("expected BBODY size rejection");
    match err {
        LedgerError::BlockTooLarge { actual, max } => {
            assert_eq!(actual, full_size);
            assert_eq!(max, block.transactions[0].body.len() + 4);
        }
        other => panic!("expected BlockTooLarge, got: {other}"),
    }
}

#[test]
fn block_body_size_sums_full_serialized_size_across_transactions() {
    let mut state = LedgerState::new(Era::Conway);

    let tx1 = make_tx_with_sizes(6, 40, Some(10), true);
    let tx2 = make_tx_with_sizes(7, 30, None, true);
    let expected_total = tx1.serialized_size() + tx2.serialized_size();
    let old_body_only_total = tx1.body.len() + tx2.body.len();
    assert!(expected_total > old_body_only_total);

    let mut params = ProtocolParameters::alonzo_defaults();
    // Above old body-only total, below full serialized total.
    params.max_block_body_size = (old_body_only_total + 1) as u32;
    state.set_protocol_params(params);

    let block = make_block(11, 2, vec![tx1, tx2]);
    let err = state.apply_block(&block).expect_err("expected BBODY size rejection");
    match err {
        LedgerError::BlockTooLarge { actual, max } => {
            assert_eq!(actual, expected_total);
            assert_eq!(max, old_body_only_total + 1);
        }
        other => panic!("expected BlockTooLarge, got: {other}"),
    }
}
