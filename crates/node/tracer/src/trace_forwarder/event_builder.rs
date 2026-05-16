//! Convert a `tracing::Event` into a cardano-tracer-shaped
//! [`super::TraceObject`].
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side adapter that bridges the
//! `tracing` crate's event-emission API (used by every Rust crate in
//! the workspace) to the upstream cardano-tracer wire format. The
//! upstream Haskell side has no `tracing` equivalent; events arrive
//! already shaped as `Cardano.Logging.Types.TraceObject` from the
//! `contra-tracer` stack. Yggdrasil's `tracing::Event`-to-
//! `TraceObject` translation closes that gap.
//!
//! The `Layer<S>` that USES this builder is the still-pending sub-
//! item in `docs/TECH-DEBT.md` "cardano-tracer Mux Layer 2/3"; this
//! module ships the pure-data transform half so the layer
//! implementation can compose with the existing
//! [`super::TraceObject::to_cbor`],
//! [`super::mini_protocol::encode_reply`], and
//! [`super::bearer::Bearer::write_sdu`] stack without further
//! data-shape work.

use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{Event, Subscriber};
use tracing_subscriber::registry::LookupSpan;

use super::{TraceDetail, TraceObject, TraceSeverity};

/// Build a [`TraceObject`] from a `tracing::Event`.
///
/// - `to_human` is set to `None`; the existing JSON formatter
///   (`yggdrasil_telemetry::haskell_json`) already produces the
///   operator-readable rendering. cardano-tracer's `to_human` is
///   reserved for the same purpose; populating it twice would
///   double-emit.
/// - `to_machine` is a JSON object of every event field — the same
///   shape the Haskell-Katip formatter produces, so a single set of
///   downstream rules can parse either the stdout JSON line or the
///   forwarded TraceObject's `to_machine` payload.
/// - `to_namespace` is `target.split("::")`. Matches the upstream
///   convention: `Net.ChainSync.Server` → `["Net", "ChainSync",
///   "Server"]`.
/// - `to_severity` is derived from `Level`. TRACE collapses to
///   `Debug` because upstream `Cardano.Logging.Types.SeverityS` has
///   no TRACE; this matches the existing `haskell_json` mapping.
/// - `to_details` defaults to [`TraceDetail::DNormal`].
/// - `to_timestamp` is `(posix_seconds, picoseconds_of_second)`
///   derived from `SystemTime::now()` — the decomposition
///   `Codec.Serialise`'s `Serialise UTCTime` instance encodes.
/// - `to_hostname` is the configured value or `"yggdrasil"`. The
///   caller supplies this so the binary that owns the network-level
///   hostname doesn't get re-read on every event.
/// - `to_thread_id` is the thread name (when set via
///   `thread::Builder::name`) or the `{:?}`-formatted std thread Id.
pub fn build_trace_object_from_event<S>(event: &Event<'_>, hostname: &str) -> TraceObject
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let metadata = event.metadata();

    // to_namespace: target split on "::"
    let to_namespace: Vec<String> = metadata.target().split("::").map(str::to_string).collect();

    // to_severity: Level → SeverityS
    let to_severity = match *metadata.level() {
        tracing::Level::ERROR => TraceSeverity::Error,
        tracing::Level::WARN => TraceSeverity::Warning,
        tracing::Level::INFO => TraceSeverity::Info,
        tracing::Level::DEBUG => TraceSeverity::Debug,
        tracing::Level::TRACE => TraceSeverity::Debug,
    };

    // to_machine: JSON-encoded field map. Reuse the same JsonFieldVisitor
    // pattern haskell_json.rs uses so the field-set is identical.
    let mut fields = serde_json::Map::new();
    let mut visitor = MachineJsonFieldVisitor(&mut fields);
    event.record(&mut visitor);
    let to_machine = serde_json::Value::Object(fields).to_string();

    // to_timestamp: (year, dayOfYear, picosecondsOfDay) from now.
    let to_timestamp = build_trace_timestamp(SystemTime::now());

    // to_thread_id: thread name or `{:?}`-formatted Id.
    let to_thread_id = match std::thread::current().name() {
        Some(name) => name.to_string(),
        None => format!("{:?}", std::thread::current().id()),
    };

    TraceObject {
        to_human: None,
        to_machine,
        to_namespace,
        to_severity,
        to_details: TraceDetail::DNormal,
        to_timestamp,
        to_hostname: hostname.to_string(),
        to_thread_id,
    }
}

