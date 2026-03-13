//! Mary-era transaction and value types.
//!
//! Mary inherits the Allegra block envelope and header format entirely.
//! The key additions are:
//! - `value` becomes multi-asset: `coin / [coin, multiasset<uint>]`.
//! - `multiasset<a>` is a nested map: `{* policy_id => {+ asset_name => a}}`.
//! - `transaction_body` key 9 (`mint`) carries a `multiasset<int64>` for
//!   minting and burning native tokens.
//! - `transaction_output` carries a `Value` instead of a bare `coin`.
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/mary/impl/cddl>

use std::collections::BTreeMap;

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::shelley::ShelleyTxIn;
use crate::error::LedgerError;

pub const MARY_NAME: &str = "Mary";

// ---------------------------------------------------------------------------
// Policy ID and asset name
// ---------------------------------------------------------------------------

/// A 28-byte Blake2b-224 hash identifying a minting policy.
///
/// CDDL: `policy_id = script_hash = hash28 = bytes .size 28`
///
/// Reference: `Cardano.Ledger.Mary.Value` — `PolicyID`.
pub type PolicyId = [u8; 28];

/// An asset name: an opaque byte string of at most 32 bytes.
///
/// CDDL: `asset_name = bytes .size (0 .. 32)`
///
/// An empty asset name (`b""`) represents the unnamed default asset
/// under a policy. The `Ord` implementation reflects CBOR canonical
/// ordering (shorter sorts first, then lexicographic).
///
/// Reference: `Cardano.Ledger.Mary.Value` — `AssetName`.
pub type AssetName = Vec<u8>;

// ---------------------------------------------------------------------------
// Multi-asset maps
// ---------------------------------------------------------------------------

/// A multi-asset map used in transaction outputs (positive quantities).
///
/// CDDL: `multiasset<uint> = {* policy_id => {+ asset_name => uint}}`
///
/// `BTreeMap` is used for deterministic CBOR serialization ordering.
///
/// Reference: `Cardano.Ledger.Mary.Value` — `MultiAsset`.
pub type MultiAsset = BTreeMap<PolicyId, BTreeMap<AssetName, u64>>;

/// A multi-asset map used in the `mint` field (signed quantities for
/// minting and burning).
///
/// CDDL: `mint = multiasset<int64>`
///
/// Reference: `Cardano.Ledger.Mary.Value` — `MultiAsset` (with signed
/// quantities in the mint context).
pub type MintAsset = BTreeMap<PolicyId, BTreeMap<AssetName, i64>>;

// ---------------------------------------------------------------------------
// Value: coin or coin + multi-asset
// ---------------------------------------------------------------------------

/// An output value: either pure lovelace or lovelace plus native tokens.
///
/// CDDL: `value = coin / [coin, multiasset<uint>]`
///
/// Reference: `Cardano.Ledger.Mary.Value` — `MaryValue`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// Pure lovelace amount (Shelley-compatible encoding).
    Coin(u64),
    /// Lovelace plus multi-asset token bundle.
    CoinAndAssets(u64, MultiAsset),
}

impl Value {
    /// Returns the lovelace amount regardless of variant.
    pub fn coin(&self) -> u64 {
        match self {
            Self::Coin(c) => *c,
            Self::CoinAndAssets(c, _) => *c,
        }
    }

    /// Returns the multi-asset bundle, if present.
    pub fn multi_asset(&self) -> Option<&MultiAsset> {
        match self {
            Self::Coin(_) => None,
            Self::CoinAndAssets(_, ma) => Some(ma),
        }
    }
}

impl CborEncode for Value {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Coin(c) => {
                enc.unsigned(*c);
            }
            Self::CoinAndAssets(c, ma) => {
                enc.array(2).unsigned(*c);
                encode_multi_asset(enc, ma);
            }
        }
    }
}

