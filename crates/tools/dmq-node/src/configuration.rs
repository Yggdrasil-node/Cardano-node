//! Configuration-file loading and CLI-vs-file-vs-defaults merge logic.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/Configuration.hs.
//!
//! Direct port of upstream's `readConfigurationFile` plus the
//! merge-and-resolve helpers used by the runtime entry point. The
//! `mkDiffusionConfiguration` helper is carved out (it touches
//! `Ouroboros.Network.Diffusion` configuration which is a substantial
//! separate port; lands when the diffusion mux wiring rounds begin).
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`mkDiffusionConfiguration`**: builds the upstream
//!   `Ouroboros.Network.Diffusion.DiffusionConfiguration` record
//!   with peer-selection / connection-manager / churn-interval
//!   tunables. Yggdrasil's port stops at the operator-facing
//!   [`Configuration`] surface; the Diffusion record is constructed
//!   in a later round when the mux wiring lands.
//! - **YAML parsing**: upstream uses `decodeFileEither @TracerConfig`
//!   from `Data.Yaml`. The Rust port currently accepts JSON only;
//!   YAML support can be layered on with `serde_yaml` when an
//!   operator workflow needs it.

use std::path::Path;

use crate::types::{Configuration, PartialConfig};

/// Errors from the configuration-file loader.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read the file from disk.
    #[error("failed to read config file `{path}': {source}")]
    Io {
        /// Path that failed to open.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// File contents could not be parsed as a `PartialConfig`.
    #[error("failed to parse config file `{path}' as JSON: {source}")]
    Parse {
        /// Path that parsed-failed.
        path: String,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
}

/// Load a `PartialConfig` from a JSON file on disk.
///
/// Mirrors upstream `readConfigurationFile :: FilePath -> IO PartialConfig`
/// modulo the YAML carve-out (Yggdrasil currently parses JSON only).
/// Missing files surface as [`ConfigError::Io`]; malformed JSON
/// surfaces as [`ConfigError::Parse`].
pub fn read_configuration_file(path: impl AsRef<Path>) -> Result<PartialConfig, ConfigError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|err| ConfigError::Io {
        path: path.display().to_string(),
        source: err,
    })?;
    serde_json::from_slice::<PartialConfig>(&bytes).map_err(|err| ConfigError::Parse {
        path: path.display().to_string(),
        source: err,
    })
}

