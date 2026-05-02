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

use crate::cbor::{
    BLOCK_BODY_ELEMENTS_MAX, CborDecode, CborEncode, Decoder, Encoder, vec_with_safe_capacity,
};
use crate::eras::shelley::{ShelleyTxIn, ShelleyUpdate};
use crate::error::LedgerError;
use crate::types::{DCert, RewardAccount};

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
            if ma.is_empty() {
                Ok(Self::Coin(coin))
            } else {
                Ok(Self::CoinAndAssets(coin, ma))
            }
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
///
/// Pre-Conway upstream decoders use `decodeWithPrunning`, so zero asset
/// quantities are pruned before UTxO validation sees the value. Policies made
/// empty by pruning are omitted from the normalized map.
fn decode_multi_asset_unsigned(dec: &mut Decoder<'_>) -> Result<MultiAsset, LedgerError> {
    let mut ma = BTreeMap::new();
    match dec.map_begin()? {
        Some(policy_count) => {
            for _ in 0..policy_count {
                let policy_raw = dec.bytes()?;
                let policy: PolicyId =
                    policy_raw
                        .try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: policy_raw.len(),
                        })?;
                let mut assets = BTreeMap::new();
                match dec.map_begin()? {
                    Some(asset_count) => {
                        for _ in 0..asset_count {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.unsigned()?;
                            insert_nonzero_asset(&mut assets, name, qty);
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.unsigned()?;
                            insert_nonzero_asset(&mut assets, name, qty);
                        }
                        dec.consume_break()?;
                    }
                }
                insert_nonempty_policy(&mut ma, policy, assets);
            }
        }
        None => {
            while !dec.is_break() {
                let policy_raw = dec.bytes()?;
                let policy: PolicyId =
                    policy_raw
                        .try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: policy_raw.len(),
                        })?;
                let mut assets = BTreeMap::new();
                match dec.map_begin()? {
                    Some(asset_count) => {
                        for _ in 0..asset_count {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.unsigned()?;
                            insert_nonzero_asset(&mut assets, name, qty);
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.unsigned()?;
                            insert_nonzero_asset(&mut assets, name, qty);
                        }
                        dec.consume_break()?;
                    }
                }
                insert_nonempty_policy(&mut ma, policy, assets);
            }
            dec.consume_break()?;
        }
    }
    Ok(ma)
}

fn insert_nonzero_asset(assets: &mut BTreeMap<AssetName, u64>, name: AssetName, qty: u64) {
    if qty != 0 {
        assets.insert(name, qty);
    }
}

