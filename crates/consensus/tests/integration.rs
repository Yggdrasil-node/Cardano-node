use yggdrasil_consensus::{ActiveSlotCoeff, ChainCandidate, leadership_threshold, select_preferred};

#[test]
fn prefers_longer_chain_candidate() {
    let left = ChainCandidate {
        block_no: 4,
        slot_no: 10,
    };
    let right = ChainCandidate {
        block_no: 5,
        slot_no: 9,
    };

    assert_eq!(select_preferred(left, right), right);
}

#[test]
fn computes_nonzero_threshold() {
    let threshold = leadership_threshold(ActiveSlotCoeff(0.05), 0.7)
        .expect("active slot coefficient within bounds should compute a threshold");
    assert!(threshold > 0.0);
}
