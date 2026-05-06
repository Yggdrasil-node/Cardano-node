// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use crate::cbor::CborEncode;
use crate::eras::alonzo::AlonzoTxOut;
use crate::eras::babbage::{BabbageTxOut, DatumOption};
use crate::eras::conway::{GovAction, ProposalProcedure, Voter};
use crate::eras::mary::Value;
use crate::eras::shelley::{ShelleyTxIn, ShelleyWitnessSet};
use crate::types::{Address, DRep, EnterpriseAddress, RewardAccount, StakeCredential};
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

#[test]
fn plutus_v1_script_hash_uses_tag_01() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    // Verify it's Blake2b-224 of [0x01, 0x01, 0x02, 0x03]
    let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x01, 0x01, 0x02, 0x03]).0;
    assert_eq!(hash, expected);
}

#[test]
fn plutus_v2_script_hash_uses_tag_02() {
    let script_bytes = vec![0xAA, 0xBB];
    let hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
    let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x02, 0xAA, 0xBB]).0;
    assert_eq!(hash, expected);
}

#[test]
fn plutus_v3_script_hash_uses_tag_03() {
    let script_bytes = vec![0xFF];
    let hash = plutus_script_hash(PlutusVersion::V3, &script_bytes);
    let expected = yggdrasil_crypto::blake2b::hash_bytes_224(&[0x03, 0xFF]).0;
    assert_eq!(hash, expected);
}

#[test]
fn collect_plutus_scripts_returns_all_versions() {
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![vec![0x01]],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![vec![0x02]],
        plutus_v3_scripts: vec![vec![0x03]],
    };
    let scripts = collect_plutus_scripts(&ws);
    assert_eq!(scripts.len(), 3);
    let h1 = plutus_script_hash(PlutusVersion::V1, &[0x01]);
    let h2 = plutus_script_hash(PlutusVersion::V2, &[0x02]);
    let h3 = plutus_script_hash(PlutusVersion::V3, &[0x03]);
    assert_eq!(scripts[&h1].0, PlutusVersion::V1);
    assert_eq!(scripts[&h2].0, PlutusVersion::V2);
    assert_eq!(scripts[&h3].0, PlutusVersion::V3);
}

#[test]
fn script_data_hash_uses_raw_witness_redeemers_and_datums_bytes() {
    // Witness set:
    // { 5: [_], 4: [_] } where both arrays use indefinite-length
    // encodings. Upstream hashes the memoized original bytes for
    // `Redeemers` and `TxDats`, not a canonical reconstruction.
    let witness = [0xa2, 0x05, 0x9f, 0xff, 0x04, 0x9f, 0x01, 0xff];

    let computed = compute_script_data_hash(Some(&witness), None, false, None, None, None, None)
        .expect("script data hash");

    let expected =
        yggdrasil_crypto::blake2b::hash_bytes_256(&[0x9f, 0xff, 0x9f, 0x01, 0xff, 0xa0]).0;
    let canonical_reencoded =
        yggdrasil_crypto::blake2b::hash_bytes_256(&[0x80, 0x81, 0x01, 0xa0]).0;

    assert_eq!(computed, expected);
    assert_ne!(computed, canonical_reencoded);
}

#[test]
fn script_data_hash_omits_present_empty_datums_field() {
    // Witness set with one redeemer and field 4 present as an empty datum
    // array. Upstream checks decoded `TxDats` emptiness, so the `80` bytes
    // for the empty field are not part of the script integrity preimage.
    let raw_redeemers = [0x81, 0x84, 0x00, 0x00, 0x00, 0x82, 0x01, 0x02];
    let witness = [
        0xa2, 0x05, 0x81, 0x84, 0x00, 0x00, 0x00, 0x82, 0x01, 0x02, 0x04, 0x80,
    ];

    let computed = compute_script_data_hash(Some(&witness), None, false, None, None, None, None)
        .expect("script data hash");

    let mut expected_preimage = Vec::new();
    expected_preimage.extend_from_slice(&raw_redeemers);
    expected_preimage.push(0xa0);
    let expected = yggdrasil_crypto::blake2b::hash_bytes_256(&expected_preimage).0;

    let mut wrong_preimage = Vec::new();
    wrong_preimage.extend_from_slice(&raw_redeemers);
    wrong_preimage.push(0x80);
    wrong_preimage.push(0xa0);
    let wrong = yggdrasil_crypto::blake2b::hash_bytes_256(&wrong_preimage).0;

    assert_eq!(computed, expected);
    assert_ne!(computed, wrong);
}

