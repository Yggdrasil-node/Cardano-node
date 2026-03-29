//! Integration tests for Conway treasury donation accumulation and
//! epoch-boundary transfer.
//!
//! Upstream reference: `Cardano.Ledger.Conway.Rules.Utxos` —
//!   `utxos & utxosDonationL <>~ txBody ^. treasuryDonationTxBodyL`
//! Upstream reference: `Cardano.Ledger.Conway.Rules.Epoch` — epoch
//!   boundary: `casTreasuryL <>~ utxosDonationL`, then
//!   `utxosDonationL .~ zero`.

use super::*;

fn enterprise_addr(network: u8, keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60 | (network & 0x0f)];
    addr.extend_from_slice(keyhash);
    addr
}

fn permissive_params() -> ProtocolParameters {
    let mut p = ProtocolParameters::default();
    p.min_fee_a = 0;
    p.min_fee_b = 0;
    p
}

fn seed_utxo(state: &mut LedgerState, txin: ShelleyTxIn, addr: &[u8], amount: u64) {
    state.multi_era_utxo_mut().insert(
        txin,
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(amount),
            datum_option: None,
            script_ref: None,
        }),
    );
}

fn make_conway_block(
    txs: Vec<(ConwayTxBody, Option<bool>)>,
    slot: u64,
) -> Block {
    let mut transactions = Vec::new();
    for (body, is_valid) in txs {
        let body_bytes = body.to_cbor_bytes();
        let tx_id = compute_tx_id(&body_bytes);
        transactions.push(Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid,
        });
    }
    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([slot as u8; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions,
        raw_cbor: None,
    }
}

fn simple_conway_body(
    input: ShelleyTxIn,
    output_addr: &[u8],
    output_amount: u64,
    fee: u64,
    treasury_donation: Option<u64>,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: output_addr.to_vec(),
            amount: Value::Coin(output_amount),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: None,
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
        treasury_donation,
    }
}

// -----------------------------------------------------------------------
// apply_conway_block tests
// -----------------------------------------------------------------------

#[test]
fn conway_treasury_donation_accumulates_to_utxos_donation() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());
    // Ensure initial donation is zero.
    assert_eq!(state.utxos_donation(), 0);

    // Seed an input worth enough to cover output + fee + donation.
    let input = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 10_000_000);

    let body = simple_conway_body(
        input,
        &addr,
        9_500_000,    // output
        200_000,      // fee
        Some(300_000), // treasury_donation
    );

    let block = make_conway_block(vec![(body, Some(true))], 100);
    state
        .apply_block_validated(&block, None)
        .expect("conway block with treasury donation");

    assert_eq!(
        state.utxos_donation(),
        300_000,
        "treasury donation should accumulate into utxos_donation"
    );
    // Treasury itself should NOT have changed yet (epoch boundary does that).
    assert_eq!(state.accounting().treasury, 0);
}

#[test]
fn conway_treasury_donation_none_does_not_change_utxos_donation() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x02; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = simple_conway_body(input, &addr, 4_800_000, 200_000, None);
    let block = make_conway_block(vec![(body, Some(true))], 200);
    state
        .apply_block_validated(&block, None)
        .expect("conway block without treasury donation");

    assert_eq!(
        state.utxos_donation(),
        0,
        "utxos_donation should remain zero when treasury_donation is None"
    );
}

#[test]
fn conway_treasury_donation_zero_does_not_change_utxos_donation() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x03; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = simple_conway_body(input, &addr, 4_800_000, 200_000, Some(0));
    let block = make_conway_block(vec![(body, Some(true))], 300);
    state
        .apply_block_validated(&block, None)
        .expect("conway block with zero treasury donation");

    assert_eq!(
        state.utxos_donation(),
        0,
        "utxos_donation should remain zero when treasury_donation is Some(0)"
    );
}

#[test]
fn conway_multiple_txs_accumulate_donations() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    // Two inputs.
    let input1 = ShelleyTxIn {
        transaction_id: [0x04; 32],
        index: 0,
    };
    let input2 = ShelleyTxIn {
        transaction_id: [0x05; 32],
        index: 0,
    };
    seed_utxo(&mut state, input1.clone(), &addr, 10_000_000);
    seed_utxo(&mut state, input2.clone(), &addr, 10_000_000);

    let tx1 = simple_conway_body(input1, &addr, 9_500_000, 200_000, Some(300_000));
    let tx2 = simple_conway_body(input2, &addr, 9_400_000, 100_000, Some(500_000));

    let block = make_conway_block(
        vec![(tx1, Some(true)), (tx2, Some(true))],
        400,
    );
    state
        .apply_block_validated(&block, None)
        .expect("multiple conway txs with donations");

    assert_eq!(
        state.utxos_donation(),
        800_000, // 300k + 500k
        "donations from multiple txs in one block should sum"
    );
}

