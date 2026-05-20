//! UTxO output builders for transaction generation.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/UTxO.hs`.
//! Ports `ToUTxO`, `ToUTxOList`, `makeToUTxOList`, `mkUTxOVariant`,
//! and `mkUTxOScript`.

use crate::script::types::{NetworkId, SigningKeyEnvelope};
use crate::tx_generator::fund::{Fund, FundWitness};
use crate::types::{AnyCardanoEra, Lovelace};
use yggdrasil_crypto::{SigningKey, hash_bytes_256};
use yggdrasil_ledger::{
    Address, AlonzoTxOut, BabbageTxOut, CborEncode, DatumOption, EnterpriseAddress, MaryTxOut,
    MultiEraTxOut, PlutusData, ShelleyTxOut, StakeCredential, Value,
    plutus_validation::{PlutusVersion, plutus_script_hash},
    vkey_hash,
};

/// Mirror of upstream `TxIx`.
pub type TxIx = u16;

/// Rust carrier for upstream `ScriptInAnyLang` language tags used by
/// `mkUTxOScript`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptLanguage {
    /// Upstream `PlutusScriptV1`.
    PlutusV1,
    /// Upstream `PlutusScriptV2`.
    PlutusV2,
    /// Upstream `PlutusScriptV3`.
    PlutusV3,
}

impl ScriptLanguage {
    fn plutus_version(self) -> PlutusVersion {
        match self {
            Self::PlutusV1 => PlutusVersion::V1,
            Self::PlutusV2 => PlutusVersion::V2,
            Self::PlutusV3 => PlutusVersion::V3,
        }
    }
}

/// Rust carrier for upstream `ScriptInAnyLang`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptInAnyLang {
    /// Plutus language tag.
    pub language: ScriptLanguage,
    /// Raw serialised Plutus script bytes.
    pub bytes: Vec<u8>,
}

impl ScriptInAnyLang {
    /// Construct a Plutus script in an upstream language wrapper.
    pub fn new(language: ScriptLanguage, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            language,
            bytes: bytes.into(),
        }
    }
}

/// Deferred fund constructor returned beside each generated output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingFund {
    era: AnyCardanoEra,
    lovelace: Lovelace,
    signing_key: Option<String>,
    witness: FundWitness,
}

impl PendingFund {
    /// Apply the generated output index and transaction id, matching
    /// upstream's `TxIx -> TxId -> Fund` closure.
    pub fn fund_for_tx_id(&self, tx_ix: TxIx, tx_id_hex: &str) -> Fund {
        let tx_in = format!("{tx_id_hex}#{tx_ix}");
        match &self.witness {
            FundWitness::KeyWitnessForSpending => Fund::key_fund(
                self.era,
                tx_in,
                self.lovelace,
                self.signing_key
                    .clone()
                    .expect("key witnessed PendingFund always stores a signing key"),
            ),
            FundWitness::ScriptWitness(_) => {
                Fund::script_fund(self.era, tx_in, self.lovelace, self.witness.clone())
            }
        }
    }
}

/// Result returned by one `ToUtxo` builder invocation.
pub type ToUtxoResult = (MultiEraTxOut, PendingFund);

/// Rust carrier for upstream `ToUTxO era`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToUtxo {
    era: AnyCardanoEra,
    network_id: NetworkId,
    target: ToUtxoTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ToUtxoTarget {
    Key {
        key_name: String,
        signing_key: SigningKeyEnvelope,
    },
    Script {
        script: ScriptInAnyLang,
        datum: PlutusData,
        witness: FundWitness,
    },
}

impl ToUtxo {
    /// Build the era-specific transaction output and deferred fund
    /// constructor for one lovelace value.
    pub fn build(&self, value: Lovelace) -> Result<ToUtxoResult, String> {
        match &self.target {
            ToUtxoTarget::Key {
                key_name,
                signing_key,
            } => {
                let address = key_address(&self.network_id, signing_key)?;
                let output = tx_out_for_era(self.era, address, value)?;
                let fund = PendingFund {
                    era: self.era,
                    lovelace: value,
                    signing_key: Some(key_name.clone()),
                    witness: FundWitness::KeyWitnessForSpending,
                };
                Ok((output, fund))
            }
            ToUtxoTarget::Script {
                script,
                datum,
                witness,
            } => {
                let output =
                    script_tx_out_for_era(self.era, &self.network_id, script, datum, value)?;
                let fund = PendingFund {
                    era: self.era,
                    lovelace: value,
                    signing_key: None,
                    witness: witness.clone(),
                };
                Ok((output, fund))
            }
        }
    }