#[test]
fn collect_datum_map_hashes_cbor() {
    let datum = PlutusData::integer(42);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![datum.clone()],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let map = collect_datum_map(&ws);
    assert_eq!(map.len(), 1);
    let cbor = datum.to_cbor_bytes();
    let hash = yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0;
    assert_eq!(map[&hash], datum);
}

#[test]
fn collect_datum_map_hashes_raw_witness_datum_bytes() {
    // Witness set { 4: [0] }, but the datum integer 0 is encoded in
    // non-canonical uint8 form `0x18 0x00`. Upstream `TxDats` hashes
    // those memoized bytes, not the canonical re-encoding `0x00`.
    let witness = [0xa1, 0x04, 0x81, 0x18, 0x00];
    let ws = ShelleyWitnessSet::from_cbor_bytes(&witness).expect("witness set");
    let datum = PlutusData::integer(0);
    assert_eq!(ws.plutus_data, vec![datum.clone()]);

    let map = collect_datum_map_from_witness_bytes(Some(&witness), &ws).expect("datum map");
    let raw_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&[0x18, 0x00]).0;
    let canonical_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&datum.to_cbor_bytes()).0;

    assert_eq!(map[&raw_hash], datum);
    assert!(!map.contains_key(&canonical_hash));
}

#[test]
fn resolve_spending_purpose() {
    let inputs = vec![
        crate::eras::shelley::ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        },
        crate::eras::shelley::ShelleyTxIn {
            transaction_id: [0xBB; 32],
            index: 1,
        },
    ];
    let redeemer = Redeemer {
        tag: 0,
        index: 1,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };
    let purpose = resolve_script_purpose(&redeemer, &inputs, &[], &[], &[], &[], &[]).unwrap();
    assert!(matches!(
        purpose,
        ScriptPurpose::Spending { tx_id, index } if tx_id == [0xBB; 32] && index == 1
    ));
}

#[test]
fn resolve_minting_purpose() {
    let policies = vec![[0xCC; 28]];
    let redeemer = Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };
    let purpose = resolve_script_purpose(&redeemer, &[], &policies, &[], &[], &[], &[]).unwrap();
    assert!(matches!(purpose, ScriptPurpose::Minting { policy_id } if policy_id == [0xCC; 28]));
}

#[test]
fn resolve_certifying_purpose_carries_certificate() {
    let certificate = DCert::AccountRegistration(StakeCredential::ScriptHash([0xDD; 28]));
    let redeemer = Redeemer {
        tag: 2,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };

    let purpose = resolve_script_purpose(
        &redeemer,
        &[],
        &[],
        std::slice::from_ref(&certificate),
        &[],
        &[],
        &[],
    )
    .unwrap();

    assert!(matches!(
        purpose,
        ScriptPurpose::Certifying { cert_index, certificate: carried }
            if cert_index == 0 && carried == certificate
    ));
}

#[test]
fn resolve_voting_purpose_carries_voter() {
    let voter = Voter::DRepScript([0xAB; 28]);
    let redeemer = Redeemer {
        tag: 4,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };

    let purpose = resolve_script_purpose(
        &redeemer,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&voter),
        &[],
    )
    .unwrap();

    assert!(matches!(purpose, ScriptPurpose::Voting { voter: carried } if carried == voter));
}

#[test]
fn resolve_proposing_purpose_carries_procedure() {
    let proposal = ProposalProcedure {
        deposit: 5,
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xCC; 28]),
        }
        .to_bytes()
        .to_vec(),
        gov_action: GovAction::InfoAction,
        anchor: crate::types::Anchor {
            url: "https://example.invalid/proposal".to_string(),
            data_hash: [0xDD; 32],
        },
    };
    let redeemer = Redeemer {
        tag: 5,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };

    let purpose = resolve_script_purpose(
        &redeemer,
        &[],
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&proposal),
    )
    .unwrap();

    assert!(matches!(
        purpose,
        ScriptPurpose::Proposing {
            proposal_index,
            proposal: carried,
        } if proposal_index == 0 && carried == proposal
    ));
}

