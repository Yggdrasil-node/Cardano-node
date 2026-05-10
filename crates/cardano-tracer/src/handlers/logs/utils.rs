//! Log-file naming + timestamp helpers — shared between
//! the file-writer and the rotator subsystems.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Utils.hs.
//!
//! Direct port of upstream's bounded subset. The pure helpers
//! ship now; the IO-bound `createEmptyLogRotation` +
//! `createOrUpdateEmptyLog` defer pending the
//! `Cardano.Tracer.Utils.modifyRegistry_` port (which itself
//! requires the full `HandleRegistry` lock-hold semantics).
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `logPrefix = "node-"`                                          | [`LOG_PREFIX`]                         |
//! | `logExtension :: LogFormat -> String`                          | [`log_extension`]                      |
//! | `symLinkName :: LogFormat -> FilePath`                         | [`sym_link_name`]                      |
//! | `isItLog :: LogFormat -> FilePath -> Bool`                     | [`is_it_log`]                          |
//! | `getTimeStampFromLog :: FilePath -> Maybe UTCTime`             | [`get_timestamp_from_log`]             |
//! | `timeStampFormat = "%Y-%m-%dT%H-%M-%S"`                        | [`TIMESTAMP_FORMAT`]                   |
//! | `createEmptyLogRotation`                                       | (deferred — see [`log_rotation_status`]) |
//! | `createOrUpdateEmptyLog`                                       | (deferred — see [`log_rotation_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`createEmptyLogRotation` / `createOrUpdateEmptyLog`**: depend
//!   on `Cardano.Tracer.Utils.modifyRegistry_` (atomic
//!   read-modify-write under a `Control.Concurrent.Extra.Lock`)
//!   which isn't ported yet. The Yggdrasil-side `HandleRegistry` is
//!   `Arc<RwLock<HashMap<...>>>` from R371, so the equivalent
//!   would use `tokio::sync::RwLock::write_lock()`. Status
//!   surfaced via [`log_rotation_status`] for downstream callers.
//! - **`Data.Time.Clock.UTCTime`**: upstream returns a
//!   `Maybe UTCTime`; Yggdrasil returns `Option<i64>` (Unix epoch
//!   milliseconds, matching the [`crate::time::get_time_ms`]
//!   convention). Same information content; sites that need a
//!   structured datetime can render via
//!   [`super::super::notifications::send::format_event_timestamp`]
//!   in reverse.

use std::path::Path;

use crate::configuration::LogFormat;

/// Filename prefix shared by all rotated log files. Mirror of
/// upstream `logPrefix :: String; logPrefix = "node-"`.
pub const LOG_PREFIX: &str = "node-";

/// Strftime-style format for the timestamp embedded in rotated log
/// filenames. Mirror of upstream
/// `timeStampFormat :: String; timeStampFormat = "%Y-%m-%dT%H-%M-%S"`.
///
/// Note: the upstream format uses `T` as a literal separator and
/// `-` between the time components (instead of the more conventional
/// `:` colon). This keeps the format filesystem-friendly on
/// Windows/macOS where `:` is reserved.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H-%M-%S";

/// File extension for a given log format. Mirror of upstream
/// `logExtension :: LogFormat -> String`.
pub fn log_extension(format: LogFormat) -> &'static str {
    match format {
        LogFormat::ForHuman => ".log",
        LogFormat::ForMachine => ".json",
    }
}

/// Filename of the operator-facing symlink that always points at
/// the latest rotated log. Mirror of upstream `symLinkName`.
pub fn sym_link_name(format: LogFormat) -> String {
    format!("node{}", log_extension(format))
}

/// `True` when `path_to_log` matches the rotated-log naming
/// convention `node-YYYY-MM-DDTHH-MM-SS.<ext>`. Mirror of upstream
/// `isItLog :: LogFormat -> FilePath -> Bool`.
pub fn is_it_log(format: LogFormat, path_to_log: &Path) -> bool {
    let Some(file_name) = path_to_log.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !file_name.starts_with(LOG_PREFIX) {
        return false;
    }
    // Extension must include the leading dot to match log_extension.
    let expected_ext = log_extension(format);
    let path_obj = Path::new(file_name);
    let has_proper_ext = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let with_dot = format!(".{ext}");
            with_dot == expected_ext
        })
        .unwrap_or(false);
    if !has_proper_ext {
        return false;
    }
    let base = path_obj.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let timestamp = base.strip_prefix(LOG_PREFIX).unwrap_or("");
    parse_log_timestamp(timestamp).is_some()
}