/// Build a fully-applied [`Configuration`] from a CLI-derived
/// [`PartialConfig`], optionally consulting a config file at
/// `cli.config_file`.
///
/// Resolution order (left-priority merge, matching upstream's
/// CLI-overrides-file-overrides-defaults semantics):
///
/// 1. CLI-derived `PartialConfig` (highest priority).
/// 2. File-derived `PartialConfig` if `cli.config_file` is set.
/// 3. [`Configuration::defaults`] for any field still unset.
///
/// Upstream:
/// ```haskell
/// runDMQ commandLineConfig = do
///   filePath <- maybe (return defaultConfigFile) ...
///   fileConfig <- readConfigurationFile filePath
///   let merged = commandLineConfig <> fileConfig <> defaultPartialConfig
///   ...
/// ```
pub fn resolve_configuration(cli: PartialConfig) -> Result<Configuration, ConfigError> {
    let merged = match cli.config_file.clone() {
        Some(path) => {
            let from_file = read_configuration_file(&path)?;
            cli.merge(from_file)
        }
        None => cli,
    };
    Ok(merged.resolve())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::types::{LocalAddress, NetworkMagic};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn write_temp_json(contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("yggdrasil-dmq-test-{pid}-{stamp}-{seq}.json"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        path
    }

    #[test]
    fn read_configuration_file_round_trips_minimal_json() {
        let path = write_temp_json(r#"{"hostAddr":"10.0.0.1","portNumber":4000}"#);
        let parsed = read_configuration_file(&path).expect("parses");
        assert_eq!(parsed.host_addr.as_deref(), Some("10.0.0.1"));
        assert_eq!(parsed.port_number, Some(4000));
        assert!(parsed.host_ipv6_addr.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_configuration_file_round_trips_full_json() {
        let path = write_temp_json(
            r#"{
                "hostAddr": "0.0.0.0",
                "hostIpv6Addr": "::",
                "portNumber": 3001,
                "localAddress": "/var/run/dmq.sock",
                "configFile": "/etc/dmq.json",
                "topologyFile": "/etc/dmq-topo.json",
                "cardanoNodeSocket": "/run/cardano.sock",
                "cardanoNetworkMagic": 764824073,
                "networkMagic": 0,
                "showVersion": false
            }"#,
        );
        let parsed = read_configuration_file(&path).expect("parses");
        assert_eq!(parsed.host_addr.as_deref(), Some("0.0.0.0"));
        assert_eq!(parsed.host_ipv6_addr.as_deref(), Some("::"));
        assert_eq!(parsed.port_number, Some(3001));
        assert_eq!(
            parsed
                .local_address
                .as_ref()
                .map(|l| l.as_path().to_str().unwrap_or("")),
            Some("/var/run/dmq.sock")
        );
        assert_eq!(
            parsed
                .config_file
                .as_ref()
                .map(|p| p.to_str().unwrap_or("")),
            Some("/etc/dmq.json")
        );
        assert_eq!(
            parsed.cardano_network_magic,
            Some(NetworkMagic(764_824_073))
        );
        assert_eq!(parsed.show_version, Some(false));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_configuration_file_missing_file_returns_io_error() {
        let result = read_configuration_file("/nonexistent/path/dmq.json");
        assert!(matches!(result, Err(ConfigError::Io { .. })));
    }

    #[test]
    fn read_configuration_file_malformed_json_returns_parse_error() {
        let path = write_temp_json("not-valid-json");
        let result = read_configuration_file(&path);
        assert!(matches!(result, Err(ConfigError::Parse { .. })));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn resolve_configuration_without_file_uses_cli_plus_defaults() {
        let cli = PartialConfig {
            host_addr: Some("10.0.0.1".to_string()),
            port_number: Some(4000),
            ..PartialConfig::default()
        };
        let resolved = resolve_configuration(cli).expect("resolves");
        assert_eq!(resolved.host_addr, "10.0.0.1");
        assert_eq!(resolved.port_number, 4000);
        // Defaults fill in:
        assert_eq!(resolved.host_ipv6_addr, "::");
        assert_eq!(resolved.config_file.to_str(), Some("dmq-node.json"));
    }

    #[test]
    fn resolve_configuration_cli_overrides_file_overrides_defaults() {
        // file says port=3002; CLI says port=4000 → CLI wins.
        let path =
            write_temp_json(r#"{"hostAddr":"0.0.0.0","portNumber":3002,"hostIpv6Addr":"fe80::1"}"#);
        let cli = PartialConfig {
            port_number: Some(4000),
            config_file: Some(path.clone()),
            ..PartialConfig::default()
        };
        let resolved = resolve_configuration(cli).expect("resolves");
        // CLI port wins.
        assert_eq!(resolved.port_number, 4000);
        // File host_addr wins (CLI didn't supply one).
        assert_eq!(resolved.host_addr, "0.0.0.0");
        // File ipv6 wins (CLI didn't supply one).
        assert_eq!(resolved.host_ipv6_addr, "fe80::1");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn resolve_configuration_returns_error_when_config_file_missing() {
        let cli = PartialConfig {
            config_file: Some(std::path::PathBuf::from("/nonexistent/dmq.json")),
            ..PartialConfig::default()
        };
        let result = resolve_configuration(cli);
        assert!(matches!(result, Err(ConfigError::Io { .. })));
    }

    #[test]
    fn resolve_configuration_uses_local_address_from_file() {
        let path = write_temp_json(r#"{"localAddress":"/var/lib/dmq/sock"}"#);
        let cli = PartialConfig {
            config_file: Some(path.clone()),
            ..PartialConfig::default()
        };
        let resolved = resolve_configuration(cli).expect("resolves");
        assert_eq!(
            resolved.local_address.as_path().to_str(),
            Some("/var/lib/dmq/sock")
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn partial_config_serializes_camel_case_field_names() {
        let p = PartialConfig {
            host_addr: Some("0.0.0.0".to_string()),
            port_number: Some(3001),
            local_address: Some(LocalAddress::new("/tmp/dmq.sock")),
            ..PartialConfig::default()
        };
        let json = serde_json::to_string(&p).expect("serializes");
        // Field names should be camelCase per upstream's JSON convention.
        assert!(json.contains("\"hostAddr\":\"0.0.0.0\""));
        assert!(json.contains("\"portNumber\":3001"));
        assert!(json.contains("\"localAddress\":\"/tmp/dmq.sock\""));
    }

    #[test]
    fn partial_config_round_trips_through_json() {
        let original = PartialConfig {
            host_addr: Some("10.0.0.1".to_string()),
            port_number: Some(4000),
            cardano_network_magic: Some(NetworkMagic(2)),
            ..PartialConfig::default()
        };
        let json = serde_json::to_string(&original).expect("serializes");
        let parsed: PartialConfig = serde_json::from_str(&json).expect("parses");
        assert_eq!(parsed, original);
    }
}
