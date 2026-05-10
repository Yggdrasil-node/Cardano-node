//! Log-file naming + timestamp helpers — shared between
//! the file-writer and the rotator subsystems.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Utils.hs.
//!
//! Direct port of upstream's surface. As of R402, both the pure
//! helpers + the IO-bound `createEmptyLogRotation` +
//! `createOrUpdateEmptyLog` ship — the latter using
//! `Arc<tokio::sync::Mutex<()>>` for the write-lock and the
//! upgraded `Registry<_, (SharedLogFile, PathBuf)>` from R402's
//! `crate::types::HandleRegistry` upgrade.
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
//! | `createEmptyLogRotation`                                       | [`create_empty_log_rotation`]          |
//! | `createOrUpdateEmptyLog`                                       | [`create_or_update_empty_log`]         |
//! | `updateSymlinkAtomically` (private)                            | inline within [`create_or_update_empty_log`] |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Data.Time.Clock.UTCTime`**: upstream returns a
//!   `Maybe UTCTime`; Yggdrasil returns `Option<i64>` (Unix epoch
//!   milliseconds, matching the [`crate::time::get_time_ms`]
//!   convention). Same information content; sites that need a
//!   structured datetime can render via
//!   [`super::super::notifications::send::format_event_timestamp`]
//!   in reverse.
//! - **Windows `createFileLink` (NTFS junctions)**: upstream uses
//!   `System.Directory.createFileLink` which on Windows creates a
//!   directory junction. Yggdrasil's
//!   [`update_symlink_atomically`] uses `std::os::unix::fs::symlink`
//!   on Unix; the non-Unix branch falls back to writing the target
//!   path as plain text (cardano-tracer is operationally Unix-only
//!   per workspace policy).

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

/// Format a Unix-epoch-millisecond timestamp into upstream's
/// rotated-log timestamp shape (`%Y-%m-%dT%H-%M-%S`). Inverse of
/// [`get_timestamp_from_log`]'s parser. Used by
/// [`create_or_update_empty_log`] when minting a fresh log filename.
pub fn format_log_timestamp(time_ms: i64) -> String {
    let total_secs = time_ms.div_euclid(1000);
    let days = total_secs.div_euclid(86_400);
    let secs_within_day = total_secs.rem_euclid(86_400);
    let h = secs_within_day / 3_600;
    let m = (secs_within_day % 3_600) / 60;
    let s = secs_within_day % 60;
    let (year, month, day) = days_since_epoch_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}-{m:02}-{s:02}")
}