fn insert_nonempty_policy(ma: &mut MultiAsset, policy: PolicyId, assets: BTreeMap<AssetName, u64>) {
    if !assets.is_empty() {
        ma.insert(policy, assets);
    }
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
    let mut ma = BTreeMap::new();
    match dec.map_begin()? {
        Some(policy_count) => {
            for _ in 0..policy_count {
                let policy_raw = dec.bytes()?;
                let policy: PolicyId =
                    policy_raw
                        .try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: policy_raw.len(),
                        })?;
                let mut assets = BTreeMap::new();
                match dec.map_begin()? {
                    Some(asset_count) => {
                        for _ in 0..asset_count {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.integer()?;
                            assets.insert(name, qty);
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.integer()?;
                            assets.insert(name, qty);
                        }
                        dec.consume_break()?;
                    }
                }
                ma.insert(policy, assets);
            }
        }
        None => {
            while !dec.is_break() {
                let policy_raw = dec.bytes()?;
                let policy: PolicyId =
                    policy_raw
                        .try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: policy_raw.len(),
                        })?;
                let mut assets = BTreeMap::new();
                match dec.map_begin()? {
                    Some(asset_count) => {
                        for _ in 0..asset_count {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.integer()?;
                            assets.insert(name, qty);
                        }
                    }
                    None => {
                        while !dec.is_break() {
                            let name = dec.bytes()?.to_vec();
                            if name.len() > 32 {
                                return Err(LedgerError::AssetNameTooLong { actual: name.len() });
                            }
                            let qty = dec.integer()?;
                            assets.insert(name, qty);
                        }
                        dec.consume_break()?;
                    }
                }
                ma.insert(policy, assets);
            }
            dec.consume_break()?;
        }
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
/// Only required fields (0–2) and optional keys 3, 4, 5, 6, 7, 8, 9 are modeled.
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
    /// Optional certificates (CDDL key 4).
    pub certificates: Option<Vec<DCert>>,
    /// Optional withdrawals: reward-account → lovelace (CDDL key 5).
    pub withdrawals: Option<BTreeMap<RewardAccount, u64>>,
    /// Optional protocol-parameter update proposal (CDDL key 6).
    pub update: Option<ShelleyUpdate>,
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
        if self.certificates.is_some() {
            field_count += 1;
        }
        if self.withdrawals.is_some() {
            field_count += 1;
        }
        if self.update.is_some() {
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

        // Key 4: certificates.
        if let Some(certs) = &self.certificates {
            enc.unsigned(4).array(certs.len() as u64);
            for cert in certs {
                cert.encode_cbor(enc);
            }
        }

        // Key 5: withdrawals.
        if let Some(withdrawals) = &self.withdrawals {
            enc.unsigned(5).map(withdrawals.len() as u64);
            for (acct, coin) in withdrawals {
                acct.encode_cbor(enc);
                enc.unsigned(*coin);
            }
        }

        // Key 6: update.
        if let Some(update) = &self.update {
            enc.unsigned(6);
            update.encode_cbor(enc);
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
        let mut certificates: Option<Vec<DCert>> = None;
        let mut withdrawals: Option<BTreeMap<RewardAccount, u64>> = None;
        let mut update: Option<ShelleyUpdate> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;
        let mut validity_interval_start: Option<u64> = None;
        let mut mint: Option<MintAsset> = None;

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array_or_set()?;
                    let mut ins = vec_with_safe_capacity(count, BLOCK_BODY_ELEMENTS_MAX);
                    for _ in 0..count {
                        ins.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    inputs = Some(ins);
                }
                1 => {
                    let count = dec.array()?;
                    let mut outs = vec_with_safe_capacity(count, BLOCK_BODY_ELEMENTS_MAX);
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
                4 => {
                    let count = dec.array_or_set()?;
                    let mut certs = vec_with_safe_capacity(count, BLOCK_BODY_ELEMENTS_MAX);
                    for _ in 0..count {
                        certs.push(DCert::decode_cbor(dec)?);
                    }
                    certificates = Some(certs);
                }
                5 => {
                    let count = dec.map()?;
                    let mut wdrl = BTreeMap::new();
                    for _ in 0..count {
                        let acct = RewardAccount::decode_cbor(dec)?;
                        let coin = dec.unsigned()?;
                        wdrl.insert(acct, coin);
                    }
                    withdrawals = Some(wdrl);
                }
                6 => {
                    update = Some(ShelleyUpdate::decode_cbor(dec)?);
                }
                7 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
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
            certificates,
            withdrawals,
            update,
            auxiliary_data_hash,
            validity_interval_start,
            mint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_txin(idx: u16) -> ShelleyTxIn {
        ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: idx,
        }
    }

    fn mk_policy() -> PolicyId {
        [0xBB; 28]
    }

    fn mk_multi_asset() -> MultiAsset {
        let mut assets = BTreeMap::new();
        assets.insert(b"token_a".to_vec(), 100);
        let mut ma = BTreeMap::new();
        ma.insert(mk_policy(), assets);
        ma
    }

    fn mk_mint_asset() -> MintAsset {
        let mut assets = BTreeMap::new();
        assets.insert(b"minted".to_vec(), 50);
        assets.insert(b"burned".to_vec(), -10);
        let mut ma = BTreeMap::new();
        ma.insert(mk_policy(), assets);
        ma
    }

    // ── Value round-trips ──────────────────────────────────────────────

    #[test]
    fn value_coin_round_trip() {
        let v = Value::Coin(5_000_000);
        let decoded = Value::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn value_coin_and_assets_round_trip() {
        let v = Value::CoinAndAssets(3_000_000, mk_multi_asset());
        let decoded = Value::from_cbor_bytes(&v.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn value_decode_prunes_zero_multi_asset_quantities() {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(3_000_000).map(2);
        enc.bytes(&[0x01; 28]).map(2);
        enc.bytes(b"zero").unsigned(0);
        enc.bytes(b"kept").unsigned(7);
        enc.bytes(&[0x02; 28]).map(1);
        enc.bytes(b"gone").unsigned(0);

        let decoded = Value::from_cbor_bytes(&enc.into_bytes()).unwrap();
        let Value::CoinAndAssets(coin, ma) = decoded else {
            panic!("expected non-empty multi-asset value after pruning");
        };

        assert_eq!(coin, 3_000_000);
        assert_eq!(ma.len(), 1);
        let assets = ma.get(&[0x01; 28]).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets.get(b"kept".as_slice()), Some(&7));
        assert!(!assets.contains_key(b"zero".as_slice()));
        assert!(!ma.contains_key(&[0x02; 28]));
    }

    #[test]
    fn value_decode_zero_only_multiasset_as_coin() {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(2_000_000).map(1);
        enc.bytes(&[0x01; 28]).map(1);
        enc.bytes(b"zero").unsigned(0);

        let decoded = Value::from_cbor_bytes(&enc.into_bytes()).unwrap();
        assert_eq!(decoded, Value::Coin(2_000_000));
    }

    #[test]
    fn value_coin_accessor() {
        assert_eq!(Value::Coin(42).coin(), 42);
        assert_eq!(Value::CoinAndAssets(99, mk_multi_asset()).coin(), 99);
    }

    #[test]
    fn value_multi_asset_accessor() {
        assert!(Value::Coin(1).multi_asset().is_none());
        let ma = mk_multi_asset();
        let v = Value::CoinAndAssets(1, ma.clone());
        assert_eq!(v.multi_asset(), Some(&ma));
    }

    #[test]
    fn value_coin_vs_coin_and_assets_differ() {
        let a = Value::Coin(1_000_000);
        let b = Value::CoinAndAssets(1_000_000, BTreeMap::new());
        assert_ne!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }

    // ── MaryTxOut round-trips ──────────────────────────────────────────

    #[test]
    fn txout_coin_only_round_trip() {
        let out = MaryTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
        };
        let decoded = MaryTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_with_assets_round_trip() {
        let out = MaryTxOut {
            address: vec![0x01; 57],
            amount: Value::CoinAndAssets(5_000_000, mk_multi_asset()),
        };
        let decoded = MaryTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    // ── MaryTxBody round-trips ─────────────────────────────────────────

    #[test]
    fn tx_body_minimal_round_trip() {
        let body = MaryTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![MaryTxOut {
                address: vec![0x61; 29],
                amount: Value::Coin(2_000_000),
            }],
            fee: 200_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
        };
        let decoded = MaryTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_mint_round_trip() {
        let body = MaryTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![MaryTxOut {
                address: vec![0x61; 29],
                amount: Value::CoinAndAssets(3_000_000, mk_multi_asset()),
            }],
            fee: 300_000,
            ttl: Some(1000),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: Some(50),
            mint: Some(mk_mint_asset()),
        };
        let decoded = MaryTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_mint_absent_vs_present_differ() {
        let base = MaryTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![MaryTxOut {
                address: vec![0x61; 29],
                amount: Value::Coin(1_000_000),
            }],
            fee: 100_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
        };
        let with_mint = MaryTxBody {
            mint: Some(mk_mint_asset()),
            ..base.clone()
        };
        assert_ne!(base.to_cbor_bytes(), with_mint.to_cbor_bytes());
    }

    // ── Multi-asset CBOR helpers ───────────────────────────────────────

    #[test]
    fn multi_asset_encode_decode_round_trip() {
        let ma = mk_multi_asset();
        let mut enc = Encoder::new();
        encode_multi_asset(&mut enc, &ma);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let decoded = decode_multi_asset_unsigned(&mut dec).unwrap();
        assert_eq!(decoded, ma);
    }

    #[test]
    fn mint_asset_encode_decode_round_trip() {
        let ma = mk_mint_asset();
        let mut enc = Encoder::new();
        encode_mint_asset(&mut enc, &ma);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let decoded = decode_mint_asset(&mut dec).unwrap();
        assert_eq!(decoded, ma);
    }

    #[test]
    fn multi_asset_empty_round_trip() {
        let ma: MultiAsset = BTreeMap::new();
        let mut enc = Encoder::new();
        encode_multi_asset(&mut enc, &ma);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let decoded = decode_multi_asset_unsigned(&mut dec).unwrap();
        assert_eq!(decoded, ma);
    }

    #[test]
    fn multi_asset_multiple_policies() {
        let mut ma = MultiAsset::new();
        let mut a1 = BTreeMap::new();
        a1.insert(b"coin_x".to_vec(), 10);
        ma.insert([0x01; 28], a1);
        let mut a2 = BTreeMap::new();
        a2.insert(b"coin_y".to_vec(), 20);
        ma.insert([0x02; 28], a2);

        let mut enc = Encoder::new();
        encode_multi_asset(&mut enc, &ma);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let decoded = decode_multi_asset_unsigned(&mut dec).unwrap();
        assert_eq!(decoded, ma);
    }

    // ── Asset name length validation (CDDL: bytes .size (0..32)) ─────────

    #[test]
    fn asset_name_32_bytes_accepted() {
        let name = vec![0xAA; 32]; // exactly 32 bytes — valid
        let mut ma = BTreeMap::new();
        let mut assets = BTreeMap::new();
        assets.insert(name, 42u64);
        ma.insert([0x01; 28], assets);

        let mut enc = Encoder::new();
        encode_multi_asset(&mut enc, &ma);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let decoded = decode_multi_asset_unsigned(&mut dec).unwrap();
        assert_eq!(decoded, ma);
    }

    #[test]
    fn asset_name_33_bytes_rejected() {
        // Manually encode a multi-asset with a 33-byte asset name
        let mut enc = Encoder::new();
        enc.map(1); // 1 policy
        enc.bytes(&[0x01; 28]); // policy id
        enc.map(1); // 1 asset
        enc.bytes(&[0xBB; 33]); // 33-byte asset name — too long
        enc.unsigned(100);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let result = decode_multi_asset_unsigned(&mut dec);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, LedgerError::AssetNameTooLong { actual: 33 }),
            "expected AssetNameTooLong, got {err:?}"
        );
    }

    #[test]
    fn mint_asset_name_33_bytes_rejected() {
        let mut enc = Encoder::new();
        enc.map(1);
        enc.bytes(&[0x01; 28]);
        enc.map(1);
        enc.bytes(&[0xCC; 33]); // 33-byte asset name
        enc.integer(50);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let result = decode_mint_asset(&mut dec);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LedgerError::AssetNameTooLong { actual: 33 }
        ));
    }
}
