//! Trace-event payload type — synthesis stand-in for upstream
//! `Cardano.Logging.TraceObject` until the `trace-dispatcher`
//! package is vendored.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side synthesis of the upstream
//! `Cardano.Logging.TraceObject` record. The upstream package is in
//! the `trace-dispatcher` repository which is **not** vendored at
//! `.reference-haskell-cardano-node/`; the field set here is
//! recovered from upstream's exhaustive field-accesses in
//! `cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Journal/Systemd.hs::mkJournalFields`
//! and `cardano-tracer/src/Cardano/Tracer/Handlers/Logs/File.hs::traceTextForHuman`/`traceTextForMachine`.
//!
//! When the `trace-dispatcher` package is eventually vendored, this
//! file should be retired in favour of a strict 1:1 port at the
//! equivalent path.
//!
//! ## Field set (recovered from upstream usage)
//!
//! | Upstream field             | Yggdrasil field        | Rationale                                              |
//! |----------------------------|------------------------|--------------------------------------------------------|
//! | `toHuman :: Maybe Text`    | `to_human: Option<String>` | Pretty-rendered message for human readers (optional). |
//! | `toMachine :: Text`        | `to_machine: String`   | Structured machine-rendered payload (required).        |
//! | `toSeverity :: SeverityS`  | `to_severity: SeverityS` | Already exported from [`crate::severity`] (R380).      |
//! | `toNamespace :: [Text]`    | `to_namespace: Vec<String>` | Hierarchical namespace path (e.g. `["BlockFetch", "Server"]`). |
//! | `toThreadId :: Text`       | `to_thread_id: String` | Originating thread identifier (string-formatted).      |
//! | `toTimestamp :: UTCTime`   | `to_timestamp_ms: i64` | Unix-epoch milliseconds (matches [`crate::time::get_time_ms`] convention). |
//!
//! ## Carve-outs (NOT ported, by design)
//!
//! - **`Cardano.Logging.LogFormatting` typeclass methods**: upstream's
//!   `forHuman`/`forMachine` instances live on the *source* type
//!   (each event-emitter has its own `instance LogFormatting`).
//!   Yggdrasil-side equivalents are local to each emit site.
//! - **`Data.Time.UTCTime` precision**: upstream stores nanoseconds;
//!   Yggdrasil stores milliseconds (matching the rest of the
//!   cardano-tracer crate's wall-clock convention from
//!   [`crate::time::get_time_ms`]). The 6-orders-of-magnitude
//!   precision drop is intentional — operational tracer output
//!   (logs + journal + Prometheus + EKG) never needs sub-millisecond
//!   timestamps; the upstream nanosecond field exists for
//!   `Data.Time.Format.formatTime` ISO-8601 fractional rendering
//!   which we replace with the "%F %T UTC" 1-second precision used
//!   in [`crate::handlers::notifications::send::format_event_timestamp`].

use crate::severity::SeverityS;

/// Single trace-forwarder event payload. Synthesis stand-in for
/// upstream `Cardano.Logging.TraceObject`.
///
/// The struct is constructed by trace-forwarder protocol acceptors
/// (R411+ pending) and consumed by:
///
/// - [`crate::handlers::logs::journal`] sink (already wired at R382
///   as a no-op; functional Systemd port deferred per workspace
///   no-FFI policy).
/// - File-handler [`crate::handlers::logs::file`] (R400 pending).
/// - Trace-objects dispatcher [`crate::handlers::logs::trace_objects`]
///   (R401 pending).
/// - Notification-engine `addNewEvent` chain (already wired at R381).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TraceObject {
    /// Optional pretty-rendered message. `None` means "fall back to
    /// `to_machine`" per upstream's `traceTextForHuman` convention
    /// (`fromMaybe toMachine toHuman`).
    pub to_human: Option<String>,
    /// Always-present structured-rendered payload (typically JSON).
    pub to_machine: String,
    /// Severity level for downstream filter dispatch.
    pub to_severity: SeverityS,
    /// Hierarchical namespace path. Order is root→leaf (e.g.
    /// `["BlockFetch", "Server", "Acquired"]`).
    pub to_namespace: Vec<String>,
    /// Originating thread identifier as a free-form string. Upstream
    /// formats `ThreadId 4` etc.; Yggdrasil-side acceptors will
    /// format `tokio::task::id()` similarly.
    pub to_thread_id: String,
    /// Event timestamp — Unix-epoch milliseconds (matches
    /// [`crate::time::get_time_ms`] convention).
    pub to_timestamp_ms: i64,
}

