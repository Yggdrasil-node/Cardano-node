use crate::eras::Era;
use crate::types::{BlockNo, HeaderHash, SlotNo, TxId};

/// A transaction identified by its body hash.
///
/// The `body` field holds the transaction's opaque serialized payload until
/// typed CBOR codec work lands. The `id` is the Blake2b-256 hash of that
/// payload.
///
/// Reference: `Cardano.Ledger.Core` — `Tx` / `TxId`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tx {
    /// Blake2b-256 hash of the serialized transaction body.
    pub id: TxId,
    /// Opaque serialized transaction body (to be replaced by typed payload).
    pub body: Vec<u8>,
}

/// A block header containing the essential chain-indexing fields.
///
/// Reference: upstream `HeaderBody` in `cardano-ledger`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    /// Hash of this header (Blake2b-256).
    pub hash: HeaderHash,
    /// Hash of the previous block header (`[0u8; 32]` for genesis successor).
    pub prev_hash: HeaderHash,
    /// Slot in which this block was issued.
    pub slot_no: SlotNo,
    /// Block height.
    pub block_no: BlockNo,
    /// Issuer verification key (opaque bytes, 32-byte Ed25519 vkey).
    pub issuer_vkey: [u8; 32],
}

/// A block carrying its header and a body of transactions.
///
/// Reference: `Ouroboros.Consensus.Block.Abstract` — `Block`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// The era this block belongs to.
    pub era: Era,
    /// Block header with chain-indexing fields.
    pub header: BlockHeader,
    /// Transactions included in this block.
    pub transactions: Vec<Tx>,
}
