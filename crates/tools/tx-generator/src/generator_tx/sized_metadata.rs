//! Sized transaction-metadata construction.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/SizedMetadata.hs`.
//! Ports the metadata cost assumptions and `mkMetadata` sizing algorithm
//! consumed by `Cardano.Benchmarking.Script.Core.toMetadata`.

use std::collections::BTreeMap;

use yggdrasil_ledger::Encoder;

use crate::types::AnyCardanoEra;

/// Mirror of upstream `maxMapSize`.
pub const MAX_MAP_SIZE: usize = 1000;

/// Mirror of upstream `maxBSSize`.
pub const MAX_BS_SIZE: usize = 64;

const MAX_LINEAR_BYTE_STRING_SIZE: usize = 23;
const FULL_CHUNK_SIZE: usize = MAX_LINEAR_BYTE_STRING_SIZE + 1;
const FULL_CHUNK_PAYLOAD_SIZE: usize = FULL_CHUNK_SIZE - 4;
const FULL_CHUNK_BASE_INDEX: u64 = 1000;

/// Metadata map value subset used by this generator slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxMetadataValue {
    /// Upstream `TxMetaBytes`.
    Bytes(Vec<u8>),
}

/// Upstream `TxMetadata` map produced by `listMetadata`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TxMetadata {
    entries: BTreeMap<u64, TxMetadataValue>,
}

impl TxMetadata {
    /// Construct a metadata map from values indexed `[0..n]`.
    pub fn list_metadata(values: Vec<TxMetadataValue>) -> Self {
        Self {
            entries: values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index as u64, value))
                .collect(),
        }
    }

    /// Construct a metadata map from sorted explicit entries.
    pub fn from_entries(entries: BTreeMap<u64, TxMetadataValue>) -> Self {
        Self { entries }
    }

    /// Number of metadata map entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when the metadata map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Borrow the byte string stored under a metadata key.
    pub fn bytes_at(&self, key: u64) -> Option<&[u8]> {
        match self.entries.get(&key) {
            Some(TxMetadataValue::Bytes(bytes)) => Some(bytes),
            None => None,
        }
    }

    /// Encode as Shelley-style transaction metadata CBOR:
    /// `{ * uint => transaction_metadatum }`.
    pub fn to_cbor_bytes(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.map(self.entries.len() as u64);
        for (key, value) in &self.entries {
            enc.unsigned(*key);
            match value {
                TxMetadataValue::Bytes(bytes) => {
                    enc.bytes(bytes);
                }
            }
        }
        enc.into_bytes()
    }
}

/// Mirror of upstream `assume_cbor_properties`.
pub fn assume_cbor_properties() -> bool {
    prop_map_costs_shelley()
        && prop_map_costs_allegra()
        && prop_map_costs_mary()
        && prop_map_costs_alonzo()
        && prop_map_costs_babbage()
        && prop_bs_costs_shelley()
        && prop_bs_costs_allegra()
        && prop_bs_costs_mary()
        && prop_bs_costs_alonzo()
        && prop_bs_costs_babbage()
        && prop_bs_costs_conway()
}

/// Mirror of upstream `prop_mapCostsShelley`.
pub fn prop_map_costs_shelley() -> bool {
    measure_map_costs(AnyCardanoEra::Shelley) == assume_map_costs(AnyCardanoEra::Shelley)
}

/// Mirror of upstream `prop_mapCostsAllegra`.
pub fn prop_map_costs_allegra() -> bool {
    measure_map_costs(AnyCardanoEra::Allegra) == assume_map_costs(AnyCardanoEra::Allegra)
}

/// Mirror of upstream `prop_mapCostsMary`.
pub fn prop_map_costs_mary() -> bool {
    measure_map_costs(AnyCardanoEra::Mary) == assume_map_costs(AnyCardanoEra::Mary)
}

/// Mirror of upstream `prop_mapCostsAlonzo`.
pub fn prop_map_costs_alonzo() -> bool {
    measure_map_costs(AnyCardanoEra::Alonzo) == assume_map_costs(AnyCardanoEra::Alonzo)
}

