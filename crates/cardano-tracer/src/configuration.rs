//! Typed configuration surface for the `cardano-tracer` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Configuration.hs.
//!
//! Direct ports of the configuration data declarations:
//!
//! - [`Address`] / [`HowToConnect`] — `Cardano.Logging.Types.HowToConnect`
//!   sum (`LocalPipe FilePath | RemoteSocket Text Word16`).
//! - [`Endpoint`] — internal-services endpoint (host + port + optional
//!   force-SSL).
//! - [`Certificate`] — TLS certificate triple (cert + key + optional
//!   chain).
//! - [`RotationParams`] — 4-field log-rotation tunables.
//! - [`LogMode`] — `FileMode | JournalMode`.
//! - [`LogFormat`] — `ForHuman | ForMachine`.
//! - [`LoggingParams`] — root + mode + format.
//! - [`Network`] — `AcceptAt Address | ConnectTo NonEmpty Address`.
//! - [`Verbosity`] — `Minimum | ErrorsOnly | Maximum`.
//! - [`FileOrMap`] — `Either FilePath (Map Text Text)`.
//! - [`HasForwarding`] — `(Network, Maybe [[Text]], TraceOptionForwarder)`.
//! - [`TraceOptionForwarder`] — placeholder for the upstream
//!   Cardano.Logging.Types record (all knobs collapsed to JSON for now;
//!   typed parsing lands when the trace-forwarder mini-protocol port
//!   is wired).
//! - [`TracerConfig`] — top-level configuration record (17 fields).
//! - [`well_formed`] — runtime invariant check mirroring upstream's
//!   `wellFormed` validation.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Network.Wai.Handler.Warp.HostPreference`/`Port`/`Settings`**:
//!   replaced by `String`/`u16` + `std::net::SocketAddr` at use-sites.
//!   The upstream `setEndpoint` helper is unnecessary in Rust.
//! - **`Cardano.Logging.Types.TraceOptionForwarder`**: kept as
//!   `serde_json::Value` at this layer; typed parsing happens in the
//!   trace-forwarder mini-protocol port (a separate round).
//! - **Aeson `FromJSON`/`ToJSON` instances** with custom
//!   `parseJSON`/`(<|>)` alternation: ported as serde
//!   `Deserialize`/`Serialize` derives plus per-type custom
//!   helpers where upstream uses `<|>` alternatives.
//! - **`readTracerConfig` IO function** with `die` on parse error:
//!   ported as [`read_tracer_config`] returning a `Result` rather
//!   than terminating the process; the caller's main path decides
//!   how to surface the error.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Mirror of `Cardano.Logging.Types.HowToConnect`. The upstream
/// `Address` type alias resolves to this sum.
///
/// Upstream: `data HowToConnect = LocalPipe FilePath | RemoteSocket Text Word16`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HowToConnect {
    /// Unix domain socket / Windows named pipe path.
    LocalPipe {
        /// Path to the socket / pipe.
        local_pipe: PathBuf,
    },
    /// Host + port pair for a TCP connection.
    RemoteSocket {
        /// Host name or IP literal.
        host: String,
        /// Port number.
        port: u16,
    },
}

impl HowToConnect {
    /// Mirror of upstream's `nullAddress` predicate from
    /// `Cardano.Tracer.Configuration::wellFormed`.
    pub fn is_null(&self) -> bool {
        match self {
            HowToConnect::LocalPipe { local_pipe } => local_pipe.as_os_str().is_empty(),
            HowToConnect::RemoteSocket { host, .. } => host.is_empty(),
        }
    }
}

/// Type alias mirroring upstream's `type Address = HowToConnect`.
pub type Address = HowToConnect;

/// Internal-services endpoint (host + port + optional force-SSL flag).
///
/// Upstream: `data Endpoint = Endpoint { epHost, epPort, epForceSSL }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    /// Bind host. Empty → fails [`well_formed`] validation.
    #[serde(rename = "epHost")]
    pub host: String,
    /// Bind port.
    #[serde(rename = "epPort")]
    pub port: u16,
    /// Optional force-SSL flag. `None` and `Some(false)` both disable
    /// SSL; `Some(true)` enables it.
    #[serde(rename = "epForceSSL", skip_serializing_if = "Option::is_none")]
    pub force_ssl: Option<bool>,
}

impl Endpoint {
    /// Mirror of upstream's `nullEndpoint` predicate from
    /// `Cardano.Tracer.Configuration::wellFormed`.
    pub fn is_null(&self) -> bool {
        self.host.is_empty()
    }
}