impl CborDecode for Value {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        if major == 0 {
            // Bare unsigned integer → pure coin.
            let coin = dec.unsigned()?;
            Ok(Self::Coin(coin))
        } else if major == 4 {
            // Array [coin, multiasset<uint>].
            let len = dec.array()?;
            if len != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: len as usize,
                });
            }
            let coin = dec.unsigned()?;
            let ma = decode_multi_asset_unsigned(dec)?;
            Ok(Self::CoinAndAssets(coin, ma))
        } else {
            Err(LedgerError::CborTypeMismatch {
                expected: 0, // unsigned or array
                actual: major,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-asset CBOR helpers
// ---------------------------------------------------------------------------

/// Encodes a `MultiAsset` (unsigned quantities) as a CBOR map-of-maps.
fn encode_multi_asset(enc: &mut Encoder, ma: &MultiAsset) {
    enc.map(ma.len() as u64);
    for (policy, assets) in ma {
        enc.bytes(policy);
        enc.map(assets.len() as u64);
        for (name, qty) in assets {
            enc.bytes(name);
            enc.unsigned(*qty);
        }
    }
}

/// Decodes a `MultiAsset` (unsigned quantities) from a CBOR map-of-maps.
fn decode_multi_asset_unsigned(dec: &mut Decoder<'_>) -> Result<MultiAsset, LedgerError> {
    let policy_count = dec.map()?;
    let mut ma = BTreeMap::new();
    for _ in 0..policy_count {
        let policy_raw = dec.bytes()?;
        let policy: PolicyId = policy_raw
            .try_into()
            .map_err(|_| LedgerError::CborInvalidLength {
                expected: 28,
                actual: policy_raw.len(),
            })?;
        let asset_count = dec.map()?;
        let mut assets = BTreeMap::new();
        for _ in 0..asset_count {
            let name = dec.bytes()?.to_vec();
            let qty = dec.unsigned()?;
            assets.insert(name, qty);
        }
        ma.insert(policy, assets);
    }
    Ok(ma)
}

/// Encodes a `MintAsset` (signed quantities) as a CBOR map-of-maps.
pub(crate) fn encode_mint_asset(enc: &mut Encoder, ma: &MintAsset) {
    enc.map(ma.len() as u64);
    for (policy, assets) in ma {
        enc.bytes(policy);
        enc.map(assets.len() as u64);
        for (name, qty) in assets {
            enc.bytes(name);
            enc.integer(*qty);
        }
    }
}

/// Decodes a `MintAsset` (signed quantities) from a CBOR map-of-maps.
pub(crate) fn decode_mint_asset(dec: &mut Decoder<'_>) -> Result<MintAsset, LedgerError> {
    let policy_count = dec.map()?;
    let mut ma = BTreeMap::new();
    for _ in 0..policy_count {
        let policy_raw = dec.bytes()?;
        let policy: PolicyId = policy_raw
            .try_into()
            .map_err(|_| LedgerError::CborInvalidLength {
                expected: 28,
                actual: policy_raw.len(),
            })?;
        let asset_count = dec.map()?;
        let mut assets = BTreeMap::new();
        for _ in 0..asset_count {
            let name = dec.bytes()?.to_vec();
            let qty = dec.integer()?;
            assets.insert(name, qty);
        }
        ma.insert(policy, assets);
    }
    Ok(ma)
}

// ---------------------------------------------------------------------------
// Mary transaction output
// ---------------------------------------------------------------------------

/// A Mary-era transaction output: an address receiving a `Value`.
///
/// CDDL: `transaction_output = [address, amount : value]`
///
/// Reference: `Cardano.Ledger.Mary.TxOut`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaryTxOut {
    /// Raw address bytes (encoding varies by address type).
    pub address: Vec<u8>,
    /// Output value: coin or coin + multi-asset bundle.
    pub amount: Value,
}

impl CborEncode for MaryTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).bytes(&self.address);
        self.amount.encode_cbor(enc);
    }
}

impl CborDecode for MaryTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let address = dec.bytes()?.to_vec();
        let amount = Value::decode_cbor(dec)?;
        Ok(Self { address, amount })
    }
}

// ---------------------------------------------------------------------------
// Mary transaction body
// ---------------------------------------------------------------------------