/// Mirror of upstream `prop_mapCostsBabbage`.
pub fn prop_map_costs_babbage() -> bool {
    measure_map_costs(AnyCardanoEra::Babbage) == assume_map_costs(AnyCardanoEra::Babbage)
}

/// Mirror of upstream `prop_mapCostsConway`.
pub fn prop_map_costs_conway() -> bool {
    measure_map_costs(AnyCardanoEra::Conway) == assume_map_costs(AnyCardanoEra::Conway)
}

/// Mirror of upstream `prop_mapCostsDijkstra`.
pub fn prop_map_costs_dijkstra() -> bool {
    measure_map_costs(AnyCardanoEra::Dijkstra) == assume_map_costs(AnyCardanoEra::Dijkstra)
}

/// Mirror of upstream `prop_bsCostsShelley`.
pub fn prop_bs_costs_shelley() -> bool {
    measure_bs_costs(AnyCardanoEra::Shelley) == Ok((37usize..=60).chain(62..=102).collect())
}

/// Mirror of upstream `prop_bsCostsAllegra`.
pub fn prop_bs_costs_allegra() -> bool {
    measure_bs_costs(AnyCardanoEra::Allegra) == Ok((39usize..=62).chain(64..=104).collect())
}

/// Mirror of upstream `prop_bsCostsMary`.
pub fn prop_bs_costs_mary() -> bool {
    measure_bs_costs(AnyCardanoEra::Mary) == Ok((39usize..=62).chain(64..=104).collect())
}

/// Mirror of upstream `prop_bsCostsAlonzo`.
pub fn prop_bs_costs_alonzo() -> bool {
    measure_bs_costs(AnyCardanoEra::Alonzo) == Ok((42usize..=65).chain(67..=107).collect())
}

/// Mirror of upstream `prop_bsCostsBabbage`.
pub fn prop_bs_costs_babbage() -> bool {
    measure_bs_costs(AnyCardanoEra::Babbage) == Ok((42usize..=65).chain(67..=107).collect())
}

/// Mirror of upstream `prop_bsCostsConway`.
pub fn prop_bs_costs_conway() -> bool {
    measure_bs_costs(AnyCardanoEra::Conway) == Ok((42usize..=65).chain(67..=107).collect())
}

/// Mirror of upstream `prop_bsCostsDijkstra`.
pub fn prop_bs_costs_dijkstra() -> bool {
    measure_bs_costs(AnyCardanoEra::Dijkstra) == Ok((42usize..=65).chain(67..=107).collect())
}

/// Mirror of upstream `measureMapCosts`.
pub fn measure_map_costs(era: AnyCardanoEra) -> Result<Vec<usize>, String> {
    (0..=MAX_MAP_SIZE)
        .map(|count| {
            let metadata = replicate_empty_bs(count);
            metadata_size(era, Some(&metadata))
        })
        .collect()
}

/// Mirror of upstream `measureBSCosts`.
pub fn measure_bs_costs(era: AnyCardanoEra) -> Result<Vec<usize>, String> {
    (0..=MAX_BS_SIZE)
        .map(|size| {
            let metadata = bs_metadata(size);
            metadata_size(era, Some(&metadata))
        })
        .collect()
}

/// Mirror of upstream `replicateEmptyBS`.
pub fn replicate_empty_bs(count: usize) -> TxMetadata {
    TxMetadata::list_metadata(vec![TxMetadataValue::Bytes(Vec::new()); count])
}

/// Mirror of upstream `bsMetadata`.
pub fn bs_metadata(size: usize) -> TxMetadata {
    TxMetadata::list_metadata(vec![TxMetadataValue::Bytes(vec![0; size])])
}