    /// Return the target era.
    pub fn era(&self) -> AnyCardanoEra {
        self.era
    }

    /// Return the wallet key name used by generated key-witnessed funds.
    pub fn key_name(&self) -> Option<&str> {
        match &self.target {
            ToUtxoTarget::Key { key_name, .. } => Some(key_name),
            ToUtxoTarget::Script { .. } => None,
        }
    }
}

/// Result returned by upstream-shaped `makeToUTxOList`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToUtxoList {
    /// Generated outputs in transaction order.
    pub outputs: Vec<MultiEraTxOut>,
    pending_funds: Vec<(TxIx, PendingFund)>,
}

impl ToUtxoList {
    /// Apply a transaction id to every deferred fund constructor.
    pub fn funds_for_tx_id(&self, tx_id_hex: &str) -> Vec<Fund> {
        self.pending_funds
            .iter()
            .map(|(tx_ix, pending)| pending.fund_for_tx_id(*tx_ix, tx_id_hex))
            .collect()
    }
}

/// Mirror of upstream `makeToUTxOList`.
///
/// Haskell uses `zip3 fkts values [TxIx 0 ..]`, so this intentionally
/// truncates to the shortest of the builder and value lists.
pub fn make_to_utxo_list(builders: &[ToUtxo], values: &[Lovelace]) -> Result<ToUtxoList, String> {
    let mut outputs = Vec::with_capacity(builders.len().min(values.len()));
    let mut pending_funds = Vec::with_capacity(outputs.capacity());

    for (idx, (builder, value)) in builders.iter().zip(values).enumerate() {
        let tx_ix = TxIx::try_from(idx).map_err(|_| "makeToUTxOList: TxIx overflow".to_string())?;
        let (output, pending) = builder.build(*value)?;
        outputs.push(output);
        pending_funds.push((tx_ix, pending));
    }

    Ok(ToUtxoList {
        outputs,
        pending_funds,
    })
}

/// Mirror of upstream `mkUTxOVariant`.
pub fn mk_utxo_variant(
    era: AnyCardanoEra,
    network_id: NetworkId,
    key_name: impl Into<String>,
    signing_key: SigningKeyEnvelope,
) -> Result<ToUtxo, String> {
    match era {
        AnyCardanoEra::Byron => Err("mkUTxOVariant: byron not supported".to_string()),
        _ => Ok(ToUtxo {
            era,
            network_id,
            target: ToUtxoTarget::Key {
                key_name: key_name.into(),
                signing_key,
            },
        }),
    }
}

/// Mirror of upstream `mkUTxOScript`.
pub fn mk_utxo_script(
    era: AnyCardanoEra,
    network_id: NetworkId,
    script: ScriptInAnyLang,
    datum: PlutusData,
    witness: FundWitness,
) -> Result<ToUtxo, String> {
    match era {
        AnyCardanoEra::Byron => Err("mkUtxOScript: scriptDataSupportedInEra==Nothing".to_string()),
        _ => Ok(ToUtxo {
            era,
            network_id,
            target: ToUtxoTarget::Script {
                script,
                datum,
                witness,
            },
        }),
    }
}

/// Mirror of upstream `keyAddress` as consumed by `mkUTxOVariant`.
pub fn key_address(
    network_id: &NetworkId,
    signing_key: &SigningKeyEnvelope,
) -> Result<Vec<u8>, String> {
    let seed = signing_key_seed(signing_key)?;
    let verification_key = SigningKey::from_bytes(seed)
        .verification_key()
        .map_err(|err| format!("keyAddress: verification key derivation failed: {err}"))?;
    let payment = StakeCredential::AddrKeyHash(vkey_hash(&verification_key.to_bytes()));
    Ok(Address::Enterprise(EnterpriseAddress {
        network: address_network_id(network_id),
        payment,
    })
    .to_bytes())
}