#[test]
fn resolve_spending_out_of_range_fails() {
    let redeemer = Redeemer {
        tag: 0,
        index: 5,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 200,
        },
    };
    let err = resolve_script_purpose(&redeemer, &[], &[], &[], &[], &[], &[]).unwrap_err();
    assert!(matches!(err, LedgerError::MissingRedeemer { .. }));
}

/// Mock evaluator that always succeeds.
struct AlwaysSucceeds;

impl PlutusEvaluator for AlwaysSucceeds {
    fn evaluate(&self, _eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
        Ok(())
    }
}

/// Mock evaluator that always fails.
struct AlwaysFails;

impl PlutusEvaluator for AlwaysFails {
    fn evaluate(&self, eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
        Err(LedgerError::PlutusScriptFailed {
            hash: eval.script_hash,
            reason: "always fails".to_string(),
        })
    }
}

struct ExpectDatum(pub PlutusData);

impl PlutusEvaluator for ExpectDatum {
    fn evaluate(&self, eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
        assert_eq!(eval.datum, Some(self.0.clone()));
        Ok(())
    }
}

#[test]
fn validate_plutus_scripts_skips_without_evaluator() {
    use std::collections::HashSet;
    // Even with required scripts, None evaluator means soft-skip.
    let mut required = HashSet::new();
    required.insert([0xAA; 28]);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![vec![0x01]],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();
    let utxo = MultiEraUtxo::new();
    let result = validate_plutus_scripts(
        None,
        Some(&wb),
        &required,
        &utxo,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn validate_minting_script_with_mock_evaluator() {
    use std::collections::HashSet;
    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let mut required = HashSet::new();
    required.insert(policy_hash);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 1, // minting
            index: 0,
            data: PlutusData::integer(42),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();
    let utxo = MultiEraUtxo::new();
    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&wb),
        &required,
        &utxo,
        &[],
        &[policy_hash],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn validate_minting_script_fails_with_rejecting_evaluator() {
    use std::collections::HashSet;
    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let mut required = HashSet::new();
    required.insert(policy_hash);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::integer(42),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();
    let utxo = MultiEraUtxo::new();
    let result = validate_plutus_scripts(
        Some(&AlwaysFails),
        Some(&wb),
        &required,
        &utxo,
        &[],
        &[policy_hash],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );
    assert!(matches!(
        result.unwrap_err(),
        LedgerError::PlutusScriptFailed { hash, .. } if hash == policy_hash
    ));
}

#[test]
fn validate_minting_script_missing_redeemer_fails_before_evaluation() {
    use std::collections::HashSet;

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let mut required = HashSet::new();
    required.insert(policy_hash);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();
    let utxo = MultiEraUtxo::new();

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&wb),
        &required,
        &utxo,
        &[],
        &[policy_hash],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(matches!(
        result,
        Err(LedgerError::MissingRedeemer { hash, purpose })
            if hash == policy_hash && purpose == "minting index 0"
    ));
}

#[test]
fn validate_plutus_scripts_empty_required_set_is_noop() {
    let required = std::collections::HashSet::new();
    let utxo = MultiEraUtxo::new();
    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        None,
        &required,
        &utxo,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn validate_spending_script_resolves_alonzo_datum_hash() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let datum = PlutusData::integer(99);
    let datum_hash = yggdrasil_crypto::blake2b::hash_bytes_256(&datum.to_cbor_bytes()).0;
    let txin = ShelleyTxIn {
        transaction_id: [0xAB; 32],
        index: 0,
    };

    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![datum.clone()],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::integer(42),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();

    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes();
    let mut utxo = MultiEraUtxo::new();
    utxo.insert(
        txin.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(1),
            datum_hash: Some(datum_hash),
        }),
    );

    let result = validate_plutus_scripts(
        Some(&ExpectDatum(datum)),
        Some(&wb),
        &required,
        &utxo,
        &[txin],
        &[],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn validate_spending_script_uses_inline_babbage_datum() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
    let datum = PlutusData::integer(7);
    let txin = ShelleyTxIn {
        transaction_id: [0xCD; 32],
        index: 1,
    };

    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::integer(1),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();

    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes();
    let mut utxo = MultiEraUtxo::new();
    utxo.insert(
        txin.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address,
            amount: Value::Coin(1),
            datum_option: Some(DatumOption::Inline(datum.clone())),
            script_ref: None,
        }),
    );

    let result = validate_plutus_scripts(
        Some(&ExpectDatum(datum)),
        Some(&wb),
        &required,
        &utxo,
        &[txin],
        &[],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn validate_spending_script_fails_when_datum_hash_missing_from_witnesses() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let txin = ShelleyTxIn {
        transaction_id: [0xEF; 32],
        index: 2,
    };

    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::integer(0),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();

    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes();
    let mut utxo = MultiEraUtxo::new();
    utxo.insert(
        txin.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(1),
            datum_hash: Some([0x44; 32]),
        }),
    );

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&wb),
        &required,
        &utxo,
        &[txin],
        &[],
        &[],
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(matches!(
        result,
        Err(LedgerError::MissingDatum { tx_id, index }) if tx_id == [0xEF; 32] && index == 2
    ));
}