/// Mirror of upstream `metadataSize` for the metadata shapes generated here.
pub fn metadata_size(era: AnyCardanoEra, metadata: Option<&TxMetadata>) -> Result<usize, String> {
    let Some(metadata) = metadata else {
        return Ok(0);
    };
    if metadata.is_empty() {
        return Ok(0);
    }

    let first_empty_item_size = metadata_item_size(0, &TxMetadataValue::Bytes(Vec::new()));
    let item_size: usize = metadata
        .entries
        .iter()
        .map(|(key, value)| metadata_item_size(*key, value))
        .sum();
    let map_len_step = usize::from(metadata.len() > MAX_LINEAR_BYTE_STRING_SIZE);
    Ok(first_map_entry_cost(era)? + item_size - first_empty_item_size + map_len_step)
}

fn metadata_item_size(key: u64, value: &TxMetadataValue) -> usize {
    let mut enc = Encoder::new();
    enc.unsigned(key);
    match value {
        TxMetadataValue::Bytes(bytes) => {
            enc.bytes(bytes);
        }
    }
    enc.into_bytes().len()
}

/// Mirror of upstream `assumeMapCosts`.
pub fn assume_map_costs(era: AnyCardanoEra) -> Result<Vec<usize>, String> {
    let first_entry = first_map_entry_cost(era)?;
    Ok(step_function(&[
        (1, 0),
        (1, first_entry),
        (22, 2),
        (233, 3),
        (744, 4),
    ]))
}

/// Mirror of upstream `assumeBSCosts` encoded as explicit era branches.
pub fn assume_bs_costs(era: AnyCardanoEra) -> Result<Vec<usize>, String> {
    match era {
        AnyCardanoEra::Byron => Err("byron not supported".to_string()),
        AnyCardanoEra::Shelley => Ok((37usize..=60).chain(62..=102).collect()),
        AnyCardanoEra::Allegra | AnyCardanoEra::Mary => {
            Ok((39usize..=62).chain(64..=104).collect())
        }
        AnyCardanoEra::Alonzo
        | AnyCardanoEra::Babbage
        | AnyCardanoEra::Conway
        | AnyCardanoEra::Dijkstra => Ok((42usize..=65).chain(67..=107).collect()),
    }
}

/// Mirror of upstream `stepFunction`.
pub fn step_function(steps: &[(usize, usize)]) -> Vec<usize> {
    let mut values = Vec::new();
    let mut total = 0usize;
    for (count, step) in steps {
        for _ in 0..*count {
            total += *step;
            values.push(total);
        }
    }
    values
}

/// Mirror of upstream `mkMetadata`.
pub fn mk_metadata(era: AnyCardanoEra, size: usize) -> Result<Option<TxMetadata>, String> {
    if size == 0 {
        return Ok(None);
    }

    let min_size = metadata_min_size(era)?;
    if size < min_size {
        return Err(format!(
            "Error : metadata must be 0 or at least {min_size} bytes in this era."
        ));
    }

    let netto_size = size - min_size;
    let full_chunk_count = netto_size / FULL_CHUNK_SIZE;
    let first_chunk_len = netto_size % FULL_CHUNK_SIZE;
    let mut entries = BTreeMap::new();
    entries.insert(0, TxMetadataValue::Bytes(vec![0; first_chunk_len]));

    let full_chunk_count = u64::try_from(full_chunk_count)
        .map_err(|_| "metadata full chunk count exceeds u64".to_string())?;
    for offset in 0..full_chunk_count {
        let index = FULL_CHUNK_BASE_INDEX
            .checked_add(offset)
            .ok_or_else(|| "metadata full chunk index overflow".to_string())?;
        entries.insert(
            index,
            TxMetadataValue::Bytes(vec![0; FULL_CHUNK_PAYLOAD_SIZE]),
        );
    }

    Ok(Some(TxMetadata::from_entries(entries)))
}

fn first_map_entry_cost(era: AnyCardanoEra) -> Result<usize, String> {
    match era {
        AnyCardanoEra::Byron => Err("byron not supported".to_string()),
        AnyCardanoEra::Shelley => Ok(37),
        AnyCardanoEra::Allegra | AnyCardanoEra::Mary => Ok(39),
        AnyCardanoEra::Alonzo
        | AnyCardanoEra::Babbage
        | AnyCardanoEra::Conway
        | AnyCardanoEra::Dijkstra => Ok(42),
    }
}

