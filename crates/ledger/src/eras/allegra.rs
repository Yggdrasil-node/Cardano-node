//! Allegra-era transaction and script types.
//!
//! Allegra inherits the Shelley block envelope and header format entirely.
//! The key differences are:
//! - `transaction_body` key 3 (TTL) becomes optional.
//! - `transaction_body` key 8 (validity_interval_start) is added (optional).
//! - `native_script` gains timelock predicates (`InvalidBefore`,
//!   `InvalidHereafter`) alongside the existing multi-sig constructors.
//! - `auxiliary_data` can now carry an auxiliary script list.
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/allegra/impl/cddl>

use std::collections::BTreeMap;

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::shelley::{ShelleyTxIn, ShelleyTxOut, ShelleyUpdate};
use crate::error::LedgerError;
use crate::types::{DCert, RewardAccount};

pub const ALLEGRA_NAME: &str = "Allegra";

// ---------------------------------------------------------------------------
// Allegra transaction body
// ---------------------------------------------------------------------------

/// Allegra-era transaction body.
///
/// Differs from Shelley in two ways:
/// - Key 3 (TTL / validity-interval upper bound) is now **optional**.
/// - Key 8 (validity-interval lower bound) is newly added and optional.
///
/// ```text
/// transaction_body =
///   { 0 : set<transaction_input>
///   , 1 : [* transaction_output]
///   , 2 : coin
///   , ? 3 : slot                  ; ttl (optional in Allegra)
///   , ? 4 : [* certificate]
///   , ? 5 : withdrawals
///   , ? 6 : update
///   , ? 7 : auxiliary_data_hash
///   , ? 8 : slot                  ; validity interval start
///   }
/// ```
///
/// Only required fields (0–2) and optional keys 3, 4, 5, 6, 7, 8 are modeled.
///
/// Reference: `Cardano.Ledger.Allegra.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllegraTxBody {
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<ShelleyTxOut>,
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
}

impl CborEncode for AllegraTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 3; // keys 0, 1, 2 are always present
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
        enc.map(field_count);

        // Key 0: inputs (set encoded as array).
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
    }
}

impl CborDecode for AllegraTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<ShelleyTxOut>> = None;
        let mut fee: Option<u64> = None;
        let mut ttl: Option<u64> = None;
        let mut certificates: Option<Vec<DCert>> = None;
        let mut withdrawals: Option<BTreeMap<RewardAccount, u64>> = None;
        let mut update: Option<ShelleyUpdate> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;
        let mut validity_interval_start: Option<u64> = None;

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
                        outs.push(ShelleyTxOut::decode_cbor(dec)?);
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
                    let count = dec.array()?;
                    let mut certs = Vec::with_capacity(count as usize);
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
                _ => {
                    // Skip unknown fields for forward compatibility.
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
        })
    }
}

// ---------------------------------------------------------------------------
// Timelock / native script
// ---------------------------------------------------------------------------

/// Allegra native script with timelock support.
///
/// Allegra extends Shelley's multi-sig scripts with temporal predicates
/// that gate script validity to slot ranges. Timelock validity intervals
/// are half-open intervals `[a, b)`.
///
/// ```text
/// native_script =
///   [  script_pubkey             ; (0, addr_keyhash)
///   // script_all                ; (1, [* native_script])
///   // script_any                ; (2, [* native_script])
///   // script_n_of_k             ; (3, n : int64, [* native_script])
///   // script_invalid_before     ; (4, slot)
///   // script_invalid_hereafter  ; (5, slot)
///   ]
/// ```
///
/// Reference: `Cardano.Ledger.Allegra.Scripts` — `Timelock`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeScript {
    /// Require a signature from the given key hash (tag 0).
    ScriptPubkey([u8; 28]),
    /// All sub-scripts must validate (tag 1).
    ScriptAll(Vec<NativeScript>),
    /// At least one sub-script must validate (tag 2).
    ScriptAny(Vec<NativeScript>),
    /// At least `n` out of the sub-scripts must validate (tag 3).
    ScriptNOfK(i64, Vec<NativeScript>),
    /// Transaction is invalid before this slot (tag 4, inclusive).
    InvalidBefore(u64),
    /// Transaction is invalid at or after this slot (tag 5, exclusive).
    InvalidHereafter(u64),
}

impl CborEncode for NativeScript {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            NativeScript::ScriptPubkey(keyhash) => {
                enc.array(2).unsigned(0).bytes(keyhash);
            }
            NativeScript::ScriptAll(scripts) => {
                enc.array(2).unsigned(1).array(scripts.len() as u64);
                for s in scripts {
                    s.encode_cbor(enc);
                }
            }
            NativeScript::ScriptAny(scripts) => {
                enc.array(2).unsigned(2).array(scripts.len() as u64);
                for s in scripts {
                    s.encode_cbor(enc);
                }
            }
            NativeScript::ScriptNOfK(n, scripts) => {
                enc.array(3).unsigned(3).integer(*n);
                enc.array(scripts.len() as u64);
                for s in scripts {
                    s.encode_cbor(enc);
                }
            }
            NativeScript::InvalidBefore(slot) => {
                enc.array(2).unsigned(4).unsigned(*slot);
            }
            NativeScript::InvalidHereafter(slot) => {
                enc.array(2).unsigned(5).unsigned(*slot);
            }
        }
    }
}