/// Compute the `(posix_seconds, picoseconds_of_second)` pair for a
/// `SystemTime` instant — the decomposition `Codec.Serialise`'s
/// `Serialise UTCTime` instance encodes.
///
/// Upstream's `encode` does `properFraction (utcTimeToPOSIXSeconds t)`
/// to split the POSIX time into a whole-seconds integer plus a
/// fractional part, then `psecs = round (frac * 1e12)`. This helper
/// produces the same pair: `posix_seconds` is whole seconds since the
/// 1970 epoch; `picoseconds_of_second` is the sub-second remainder
/// scaled to picoseconds (`0 ≤ psecs < 1_000_000_000_000`).
///
/// Note `SystemTime` resolution on most platforms is nanoseconds, so
/// the picosecond field is always a multiple of 1000.
pub fn build_trace_timestamp(when: SystemTime) -> (u64, u64) {
    let dur = when.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    // nanos → picoseconds (×1000); subsec_nanos is < 1e9 so the
    // result is < 1e12, satisfying the upstream `0 ≤ psecs < 1e12`.
    let picos_of_sec = u64::from(dur.subsec_nanos()) * 1_000;
    (secs, picos_of_sec)
}

/// Tracing field visitor that emits each field as a JSON value into
/// a target Map. Mirrors `haskell_json::JsonFieldVisitor` so the
/// `to_machine` payload shape matches the stdout JSON shape exactly.
struct MachineJsonFieldVisitor<'a>(&'a mut serde_json::Map<String, serde_json::Value>);

impl tracing::field::Visit for MachineJsonFieldVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if let Some(n) = serde_json::Number::from_f64(value) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(n));
        } else {
            self.0.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::String(format!("{value:?}")),
        );
    }
}

#[cfg(test)]
mod event_builder_tests {
    use super::*;

    /// `(posix_seconds, picoseconds_of_second)` for known UTC
    /// instants — whole-second times produce a zero picosecond field.
    #[test]
    fn timestamp_posix_secs_known_dates() {
        // 2026-01-01T00:00:00Z = 1_767_225_600 POSIX seconds.
        let unix_2026_01_01 =
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_767_225_600);
        assert_eq!(build_trace_timestamp(unix_2026_01_01), (1_767_225_600, 0));

        // The 1970 epoch itself → (0, 0).
        assert_eq!(build_trace_timestamp(SystemTime::UNIX_EPOCH), (0, 0));
    }

    /// The sub-second remainder is scaled nanoseconds → picoseconds
    /// (×1000), and the whole-seconds component is unaffected.
    #[test]
    fn timestamp_picoseconds_of_second() {
        // 2026-05-15T00:00:00Z + 789 ms.
        let when = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_778_803_200)
            + std::time::Duration::from_millis(789);
        let (secs, picos) = build_trace_timestamp(when);
        assert_eq!(secs, 1_778_803_200);
        // 789 ms = 789_000_000 ns → ×1000 = 789_000_000_000 ps.
        assert_eq!(picos, 789_000_000_000);
        // Picoseconds of a second must stay strictly below 1e12.
        assert!(picos < 1_000_000_000_000);

        // A nanosecond-resolution remainder scales by exactly 1000.
        let when_ns = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(100)
            + std::time::Duration::from_nanos(123_456_789);
        assert_eq!(build_trace_timestamp(when_ns), (100, 123_456_789_000));
    }

    /// A built `TraceObject`'s timestamp survives a `to_cbor` /
    /// `from_cbor_bytes` round-trip — proves the new
    /// `(secs, picos)` pair lines up with the `Serialise UTCTime`
    /// codec end-to-end.
    #[test]
    fn built_trace_object_timestamp_round_trips_through_cbor() {
        let when = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_778_803_200)
            + std::time::Duration::from_millis(456);
        let ts = build_trace_timestamp(when);
        let obj = TraceObject {
            to_human: None,
            to_machine: "{}".into(),
            to_namespace: vec!["Yggdrasil".into()],
            to_severity: TraceSeverity::Info,
            to_details: TraceDetail::DNormal,
            to_timestamp: ts,
            to_hostname: "h".into(),
            to_thread_id: "t".into(),
        };
        let decoded = TraceObject::from_cbor_bytes(&obj.to_cbor()).expect("round-trip");
        assert_eq!(decoded.to_timestamp, ts);
        assert_eq!(decoded, obj);
    }
}