fn metadata_min_size(era: AnyCardanoEra) -> Result<usize, String> {
    match era {
        AnyCardanoEra::Byron => Err("byron not supported".to_string()),
        AnyCardanoEra::Shelley => Ok(37),
        AnyCardanoEra::Allegra
        | AnyCardanoEra::Mary
        | AnyCardanoEra::Alonzo
        | AnyCardanoEra::Babbage
        | AnyCardanoEra::Conway
        | AnyCardanoEra::Dijkstra => Ok(39),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assumed_cbor_properties_match_upstream_sentinel() {
        assert!(assume_cbor_properties());
        assert!(prop_map_costs_conway());
        assert!(prop_map_costs_dijkstra());
        assert!(prop_bs_costs_dijkstra());
    }

    #[test]
    fn step_function_scans_repeated_steps() {
        assert_eq!(step_function(&[(1, 0), (2, 3), (1, 4)]), vec![0, 3, 6, 10]);
    }

    #[test]
    fn map_cost_step_boundaries_match_upstream_assumption() {
        let costs = assume_map_costs(AnyCardanoEra::Conway).expect("conway costs");

        assert_eq!(costs.len(), MAX_MAP_SIZE + 1);
        assert_eq!(costs[0], 0);
        assert_eq!(costs[1], 42);
        assert_eq!(costs[23], 86);
        assert_eq!(costs[256], 785);
        assert_eq!(costs[1000], 3761);
    }

    #[test]
    fn byte_string_costs_keep_cbor_header_step() {
        let costs = assume_bs_costs(AnyCardanoEra::Shelley).expect("shelley costs");

        assert_eq!(costs.len(), MAX_BS_SIZE + 1);
        assert_eq!(costs[0], 37);
        assert_eq!(costs[23], 60);
        assert_eq!(costs[24], 62);
        assert_eq!(costs[64], 102);
    }

    #[test]
    fn mk_metadata_zero_selects_tx_metadata_none() {
        assert_eq!(mk_metadata(AnyCardanoEra::Conway, 0).expect("zero"), None);
    }

    #[test]
    fn mk_metadata_rejects_too_small_nonzero_payloads() {
        let err = mk_metadata(AnyCardanoEra::Conway, 38).expect_err("too small");

        assert_eq!(
            err,
            "Error : metadata must be 0 or at least 39 bytes in this era."
        );
    }

    #[test]
    fn mk_metadata_uses_empty_first_chunk_at_minimum() {
        let metadata = mk_metadata(AnyCardanoEra::Conway, 39)
            .expect("metadata")
            .expect("some metadata");

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata.bytes_at(0), Some(&[][..]));
        assert_eq!(metadata.to_cbor_bytes(), vec![0xa1, 0x00, 0x40]);
    }

    #[test]
    fn mk_metadata_uses_twenty_byte_full_chunks() {
        let metadata = mk_metadata(AnyCardanoEra::Conway, 63)
            .expect("metadata")
            .expect("some metadata");

        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata.bytes_at(0), Some(&[][..]));
        assert_eq!(metadata.bytes_at(1000), Some(&vec![0; 20][..]));

        let encoded = metadata.to_cbor_bytes();
        assert_eq!(&encoded[..6], &[0xa2, 0x00, 0x40, 0x19, 0x03, 0xe8]);
        assert_eq!(encoded[6], 0x54);
        assert_eq!(&encoded[7..], &vec![0; 20][..]);
    }

    #[test]
    fn mk_metadata_remainder_stays_in_first_chunk() {
        let metadata = mk_metadata(AnyCardanoEra::Conway, 64)
            .expect("metadata")
            .expect("some metadata");

        assert_eq!(metadata.bytes_at(0), Some(&[0][..]));
        assert_eq!(metadata.bytes_at(1000), Some(&vec![0; 20][..]));
    }
}
