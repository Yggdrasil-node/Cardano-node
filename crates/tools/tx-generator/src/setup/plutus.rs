//! Plutus script loading helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Setup/Plutus.hs`.
//! Ports `readPlutusScript` text-envelope loading and
//! `preExecutePlutusScript` execution-unit pre-running for the
//! pure-Rust tx-generator.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yggdrasil_ledger::{Decoder, PlutusData, ProtocolParameters};
use yggdrasil_plutus::{
    CekMachine, Constant, ExBudget, Term, decode_script_bytes,
    decode_script_bytes_allowing_remainder,
};

use crate::tx_generator::utxo::{ScriptInAnyLang, ScriptLanguage};
use crate::types::{ExecutionUnits, PlutusScriptRef, TxGenPlutusResolvedTo};

#[derive(Deserialize)]
struct TextEnvelope {
    #[serde(rename = "type")]
    envelope_type: String,
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}

/// Mirror of upstream `readPlutusScript`.
pub fn read_plutus_script(
    script_ref: &PlutusScriptRef,
) -> Result<(ScriptInAnyLang, TxGenPlutusResolvedTo), String> {
    match script_ref {
        PlutusScriptRef::Named(name) => {
            let file_name = PathBuf::from(format!("{name}.plutus"));
            let raw = fallback_script_text(name).ok_or_else(|| {
                format!(
                    "readPlutusScript: fallback script {} is not bundled",
                    file_name.display()
                )
            })?;
            let script = read_plutus_script_text(raw, &file_name)?;
            Ok((script, TxGenPlutusResolvedTo::ResolvedToFallback(file_name)))
        }
        PlutusScriptRef::File(path) => {
            let script = read_plutus_script_file(path)?;
            Ok((
                script,
                TxGenPlutusResolvedTo::ResolvedToFileName(path.clone()),
            ))
        }
    }
}

fn read_plutus_script_file(path: &Path) -> Result<ScriptInAnyLang, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("readPlutusScript: {}: {err}", path.display()))?;
    read_plutus_script_text(&raw, path)
}

fn read_plutus_script_text(raw: &str, source: &Path) -> Result<ScriptInAnyLang, String> {
    let envelope: TextEnvelope = serde_json::from_str(raw)
        .map_err(|err| format!("readPlutusScript: {}: {err}", source.display()))?;
    let language = script_language_from_text_envelope(&envelope.envelope_type)?;
    let script_bytes = decode_text_envelope_cbor_bytes(&envelope.cbor_hex)?;
    Ok(ScriptInAnyLang::new(language, script_bytes))
}

fn fallback_script_text(name: &str) -> Option<&'static str> {
    match name {
        "EcdsaSecp256k1Loop" => Some(include_str!(
            "../../scripts-fallback/EcdsaSecp256k1Loop.plutus"
        )),
        "HashOntoG2AndAdd" => Some(include_str!(
            "../../scripts-fallback/HashOntoG2AndAdd.plutus"
        )),
        "Loop" => Some(include_str!("../../scripts-fallback/Loop.plutus")),
        "Loop2024" => Some(include_str!("../../scripts-fallback/Loop2024.plutus")),
        "LoopV3" => Some(include_str!("../../scripts-fallback/LoopV3.plutus")),
        "Ripemd160" => Some(include_str!("../../scripts-fallback/Ripemd160.plutus")),
        "SchnorrSecp256k1Loop" => Some(include_str!(
            "../../scripts-fallback/SchnorrSecp256k1Loop.plutus"
        )),
        _ => None,
    }
}

fn script_language_from_text_envelope(envelope_type: &str) -> Result<ScriptLanguage, String> {
    match envelope_type {
        "PlutusScriptV1" | "PlutusScriptV1_Script" => Ok(ScriptLanguage::PlutusV1),
        "PlutusScriptV2" | "PlutusScriptV2_Script" => Ok(ScriptLanguage::PlutusV2),
        "PlutusScriptV3" | "PlutusScriptV3_Script" => Ok(ScriptLanguage::PlutusV3),
        other => Err(format!(
            "readPlutusScript: only PlutusScript supported, found: {other}"
        )),
    }
}