#[test]
fn validate_certifying_script_resolves_drep_script_hash() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 2,
            index: 0,
            data: PlutusData::integer(5),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let certs = vec![DCert::DelegationToDrep(
        StakeCredential::AddrKeyHash([0x11; 28]),
        DRep::ScriptHash(script_hash),
    )];
    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);
    let utxo = MultiEraUtxo::new();

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&ws.to_cbor_bytes()),
        &required,
        &utxo,
        &[],
        &[],
        &certs,
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn validate_certifying_script_skips_legacy_registration_without_redeemer() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 2,
            index: 1,
            data: PlutusData::integer(0),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let certs = vec![
        DCert::AccountRegistration(StakeCredential::ScriptHash(script_hash)),
        DCert::DelegationToStakePool(StakeCredential::ScriptHash(script_hash), [0x44; 28]),
    ];
    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);
    let utxo = MultiEraUtxo::new();

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&ws.to_cbor_bytes()),
        &required,
        &utxo,
        &[],
        &[],
        &certs,
        &[],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn validate_rewarding_script_requires_script_reward_account() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);
    let reward_account = RewardAccount {
        network: 1,
        credential: StakeCredential::ScriptHash(script_hash),
    };
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 3,
            index: 0,
            data: PlutusData::integer(8),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);
    let utxo = MultiEraUtxo::new();

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&ws.to_cbor_bytes()),
        &required,
        &utxo,
        &[],
        &[],
        &[],
        &[reward_account.to_bytes().to_vec()],
        &[],
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn validate_voting_script_resolves_script_voter_hash() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V3, &script_bytes);
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 4,
            index: 0,
            data: PlutusData::integer(9),
            ex_units: ExUnits {
                mem: 1000,
                steps: 2000,
            },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![script_bytes],
    };
    let mut required = std::collections::HashSet::new();
    required.insert(script_hash);
    let utxo = MultiEraUtxo::new();
    let voters = vec![Voter::DRepScript(script_hash)];

    let result = validate_plutus_scripts(
        Some(&AlwaysSucceeds),
        Some(&ws.to_cbor_bytes()),
        &required,
        &utxo,
        &[],
        &[],
        &[],
        &[],
        &voters,
        &[],
        &TxContext::default(),
        None,
    );

    assert!(result.is_ok());
}

// -----------------------------------------------------------------------
// Language view encoding parity tests
// -----------------------------------------------------------------------

/// Helper: produce language-views encoding for a witness set containing
/// one dummy Plutus script of the given version, with protocol params
/// carrying the given cost model values for that language.
fn encode_views_for_single_lang(version: PlutusVersion, cm_values: &[i64]) -> Vec<u8> {
    let script = vec![0xDE, 0xAD]; // dummy script bytes
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: if version == PlutusVersion::V1 {
            vec![script.clone()]
        } else {
            vec![]
        },
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: if version == PlutusVersion::V2 {
            vec![script.clone()]
        } else {
            vec![]
        },
        plutus_v3_scripts: if version == PlutusVersion::V3 {
            vec![script]
        } else {
            vec![]
        },
    };
    let mut pp = ProtocolParameters::default();
    let mut cm_map = std::collections::BTreeMap::new();
    cm_map.insert(version.cost_model_key(), cm_values.to_vec());
    pp.cost_models = Some(cm_map);
    encode_language_views_for_script_data_hash(&ws, Some(&pp), None, None, None, None)
}

