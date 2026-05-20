//! JSON helpers for transaction-generator scripts.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Aeson.hs`.
//! Ports the upstream script JSON entry points: `testJSONRoundTrip`,
//! deterministic pretty printing, `scanScriptFile`, `parseJSONFile`,
//! and `parseScriptFileAeson`. YAML rendering and protocol-parameter
//! decoding land with the later runtime `Script/Core` slice.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::script::types::{Action, Script};

/// Error emitted while scanning or decoding a script JSON file.
#[derive(Debug, thiserror::Error)]
pub enum ScriptJsonError {
    /// The script file could not be read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not syntactically valid JSON.
    #[error(
        "error while parsing json value :\nfile :{file}\nline number {line}\nmessage : {message}\n"
    )]
    Scan {
        /// File being scanned.
        file: String,
        /// 1-based line number reported by the JSON parser.
        line: usize,
        /// Parser diagnostic.
        message: String,
    },
    /// JSON was syntactically valid but not an upstream script shape.
    #[error("{0}")]
    Decode(String),
}

/// Mirror of upstream `testJSONRoundTrip`.
pub fn test_json_round_trip(script: &[Action]) -> Option<String> {
    let value = match serde_json::to_value(script) {
        Ok(value) => value,
        Err(err) => return Some(err.to_string()),
    };
    match serde_json::from_value::<Script>(value) {
        Ok(round_trip) if round_trip == script => None,
        Ok(_) => Some("compare: not equal".to_string()),
        Err(err) => Some(err.to_string()),
    }
}

/// Mirror of upstream `prettyPrintOrdered`.
pub fn pretty_print_ordered<T>(value: &T) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    Ok(rendered)
}

/// Mirror of upstream `prettyPrint`.
pub fn pretty_print(script: &[Action]) -> Result<String, serde_json::Error> {
    pretty_print_ordered(&script)
}

/// Mirror of upstream `scanScriptFile`.
pub fn scan_script_file(file_path: impl AsRef<Path>) -> Result<Value, ScriptJsonError> {
    let file_path = file_path.as_ref();
    let input = fs::read_to_string(file_path)?;
    serde_json::from_str(&input).map_err(|err| ScriptJsonError::Scan {
        file: file_path.display().to_string(),
        line: err.line(),
        message: err.to_string(),
    })
}

/// Mirror of upstream `parseJSONFile`.
pub fn parse_json_file<T>(file_path: impl AsRef<Path>) -> Result<T, ScriptJsonError>
where
    T: DeserializeOwned,
{
    let value = scan_script_file(file_path)?;
    serde_json::from_value(value).map_err(|err| ScriptJsonError::Decode(err.to_string()))
}

/// Mirror of upstream `parseScriptFileAeson`.
pub fn parse_script_file_aeson(file_path: impl AsRef<Path>) -> Result<Script, ScriptJsonError> {
    parse_json_file(file_path)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::script::types::{
        Generator, NetworkId, PayMode, ProtocolParametersSource, SubmitMode,
    };
    use crate::types::{AnyCardanoEra, TxGenTxParams};

    fn temp_file(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "yggdrasil-tx-generator-aeson-{name}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    #[test]
    fn round_trip_generated_actions() {
        let script = vec![
            Action::SetNetworkId(NetworkId::Testnet(42)),
            Action::SetProtocolParameters(ProtocolParametersSource::QueryLocalNode),
            Action::Submit(
                AnyCardanoEra::Conway,
                SubmitMode::LocalSocket,
                TxGenTxParams {
                    tx_param_fee: 1,
                    tx_param_add_tx_size: 2,
                    tx_param_ttl: 3,
                },
                Generator::Take(
                    1,
                    Box::new(Generator::NtoM(
                        "wallet".to_string(),
                        PayMode::PayToAddr("key".to_string(), "dest".to_string()),
                        1,
                        2,
                        None,
                        None,
                    )),
                ),
            ),
            Action::WaitBenchmark,
        ];

        assert_eq!(test_json_round_trip(&script), None);
    }

    #[test]
    fn parse_script_file_accepts_object_with_single_field_actions() {
        let path = temp_file("valid");
        fs::write(
            &path,
            serde_json::to_string_pretty(&json!([
                { "SetNetworkId": { "Testnet": 42 } },
                { "InitWallet": "wallet" },
                { "Delay": 0.25 },
                { "CancelBenchmark": [] }
            ]))
            .expect("json render"),
        )
        .expect("write script");

        let script = parse_script_file_aeson(&path).expect("parse script");
        let _ = fs::remove_file(&path);

        assert_eq!(
            script,
            vec![
                Action::SetNetworkId(NetworkId::Testnet(42)),
                Action::InitWallet("wallet".to_string()),
                Action::Delay(0.25),
                Action::CancelBenchmark,
            ]
        );
    }

    #[test]
    fn parse_script_file_reports_json_line_number() {
        let path = temp_file("invalid");
        fs::write(&path, "[\n  { \"Delay\": 1.0 },\n  bad\n]").expect("write script");

        let err = parse_script_file_aeson(&path).expect_err("invalid json rejected");
        let _ = fs::remove_file(&path);

        let message = err.to_string();
        assert!(message.contains("error while parsing json value"));
        assert!(message.contains("line number 3"));
    }
}
