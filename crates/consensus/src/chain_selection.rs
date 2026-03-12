#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainCandidate {
    pub block_no: u64,
    pub slot_no: u64,
}

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
