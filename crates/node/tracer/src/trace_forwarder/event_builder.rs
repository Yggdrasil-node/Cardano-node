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
/// - `to_timestamp` is `(year, dayOfYear, picosecondsOfDay)` derived
///   from `SystemTime::now()`.
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

/// Compute the `(year, dayOfYear, picosecondsOfDay)` triple for a
/// `SystemTime` instant.
///
/// `year` is the Gregorian calendar year; `dayOfYear` is 1-indexed
/// (Jan 1 = 1); `picosecondsOfDay` is total picoseconds since 00:00:00
/// UTC of the same day.
///
/// Uses the Howard Hinnant civil-date algorithm to convert seconds-
/// since-epoch into a calendar date without a chrono / time dep.
pub fn build_trace_timestamp(when: SystemTime) -> (u64, u64, u64) {
    let dur = when.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let nanos_of_sec = dur.subsec_nanos();

    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;

    // Civil-date conversion (Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y_anchor_mar = (yoe as i64) + era * 400;
    let doy_anchor_mar = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy_anchor_mar + 2) / 153;
    let d = doy_anchor_mar - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 {
        y_anchor_mar + 1
    } else {
        y_anchor_mar
    };

    // Day-of-year from (year, month, day):
    // cumulative days at start of each month in a non-leap year.
    const CUM_DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap_add = if is_leap_year(year) && m > 2 { 1 } else { 0 };
    let day_of_year = CUM_DAYS[(m - 1) as usize] + d as u64 + leap_add;

    // picosecondsOfDay = (seconds_of_day * 1e12) + (nanos * 1000).
    let picos_of_day = secs_of_day * 1_000_000_000_000 + (nanos_of_sec as u64) * 1_000;

    (year as u64, day_of_year, picos_of_day)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
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

    /// Year-day-of-year-picosecond conversion for a known UTC
    /// instant. 2026-01-01T00:00:00Z → (2026, 1, 0). 2026-12-31T23:59:59Z
    /// is day-of-year 365 (2026 is non-leap). Leap-year check:
    /// 2024-12-31T00:00:00Z → (2024, 366, 0).
    #[test]
    fn timestamp_year_doy_picos_known_dates() {
        // 2026-01-01T00:00:00Z → unix epoch + (56 years from 1970-01-01).
        // Compute via std::time arithmetic to avoid chrono dep.
        let unix_2026_01_01 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1767225600); // 2026-01-01T00:00:00Z
        let (year, doy, picos) = build_trace_timestamp(unix_2026_01_01);
        assert_eq!(year, 2026);
        assert_eq!(doy, 1);
        assert_eq!(picos, 0);

        // 2024-12-31T00:00:00Z. Day-of-year 366 (leap).
        let unix_2024_12_31 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1735603200);
        let (year, doy, picos) = build_trace_timestamp(unix_2024_12_31);
        assert_eq!(year, 2024);
        assert_eq!(doy, 366);
        assert_eq!(picos, 0);

        // 2025-12-31T00:00:00Z. Day-of-year 365 (non-leap).
        let unix_2025_12_31 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1767139200);
        let (year, doy, _) = build_trace_timestamp(unix_2025_12_31);
        assert_eq!(year, 2025);
        assert_eq!(doy, 365);
    }

    /// Picoseconds-of-day at 12:34:56.789_000_000 UTC on a known day.
    #[test]
    fn timestamp_picos_of_day_at_midday() {
        // 2026-05-15T12:34:56.789Z = 2026-05-15T00:00:00Z + (12*3600
        // + 34*60 + 56) seconds + 789 millis.
        let base = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1778803200); // 2026-05-15T00:00:00Z
        let when = base
            + std::time::Duration::from_secs(12 * 3600 + 34 * 60 + 56)
            + std::time::Duration::from_millis(789);
        let (_year, _doy, picos) = build_trace_timestamp(when);
        // (45296 secs) * 1e12 + 789_000_000 nanos * 1000.
        let expected = (12_u64 * 3600 + 34 * 60 + 56) * 1_000_000_000_000 + 789_000_000 * 1_000;
        assert_eq!(picos, expected);
    }

    /// Leap-year helper handles century rule.
    #[test]
    fn is_leap_year_examples() {
        assert!(is_leap_year(2024)); // div by 4, not by 100
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(1900)); // div by 100, not by 400
        assert!(is_leap_year(2000)); // div by 400
        assert!(is_leap_year(2400)); // div by 400
    }
}