/// Mary-era transaction body.
///
/// Extends Allegra by adding key 9 (`mint : multiasset<int64>`) for native
/// token minting and burning.
///
/// ```text
/// transaction_body =
///   { 0 : set<transaction_input>
///   , 1 : [* transaction_output]
///   , 2 : coin
///   , ? 3 : slot                  ; ttl (optional since Allegra)
///   , ? 4 : [* certificate]
///   , ? 5 : withdrawals
///   , ? 6 : update
///   , ? 7 : auxiliary_data_hash
///   , ? 8 : slot                  ; validity interval start
///   , ? 9 : mint                  ; NEW in Mary
///   }
/// ```
///
/// Only required fields (0–2) and optional keys 3, 7, 8, 9 are modeled.
/// Certificates (4), withdrawals (5), and update (6) will be added later.
///
/// Reference: `Cardano.Ledger.Mary.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaryTxBody {
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<MaryTxOut>,
    /// Transaction fee in lovelace (CDDL key 2).
    pub fee: u64,
    /// Optional upper-bound slot (TTL); tx invalid after this slot (CDDL key 3).
    pub ttl: Option<u64>,
    /// Optional auxiliary data hash (CDDL key 7).
    pub auxiliary_data_hash: Option<[u8; 32]>,
    /// Optional lower-bound slot; tx invalid before this slot (CDDL key 8).
    pub validity_interval_start: Option<u64>,
    /// Optional mint field for native token creation/destruction (CDDL key 9).
    pub mint: Option<MintAsset>,
}

impl CborEncode for MaryTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 3; // keys 0, 1, 2 always present
        if self.ttl.is_some() {
            field_count += 1;
        }
        if self.auxiliary_data_hash.is_some() {
            field_count += 1;
        }
        if self.validity_interval_start.is_some() {
            field_count += 1;
        }
        if self.mint.is_some() {
            field_count += 1;
        }
        enc.map(field_count);

        // Key 0: inputs.
        enc.unsigned(0).array(self.inputs.len() as u64);
        for input in &self.inputs {
            input.encode_cbor(enc);
        }

        // Key 1: outputs.
        enc.unsigned(1).array(self.outputs.len() as u64);
        for output in &self.outputs {
            output.encode_cbor(enc);
        }

        // Key 2: fee.
        enc.unsigned(2).unsigned(self.fee);

        // Key 3: ttl (optional).
        if let Some(ttl) = self.ttl {
            enc.unsigned(3).unsigned(ttl);
        }

        // Key 7: auxiliary_data_hash (optional).
        if let Some(hash) = &self.auxiliary_data_hash {
            enc.unsigned(7).bytes(hash);
        }

        // Key 8: validity_interval_start (optional).
        if let Some(start) = self.validity_interval_start {
            enc.unsigned(8).unsigned(start);
        }

        // Key 9: mint (optional).
        if let Some(mint) = &self.mint {
            enc.unsigned(9);
            encode_mint_asset(enc, mint);
        }
    }
}

impl CborDecode for MaryTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<MaryTxOut>> = None;
        let mut fee: Option<u64> = None;
        let mut ttl: Option<u64> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;
        let mut validity_interval_start: Option<u64> = None;
        let mut mint: Option<MintAsset> = None;

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array()?;
                    let mut ins = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        ins.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    inputs = Some(ins);
                }
                1 => {
                    let count = dec.array()?;
                    let mut outs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        outs.push(MaryTxOut::decode_cbor(dec)?);
                    }
                    outputs = Some(outs);
                }
                2 => {
                    fee = Some(dec.unsigned()?);
                }
                3 => {
                    ttl = Some(dec.unsigned()?);
                }
                7 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into()
                            .map_err(|_| LedgerError::CborInvalidLength {
                                expected: 32,
                                actual: raw.len(),
                            })?;
                    auxiliary_data_hash = Some(hash);
                }
                8 => {
                    validity_interval_start = Some(dec.unsigned()?);
                }
                9 => {
                    mint = Some(decode_mint_asset(dec)?);
                }
                _ => {
                    dec.skip()?;
                }
            }
        }

        Ok(Self {
            inputs: inputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            outputs: outputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            fee: fee.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            ttl,
            auxiliary_data_hash,
            validity_interval_start,
            mint,
        })
    }
}