/// Parse the timestamp embedded in a rotated-log filename and
/// return the Unix-epoch-millisecond representation. Mirror of
/// upstream `getTimeStampFromLog :: FilePath -> Maybe UTCTime`.
///
/// Returns `None` if the path's basename doesn't match the
/// rotated-log convention or the timestamp portion isn't a valid
/// `%Y-%m-%dT%H-%M-%S` rendering.
pub fn get_timestamp_from_log(path_to_log: &Path) -> Option<i64> {
    let file_name = path_to_log.file_name().and_then(|n| n.to_str())?;
    let path_obj = Path::new(file_name);
    let base = path_obj.file_stem().and_then(|s| s.to_str())?;
    let timestamp = base.strip_prefix(LOG_PREFIX)?;
    parse_log_timestamp(timestamp)
}

/// Parse upstream's `%Y-%m-%dT%H-%M-%S` timestamp string into
/// Unix-epoch milliseconds. Returns `None` on any malformed input
/// (mirroring upstream's `parseTimeM True ...` strict-parse behavior).
fn parse_log_timestamp(timestamp: &str) -> Option<i64> {
    // Expected shape: "YYYY-MM-DDTHH-MM-SS" (19 chars).
    let bytes = timestamp.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    // Positions: yyyy at 0..4, '-' at 4, mm at 5..7, '-' at 7,
    // dd at 8..10, 'T' at 10, hh at 11..13, '-' at 13, mm at 14..16,
    // '-' at 16, ss at 17..19.
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b'-'
        || bytes[16] != b'-'
    {
        return None;
    }
    let year = parse_uint(&timestamp[0..4])? as i64;
    let month = parse_uint(&timestamp[5..7])? as i64;
    let day = parse_uint(&timestamp[8..10])? as i64;
    let hour = parse_uint(&timestamp[11..13])? as i64;
    let minute = parse_uint(&timestamp[14..16])? as i64;
    let second = parse_uint(&timestamp[17..19])? as i64;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return None;
    }
    let days = ymd_to_days_since_epoch(year, month, day)?;
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(secs * 1000)
}

fn parse_uint(s: &str) -> Option<u64> {
    s.parse::<u64>().ok()
}