fn decode_text_envelope_cbor_bytes(cbor_hex: &str) -> Result<Vec<u8>, String> {
    let bytes = hex::decode(cbor_hex.trim())
        .map_err(|err| format!("readPlutusScript: cborHex is not valid hex: {err}"))?;
    let mut decoder = Decoder::new(&bytes);
    let script = decoder
        .bytes_owned()
        .map_err(|err| format!("readPlutusScript: cborHex is not a CBOR bytes value: {err}"))?;
    if !decoder.is_empty() {
        return Err(format!(
            "readPlutusScript: cborHex has {} trailing bytes",
            decoder.remaining()
        ));
    }
    Ok(script)
}

/// Mirror of upstream `preExecutePlutusScript`.
pub fn pre_execute_plutus_script(
    protocol_parameters: &ProtocolParameters,
    script: &ScriptInAnyLang,
    datum: &PlutusData,
    redeemer: &PlutusData,
) -> Result<ExecutionUnits, String> {
    let version = script.language.plutus_version();
    let cost_model_values = protocol_parameters
        .cost_models
        .as_ref()
        .and_then(|models| models.get(&version.cost_model_key()))
        .ok_or_else(|| {
            format!(
                "preExecutePlutusScript: cost model unavailable for: {:?}",
                script.language
            )
        })?;

    let cost_model =
        yggdrasil_node_genesis::build_plutus_cost_model_from_protocol_values_for_protocol(
            version,
            protocol_parameters.protocol_version,
            cost_model_values,
        )
        .map_err(|err| {
            format!(
                "preExecutePlutusScript: invalid cost model for {:?} at protocol {:?} ({} values): {err}",
                script.language,
                protocol_parameters.protocol_version,
                cost_model_values.len()
            )
        })?;

    let program = match script.language {
        ScriptLanguage::PlutusV1 | ScriptLanguage::PlutusV2 => {
            decode_script_bytes_allowing_remainder(&script.bytes)
        }
        ScriptLanguage::PlutusV3 => decode_script_bytes(&script.bytes),
    }
    .map_err(|err| format!("preExecutePlutusScript: could not deserialise script: {err}"))?;

    let max_tx_ex_units = protocol_parameters.max_tx_ex_units.ok_or_else(|| {
        "preExecutePlutusScript: Cannot determine protocolParamMaxTxExUnits".to_string()
    })?;
    let initial_budget = ExBudget::new(
        i64::try_from(max_tx_ex_units.steps).map_err(|_| {
            "preExecutePlutusScript: max_tx_ex_units steps exceed evaluator range".to_string()
        })?,
        i64::try_from(max_tx_ex_units.mem).map_err(|_| {
            "preExecutePlutusScript: max_tx_ex_units memory exceed evaluator range".to_string()
        })?,
    );

    let applied = match script.language {
        ScriptLanguage::PlutusV1 | ScriptLanguage::PlutusV2 => apply_plutus_data_args(
            program.term,
            vec![
                datum.clone(),
                redeemer.clone(),
                dummy_context_v1v2(script.language),
            ],
        ),
        ScriptLanguage::PlutusV3 => {
            apply_plutus_data_args(program.term, vec![dummy_context_v3(datum, redeemer)])
        }
    };

    let mut machine = CekMachine::new(initial_budget, cost_model);
    machine
        .evaluate(applied)
        .map_err(|err| format!("preExecutePlutusScript: Plutus evaluation failed: {err}"))?;

    let remaining = machine.remaining_budget();
    let spent_cpu = initial_budget.cpu - remaining.cpu;
    let spent_mem = initial_budget.mem - remaining.mem;
    if spent_cpu < 0 || spent_mem < 0 {
        return Err(format!(
            "preExecutePlutusScript: evaluator reported negative spend cpu={spent_cpu}, mem={spent_mem}"
        ));
    }
    Ok(ExecutionUnits {
        execution_steps: u64::try_from(spent_cpu).map_err(|_| {
            "preExecutePlutusScript: spent steps exceed execution-unit range".to_string()
        })?,
        execution_memory: u64::try_from(spent_mem).map_err(|_| {
            "preExecutePlutusScript: spent memory exceed execution-unit range".to_string()
        })?,
    })
}