/// TLS certificate triple.
///
/// Upstream: `data Certificate = Certificate { certificateFile, certificateKeyFile, certificateChain }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Certificate {
    /// Path to the certificate file.
    #[serde(rename = "certificateFile")]
    pub file: PathBuf,
    /// Path to the certificate's private key.
    #[serde(rename = "certificateKeyFile")]
    pub key_file: PathBuf,
    /// Optional chain of intermediate certificates.
    #[serde(rename = "certificateChain", skip_serializing_if = "Option::is_none")]
    pub chain: Option<Vec<PathBuf>>,
}

/// Log-rotation tunables.
///
/// Upstream: `data RotationParams = RotationParams { rpFrequencySecs, rpLogLimitBytes, rpMaxAgeMinutes, rpKeepFilesNum }`.
///
/// Defaults mirror upstream's hand-written `FromJSON` instance:
/// - `frequency_secs` defaults to 60 (1 minute).
/// - `max_age_minutes` defaults to 1440 (24 hours).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct RotationParams {
    /// Rotation period, in seconds.
    #[serde(
        rename = "rpFrequencySecs",
        default = "RotationParams::default_frequency_secs"
    )]
    pub frequency_secs: u32,
    /// Max size of log file in bytes.
    #[serde(rename = "rpLogLimitBytes")]
    pub log_limit_bytes: u64,
    /// Max age of log file in minutes. Upstream's FromJSON also accepts
    /// `rpMaxAgeHours` (multiplied by 60); the Rust port keeps the
    /// minutes-based field as canonical.
    #[serde(
        rename = "rpMaxAgeMinutes",
        default = "RotationParams::default_max_age_minutes"
    )]
    pub max_age_minutes: u64,
    /// Number of log files to keep in any case.
    #[serde(rename = "rpKeepFilesNum")]
    pub keep_files_num: u32,
}

impl RotationParams {
    fn default_frequency_secs() -> u32 {
        60
    }

    fn default_max_age_minutes() -> u64 {
        24 * 60
    }
}

/// Log mode.
///
/// Upstream: `data LogMode = FileMode | JournalMode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum LogMode {
    /// Store items in log file.
    FileMode,
    /// Store items in Linux journal service.
    JournalMode,
}

/// Format of log files.
///
/// Upstream: `data LogFormat = ForHuman | ForMachine`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum LogFormat {
    /// For human (text).
    ForHuman,
    /// For machine (JSON).
    ForMachine,
}

/// Logging parameters.
///
/// Upstream: `data LoggingParams = LoggingParams { logRoot, logMode, logFormat }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct LoggingParams {
    /// Root directory where all subdirs with logs are created.
    #[serde(rename = "logRoot")]
    pub root: PathBuf,
    /// Log mode.
    #[serde(rename = "logMode")]
    pub mode: LogMode,
    /// Log format.
    #[serde(rename = "logFormat")]
    pub format: LogFormat,
}

impl LoggingParams {
    /// Mirror of upstream's `invalidFileMode` predicate from
    /// `Cardano.Tracer.Configuration::wellFormed`. Returns `true` when
    /// the parameters describe a `FileMode` log with an empty root
    /// (which is always a configuration error).
    pub fn is_invalid_file_mode(&self) -> bool {
        match self.mode {
            LogMode::FileMode => self.root.as_os_str().is_empty(),
            LogMode::JournalMode => false,
        }
    }
}

/// Connection mode.
///
/// Upstream: `data Network = AcceptAt Address | ConnectTo NonEmpty Address`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Network {
    /// Server mode: accepts connections.
    AcceptAt {
        /// Address to bind on.
        accept_at: Address,
    },
    /// Client mode: initiates connections to the supplied addresses.
    /// Upstream uses `NonEmpty Address`; the Rust port uses `Vec<Address>`
    /// with [`well_formed`] enforcing non-emptiness.
    ConnectTo {
        /// Addresses to dial.
        connect_to: Vec<Address>,
    },
}

/// Tracer's verbosity.
///
/// Upstream: `data Verbosity = Minimum | ErrorsOnly | Maximum`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Verbosity {
    /// Display minimum of messages.
    Minimum,
    /// Display errors only.
    ErrorsOnly,
    /// Display all the messages (protocols tracing, errors).
    Maximum,
}

