//! JSON-deserialization + file-path-adjustment instances for the
//! db-synthesizer typed config types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/Orphans.hs.
//!
//! Direct port of the typeclass-instance surface upstream uses to
//! parse a node-config JSON file into a [`NodeConfigStub`] and
//! adjust embedded file paths. Upstream's "orphan instances" pattern
//! exists because Haskell's open-world typeclass dispatch lets a
//! third module declare instances for types it does not own — the
//! Rust port instead places `serde::Deserialize` impls and the
//! [`AdjustFilePaths`] trait + impls in this module to keep the
//! file-mirror parity intact.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                                  |
//! |---------------------------------------------------------|--------------------------------------------|
//! | `instance FromJSON NodeConfigStub`                      | `serde::Deserialize` impl on `NodeConfigStub` |
//! | `instance AdjustFilePaths NodeConfigStub`               | [`AdjustFilePaths`] impl on `NodeConfigStub` |
//! | `instance AdjustFilePaths NodeCredentials`              | [`AdjustFilePaths`] impl on `NodeCredentials` |
//! | `instance FromJSON NodeHardForkProtocolConfiguration`   | (carve-out — see below)                     |
//! | `instance FromJSON NodeByronProtocolConfiguration`      | (carve-out — see below)                     |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`NodeHardForkProtocolConfiguration` + `NodeByronProtocolConfiguration`
//!   FromJSON instances**: upstream re-implements these instances
//!   here to avoid an import dependency on `Cardano.Node.Configuration.POM`
//!   (the node-side configuration module). Upstream's own comment
//!   declares them DUPLICATE. The Yggdrasil-side parallel types live
//!   in the runtime layer (e.g. node configuration in
//!   `node/src/config.rs`), and db-synthesizer does not need to
//!   re-deserialize them — it operates on the raw JSON `Value`
//!   stashed in [`NodeConfigStub::node_config`] and feeds that to
//!   the runtime layer when wiring up the synthesizer's underlying
//!   protocol info.
//! - **`Cardano.Chain.Update.ApplicationName` hard-coded
//!   "cardano-sl"**: upstream's `NodeByronProtocolConfiguration`
//!   parser hard-codes the Byron application name to `"cardano-sl"`.
//!   When Yggdrasil eventually grows its own
//!   `NodeByronProtocolConfiguration` type at the node-runtime layer,
//!   it will mirror this constant.

use std::path::PathBuf;

use serde::Deserialize;

use crate::types::{NodeConfigStub, NodeCredentials};

/// Trait for adjusting embedded file paths inside a configuration
/// record. Mirror of upstream
/// `class AdjustFilePaths a where adjustFilePaths :: (FilePath -> FilePath) -> a -> a`.
///
/// Used by the db-synthesizer to canonicalize relative paths inside
/// a parsed node-config JSON against the directory the JSON file
/// itself lives in (mirroring upstream's
/// `Cardano.Node.Configuration.POM` adjustments).
pub trait AdjustFilePaths {
    /// Apply `f` to every embedded `PathBuf` and return a new value.
    fn adjust_file_paths<F>(self, f: F) -> Self
    where
        F: Fn(PathBuf) -> PathBuf;
}

impl AdjustFilePaths for NodeConfigStub {
    fn adjust_file_paths<F>(self, f: F) -> Self
    where
        F: Fn(PathBuf) -> PathBuf,
    {
        NodeConfigStub {
            node_config: self.node_config,
            alonzo_genesis_file: f(self.alonzo_genesis_file),
            shelley_genesis_file: f(self.shelley_genesis_file),
            byron_genesis_file: f(self.byron_genesis_file),
            conway_genesis_file: f(self.conway_genesis_file),
            dijkstra_genesis_file: self.dijkstra_genesis_file.map(&f),
        }
    }
}