/// Mirror of the script-payment address built by upstream `mkUTxOScript`.
pub fn script_address(network_id: &NetworkId, script: &ScriptInAnyLang) -> Vec<u8> {
    let script_hash = script_hash(script);
    Address::Enterprise(EnterpriseAddress {
        network: address_network_id(network_id),
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes()
}

/// Mirror of upstream `hashScript` for Plutus scripts.
pub fn script_hash(script: &ScriptInAnyLang) -> [u8; 28] {
    plutus_script_hash(script.language.plutus_version(), &script.bytes)
}

/// Mirror of upstream `hashScriptDataBytes . unsafeHashableScriptData`.
pub fn script_data_hash(datum: &PlutusData) -> [u8; 32] {
    hash_bytes_256(&datum.to_cbor_bytes()).0
}

fn tx_out_for_era(
    era: AnyCardanoEra,
    address: Vec<u8>,
    lovelace: Lovelace,
) -> Result<MultiEraTxOut, String> {
    match era {
        AnyCardanoEra::Byron => Err("mkUTxOVariant: byron not supported".to_string()),
        AnyCardanoEra::Shelley | AnyCardanoEra::Allegra => {
            Ok(MultiEraTxOut::Shelley(ShelleyTxOut {
                address,
                amount: lovelace,
            }))
        }
        AnyCardanoEra::Mary => Ok(MultiEraTxOut::Mary(MaryTxOut {
            address,
            amount: Value::Coin(lovelace),
        })),
        AnyCardanoEra::Alonzo => Ok(MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(lovelace),
            datum_hash: None,
        })),
        AnyCardanoEra::Babbage | AnyCardanoEra::Conway | AnyCardanoEra::Dijkstra => {
            Ok(MultiEraTxOut::Babbage(BabbageTxOut {
                address,
                amount: Value::Coin(lovelace),
                datum_option: None,
                script_ref: None,
            }))
        }
    }
}

fn script_tx_out_for_era(
    era: AnyCardanoEra,
    network_id: &NetworkId,
    script: &ScriptInAnyLang,
    datum: &PlutusData,
    lovelace: Lovelace,
) -> Result<MultiEraTxOut, String> {
    if !script_data_supported_in_era(era) {
        return Err("mkUtxOScript: scriptDataSupportedInEra==Nothing".to_string());
    }
    if !script_language_supported_in_era(era, script.language) {
        return Err("mkUtxOScript: scriptLanguageSupportedInEra==Nothing".to_string());
    }

    let address = script_address(network_id, script);
    let datum_hash = script_data_hash(datum);
    match era {
        AnyCardanoEra::Alonzo => Ok(MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(lovelace),
            datum_hash: Some(datum_hash),
        })),
        AnyCardanoEra::Babbage | AnyCardanoEra::Conway | AnyCardanoEra::Dijkstra => {
            Ok(MultiEraTxOut::Babbage(BabbageTxOut {
                address,
                amount: Value::Coin(lovelace),
                datum_option: Some(DatumOption::Hash(datum_hash)),
                script_ref: None,
            }))
        }
        AnyCardanoEra::Byron
        | AnyCardanoEra::Shelley
        | AnyCardanoEra::Allegra
        | AnyCardanoEra::Mary => Err("mkUtxOScript: scriptDataSupportedInEra==Nothing".to_string()),
    }
}

fn script_data_supported_in_era(era: AnyCardanoEra) -> bool {
    matches!(
        era,
        AnyCardanoEra::Alonzo
            | AnyCardanoEra::Babbage
            | AnyCardanoEra::Conway
            | AnyCardanoEra::Dijkstra
    )
}

fn script_language_supported_in_era(era: AnyCardanoEra, language: ScriptLanguage) -> bool {
    match language {
        ScriptLanguage::PlutusV1 => matches!(
            era,
            AnyCardanoEra::Alonzo
                | AnyCardanoEra::Babbage
                | AnyCardanoEra::Conway
                | AnyCardanoEra::Dijkstra
        ),
        ScriptLanguage::PlutusV2 => matches!(
            era,
            AnyCardanoEra::Babbage | AnyCardanoEra::Conway | AnyCardanoEra::Dijkstra
        ),
        ScriptLanguage::PlutusV3 => {
            matches!(era, AnyCardanoEra::Conway | AnyCardanoEra::Dijkstra)
        }
    }
}

fn address_network_id(network_id: &NetworkId) -> u8 {
    match network_id {
        NetworkId::Mainnet => 1,
        NetworkId::Testnet(_) => 0,
    }
}