fn days_since_epoch_to_ymd(days: i64) -> (i32, u32, u32) {
    // Mirror of crate::handlers::notifications::send::days_since_epoch_to_ymd
    // (Howard Hinnant's civil_from_days). Duplicated here rather than
    // exposed publicly because the function is implementation detail
    // of the timestamp formatters in two unrelated subsystems.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

/// Errors returned by the log-rotation helpers.
#[derive(Debug, thiserror::Error)]
pub enum LogRotationError {
    /// I/O error during file open / creation / symlink update.
    #[error("log-rotation IO failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Create a fresh log file under `sub_dir_for_logs` with the current
/// timestamp embedded in the filename, register the resulting handle
/// in the [`crate::types::HandleRegistry`], and update the
/// `node.<ext>` symlink atomically. Mirror of upstream
/// `createOrUpdateEmptyLog`.
///
/// The function:
/// 1. Mints `<sub_dir>/node-YYYY-MM-DDTHH-MM-SS.<ext>` using
///    [`format_log_timestamp`] for the timestamp + [`log_extension`]
///    for the format-specific extension.
/// 2. Opens the file write-only, truncating any pre-existing file.
/// 3. If a previous handle was registered under `key`, drops it
///    (the underlying file descriptor is closed when the last `Arc`
///    reference goes out of scope).
/// 4. Inserts the new `(handle, path)` pair into the registry.
/// 5. Atomically swaps the `<sub_dir>/node.<ext>` symlink to point at
///    the new file.
///
/// Acquires `current_log_lock` for the duration of the operation
/// (matches upstream's `withLock currentLogLock do ...`).
pub async fn create_or_update_empty_log(
    current_log_lock: &std::sync::Arc<tokio::sync::Mutex<()>>,
    key: &crate::types::HandleRegistryKey,
    registry: &crate::types::HandleRegistry,
    sub_dir_for_logs: &Path,
    now_ms: i64,
) -> Result<std::path::PathBuf, LogRotationError> {
    let _guard = current_log_lock.lock().await;
    let format = key.1.format;
    let ts = format_log_timestamp(now_ms);
    let filename = format!("{LOG_PREFIX}{ts}{}", log_extension(format));
    let path_to_log = sub_dir_for_logs.join(&filename);

    // Ensure the parent directory exists (mirror of upstream's
    // `createDirectoryIfMissing True subDirForLogs` from
    // `createEmptyLogRotation`).
    if let Some(parent) = path_to_log.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Open the new log file for writing (truncate semantics match
    // upstream's `openFile WriteMode`).
    let new_handle = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path_to_log)
        .await?;
    let shared_handle: crate::types::SharedLogFile =
        std::sync::Arc::new(tokio::sync::Mutex::new(new_handle));

    // Replace the existing registry entry (if any). Dropping the
    // previous Arc<Mutex<File>> closes the old file descriptor.
    let _previous = registry.insert(key.clone(), (shared_handle, path_to_log.clone()));

    // Atomically swap the convenience symlink — wrap in spawn_blocking
    // since std::fs::rename / symlink are blocking.
    let symlink_format = format;
    let path_for_symlink = path_to_log.clone();
    let sub_dir_owned = sub_dir_for_logs.to_path_buf();
    tokio::task::spawn_blocking(move || {
        update_symlink_atomically(symlink_format, &sub_dir_owned, &path_for_symlink)
    })
    .await
    .map_err(|join_err| std::io::Error::other(format!("spawn_blocking: {join_err}")))??;

    Ok(path_to_log)
}

/// Convenience wrapper mirroring upstream `createEmptyLogRotation`:
/// ensures the sub-directory exists, then delegates to
/// [`create_or_update_empty_log`].
pub async fn create_empty_log_rotation(
    current_log_lock: &std::sync::Arc<tokio::sync::Mutex<()>>,
    key: &crate::types::HandleRegistryKey,
    registry: &crate::types::HandleRegistry,
    sub_dir_for_logs: &Path,
    now_ms: i64,
) -> Result<std::path::PathBuf, LogRotationError> {
    tokio::fs::create_dir_all(sub_dir_for_logs).await?;
    create_or_update_empty_log(current_log_lock, key, registry, sub_dir_for_logs, now_ms).await
}

/// Atomic symlink swap — mirror of upstream
/// `updateSymlinkAtomically`. Removes the `node.<ext>.tmp` file if
/// present, creates a fresh symlink at that path pointing at
/// `path_to_log`, then renames the tmp link onto `node.<ext>`. The
/// rename is atomic on POSIX filesystems; mid-operation crash
/// recovery is safe (operators only ever see either the old symlink
/// or the new one, never a missing or half-written link).
fn update_symlink_atomically(
    format: crate::configuration::LogFormat,
    sub_dir_for_logs: &Path,
    path_to_log: &Path,
) -> Result<(), LogRotationError> {
    let symlink_name = sym_link_name(format);
    let symlink = sub_dir_for_logs.join(&symlink_name);
    let symlink_tmp = sub_dir_for_logs.join(format!("{symlink_name}.tmp"));

    // Best-effort cleanup of any stale tmp file.
    match std::fs::remove_file(&symlink_tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(LogRotationError::Io(e)),
    }

    // Create the new tmp symlink.
    #[cfg(unix)]
    std::os::unix::fs::symlink(path_to_log, &symlink_tmp)?;
    #[cfg(not(unix))]
    {
        // Windows fallback: createFileLink in upstream uses NTFS
        // junctions. Rust's std::os::windows::fs::symlink_file
        // requires admin privileges in some configurations. For now,
        // copy the path to avoid a hard symlink failure on non-Unix
        // hosts; the node is operationally Unix-only per workspace
        // policy.
        std::fs::write(&symlink_tmp, path_to_log.to_string_lossy().as_bytes())?;
    }

    // Atomically rename onto the canonical symlink path.
    std::fs::rename(&symlink_tmp, &symlink)?;
    Ok(())
}

/// Status descriptor for the previously-deferred IO-bound rotation
/// helpers. Kept around so call sites that still query for the
/// status can see the round at which the closure landed.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LogRotationStatus {
    /// One-line summary of the deferral status.
    pub status: &'static str,
    /// Round at which the helpers landed.
    pub closed_at_round: &'static str,
}

/// Get the closure-status descriptor for the rotation helpers.
/// Returns the R402 closure marker (was a deferred status until
/// R402; the helpers now ship as
/// [`create_empty_log_rotation`] + [`create_or_update_empty_log`] +
/// [`update_symlink_atomically`]).
pub fn log_rotation_status() -> LogRotationStatus {
    LogRotationStatus {
        status: "closed at R402",
        closed_at_round: "R402",
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
    fn log_rotation_status_describes_closure() {
        let s = log_rotation_status();
        assert_eq!(s.status, "closed at R402");
        assert_eq!(s.closed_at_round, "R402");
    }

    #[test]
    fn format_log_timestamp_is_inverse_of_parser() {
        // 1700000000000 ms = 2023-11-14T22-13-20.
        assert_eq!(
            format_log_timestamp(1_700_000_000_000),
            "2023-11-14T22-13-20"
        );
        // Round-trip: format → embed in filename → parse back.
        let fname = format!(
            "{LOG_PREFIX}{}.json",
            format_log_timestamp(1_700_000_000_000),
        );
        let parsed = get_timestamp_from_log(std::path::Path::new(&fname));
        assert_eq!(parsed, Some(1_700_000_000_000));
    }

    #[test]
    fn format_log_timestamp_unix_epoch() {
        assert_eq!(format_log_timestamp(0), "1970-01-01T00-00-00");
    }

    /// Helper: spawn a one-shot tempdir under std::env::temp_dir().
    fn rotation_tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-tracer-rotation-test-{pid}-{nanos}-{id}",
        ));
        std::fs::create_dir_all(&path).expect("create tempdir root");
        path
    }

    #[tokio::test]
    async fn create_or_update_empty_log_creates_file_and_symlink() {
        use crate::configuration::{LogFormat, LogMode, LoggingParams};
        use crate::types::{HandleRegistry, Registry};

        let tmp = rotation_tempdir();
        let registry: HandleRegistry = Registry::new();
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let params = LoggingParams {
            root: tmp.clone(),
            mode: LogMode::FileMode,
            format: LogFormat::ForMachine,
        };
        let key = ("node-7".to_string(), params);

        let result =
            create_or_update_empty_log(&lock, &key, &registry, &tmp, 1_700_000_000_000).await;
        assert!(result.is_ok(), "rotation: {:?}", result.err());
        let path = result.expect("path");
        assert!(path.exists());
        assert!(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .starts_with("node-2023-11-14T22-13-20")
        );

        // Symlink should exist + point at the new file.
        let symlink = tmp.join("node.json");
        assert!(symlink.exists());

        // Registry should contain the new entry.
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1);
    }

    #[tokio::test]
    async fn create_empty_log_rotation_creates_subdir_if_missing() {
        use crate::configuration::{LogFormat, LogMode, LoggingParams};
        use crate::types::{HandleRegistry, Registry};

        let tmp = rotation_tempdir();
        let sub = tmp.join("missing").join("nested");
        let registry: HandleRegistry = Registry::new();
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let params = LoggingParams {
            root: sub.clone(),
            mode: LogMode::FileMode,
            format: LogFormat::ForHuman,
        };
        let key = ("node-x".to_string(), params);

        let result =
            create_empty_log_rotation(&lock, &key, &registry, &sub, 1_700_000_000_000).await;
        assert!(result.is_ok(), "rotation: {:?}", result.err());
        assert!(sub.exists());
    }

    #[tokio::test]
    async fn create_or_update_empty_log_replaces_previous_handle() {
        use crate::configuration::{LogFormat, LogMode, LoggingParams};
        use crate::types::{HandleRegistry, Registry};

        let tmp = rotation_tempdir();
        let registry: HandleRegistry = Registry::new();
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let params = LoggingParams {
            root: tmp.clone(),
            mode: LogMode::FileMode,
            format: LogFormat::ForMachine,
        };
        let key = ("node-7".to_string(), params);

        // First rotation.
        let first = create_or_update_empty_log(&lock, &key, &registry, &tmp, 1_700_000_000_000)
            .await
            .expect("first");
        // Second rotation at a different timestamp.
        let second = create_or_update_empty_log(&lock, &key, &registry, &tmp, 1_700_000_060_000)
            .await
            .expect("second");
        assert_ne!(first, second);

        // Registry holds a single entry pointing at the second file.
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1);
        let (_, (_, registered_path)) = snapshot.into_iter().next().expect("entry");
        assert_eq!(registered_path, second);
    }
}