impl TraceObject {
    /// Construct an event with all fields explicit. Production sites
    /// that read from a live trace-forwarder packet build the struct
    /// directly; this constructor is here for tests + synthetic
    /// emit sites.
    pub fn new(
        to_human: Option<String>,
        to_machine: String,
        to_severity: SeverityS,
        to_namespace: Vec<String>,
        to_thread_id: String,
        to_timestamp_ms: i64,
    ) -> Self {
        TraceObject {
            to_human,
            to_machine,
            to_severity,
            to_namespace,
            to_thread_id,
            to_timestamp_ms,
        }
    }

    /// The text to render for human-readable sinks. Mirror of
    /// upstream's `traceTextForHuman :: TraceObject -> Text`
    /// (`fromMaybe toMachine toHuman`).
    pub fn render_for_human(&self) -> &str {
        self.to_human.as_deref().unwrap_or(&self.to_machine)
    }

    /// The text to render for machine-readable sinks. Mirror of
    /// upstream's `traceTextForMachine :: TraceObject -> Text`
    /// (always returns `toMachine`).
    pub fn render_for_machine(&self) -> &str {
        &self.to_machine
    }

    /// Render the namespace as a dot-separated string (e.g.
    /// `"BlockFetch.Server.Acquired"`). Used by the journal +
    /// systemd-journal sinks for the `namespace` field.
    pub fn namespace_dotted(&self) -> String {
        self.to_namespace.join(".")
    }
}