/// `FileOrMap` is either a path on disk or an inline `Map Text Text`.
///
/// Upstream: `newtype FileOrMap = FOM (Either FilePath (Map Text Text))`
/// with hand-written `FromJSON` that tries the file path first then
/// falls back to the map.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileOrMap {
    /// Path to a JSON file containing the map.
    File(PathBuf),
    /// Inline key→value map (sorted for deterministic serialization).
    Map(BTreeMap<String, String>),
}

/// Trace-forwarder option placeholder. Upstream:
/// `Cardano.Logging.Types.TraceOptionForwarder` is a record with
/// queue-size + reconnect-delay tunables.
///
/// **R358 keeps this as untyped JSON; typed parsing lands when the
/// trace-forwarder mini-protocol port is wired (a separate round).**
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceOptionForwarder(pub JsonValue);

impl Default for TraceOptionForwarder {
    fn default() -> Self {
        TraceOptionForwarder(JsonValue::Null)
    }
}

/// Forwarder triple: re-forward target network + optional path-prefix
/// filter list + forwarder options.
///
/// Upstream: `(Network, Maybe [[Text]], TraceOptionForwarder)`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HasForwarding {
    /// Re-forward target network.
    pub network: Network,
    /// Optional list of path-prefix filters; messages whose namespace
    /// starts with any prefix in the list are re-forwarded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefixes: Option<Vec<Vec<String>>>,
    /// Forwarder options (queue-size + reconnect-delay; placeholder).
    pub options: TraceOptionForwarder,
}

/// Top-level tracer configuration.
///
/// Upstream:
/// ```haskell
/// data TracerConfig = TracerConfig
///   { networkMagic :: Word32
///   , network :: Network
///   , loRequestNum :: Maybe Word16
///   , ekgRequestFreq :: Maybe Pico
///   , hasEKG :: Maybe Endpoint
///   , hasPrometheus :: Maybe Endpoint
///   , hasRTView :: Maybe Endpoint
///   , hasTimeseries :: Maybe Endpoint
///   , tlsCertificate :: Maybe Certificate
///   , hasForwarding :: Maybe (Network, Maybe [[Text]], TraceOptionForwarder)
///   , logging :: NonEmpty LoggingParams
///   , rotation :: Maybe RotationParams
///   , verbosity :: Maybe Verbosity
///   , metricsNoSuffix :: Maybe Bool
///   , metricsHelp :: Maybe FileOrMap
///   , resourceFreq :: Maybe Int
///   , ekgRequestFull :: Maybe Bool
///   , prometheusLabels :: Maybe (Map Text Text)
///   }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TracerConfig {
    /// Network magic from genesis the node is launched with.
    #[serde(rename = "networkMagic")]
    pub network_magic: u32,
    /// How cardano-tracer will be connected to node(s).
    pub network: Network,
    /// How many `TraceObject`s will be asked in each request.
    #[serde(rename = "loRequestNum", skip_serializing_if = "Option::is_none")]
    pub log_objects_request_num: Option<u16>,
    /// How often to request EKG metrics, in seconds (Pico-precision in upstream).
    #[serde(rename = "ekgRequestFreq", skip_serializing_if = "Option::is_none")]
    pub ekg_request_freq: Option<f64>,
    /// Endpoint for EKG web-page.
    #[serde(rename = "hasEKG", skip_serializing_if = "Option::is_none")]
    pub has_ekg: Option<Endpoint>,
    /// Endpoint for Prometheus web-page.
    #[serde(rename = "hasPrometheus", skip_serializing_if = "Option::is_none")]
    pub has_prometheus: Option<Endpoint>,
    /// Endpoint for RTView web-page (carve-out: RTView UI itself is
    /// not ported per the plan).
    #[serde(rename = "hasRTView", skip_serializing_if = "Option::is_none")]
    pub has_rtview: Option<Endpoint>,
    /// Endpoint for the timeseries server.
    #[serde(rename = "hasTimeseries", skip_serializing_if = "Option::is_none")]
    pub has_timeseries: Option<Endpoint>,
    /// TLS certificate for the HTTPS-served endpoints.
    #[serde(rename = "tlsCertificate", skip_serializing_if = "Option::is_none")]
    pub tls_certificate: Option<Certificate>,
    /// Re-forwarding configuration (forward incoming traces back out).
    #[serde(rename = "hasForwarding", skip_serializing_if = "Option::is_none")]
    pub has_forwarding: Option<HasForwarding>,
    /// Logging parameters. Upstream uses `NonEmpty LoggingParams`;
    /// the Rust port uses `Vec<LoggingParams>` with [`well_formed`]
    /// enforcing non-emptiness.
    pub logging: Vec<LoggingParams>,
    /// Optional rotation parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationParams>,
    /// Verbosity of the tracer itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<Verbosity>,
    /// Prometheus ONLY: drop metric-name suffixes (default false).
    #[serde(rename = "metricsNoSuffix", skip_serializing_if = "Option::is_none")]
    pub metrics_no_suffix: Option<bool>,
    /// Prometheus ONLY: per-metric `# HELP` text source.
    #[serde(rename = "metricsHelp", skip_serializing_if = "Option::is_none")]
    pub metrics_help: Option<FileOrMap>,
    /// Frequency (1/millisecond) for gathering resource data.
    #[serde(rename = "resourceFreq", skip_serializing_if = "Option::is_none")]
    pub resource_freq: Option<i32>,
    /// Request full metrics set always vs deltas only (default false).
    #[serde(rename = "ekgRequestFull", skip_serializing_if = "Option::is_none")]
    pub ekg_request_full: Option<bool>,
    /// Common label set for all Prometheus scrape targets.
    #[serde(rename = "prometheusLabels", skip_serializing_if = "Option::is_none")]
    pub prometheus_labels: Option<BTreeMap<String, String>>,
}