#[test]
fn v1_language_view_key_is_byte_string() {
    let cm = vec![1i64, 2, 3];
    let bytes = encode_views_for_single_lang(PlutusVersion::V1, &cm);
    // Map(1) { bytes(1, [0x00]) => bytes(...) }
    // 0xa1 = map(1)
    assert_eq!(bytes[0], 0xa1);
    // Key: 0x41 0x00 = byte string of length 1 containing [0x00]
    assert_eq!(bytes[1], 0x41);
    assert_eq!(bytes[2], 0x00);
    // Value starts with a byte string header (major type 2)
    assert!(
        (bytes[3] & 0xe0) == 0x40,
        "V1 value should be a CBOR byte string"
    );
}

#[test]
fn v2_language_view_key_is_unsigned_int() {
    let cm = vec![10i64, 20, 30];
    let bytes = encode_views_for_single_lang(PlutusVersion::V2, &cm);
    // Map(1) { unsigned(1) => array(...) }
    assert_eq!(bytes[0], 0xa1);
    // Key: 0x01 = CBOR unsigned integer 1
    assert_eq!(bytes[1], 0x01);
    // Value starts with an array header (major type 4), NOT byte string
    assert!(
        (bytes[2] & 0xe0) == 0x80,
        "V2 value should be a CBOR array, not byte string"
    );
}

#[test]
fn v3_language_view_key_is_unsigned_int() {
    let cm = vec![100i64];
    let bytes = encode_views_for_single_lang(PlutusVersion::V3, &cm);
    // Map(1) { unsigned(2) => array(...) }
    assert_eq!(bytes[0], 0xa1);
    // Key: 0x02 = CBOR unsigned integer 2
    assert_eq!(bytes[1], 0x02);
    // Value starts with an array header (major type 4)
    assert!((bytes[2] & 0xe0) == 0x80, "V3 value should be a CBOR array");
}

#[test]
fn v1_cost_model_uses_indefinite_array() {
    let cm = vec![5i64, 10];
    let bytes = encode_views_for_single_lang(PlutusVersion::V1, &cm);
    // After map header (0xa1) + key (0x41, 0x00) + byte-string header,
    // the byte-string payload should start with 0x9f (indefinite array)
    // and end with 0xff (break).
    // Map(1) = 0xa1, key = 0x41 0x00, value = bytes(N, payload)
    // Skip to byte-string payload:
    let value_start = 3; // after map header + key
    // Decode byte string header to find payload start
    let major = bytes[value_start] >> 5;
    assert_eq!(major, 2, "value should be byte string");
    // Additional info tells length
    let info = bytes[value_start] & 0x1f;
    let (payload_start, _payload_len) = match info {
        0..=23 => (value_start + 1, info as usize),
        24 => (value_start + 2, bytes[value_start + 1] as usize),
        _ => panic!("unexpected byte string length encoding"),
    };
    // First byte of payload should be indefinite array start
    assert_eq!(
        bytes[payload_start], 0x9f,
        "V1 cost model should use indefinite array"
    );
    // Last byte should be break
    assert_eq!(
        *bytes.last().unwrap(),
        0xff,
        "V1 cost model should end with break"
    );
}

#[test]
fn v2_cost_model_uses_definite_array() {
    let cm = vec![7i64, 8, 9];
    let bytes = encode_views_for_single_lang(PlutusVersion::V2, &cm);
    // After map header (0xa1) + key (0x01), value starts directly
    let value_start = 2;
    let major = bytes[value_start] >> 5;
    assert_eq!(major, 4, "V2 cost model should be a definite array");
}