fn signing_key_seed(signing_key: &SigningKeyEnvelope) -> Result<[u8; 32], String> {
    if !signing_key
        .envelope_type
        .contains("PaymentSigningKeyShelley_ed25519")
    {
        return Err(format!(
            "keyAddress: expected PaymentSigningKeyShelley_ed25519 envelope, got {}",
            signing_key.envelope_type
        ));
    }

    let bytes = hex::decode(signing_key.cbor_hex.trim())
        .map_err(|err| format!("keyAddress: cborHex is not valid hex: {err}"))?;
    if bytes.len() != 34 {
        return Err(format!(
            "keyAddress: expected 34 bytes of cborHex (2-byte CBOR prefix + 32-byte key), got {}",
            bytes.len()
        ));
    }
    if bytes[0] != 0x58 || bytes[1] != 0x20 {
        return Err("keyAddress: expected CBOR bytes-32 prefix 0x5820".to_string());
    }

    bytes[2..]
        .try_into()
        .map_err(|_| "keyAddress: expected 32-byte signing key payload".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_generator::fund::{get_fund_key, get_fund_witness};

    const TX_ID: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn signing_key(byte: u8) -> SigningKeyEnvelope {
        SigningKeyEnvelope::payment_signing_key_shelley(format!("5820{}", hex::encode([byte; 32])))
    }

    #[test]
    fn mk_utxo_variant_builds_conway_enterprise_key_output_and_fund() {
        let builder = mk_utxo_variant(
            AnyCardanoEra::Conway,
            NetworkId::Testnet(42),
            "pay-key",
            signing_key(7),
        )
        .expect("builder");

        let (output, pending) = builder.build(1_234_567).expect("output");

        assert!(matches!(output, MultiEraTxOut::Babbage(_)));
        assert_eq!(output.coin(), 1_234_567);
        assert_eq!(output.address().len(), 29);
        assert_eq!(output.address()[0], 0x60);

        let fund = pending.fund_for_tx_id(3, TX_ID);
        assert_eq!(fund.era, AnyCardanoEra::Conway);
        assert_eq!(fund.tx_in, format!("{TX_ID}#3"));
        assert_eq!(fund.lovelace, 1_234_567);
        assert_eq!(fund.key_name, "pay-key");
        assert_eq!(get_fund_key(&fund), Some("pay-key"));
    }

    #[test]
    fn key_address_uses_mainnet_low_nibble() {
        let address = key_address(&NetworkId::Mainnet, &signing_key(9)).expect("address");

        assert_eq!(address.len(), 29);
        assert_eq!(address[0], 0x61);
    }

    #[test]
    fn output_shape_tracks_era_family() {
        let key = signing_key(11);
        for (era, expected_tag) in [
            (AnyCardanoEra::Shelley, 0),
            (AnyCardanoEra::Allegra, 0),
            (AnyCardanoEra::Mary, 1),
            (AnyCardanoEra::Alonzo, 2),
            (AnyCardanoEra::Babbage, 3),
            (AnyCardanoEra::Conway, 3),
            (AnyCardanoEra::Dijkstra, 3),
        ] {
            let builder =
                mk_utxo_variant(era, NetworkId::Testnet(0), "pay-key", key.clone()).expect("era");
            let (output, _) = builder.build(10).expect("output");
            let tag = match output {
                MultiEraTxOut::Shelley(_) => 0,
                MultiEraTxOut::Mary(_) => 1,
                MultiEraTxOut::Alonzo(_) => 2,
                MultiEraTxOut::Babbage(_) => 3,
            };
            assert_eq!(tag, expected_tag, "era {era:?}");
        }
    }

    #[test]
    fn make_to_utxo_list_zips_values_and_assigns_indexes() {
        let builders = vec![
            mk_utxo_variant(
                AnyCardanoEra::Conway,
                NetworkId::Testnet(0),
                "key-a",
                signing_key(1),
            )
            .expect("a"),
            mk_utxo_variant(
                AnyCardanoEra::Conway,
                NetworkId::Testnet(0),
                "key-b",
                signing_key(2),
            )
            .expect("b"),
        ];

        let list = make_to_utxo_list(&builders, &[10, 20, 30]).expect("list");

        assert_eq!(list.outputs.len(), 2);
        assert_eq!(
            list.funds_for_tx_id(TX_ID)
                .into_iter()
                .map(|fund| (fund.tx_in, fund.lovelace, fund.key_name))
                .collect::<Vec<_>>(),
            vec![
                (format!("{TX_ID}#0"), 10, "key-a".to_string()),
                (format!("{TX_ID}#1"), 20, "key-b".to_string())
            ]
        );
    }

    #[test]
    fn mk_utxo_script_builds_alonzo_script_output_and_fund() {
        let datum = PlutusData::integer(0);
        let script = ScriptInAnyLang::new(ScriptLanguage::PlutusV1, [0x01, 0x02, 0x03]);
        let builder = mk_utxo_script(
            AnyCardanoEra::Alonzo,
            NetworkId::Testnet(42),
            script.clone(),
            datum.clone(),
            FundWitness::ScriptWitness("validator".to_string()),
        )
        .expect("builder");

        let (output, pending) = builder.build(2_000_000).expect("script output");

        assert!(matches!(output, MultiEraTxOut::Alonzo(_)));
        assert_eq!(output.coin(), 2_000_000);
        assert_eq!(output.address().len(), 29);
        assert_eq!(output.address()[0], 0x70);
        assert_eq!(&output.address()[1..29], script_hash(&script).as_slice());
        assert_eq!(output.datum_hash(), Some(script_data_hash(&datum)));

        let fund = pending.fund_for_tx_id(0, TX_ID);
        assert_eq!(fund.era, AnyCardanoEra::Alonzo);
        assert_eq!(fund.tx_in, format!("{TX_ID}#0"));
        assert_eq!(fund.lovelace, 2_000_000);
        assert_eq!(get_fund_key(&fund), None);
        assert_eq!(
            get_fund_witness(AnyCardanoEra::Alonzo, &fund),
            Ok(FundWitness::ScriptWitness("validator".to_string()))
        );
        assert_eq!(fund.fund_in_era().fund_signing_key, None);
    }

    #[test]
    fn mk_utxo_script_builds_babbage_datum_hash_output() {
        let datum = PlutusData::Bytes(vec![0xaa, 0xbb]);
        let script = ScriptInAnyLang::new(ScriptLanguage::PlutusV2, [0x04, 0x05]);
        let builder = mk_utxo_script(
            AnyCardanoEra::Babbage,
            NetworkId::Mainnet,
            script,
            datum.clone(),
            FundWitness::ScriptWitness("validator".to_string()),
        )
        .expect("builder");

        let (output, _) = builder.build(3_000_000).expect("script output");

        assert!(matches!(output, MultiEraTxOut::Babbage(_)));
        assert_eq!(output.address()[0], 0x71);
        assert_eq!(output.datum_hash(), Some(script_data_hash(&datum)));
    }

    #[test]
    fn mk_utxo_script_rejects_unsupported_data_and_language_eras() {
        let datum = PlutusData::integer(0);
        let script_v1 = ScriptInAnyLang::new(ScriptLanguage::PlutusV1, [0x01]);
        let shelley = mk_utxo_script(
            AnyCardanoEra::Shelley,
            NetworkId::Testnet(0),
            script_v1,
            datum.clone(),
            FundWitness::ScriptWitness("validator".to_string()),
        )
        .expect("builder");

        assert_eq!(
            shelley.build(1).expect_err("unsupported data era"),
            "mkUtxOScript: scriptDataSupportedInEra==Nothing"
        );

        let script_v2 = ScriptInAnyLang::new(ScriptLanguage::PlutusV2, [0x02]);
        let alonzo = mk_utxo_script(
            AnyCardanoEra::Alonzo,
            NetworkId::Testnet(0),
            script_v2,
            datum,
            FundWitness::ScriptWitness("validator".to_string()),
        )
        .expect("builder");

        assert_eq!(
            alonzo.build(1).expect_err("unsupported language era"),
            "mkUtxOScript: scriptLanguageSupportedInEra==Nothing"
        );
    }

    #[test]
    fn key_address_rejects_malformed_signing_key_envelope() {
        let bad = SigningKeyEnvelope::payment_signing_key_shelley("5820abcd");
        let err = key_address(&NetworkId::Testnet(0), &bad).expect_err("bad key");

        assert!(err.contains("expected 34 bytes"));
    }

    #[test]
    fn mk_utxo_variant_rejects_byron() {
        assert_eq!(
            mk_utxo_variant(
                AnyCardanoEra::Byron,
                NetworkId::Testnet(0),
                "key",
                signing_key(3)
            ),
            Err("mkUTxOVariant: byron not supported".to_string())
        );
    }
}