impl AdjustFilePaths for NodeCredentials {
    fn adjust_file_paths<F>(self, f: F) -> Self
    where
        F: Fn(PathBuf) -> PathBuf,
    {
        NodeCredentials {
            cert_file: self.cert_file.map(&f),
            vrf_file: self.vrf_file.map(&f),
            kes_file: self.kes_file.map(&f),
            bulk_file: self.bulk_file.map(&f),
        }
    }
}

/// Errors from JSON-deserializing a node-config stub.
#[derive(Debug, thiserror::Error)]
pub enum NodeConfigStubParseError {
    /// `Protocol` field absent or non-string.
    #[error("nodeConfig.Protocol expected: Cardano; missing or not a string")]
    ProtocolMissing,
    /// `Protocol` field present but did not equal `"Cardano"`.
    #[error("nodeConfig.Protocol expected: Cardano; found: {0}")]
    ProtocolMismatch(String),
    /// A required path field is absent or non-string.
    #[error("nodeConfig.{field} expected: string path; missing or not a string")]
    RequiredPathMissing {
        /// JSON key name of the missing field.
        field: &'static str,
    },
    /// A required path field has an invalid type.
    #[error("nodeConfig.{field} expected: string path; got non-string JSON value")]
    InvalidPathType {
        /// JSON key name of the malformed field.
        field: &'static str,
    },
    /// Top-level value is not a JSON object.
    #[error("nodeConfig expected: JSON object; got {0}")]
    NotAnObject(String),
}

/// Custom serde::Deserialize for [`NodeConfigStub`] enforcing
/// upstream's "Protocol = Cardano" assertion. Mirror of upstream
/// `instance FromJSON NodeConfigStub`.
impl<'de> Deserialize<'de> for NodeConfigStub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer)?;
        parse_node_config_stub(raw).map_err(serde::de::Error::custom)
    }
}

/// Public parse entry-point for ad-hoc JSON-Value deserialization.
/// Mirror of upstream's
/// `parseJSON val = withObject "NodeConfigStub" (parse' val) val`
/// pattern.
pub fn parse_node_config_stub(
    value: serde_json::Value,
) -> Result<NodeConfigStub, NodeConfigStubParseError> {
    let obj = match &value {
        serde_json::Value::Object(map) => map,
        other => {
            return Err(NodeConfigStubParseError::NotAnObject(
                describe_json_value_kind(other).to_string(),
            ));
        }
    };

    let protocol = obj
        .get("Protocol")
        .and_then(|v| v.as_str())
        .ok_or(NodeConfigStubParseError::ProtocolMissing)?;
    if protocol != "Cardano" {
        return Err(NodeConfigStubParseError::ProtocolMismatch(
            protocol.to_string(),
        ));
    }

    let alonzo = required_path_field(obj, "AlonzoGenesisFile")?;
    let shelley = required_path_field(obj, "ShelleyGenesisFile")?;
    let byron = required_path_field(obj, "ByronGenesisFile")?;
    let conway = required_path_field(obj, "ConwayGenesisFile")?;
    let dijkstra = optional_path_field(obj, "DijkstraGenesisFile")?;

    Ok(NodeConfigStub {
        node_config: value,
        alonzo_genesis_file: alonzo,
        shelley_genesis_file: shelley,
        byron_genesis_file: byron,
        conway_genesis_file: conway,
        dijkstra_genesis_file: dijkstra,
    })
}

fn required_path_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<PathBuf, NodeConfigStubParseError> {
    let value = obj
        .get(field)
        .ok_or(NodeConfigStubParseError::RequiredPathMissing { field })?;
    match value {
        serde_json::Value::String(s) => Ok(PathBuf::from(s)),
        _ => Err(NodeConfigStubParseError::InvalidPathType { field }),
    }
}

fn optional_path_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<Option<PathBuf>, NodeConfigStubParseError> {
    match obj.get(field) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(PathBuf::from(s))),
        Some(_) => Err(NodeConfigStubParseError::InvalidPathType { field }),
    }
}