#[test]
fn mixed_v1_v2_ordering_follows_shortlex() {
    // When both V1 and V2 are present, V2 key (0x01, 1 byte) should
    // come BEFORE V1 key (0x41 0x00, 2 bytes) per upstream shortLex.
    let script = vec![0xDE, 0xAD];
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script.clone()],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![script],
        plutus_v3_scripts: vec![],
    };
    let mut pp = ProtocolParameters::default();
    let mut cm_map = std::collections::BTreeMap::new();
    cm_map.insert(0u8, vec![1i64, 2]);
    cm_map.insert(1u8, vec![3i64, 4]);
    pp.cost_models = Some(cm_map);

    let bytes = encode_language_views_for_script_data_hash(&ws, Some(&pp), None, None, None, None);
    // Map(2) = 0xa2
    assert_eq!(bytes[0], 0xa2);
    // First key: V2 = 0x01 (unsigned int 1, 1 byte) — shorter
    assert_eq!(bytes[1], 0x01);
    // Scan past V2 value (definite array) to find second key
    // Second key should be V1 = 0x41 0x00 (byte string, 2 bytes)
    let mut pos = 2;
    // Skip V2 value: definite array of 2 elements
    // 0x82 = array(2), then two integers
    assert_eq!(bytes[pos] >> 5, 4, "V2 value should be array");
    let arr_len = (bytes[pos] & 0x1f) as usize;
    pos += 1;
    for _ in 0..arr_len {
        // Skip each integer (could be 1 byte for small values)
        match bytes[pos] {
            0..=23 => pos += 1,
            24 => pos += 2,
            _ => panic!("test values should be small"),
        }
    }
    // Now we should be at V1 key
    assert_eq!(bytes[pos], 0x41, "second key should be V1 byte string");
    assert_eq!(bytes[pos + 1], 0x00, "second key payload should be 0x00");
}

#[test]
fn collect_scripts_includes_spending_input_reference_scripts() {
    // Upstream `getBabbageScriptsProvided` uses
    // `referenceInputsTxBodyL ∪ inputsTxBodyL` — scripts from
    // spending-input UTxOs should be collected, not just reference inputs.
    use crate::eras::babbage::BabbageTxOut;
    use crate::eras::mary::Value;
    use crate::eras::shelley::ShelleyTxIn;
    use crate::plutus::{Script, ScriptRef};
    use crate::utxo::MultiEraTxOut;
    use crate::utxo::MultiEraUtxo;

    let script_bytes = vec![0xAA, 0xBB, 0xCC];
    let script_hash = plutus_script_hash(PlutusVersion::V2, &script_bytes);

    let spending_input = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };

    let mut utxo = MultiEraUtxo::new();
    utxo.insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV2(script_bytes.clone()))),
        }),
    );

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };

    // Without spending_inputs — script should NOT be found
    let scripts_without = collect_all_plutus_scripts(&ws, &utxo, None, None);
    assert!(
        !scripts_without.contains_key(&script_hash),
        "should not find spending-input ref script when spending_inputs=None",
    );

    // With spending_inputs — script should be found
    let scripts_with = collect_all_plutus_scripts(&ws, &utxo, None, Some(&[spending_input]));
    assert!(
        scripts_with.contains_key(&script_hash),
        "should find spending-input reference script when spending_inputs provided",
    );
    let (version, bytes) = scripts_with.get(&script_hash).unwrap();
    assert_eq!(*version, PlutusVersion::V2);
    assert_eq!(bytes, &script_bytes);
}

/// Phase-1 check: Plutus-locked spending inputs whose datum hash is
/// not in the witness datum map must be rejected with
/// `MissingRequiredDatums` before script evaluation (not Phase-2).
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`.
#[test]
fn validate_supplemental_datums_rejects_missing_required_datum() {
    let script_bytes = vec![0x01, 0x02, 0x03];
    let script_hash = plutus_script_hash(PlutusVersion::V1, &script_bytes);

    let txin = ShelleyTxIn {
        transaction_id: [0xAB; 32],
        index: 0,
    };
    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes();

    let mut utxo = MultiEraUtxo::new();
    let datum_hash = [0x99; 32];
    utxo.insert(
        txin.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(5_000_000),
            datum_hash: Some(datum_hash),
        }),
    );

    // Witness set with the PlutusV1 script but NO datum entries.
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let wb = ws.to_cbor_bytes();

    let result = validate_supplemental_datums(Some(&wb), &utxo, &[txin], &[], &[]);
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredDatums { hash }) if hash == datum_hash),
        "must reject with MissingRequiredDatums when datum hash not in witness set, got: {:?}",
        result,
    );
}
