//! UTxO output builders for transaction generation.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/UTxO.hs`.
//! Ports `ToUTxO`, `ToUTxOList`, `makeToUTxOList`, and the key
//! payment-output half of `mkUTxOVariant`. The script-output half of
//! upstream `mkUTxOScript` remains tied to the later Plutus witness
//! builder slice.

use crate::script::types::{NetworkId, SigningKeyEnvelope};
use crate::tx_generator::fund::{Fund, FundWitness};
use crate::types::{AnyCardanoEra, Lovelace};
use yggdrasil_crypto::SigningKey;
use yggdrasil_ledger::{
    Address, AlonzoTxOut, BabbageTxOut, EnterpriseAddress, MaryTxOut, MultiEraTxOut, ShelleyTxOut,
    StakeCredential, Value, vkey_hash,
};

/// Mirror of upstream `TxIx`.
pub type TxIx = u16;

/// Deferred fund constructor returned beside each generated output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingFund {
    era: AnyCardanoEra,
    lovelace: Lovelace,
    key_name: String,
    witness: FundWitness,
}

impl PendingFund {
    /// Apply the generated output index and transaction id, matching
    /// upstream's `TxIx -> TxId -> Fund` closure.
    pub fn fund_for_tx_id(&self, tx_ix: TxIx, tx_id_hex: &str) -> Fund {
        match &self.witness {
            FundWitness::KeyWitnessForSpending => Fund::key_fund(
                self.era,
                format!("{tx_id_hex}#{tx_ix}"),
                self.lovelace,
                self.key_name.clone(),
            ),
            FundWitness::ScriptWitness(script) => Fund {
                era: self.era,
                tx_in: format!("{tx_id_hex}#{tx_ix}"),
                lovelace: self.lovelace,
                key_name: script.clone(),
            },
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
    key_name: String,
    signing_key: SigningKeyEnvelope,
}

impl ToUtxo {
    /// Build the era-specific transaction output and deferred fund
    /// constructor for one lovelace value.
    pub fn build(&self, value: Lovelace) -> Result<ToUtxoResult, String> {
        let address = key_address(&self.network_id, &self.signing_key)?;
        let output = tx_out_for_era(self.era, address, value)?;
        let fund = PendingFund {
            era: self.era,
            lovelace: value,
            key_name: self.key_name.clone(),
            witness: FundWitness::KeyWitnessForSpending,
        };
        Ok((output, fund))
    }

    /// Return the target era.
    pub fn era(&self) -> AnyCardanoEra {
        self.era
    }

    /// Return the wallet key name used by the generated funds.
    pub fn key_name(&self) -> &str {
        &self.key_name
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
            key_name: key_name.into(),
            signing_key,
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
