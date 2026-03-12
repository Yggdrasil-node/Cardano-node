use crate::eras::Era;

/// A minimal transaction wrapper used by the foundation slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tx {
    pub id: String,
}

/// A minimal block wrapper that carries era, slot, and transactions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub era: Era,
    pub slot_no: u64,
    pub transactions: Vec<Tx>,
}