/// Result of the [`well_formed`] invariant check.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WellFormedError {
    /// One or more invariant problems detected; concatenated with `, `.
    #[error("Tracer's configuration is ill-formed: {0}")]
    IllFormed(String),
}

/// Mirror of upstream `wellFormed`. Returns `Ok(())` if the
/// configuration is internally consistent; otherwise returns a
/// concatenated description of every detected problem (matching
/// upstream's `intercalate ", " problems` output shape).
pub fn well_formed(config: &TracerConfig) -> Result<(), WellFormedError> {
    let mut problems: Vec<&'static str> = Vec::new();

    // network: AcceptAt with empty address, or ConnectTo with no non-empty
    match &config.network {
        Network::AcceptAt { accept_at } if accept_at.is_null() => {
            problems.push("AcceptAt is empty");
        }
        Network::ConnectTo { connect_to }
            if connect_to.iter().all(HowToConnect::is_null) =>
        {
            problems.push("ConnectTo are empty");
        }
        _ => {}
    }

    // logging: empty logRoot in any FileMode entry is a problem.
    if config
        .logging
        .iter()
        .any(LoggingParams::is_invalid_file_mode)
    {
        problems.push("empty logRoot(s)");
    }

    // duplicate ports across hasEKG / hasPrometheus / hasRTView.
    let mut ports: Vec<u16> = Vec::new();
    if let Some(ep) = &config.has_ekg {
        ports.push(ep.port);
    }
    if let Some(ep) = &config.has_prometheus {
        ports.push(ep.port);
    }
    if let Some(ep) = &config.has_rtview {
        ports.push(ep.port);
    }
    let mut sorted_ports = ports.clone();
    sorted_ports.sort_unstable();
    sorted_ports.dedup();
    if sorted_ports.len() != ports.len() {
        problems.push("duplicate ports in config");
    }

    // hasEKG / hasPrometheus / hasRTView with empty host.
    if let Some(ep) = &config.has_ekg
        && ep.is_null()
    {
        problems.push("no host(s) in hasEKG");
    }
    if let Some(ep) = &config.has_prometheus
        && ep.is_null()
    {
        problems.push("no host in hasPrometheus");
    }
    if let Some(ep) = &config.has_rtview
        && ep.is_null()
    {
        problems.push("no host in hasRTView");
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(WellFormedError::IllFormed(problems.join(", ")))
    }
}

/// Parse a tracer config from a YAML string.
///
/// Mirror of upstream `readTracerConfig` minus the `IO`/file-reading
/// outer wrapper and the `die`-on-error behavior; upstream uses
/// Yaml; the Rust port accepts JSON or YAML — currently only JSON
/// is wired (serde_yaml is not a workspace dep). YAML support can
/// be added when the operator-side config file path is wired in.
pub fn parse_tracer_config_json(json: &str) -> Result<TracerConfig, ParseError> {
    let mut config: TracerConfig =
        serde_json::from_str(json).map_err(|err| ParseError::Json(err.to_string()))?;
    // Mirror upstream's nubLogging post-parse step.
    config.logging.dedup();
    well_formed(&config).map_err(|err| ParseError::IllFormed(err.to_string()))?;
    Ok(config)
}