impl CborDecode for NativeScript {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;

        match tag {
            0 => {
                if arr_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: arr_len as usize,
                    });
                }
                let raw = dec.bytes()?;
                let keyhash: [u8; 28] =
                    raw.try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 28,
                            actual: raw.len(),
                        })?;
                Ok(NativeScript::ScriptPubkey(keyhash))
            }
            1 => {
                if arr_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: arr_len as usize,
                    });
                }
                let count = dec.array()?;
                let mut scripts = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    scripts.push(NativeScript::decode_cbor(dec)?);
                }
                Ok(NativeScript::ScriptAll(scripts))
            }
            2 => {
                if arr_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: arr_len as usize,
                    });
                }
                let count = dec.array()?;
                let mut scripts = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    scripts.push(NativeScript::decode_cbor(dec)?);
                }
                Ok(NativeScript::ScriptAny(scripts))
            }
            3 => {
                if arr_len != 3 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 3,
                        actual: arr_len as usize,
                    });
                }
                let n = dec.integer()?;
                let count = dec.array()?;
                let mut scripts = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    scripts.push(NativeScript::decode_cbor(dec)?);
                }
                Ok(NativeScript::ScriptNOfK(n, scripts))
            }
            4 => {
                if arr_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: arr_len as usize,
                    });
                }
                let slot = dec.unsigned()?;
                Ok(NativeScript::InvalidBefore(slot))
            }
            5 => {
                if arr_len != 2 {
                    return Err(LedgerError::CborInvalidLength {
                        expected: 2,
                        actual: arr_len as usize,
                    });
                }
                let slot = dec.unsigned()?;
                Ok(NativeScript::InvalidHereafter(slot))
            }
            other => Err(LedgerError::CborTypeMismatch {
                expected: 0, // script tag
                actual: other as u8,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_txin(idx: u16) -> ShelleyTxIn {
        ShelleyTxIn { transaction_id: [0xAA; 32], index: idx }
    }

    fn mk_txout() -> ShelleyTxOut {
        ShelleyTxOut { address: vec![0x61; 29], amount: 2_000_000 }
    }

    // ── NativeScript round-trips ────────────────────────────────────────

    #[test]
    fn script_pubkey_round_trip() {
        let s = NativeScript::ScriptPubkey([0x01; 28]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_all_round_trip() {
        let s = NativeScript::ScriptAll(vec![
            NativeScript::ScriptPubkey([0x02; 28]),
            NativeScript::ScriptPubkey([0x03; 28]),
        ]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_any_round_trip() {
        let s = NativeScript::ScriptAny(vec![
            NativeScript::InvalidBefore(100),
            NativeScript::InvalidHereafter(200),
        ]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_n_of_k_round_trip() {
        let s = NativeScript::ScriptNOfK(2, vec![
            NativeScript::ScriptPubkey([0x04; 28]),
            NativeScript::ScriptPubkey([0x05; 28]),
            NativeScript::ScriptPubkey([0x06; 28]),
        ]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn invalid_before_round_trip() {
        let s = NativeScript::InvalidBefore(42);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn invalid_hereafter_round_trip() {
        let s = NativeScript::InvalidHereafter(999);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_all_empty_round_trip() {
        let s = NativeScript::ScriptAll(vec![]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn nested_scripts_round_trip() {
        let s = NativeScript::ScriptAll(vec![
            NativeScript::ScriptAny(vec![
                NativeScript::ScriptPubkey([0x07; 28]),
                NativeScript::InvalidBefore(10),
            ]),
            NativeScript::ScriptNOfK(1, vec![
                NativeScript::InvalidHereafter(50),
            ]),
        ]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn script_n_of_k_zero_threshold() {
        let s = NativeScript::ScriptNOfK(0, vec![
            NativeScript::ScriptPubkey([0x08; 28]),
        ]);
        let decoded = NativeScript::from_cbor_bytes(&s.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, s);
    }

    #[test]
    fn different_scripts_encode_differently() {
        let a = NativeScript::InvalidBefore(100);
        let b = NativeScript::InvalidHereafter(100);
        assert_ne!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }

    // ── AllegraTxBody round-trips ───────────────────────────────────────

    #[test]
    fn tx_body_minimal_round_trip() {
        let body = AllegraTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_txout()],
            fee: 200_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
        };
        let decoded = AllegraTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_ttl_round_trip() {
        let body = AllegraTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_txout()],
            fee: 150_000,
            ttl: Some(500),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
        };
        let decoded = AllegraTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_validity_interval_start_round_trip() {
        let body = AllegraTxBody {
            inputs: vec![mk_txin(1)],
            outputs: vec![mk_txout()],
            fee: 180_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: Some(100),
        };
        let decoded = AllegraTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_ttl_and_validity_start_round_trip() {
        let body = AllegraTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_txout()],
            fee: 200_000,
            ttl: Some(1000),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: Some([0xDD; 32]),
            validity_interval_start: Some(50),
        };
        let decoded = AllegraTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_optional_ttl_absent_vs_present_differ() {
        let no_ttl = AllegraTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_txout()],
            fee: 200_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
        };
        let with_ttl = AllegraTxBody {
            ttl: Some(100),
            ..no_ttl.clone()
        };
        assert_ne!(no_ttl.to_cbor_bytes(), with_ttl.to_cbor_bytes());
    }
}