fn apply_plutus_data_args(term: Term, args: Vec<PlutusData>) -> Term {
    args.into_iter().fold(term, |fun, arg| {
        Term::Apply(Box::new(fun), Box::new(data_term(arg)))
    })
}

fn data_term(data: PlutusData) -> Term {
    Term::Constant(Constant::Data(data))
}

fn dummy_context_v1v2(language: ScriptLanguage) -> PlutusData {
    let version = language.plutus_version();
    PlutusData::Constr(
        0,
        vec![
            dummy_tx_info_v1v2(language),
            PlutusData::Constr(1, vec![dummy_out_ref(version)]),
        ],
    )
}

fn dummy_tx_info_v1v2(language: ScriptLanguage) -> PlutusData {
    match language {
        ScriptLanguage::PlutusV1 => PlutusData::Constr(
            0,
            vec![
                PlutusData::List(vec![]),
                PlutusData::List(vec![]),
                PlutusData::Map(vec![]),
                PlutusData::Map(vec![]),
                PlutusData::List(vec![]),
                PlutusData::List(vec![]),
                always_interval(),
                PlutusData::List(vec![]),
                PlutusData::List(vec![]),
                tx_id_v1v2_empty(),
            ],
        ),
        ScriptLanguage::PlutusV2 => PlutusData::Constr(
            0,
            vec![
                PlutusData::List(vec![]),
                PlutusData::List(vec![]),
                PlutusData::List(vec![]),
                PlutusData::Map(vec![]),
                PlutusData::Map(vec![]),
                PlutusData::List(vec![]),
                PlutusData::Map(vec![]),
                always_interval(),
                PlutusData::List(vec![]),
                PlutusData::Map(vec![]),
                PlutusData::Map(vec![]),
                tx_id_v1v2_empty(),
            ],
        ),
        ScriptLanguage::PlutusV3 => unreachable!("V3 uses dummy_tx_info_v3"),
    }
}

fn dummy_context_v3(datum: &PlutusData, redeemer: &PlutusData) -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            dummy_tx_info_v3(),
            redeemer.clone(),
            PlutusData::Constr(
                1,
                vec![
                    dummy_out_ref(yggdrasil_ledger::plutus_validation::PlutusVersion::V3),
                    maybe_data(Some(datum.clone())),
                ],
            ),
        ],
    )
}

fn dummy_tx_info_v3() -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::List(vec![]),
            PlutusData::List(vec![]),
            PlutusData::List(vec![]),
            PlutusData::integer(0),
            PlutusData::Map(vec![]),
            PlutusData::List(vec![]),
            PlutusData::Map(vec![]),
            always_interval(),
            PlutusData::List(vec![]),
            PlutusData::Map(vec![]),
            PlutusData::Map(vec![]),
            PlutusData::Bytes(vec![]),
            PlutusData::Map(vec![]),
            PlutusData::List(vec![]),
            maybe_data(None),
            maybe_data(None),
        ],
    )
}

fn always_interval() -> PlutusData {
    PlutusData::Constr(
        0,
        vec![
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(0, vec![]), PlutusData::Constr(1, vec![])],
            ),
            PlutusData::Constr(
                0,
                vec![PlutusData::Constr(2, vec![]), PlutusData::Constr(1, vec![])],
            ),
        ],
    )
}

fn tx_id_v1v2_empty() -> PlutusData {
    PlutusData::Constr(0, vec![PlutusData::Bytes(vec![])])
}