/// Convert (year, month, day) to the count of days since 1970-01-01.
/// Inverse of [`crate::handlers::notifications::send::format_event_timestamp`]'s
/// epoch-arithmetic. Returns `None` for invalid date components
/// (e.g. day 31 in February).
fn ymd_to_days_since_epoch(year: i64, month: i64, day: i64) -> Option<i64> {
    if !is_valid_date(year, month, day) {
        return None;
    }
    // Howard Hinnant's days_from_civil algorithm (public domain).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m_minus_3 = if month <= 2 { month + 9 } else { month - 3 };
    let doy = (153 * m_minus_3 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn is_valid_date(year: i64, month: i64, day: i64) -> bool {
    if !(1..=12).contains(&month) {
        return false;
    }
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // Gregorian leap-year rule.
            let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if is_leap { 29 } else { 28 }
        }
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

/// Status descriptor for the deferred IO-bound rotation helpers.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LogRotationStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing upstream port.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the rotation helpers.
pub fn log_rotation_status() -> LogRotationStatus {
    LogRotationStatus {
        status: "deferred",
        depends_on: "Cardano.Tracer.Utils.modifyRegistry_ (atomic registry update under Lock); HandleRegistry surface from R371",
        deferred_round: "R391+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_prefix_matches_upstream() {
        assert_eq!(LOG_PREFIX, "node-");
    }

    #[test]
    fn timestamp_format_matches_upstream() {
        assert_eq!(TIMESTAMP_FORMAT, "%Y-%m-%dT%H-%M-%S");
    }

    #[test]
    fn log_extension_for_human() {
        assert_eq!(log_extension(LogFormat::ForHuman), ".log");
    }

    #[test]
    fn log_extension_for_machine() {
        assert_eq!(log_extension(LogFormat::ForMachine), ".json");
    }

    #[test]
    fn sym_link_name_for_human_uses_log_extension() {
        assert_eq!(sym_link_name(LogFormat::ForHuman), "node.log");
    }

    #[test]
    fn sym_link_name_for_machine_uses_json_extension() {
        assert_eq!(sym_link_name(LogFormat::ForMachine), "node.json");
    }

    #[test]
    fn is_it_log_accepts_canonical_human_log() {
        assert!(is_it_log(
            LogFormat::ForHuman,
            Path::new("/var/log/node-2021-11-29T09-55-04.log"),
        ));
    }

    #[test]
    fn is_it_log_accepts_canonical_machine_log() {
        assert!(is_it_log(
            LogFormat::ForMachine,
            Path::new("/var/log/node-2021-11-29T09-55-04.json"),
        ));
    }

    #[test]
    fn is_it_log_rejects_wrong_extension() {
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("node-2021-11-29T09-55-04.json"),
        ));
        assert!(!is_it_log(
            LogFormat::ForMachine,
            Path::new("node-2021-11-29T09-55-04.log"),
        ));
    }

    #[test]
    fn is_it_log_rejects_missing_prefix() {
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("daemon-2021-11-29T09-55-04.log"),
        ));
    }

    #[test]
    fn is_it_log_rejects_malformed_timestamp() {
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("node-not-a-timestamp.log"),
        ));
    }

    #[test]
    fn is_it_log_rejects_invalid_calendar_date() {
        // February 30 is invalid.
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("node-2021-02-30T09-55-04.log"),
        ));
    }

    #[test]
    fn is_it_log_rejects_invalid_hour() {
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("node-2021-11-29T25-55-04.log"),
        ));
    }

    #[test]
    fn is_it_log_rejects_no_extension() {
        assert!(!is_it_log(
            LogFormat::ForHuman,
            Path::new("node-2021-11-29T09-55-04"),
        ));
    }

    #[test]
    fn get_timestamp_from_log_unix_epoch() {
        // node-1970-01-01T00-00-00.log → 0 ms.
        let path = Path::new("node-1970-01-01T00-00-00.log");
        assert_eq!(get_timestamp_from_log(path), Some(0));
    }

    #[test]
    fn get_timestamp_from_log_known_value() {
        // 2023-11-14T22-13-20 = 1700000000 secs = 1700000000000 ms.
        let path = Path::new("node-2023-11-14T22-13-20.json");
        assert_eq!(get_timestamp_from_log(path), Some(1_700_000_000_000));
    }

    #[test]
    fn get_timestamp_from_log_with_directory_prefix() {
        let path = Path::new("/var/log/cardano-tracer/node-2023-11-14T22-13-20.json");
        assert_eq!(get_timestamp_from_log(path), Some(1_700_000_000_000));
    }

    #[test]
    fn get_timestamp_from_log_returns_none_for_malformed() {
        let path = Path::new("node-not-a-timestamp.log");
        assert!(get_timestamp_from_log(path).is_none());
    }

    #[test]
    fn get_timestamp_from_log_returns_none_for_invalid_calendar_date() {
        let path = Path::new("node-2021-02-30T00-00-00.log");
        assert!(get_timestamp_from_log(path).is_none());
    }

    #[test]
    fn get_timestamp_from_log_returns_none_for_missing_prefix() {
        let path = Path::new("foo-2023-11-14T22-13-20.log");
        assert!(get_timestamp_from_log(path).is_none());
    }

    #[test]
    fn get_timestamp_from_log_handles_leap_year_feb_29() {
        // 2020-02-29 is a valid leap-day.
        let path = Path::new("node-2020-02-29T12-00-00.log");
        assert!(get_timestamp_from_log(path).is_some());
    }

    #[test]
    fn get_timestamp_from_log_rejects_feb_29_in_non_leap_year() {
        // 2021 is not a leap year.
        let path = Path::new("node-2021-02-29T12-00-00.log");
        assert!(get_timestamp_from_log(path).is_none());
    }

    #[test]
    fn ymd_to_days_round_trips_with_send_format_event_timestamp() {
        // Round-trip: ymd → days → ms → format_event_timestamp.
        use crate::handlers::notifications::send::format_event_timestamp;
        let days = ymd_to_days_since_epoch(2023, 11, 14).expect("valid date");
        let secs = days * 86_400 + 22 * 3_600 + 13 * 60 + 20;
        let ms = secs * 1000;
        assert_eq!(format_event_timestamp(ms), "2023-11-14 22:13:20 UTC");
    }

    #[test]
    fn log_rotation_status_describes_deferral() {
        let s = log_rotation_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("modifyRegistry_"));
        assert_eq!(s.deferred_round, "R391+");
    }
}