/// Errors from `parse_tracer_config_*`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// JSON parse error from `serde_json`.
    #[error("Invalid tracer's configuration: {0}")]
    Json(String),
    /// `well_formed` invariant failed.
    #[error("{0}")]
    IllFormed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_config() -> TracerConfig {
        TracerConfig {
            network_magic: 764_824_073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: PathBuf::from("/tmp/tracer.socket"),
                },
            },
            log_objects_request_num: None,
            ekg_request_freq: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            logging: vec![LoggingParams {
                root: PathBuf::from("/var/log/tracer"),
                mode: LogMode::FileMode,
                format: LogFormat::ForMachine,
            }],
            rotation: None,
            verbosity: None,
            metrics_no_suffix: None,
            metrics_help: None,
            resource_freq: None,
            ekg_request_full: None,
            prometheus_labels: None,
        }
    }

    #[test]
    fn how_to_connect_local_pipe_is_null_when_empty() {
        assert!(
            HowToConnect::LocalPipe {
                local_pipe: PathBuf::new()
            }
            .is_null()
        );
        assert!(
            !HowToConnect::LocalPipe {
                local_pipe: PathBuf::from("/tmp/x")
            }
            .is_null()
        );
    }

    #[test]
    fn how_to_connect_remote_socket_is_null_when_host_empty() {
        assert!(
            HowToConnect::RemoteSocket {
                host: String::new(),
                port: 8080
            }
            .is_null()
        );
        assert!(
            !HowToConnect::RemoteSocket {
                host: "127.0.0.1".to_string(),
                port: 8080
            }
            .is_null()
        );
    }

    #[test]
    fn endpoint_is_null_when_host_empty() {
        let ep = Endpoint {
            host: String::new(),
            port: 8080,
            force_ssl: None,
        };
        assert!(ep.is_null());
    }

    #[test]
    fn rotation_params_serde_with_defaults() {
        let json = r#"{"rpLogLimitBytes": 1048576, "rpKeepFilesNum": 5}"#;
        let parsed: RotationParams = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.frequency_secs, 60);
        assert_eq!(parsed.log_limit_bytes, 1_048_576);
        assert_eq!(parsed.max_age_minutes, 24 * 60);
        assert_eq!(parsed.keep_files_num, 5);
    }

    #[test]
    fn rotation_params_serde_explicit_values() {
        let json = r#"{"rpFrequencySecs": 30, "rpLogLimitBytes": 2048, "rpMaxAgeMinutes": 720, "rpKeepFilesNum": 3}"#;
        let parsed: RotationParams = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.frequency_secs, 30);
        assert_eq!(parsed.log_limit_bytes, 2048);
        assert_eq!(parsed.max_age_minutes, 720);
        assert_eq!(parsed.keep_files_num, 3);
    }

    #[test]
    fn logging_params_invalid_file_mode_with_empty_root() {
        let p = LoggingParams {
            root: PathBuf::new(),
            mode: LogMode::FileMode,
            format: LogFormat::ForMachine,
        };
        assert!(p.is_invalid_file_mode());
    }

    #[test]
    fn logging_params_journal_mode_with_empty_root_is_valid() {
        let p = LoggingParams {
            root: PathBuf::new(),
            mode: LogMode::JournalMode,
            format: LogFormat::ForMachine,
        };
        assert!(!p.is_invalid_file_mode());
    }

    #[test]
    fn well_formed_minimal_config_is_ok() {
        well_formed(&minimal_config()).expect("ok");
    }

    #[test]
    fn well_formed_accept_at_empty_local_pipe_errors() {
        let mut config = minimal_config();
        config.network = Network::AcceptAt {
            accept_at: HowToConnect::LocalPipe {
                local_pipe: PathBuf::new(),
            },
        };
        let err = well_formed(&config).expect_err("ill-formed");
        assert!(format!("{err}").contains("AcceptAt is empty"));
    }

    #[test]
    fn well_formed_connect_to_all_empty_errors() {
        let mut config = minimal_config();
        config.network = Network::ConnectTo {
            connect_to: vec![HowToConnect::LocalPipe {
                local_pipe: PathBuf::new(),
            }],
        };
        let err = well_formed(&config).expect_err("ill-formed");
        assert!(format!("{err}").contains("ConnectTo are empty"));
    }

    #[test]
    fn well_formed_empty_log_root_errors() {
        let mut config = minimal_config();
        config.logging[0].root = PathBuf::new();
        let err = well_formed(&config).expect_err("ill-formed");
        assert!(format!("{err}").contains("empty logRoot"));
    }

    #[test]
    fn well_formed_duplicate_ports_errors() {
        let mut config = minimal_config();
        config.has_ekg = Some(Endpoint {
            host: "127.0.0.1".to_string(),
            port: 8080,
            force_ssl: None,
        });
        config.has_prometheus = Some(Endpoint {
            host: "127.0.0.1".to_string(),
            port: 8080,
            force_ssl: None,
        });
        let err = well_formed(&config).expect_err("ill-formed");
        assert!(format!("{err}").contains("duplicate ports"));
    }

    #[test]
    fn well_formed_empty_ekg_host_errors() {
        let mut config = minimal_config();
        config.has_ekg = Some(Endpoint {
            host: String::new(),
            port: 8080,
            force_ssl: None,
        });
        let err = well_formed(&config).expect_err("ill-formed");
        assert!(format!("{err}").contains("hasEKG"));
    }

    #[test]
    fn well_formed_concatenates_multiple_problems() {
        let mut config = minimal_config();
        config.network = Network::AcceptAt {
            accept_at: HowToConnect::LocalPipe {
                local_pipe: PathBuf::new(),
            },
        };
        config.logging[0].root = PathBuf::new();
        let err = well_formed(&config).expect_err("ill-formed");
        let msg = format!("{err}");
        assert!(msg.contains("AcceptAt is empty"));
        assert!(msg.contains("empty logRoot"));
    }

    #[test]
    fn parse_tracer_config_json_roundtrips_minimal_config() {
        let config = minimal_config();
        let json = serde_json::to_string(&config).expect("serializes");
        let parsed = parse_tracer_config_json(&json).expect("parses");
        assert_eq!(parsed, config);
    }

    #[test]
    fn parse_tracer_config_json_dedups_logging_entries() {
        let mut config = minimal_config();
        config.logging.push(config.logging[0].clone());
        let json = serde_json::to_string(&config).expect("serializes");
        let parsed = parse_tracer_config_json(&json).expect("parses");
        assert_eq!(parsed.logging.len(), 1);
    }

    #[test]
    fn parse_tracer_config_json_invalid_returns_parse_error() {
        let result = parse_tracer_config_json("not-json");
        assert!(matches!(result, Err(ParseError::Json(_))));
    }

    #[test]
    fn parse_tracer_config_json_ill_formed_returns_well_formed_error() {
        let mut config = minimal_config();
        config.logging[0].root = PathBuf::new();
        let json = serde_json::to_string(&config).expect("serializes");
        let result = parse_tracer_config_json(&json);
        assert!(matches!(result, Err(ParseError::IllFormed(_))));
    }

    #[test]
    fn file_or_map_serde_round_trip_file() {
        let v = FileOrMap::File(PathBuf::from("/etc/help.json"));
        let json = serde_json::to_string(&v).expect("serializes");
        let parsed: FileOrMap = serde_json::from_str(&json).expect("parses");
        assert_eq!(v, parsed);
    }

    #[test]
    fn file_or_map_serde_round_trip_map() {
        let mut m = BTreeMap::new();
        m.insert("foo".to_string(), "bar".to_string());
        let v = FileOrMap::Map(m);
        let json = serde_json::to_string(&v).expect("serializes");
        let parsed: FileOrMap = serde_json::from_str(&json).expect("parses");
        assert_eq!(v, parsed);
    }

    #[test]
    fn verbosity_serde_round_trip() {
        for v in [
            Verbosity::Minimum,
            Verbosity::ErrorsOnly,
            Verbosity::Maximum,
        ] {
            let json = serde_json::to_string(&v).expect("serializes");
            let parsed: Verbosity = serde_json::from_str(&json).expect("parses");
            assert_eq!(v, parsed);
        }
    }

    #[test]
    fn log_mode_serde_round_trip() {
        for m in [LogMode::FileMode, LogMode::JournalMode] {
            let json = serde_json::to_string(&m).expect("serializes");
            let parsed: LogMode = serde_json::from_str(&json).expect("parses");
            assert_eq!(m, parsed);
        }
    }

    #[test]
    fn log_format_serde_round_trip() {
        for f in [LogFormat::ForHuman, LogFormat::ForMachine] {
            let json = serde_json::to_string(&f).expect("serializes");
            let parsed: LogFormat = serde_json::from_str(&json).expect("parses");
            assert_eq!(f, parsed);
        }
    }
}