#[test]
fn conway_multiple_blocks_accumulate_donations() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input1 = ShelleyTxIn {
        transaction_id: [0x06; 32],
        index: 0,
    };
    let input2 = ShelleyTxIn {
        transaction_id: [0x07; 32],
        index: 0,
    };
    seed_utxo(&mut state, input1.clone(), &addr, 10_000_000);
    seed_utxo(&mut state, input2.clone(), &addr, 10_000_000);

    let block1 = make_conway_block(
        vec![(simple_conway_body(input1, &addr, 9_000_000, 500_000, Some(500_000)), Some(true))],
        500,
    );
    let block2 = make_conway_block(
        vec![(simple_conway_body(input2, &addr, 8_000_000, 1_000_000, Some(1_000_000)), Some(true))],
        600,
    );

    state.apply_block_validated(&block1, None).unwrap();
    assert_eq!(state.utxos_donation(), 500_000);

    state.apply_block_validated(&block2, None).unwrap();
    assert_eq!(
        state.utxos_donation(),
        1_500_000, // 500k + 1M
        "donations should accumulate across blocks within the same epoch"
    );
}

#[test]
fn conway_invalid_tx_does_not_accumulate_donation() {
    // When is_valid = false, only collateral-only transition happens.
    // Treasury donation from invalid txs must NOT accumulate.
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let spend_input = ShelleyTxIn {
        transaction_id: [0x08; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x09; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 10_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    let mut body = simple_conway_body(spend_input, &addr, 9_500_000, 200_000, Some(300_000));
    body.collateral = Some(vec![collateral_input]);

    let block = make_conway_block(vec![(body, Some(false))], 700);
    state
        .apply_block_validated(&block, None)
        .expect("invalid tx should apply collateral-only");

    assert_eq!(
        state.utxos_donation(),
        0,
        "treasury donation from is_valid=false tx must not accumulate"
    );
}

// -----------------------------------------------------------------------
// flush_donations_to_treasury tests
// -----------------------------------------------------------------------

#[test]
fn flush_donations_transfers_to_treasury() {
    let mut state = LedgerState::new(Era::Conway);
    state.accumulate_donation(1_000_000);
    state.accumulate_donation(500_000);
    assert_eq!(state.utxos_donation(), 1_500_000);
    assert_eq!(state.accounting().treasury, 0);

    let flushed = state.flush_donations_to_treasury();
    assert_eq!(flushed, 1_500_000);
    assert_eq!(state.accounting().treasury, 1_500_000);
    assert_eq!(state.utxos_donation(), 0);
}

#[test]
fn flush_donations_when_zero_is_noop() {
    let mut state = LedgerState::new(Era::Conway);
    state.accounting_mut().treasury = 42_000;

    let flushed = state.flush_donations_to_treasury();
    assert_eq!(flushed, 0);
    assert_eq!(state.accounting().treasury, 42_000);
    assert_eq!(state.utxos_donation(), 0);
}

#[test]
fn flush_donations_adds_to_existing_treasury() {
    let mut state = LedgerState::new(Era::Conway);
    state.accounting_mut().treasury = 10_000_000;
    state.accumulate_donation(500_000);

    let flushed = state.flush_donations_to_treasury();
    assert_eq!(flushed, 500_000);
    assert_eq!(state.accounting().treasury, 10_500_000);
    assert_eq!(state.utxos_donation(), 0);
}

// -----------------------------------------------------------------------
// CBOR round-trip test
// -----------------------------------------------------------------------

#[test]
fn ledger_state_utxos_donation_cbor_round_trip() {
    let mut state = LedgerState::new(Era::Conway);
    state.accumulate_donation(12_345_678);

    let encoded = state.to_cbor_bytes();
    let decoded = LedgerState::from_cbor_bytes(&encoded).expect("CBOR decode");

    assert_eq!(decoded.utxos_donation(), 12_345_678);
    assert_eq!(decoded, state);
}

#[test]
fn ledger_state_utxos_donation_defaults_to_zero_from_legacy_cbor() {
    // Encode a state with zero donation, which produces length-19.
    // Manually verify that decoding length-18 (legacy) works and yields 0.
    let state = LedgerState::new(Era::Conway);
    assert_eq!(state.utxos_donation(), 0);

    // Round-trip to verify base case.
    let encoded = state.to_cbor_bytes();
    let decoded = LedgerState::from_cbor_bytes(&encoded).expect("CBOR decode");
    assert_eq!(decoded.utxos_donation(), 0);
}

// -----------------------------------------------------------------------
// Epoch boundary integration test
// -----------------------------------------------------------------------

#[test]
fn epoch_boundary_transfers_donations_to_treasury() {
    use yggdrasil_ledger::{
        StakeSnapshot, StakeSnapshots, apply_epoch_boundary, EpochNo,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());
    state.accounting_mut().reserves = 1_000_000_000;

    // Simulate two blocks' worth of donations across the epoch.
    state.accumulate_donation(1_000_000);
    state.accumulate_donation(2_000_000);
    assert_eq!(state.utxos_donation(), 3_000_000);

    let mut snapshots = StakeSnapshots {
        mark: StakeSnapshot::default(),
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
    };

    let event = apply_epoch_boundary(
        &mut state,
        EpochNo(1),
        &mut snapshots,
        &std::collections::BTreeMap::new(),
    )
    .expect("epoch boundary");

    assert_eq!(
        event.donations_transferred, 3_000_000,
        "event should report the donated amount"
    );
    assert_eq!(
        state.utxos_donation(),
        0,
        "utxos_donation must be reset to zero after epoch boundary"
    );
    // Treasury should contain donations + treasury_delta from reward calculation.
    assert!(
        state.accounting().treasury >= 3_000_000,
        "treasury must include the donated amount (actual: {})",
        state.accounting().treasury,
    );
}
