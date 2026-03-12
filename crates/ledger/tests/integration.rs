use yggdrasil_ledger::{Block, Era, LedgerState};

#[test]
fn applies_block_for_matching_era() {
    let mut state = LedgerState::new(Era::Shelley);
    let block = Block {
        era: Era::Shelley,
        slot_no: 42,
        transactions: Vec::new(),
    };

    state
        .apply_block(&block)
        .expect("matching era block should apply to ledger state");
    assert_eq!(state.tip_slot, 42);
}