impl Default for TraceObject {
    /// Construct an all-defaults event. Useful as a placeholder in
    /// tests + synthesis sites. Severity defaults to `Debug` per
    /// `SeverityS::default()`.
    fn default() -> Self {
        TraceObject {
            to_human: None,
            to_machine: String::new(),
            to_severity: SeverityS::default(),
            to_namespace: Vec::new(),
            to_thread_id: String::new(),
            to_timestamp_ms: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec — R437 synthesis carve-out
// ---------------------------------------------------------------------------
//
// **Strict mirror:** none. Yggdrasil-side synthesis carve-out
// for the upstream `Cardano.Logging.TraceObject` Serialise
// instance. Source isn't vendored under
// `.reference-haskell-cardano-node/` (the cardano-logging package
// is a Hackage dep of cardano-node, not a vendored sibling); R437
// ships a Yggdrasil-canonical wire shape that round-trips
// internally without claiming byte-equivalence to upstream.
//
// **Operator caveat:** an operator wiring yggdrasil cardano-tracer
// against a yggdrasil-equivalent forwarder will see consistent
// traffic. An operator wiring yggdrasil cardano-tracer against an
// **upstream cardano-node** will NOT — the upstream Serialise
// instance encodes differently (the cardano-logging Hackage source
// would need to be consulted to derive the byte-exact wire shape).
// This is documented as a follow-on parity-arc item.
//
// Wire format (Yggdrasil canonical):
//   `[to_human (text or null), to_machine (text), to_severity_code
//   (uint 0-7 per RFC 5424), to_namespace (text array),
//   to_thread_id (text), to_timestamp_ms (int)]`
//
// The 6-element CBOR array is more compact than a string-keyed
// CBOR map; tests in this file lock the byte format down so a
// careless edit can't silently break the wire.

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

impl TraceObject {
    /// Encode this trace object to CBOR bytes. R437 ships a
    /// Yggdrasil-canonical 6-field array shape; see module-level
    /// CBOR-codec comment for the operator caveat about upstream-
    /// byte-equivalence.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.array(6);
        // Field 0: to_human (text or null).
        match &self.to_human {
            Some(s) => {
                enc.text(s);
            }
            None => {
                enc.null();
            }
        }
        // Field 1: to_machine (text).
        enc.text(&self.to_machine);
        // Field 2: to_severity_code (uint 0-7).
        enc.unsigned(u64::from(self.to_severity.syslog_code()));
        // Field 3: to_namespace (text array).
        enc.array(self.to_namespace.len() as u64);
        for ns in &self.to_namespace {
            enc.text(ns);
        }
        // Field 4: to_thread_id (text).
        enc.text(&self.to_thread_id);
        // Field 5: to_timestamp_ms (int).
        // CBOR encodes negative ints with major type 1; we use
        // signed() rather than unsigned() to handle pre-1970
        // timestamps cleanly. (Yggdrasil's tracer ts is Unix-
        // epoch ms; pre-epoch values are operationally improbable
        // but the wire is symmetric.)
        if self.to_timestamp_ms >= 0 {
            enc.unsigned(self.to_timestamp_ms as u64);
        } else {
            enc.signed(self.to_timestamp_ms);
        }
        enc.into_bytes()
    }

    /// Decode a trace object from CBOR bytes. R437 mirror of
    /// [`Self::to_cbor`]. Returns [`LedgerError::CborInvalidLength`]
    /// if the array shape doesn't match the 6-field Yggdrasil-
    /// canonical layout.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(bytes);
        let len = dec.array()?;
        if len != 6 {
            return Err(LedgerError::CborInvalidLength {
                expected: 6,
                actual: len as usize,
            });
        }
        // Field 0: to_human (text or null).
        let to_human = if dec.peek_is_null() {
            dec.null()?;
            None
        } else {
            Some(dec.text()?.to_owned())
        };
        // Field 1: to_machine.
        let to_machine = dec.text()?.to_owned();
        // Field 2: to_severity_code.
        let code = dec.unsigned()? as u8;
        let to_severity = SeverityS::from_syslog_code(code).ok_or_else(|| {
            LedgerError::CborDecodeError(format!(
                "TraceObject: invalid syslog severity code {code} (must be 0-7)"
            ))
        })?;
        // Field 3: to_namespace.
        let ns_len = dec.array()?;
        let mut to_namespace = Vec::with_capacity(ns_len as usize);
        for _ in 0..ns_len {
            to_namespace.push(dec.text()?.to_owned());
        }
        // Field 4: to_thread_id.
        let to_thread_id = dec.text()?.to_owned();
        // Field 5: to_timestamp_ms.
        let to_timestamp_ms = dec.signed()?;
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(TraceObject {
            to_human,
            to_machine,
            to_severity,
            to_namespace,
            to_thread_id,
            to_timestamp_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> TraceObject {
        TraceObject::new(
            Some("BlockFetch acquired block 8675309".to_string()),
            r#"{"event":"BlockFetchAcquired","block":8675309}"#.to_string(),
            SeverityS::Info,
            vec![
                "BlockFetch".to_string(),
                "Server".to_string(),
                "Acquired".to_string(),
            ],
            "tokio-task-42".to_string(),
            1_700_000_000_000,
        )
    }

    #[test]
    fn new_builds_event_with_all_fields() {
        let event = sample_event();
        assert!(event.to_human.is_some());
        assert!(event.to_machine.starts_with('{'));
        assert_eq!(event.to_severity, SeverityS::Info);
        assert_eq!(event.to_namespace.len(), 3);
        assert_eq!(event.to_thread_id, "tokio-task-42");
        assert_eq!(event.to_timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn default_uses_debug_severity_and_empty_strings() {
        let event = TraceObject::default();
        assert!(event.to_human.is_none());
        assert!(event.to_machine.is_empty());
        assert_eq!(event.to_severity, SeverityS::Debug);
        assert!(event.to_namespace.is_empty());
        assert!(event.to_thread_id.is_empty());
        assert_eq!(event.to_timestamp_ms, 0);
    }

    #[test]
    fn render_for_human_uses_to_human_when_present() {
        let event = sample_event();
        assert_eq!(
            event.render_for_human(),
            "BlockFetch acquired block 8675309",
        );
    }

    #[test]
    fn render_for_human_falls_back_to_machine_when_human_is_none() {
        let mut event = sample_event();
        event.to_human = None;
        assert_eq!(
            event.render_for_human(),
            r#"{"event":"BlockFetchAcquired","block":8675309}"#,
        );
    }

    #[test]
    fn render_for_machine_always_returns_to_machine() {
        let event = sample_event();
        assert_eq!(
            event.render_for_machine(),
            r#"{"event":"BlockFetchAcquired","block":8675309}"#,
        );
        // Even with to_human present, render_for_machine returns
        // to_machine — never falls back.
        let mut event2 = sample_event();
        event2.to_machine = "different".to_string();
        assert_eq!(event2.render_for_machine(), "different");
    }

    #[test]
    fn namespace_dotted_joins_with_periods() {
        let event = sample_event();
        assert_eq!(event.namespace_dotted(), "BlockFetch.Server.Acquired");
    }

    #[test]
    fn namespace_dotted_handles_empty_namespace() {
        let event = TraceObject::default();
        assert_eq!(event.namespace_dotted(), "");
    }

    #[test]
    fn namespace_dotted_handles_single_element() {
        let event = TraceObject {
            to_namespace: vec!["Solo".to_string()],
            ..TraceObject::default()
        };
        assert_eq!(event.namespace_dotted(), "Solo");
    }

    #[test]
    fn equality_works_across_clones() {
        let event = sample_event();
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn inequality_works_when_severity_differs() {
        let mut event1 = sample_event();
        let mut event2 = sample_event();
        event1.to_severity = SeverityS::Info;
        event2.to_severity = SeverityS::Error;
        assert_ne!(event1, event2);
    }

    // R437 CBOR codec round-trip tests ----------------------------------

    #[test]
    fn cbor_round_trips_full_event() {
        let event = sample_event();
        let bytes = event.to_cbor();
        let decoded = TraceObject::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, event);
    }

    #[test]
    fn cbor_round_trips_event_with_no_human_render() {
        let mut event = sample_event();
        event.to_human = None;
        let bytes = event.to_cbor();
        let decoded = TraceObject::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, event);
        assert!(decoded.to_human.is_none());
    }

    #[test]
    fn cbor_round_trips_default_event() {
        let event = TraceObject::default();
        let bytes = event.to_cbor();
        let decoded = TraceObject::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, event);
    }

    #[test]
    fn cbor_round_trips_each_severity_level() {
        let severities = [
            SeverityS::Debug,
            SeverityS::Info,
            SeverityS::Notice,
            SeverityS::Warning,
            SeverityS::Error,
            SeverityS::Critical,
            SeverityS::Alert,
            SeverityS::Emergency,
        ];
        for sev in severities {
            let mut event = sample_event();
            event.to_severity = sev;
            let bytes = event.to_cbor();
            let decoded = TraceObject::from_cbor(&bytes).expect("decode");
            assert_eq!(decoded.to_severity, sev);
        }
    }

    #[test]
    fn cbor_round_trips_pre_epoch_negative_timestamp() {
        let mut event = sample_event();
        event.to_timestamp_ms = -1_500_000_000_000;
        let bytes = event.to_cbor();
        let decoded = TraceObject::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded.to_timestamp_ms, -1_500_000_000_000);
    }

    #[test]
    fn cbor_round_trips_empty_namespace() {
        let mut event = sample_event();
        event.to_namespace.clear();
        let bytes = event.to_cbor();
        let decoded = TraceObject::from_cbor(&bytes).expect("decode");
        assert!(decoded.to_namespace.is_empty());
    }

    #[test]
    fn cbor_decode_rejects_wrong_array_length() {
        // 5-element array — must reject.
        let mut enc = Encoder::new();
        enc.array(5)
            .text("h")
            .text("m")
            .unsigned(6)
            .array(0)
            .text("t");
        let bytes = enc.into_bytes();
        let result = TraceObject::from_cbor(&bytes);
        assert!(matches!(
            result,
            Err(LedgerError::CborInvalidLength {
                expected: 6,
                actual: 5
            })
        ));
    }

    #[test]
    fn cbor_decode_rejects_invalid_severity_code() {
        // 6-element array with severity 99 (out of 0-7 range).
        let mut enc = Encoder::new();
        enc.array(6)
            .null()
            .text("m")
            .unsigned(99)
            .array(0)
            .text("t")
            .unsigned(0);
        let bytes = enc.into_bytes();
        let result = TraceObject::from_cbor(&bytes);
        match result {
            Err(LedgerError::CborDecodeError(msg)) => {
                assert!(msg.contains("invalid syslog severity code 99"));
            }
            other => panic!("expected CborDecodeError, got {other:?}"),
        }
    }

    #[test]
    fn cbor_decode_rejects_trailing_bytes() {
        let event = sample_event();
        let mut bytes = event.to_cbor();
        bytes.push(0xFF);
        let result = TraceObject::from_cbor(&bytes);
        assert!(matches!(result, Err(LedgerError::CborTrailingBytes(_))));
    }
}
