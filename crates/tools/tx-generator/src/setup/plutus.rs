//! Plutus script loading helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Setup/Plutus.hs`.
//! Ports `readPlutusScript` text-envelope loading for the pure-Rust
//! tx-generator. `preExecutePlutusScript` remains an explicit runtime
//! boundary until the Plutus evaluator is wired into the generator.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yggdrasil_ledger::Decoder;

use crate::tx_generator::utxo::{ScriptInAnyLang, ScriptLanguage};
use crate::types::{PlutusScriptRef, TxGenPlutusResolvedTo};

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

/// Boundary for upstream `preExecutePlutusScript`.
pub fn pre_execute_plutus_script() -> Result<(), String> {
    Err("preExecutePlutusScript: Plutus evaluator integration is not yet implemented".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