fn dummy_out_ref(version: yggdrasil_ledger::plutus_validation::PlutusVersion) -> PlutusData {
    let tx_id = match version {
        yggdrasil_ledger::plutus_validation::PlutusVersion::V1
        | yggdrasil_ledger::plutus_validation::PlutusVersion::V2 => tx_id_v1v2_empty(),
        yggdrasil_ledger::plutus_validation::PlutusVersion::V3 => PlutusData::Bytes(vec![]),
    };
    PlutusData::Constr(0, vec![tx_id, PlutusData::integer(0)])
}

fn maybe_data(data: Option<PlutusData>) -> PlutusData {
    match data {
        Some(data) => PlutusData::Constr(0, vec![data]),
        None => PlutusData::Constr(1, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use yggdrasil_ledger::eras::alonzo::ExUnits;

    #[test]
    fn named_script_resolves_to_fallback_file() {
        let (script, resolved_to) =
            read_plutus_script(&PlutusScriptRef::Named("Loop".to_string())).expect("script");

        assert_eq!(script.language, ScriptLanguage::PlutusV1);
        assert!(!script.bytes.is_empty());
        assert_eq!(
            resolved_to,
            TxGenPlutusResolvedTo::ResolvedToFallback(PathBuf::from("Loop.plutus"))
        );
    }

    #[test]
    fn non_plutus_text_envelope_is_rejected() {
        let err = script_language_from_text_envelope("SimpleScriptV2")
            .expect_err("native script is rejected");

        assert_eq!(
            err,
            "readPlutusScript: only PlutusScript supported, found: SimpleScriptV2"
        );
    }

    #[test]
    fn cbor_hex_decodes_outer_text_envelope_bytes() {
        let script = decode_text_envelope_cbor_bytes("43010203").expect("bytes");

        assert_eq!(script, vec![1, 2, 3]);
    }

    #[test]
    fn pre_execute_v1_counts_three_argument_dummy_context() {
        let script = ScriptInAnyLang::new(
            ScriptLanguage::PlutusV1,
            cbor_bytes(&[0x01, 0x00, 0x00, 0x22, 0x24, 0x98, 0x00]),
        );
        let units = pre_execute_plutus_script(
            &protocol_parameters_with_v1_cost_model(),
            &script,
            &PlutusData::integer(0),
            &PlutusData::integer(1),
        )
        .expect("pre-execute");

        assert!(units.execution_steps > 0);
        assert!(units.execution_memory > 0);
    }

    #[test]
    fn pre_execute_reports_missing_cost_model_by_language() {
        let script = ScriptInAnyLang::new(
            ScriptLanguage::PlutusV2,
            cbor_bytes(&[0x01, 0x00, 0x00, 0x22, 0x24, 0x98, 0x00]),
        );
        let err = pre_execute_plutus_script(
            &protocol_parameters_with_v1_cost_model(),
            &script,
            &PlutusData::integer(0),
            &PlutusData::integer(1),
        )
        .expect_err("missing V2 model");

        assert_eq!(
            err,
            "preExecutePlutusScript: cost model unavailable for: PlutusV2"
        );
    }

    fn protocol_parameters_with_v1_cost_model() -> ProtocolParameters {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../../data/protocol-parameters.json"))
                .expect("protocol parameters JSON");
        let v1_model: Vec<i64> =
            serde_json::from_value(raw["costModels"]["PlutusV1"].clone()).expect("V1 model");
        let mut cost_models = BTreeMap::new();
        cost_models.insert(0, v1_model);
        let mut params = ProtocolParameters::alonzo_defaults();
        params.protocol_version = Some((6, 0));
        params.cost_models = Some(cost_models);
        params.max_tx_ex_units = Some(ExUnits {
            mem: 10_000_000,
            steps: 10_000_000_000,
        });
        params
    }

    fn cbor_bytes(payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 24);
        let mut out = Vec::with_capacity(payload.len() + 1);
        out.push(0x40 | payload.len() as u8);
        out.extend_from_slice(payload);
        out
    }
}