fn describe_json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample_object_json() -> &'static str {
        r#"{
            "Protocol": "Cardano",
            "AlonzoGenesisFile": "config/alonzo.json",
            "ShelleyGenesisFile": "config/shelley.json",
            "ByronGenesisFile": "config/byron.json",
            "ConwayGenesisFile": "config/conway.json",
            "DijkstraGenesisFile": "config/dijkstra.json"
        }"#
    }

    #[test]
    fn parses_complete_node_config_stub_with_all_genesis_files() {
        let stub: NodeConfigStub = serde_json::from_str(sample_object_json()).expect("parses");
        assert_eq!(
            stub.alonzo_genesis_file,
            PathBuf::from("config/alonzo.json"),
        );
        assert_eq!(
            stub.shelley_genesis_file,
            PathBuf::from("config/shelley.json"),
        );
        assert_eq!(stub.byron_genesis_file, PathBuf::from("config/byron.json"));
        assert_eq!(
            stub.conway_genesis_file,
            PathBuf::from("config/conway.json"),
        );
        assert_eq!(
            stub.dijkstra_genesis_file,
            Some(PathBuf::from("config/dijkstra.json")),
        );
    }

    #[test]
    fn parses_node_config_stub_without_dijkstra_field() {
        let json = r#"{
            "Protocol": "Cardano",
            "AlonzoGenesisFile": "a.json",
            "ShelleyGenesisFile": "s.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json"
        }"#;
        let stub: NodeConfigStub = serde_json::from_str(json).expect("parses");
        assert_eq!(stub.dijkstra_genesis_file, None);
    }

    #[test]
    fn parses_node_config_stub_with_explicit_null_dijkstra() {
        let json = r#"{
            "Protocol": "Cardano",
            "AlonzoGenesisFile": "a.json",
            "ShelleyGenesisFile": "s.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json",
            "DijkstraGenesisFile": null
        }"#;
        let stub: NodeConfigStub = serde_json::from_str(json).expect("parses");
        assert_eq!(stub.dijkstra_genesis_file, None);
    }

    #[test]
    fn rejects_non_object_top_level() {
        let json = "[1, 2, 3]";
        let parsed: Result<NodeConfigStub, _> = serde_json::from_str(json);
        assert!(parsed.is_err());
        let err_string = format!("{}", parsed.unwrap_err());
        assert!(err_string.contains("array"), "got: {err_string}");
    }

    #[test]
    fn rejects_wrong_protocol() {
        let json = r#"{
            "Protocol": "Shelley",
            "AlonzoGenesisFile": "a.json",
            "ShelleyGenesisFile": "s.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json"
        }"#;
        let parsed: Result<NodeConfigStub, _> = serde_json::from_str(json);
        assert!(parsed.is_err());
        let err_string = format!("{}", parsed.unwrap_err());
        // Mirror upstream's exact wording.
        assert!(err_string.contains("expected: Cardano"));
        assert!(err_string.contains("found: Shelley"));
    }

    #[test]
    fn rejects_missing_protocol() {
        let json = r#"{
            "AlonzoGenesisFile": "a.json",
            "ShelleyGenesisFile": "s.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json"
        }"#;
        let parsed: Result<NodeConfigStub, _> = serde_json::from_str(json);
        assert!(parsed.is_err());
    }

    #[test]
    fn rejects_missing_required_genesis_file() {
        let json = r#"{
            "Protocol": "Cardano",
            "AlonzoGenesisFile": "a.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json"
        }"#;
        let parsed: Result<NodeConfigStub, _> = serde_json::from_str(json);
        assert!(parsed.is_err());
        let err_string = format!("{}", parsed.unwrap_err());
        assert!(err_string.contains("ShelleyGenesisFile"));
    }

    #[test]
    fn rejects_non_string_genesis_path() {
        let json = r#"{
            "Protocol": "Cardano",
            "AlonzoGenesisFile": 42,
            "ShelleyGenesisFile": "s.json",
            "ByronGenesisFile": "b.json",
            "ConwayGenesisFile": "c.json"
        }"#;
        let parsed: Result<NodeConfigStub, _> = serde_json::from_str(json);
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_node_config_stub_preserves_node_config_value() {
        let raw: serde_json::Value =
            serde_json::from_str(sample_object_json()).expect("valid JSON");
        let stub = parse_node_config_stub(raw.clone()).expect("parses");
        // The raw JSON value is preserved on the struct.
        assert_eq!(stub.node_config, raw);
    }

    #[test]
    fn adjust_file_paths_for_node_config_stub_applies_to_all_paths() {
        let stub = NodeConfigStub {
            node_config: serde_json::Value::Null,
            alonzo_genesis_file: PathBuf::from("alonzo.json"),
            shelley_genesis_file: PathBuf::from("shelley.json"),
            byron_genesis_file: PathBuf::from("byron.json"),
            conway_genesis_file: PathBuf::from("conway.json"),
            dijkstra_genesis_file: Some(PathBuf::from("dijkstra.json")),
        };
        let prefix = Path::new("/etc/cardano");
        let adjusted = stub.adjust_file_paths(|p| prefix.join(p));
        assert_eq!(
            adjusted.alonzo_genesis_file,
            Path::new("/etc/cardano/alonzo.json"),
        );
        assert_eq!(
            adjusted.shelley_genesis_file,
            Path::new("/etc/cardano/shelley.json"),
        );
        assert_eq!(
            adjusted.byron_genesis_file,
            Path::new("/etc/cardano/byron.json"),
        );
        assert_eq!(
            adjusted.conway_genesis_file,
            Path::new("/etc/cardano/conway.json"),
        );
        assert_eq!(
            adjusted.dijkstra_genesis_file,
            Some(PathBuf::from("/etc/cardano/dijkstra.json")),
        );
    }

    #[test]
    fn adjust_file_paths_for_node_config_stub_passes_through_none_dijkstra() {
        let stub = NodeConfigStub {
            node_config: serde_json::Value::Null,
            alonzo_genesis_file: PathBuf::from("a"),
            shelley_genesis_file: PathBuf::from("s"),
            byron_genesis_file: PathBuf::from("b"),
            conway_genesis_file: PathBuf::from("c"),
            dijkstra_genesis_file: None,
        };
        let adjusted = stub.adjust_file_paths(|p| PathBuf::from("/abs").join(p));
        assert_eq!(adjusted.dijkstra_genesis_file, None);
    }

    #[test]
    fn adjust_file_paths_for_node_credentials_applies_to_all_present_paths() {
        let creds = NodeCredentials {
            cert_file: Some(PathBuf::from("cert.json")),
            vrf_file: Some(PathBuf::from("vrf.skey")),
            kes_file: None,
            bulk_file: Some(PathBuf::from("bulk.json")),
        };
        let prefix = Path::new("/var/cardano/keys");
        let adjusted = creds.adjust_file_paths(|p| prefix.join(p));
        assert_eq!(
            adjusted.cert_file,
            Some(PathBuf::from("/var/cardano/keys/cert.json")),
        );
        assert_eq!(
            adjusted.vrf_file,
            Some(PathBuf::from("/var/cardano/keys/vrf.skey")),
        );
        assert_eq!(adjusted.kes_file, None);
        assert_eq!(
            adjusted.bulk_file,
            Some(PathBuf::from("/var/cardano/keys/bulk.json")),
        );
    }

    #[test]
    fn adjust_file_paths_for_node_credentials_pass_through_all_none() {
        let creds = NodeCredentials::default();
        let adjusted = creds.adjust_file_paths(|p| PathBuf::from("/abs").join(p));
        assert_eq!(adjusted.cert_file, None);
        assert_eq!(adjusted.vrf_file, None);
        assert_eq!(adjusted.kes_file, None);
        assert_eq!(adjusted.bulk_file, None);
    }
}
