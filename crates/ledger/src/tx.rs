use crate::eras::Era;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tx {
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub era: Era,
    pub slot_no: u64,
    pub transactions: Vec<Tx>,
}
