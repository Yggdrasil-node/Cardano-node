/// A minimal chain candidate ordered by block number and slot number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainCandidate {
    pub block_no: u64,
    pub slot_no: u64,
}

/// Selects the preferred chain candidate using the current simplified ordering.
pub fn select_preferred(
    left: ChainCandidate,
    right: ChainCandidate,
) -> ChainCandidate {
    if (right.block_no, right.slot_no) > (left.block_no, left.slot_no) {
        right
    } else {
        left
    }
}
