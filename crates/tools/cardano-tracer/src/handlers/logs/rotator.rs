//! Log rotation policy — pure helpers for filtering, sorting,
//! retention, and age-checking.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Rotator.hs.
//!
//! Direct port of upstream's bounded subset. The pure helpers ship
//! now; the IO orchestration (`runLogsRotator`, `launchRotator`,
//! `checkRootDir`) defers pending the
//! `Cardano.Tracer.Utils.{showProblemIfAny, readRegistry}` ports +
//! the tracer-trace channel.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `loggingParamsForFiles` filter+nub                             | [`logging_params_for_files`]           |
//! | `checkIfThereAreOldLogs`                                       | [`check_if_there_are_old_logs`]        |
//! | `logsToRemove` retention computation                           | [`logs_to_remove`]                     |
//! | `logIsFull` size check                                         | [`log_is_full`]                        |
//! | `runLogsRotator :: TracerEnv -> IO ()`                         | (deferred — see [`run_logs_rotator_status`]) |
//! | `launchRotator`                                                | (deferred — same)                      |
//! | `checkRootDir`                                                 | (deferred — same)                      |
//! | `checkLogs`                                                    | (deferred — same)                      |
//! | `checkIfCurrentLogIsFull` (IO-bound)                           | (deferred — same)                      |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`runLogsRotator` / `launchRotator` / `checkRootDir` /
//!   `checkLogs` / `checkIfCurrentLogIsFull` (IO-bound)**: depend on
//!   `Cardano.Tracer.Utils.{showProblemIfAny, readRegistry}` (both
//!   unported) + tracer-trace channel + the deferred
//!   `createOrUpdateEmptyLog` from [`super::utils`]. Status surfaced
//!   via [`run_logs_rotator_status`] for downstream callers.
//! - **`Data.Time.diffUTCTime`**: upstream's age computation runs
//!   `now `diffUTCTime` ts` to get a `NominalDiffTime` then divides
//!   by `10^12` to get seconds. The Rust port works directly with
//!   Unix-epoch milliseconds (matching [`super::utils::get_timestamp_from_log`]
//!   semantics) — `(now_ms - log_ms) / 1000` gives seconds without
//!   the upstream picosecond intermediate.

use std::path::{Path, PathBuf};

use crate::configuration::{LogMode, LoggingParams};

use super::utils::get_timestamp_from_log;

/// Filter the operator-supplied logging params to just those whose
/// `logMode` is `FileMode` (the only mode that needs rotation),
/// dedup-ing the result. Mirror of upstream
/// `loggingParamsForFiles = nub (NE.filter filesOnly logging)`.
pub fn logging_params_for_files(logging: &[LoggingParams]) -> Vec<LoggingParams> {
    let mut out = Vec::new();
    for params in logging {
        if params.mode != LogMode::FileMode {
            continue;
        }
        if !out.contains(params) {
            out.push(params.clone());
        }
    }
    out
}

/// Decide whether the current log file is "full" given its current
/// byte-size + the operator's `rpLogLimitBytes` threshold. Mirror
/// of upstream's `logIsFull = fromIntegral size >= maxSizeInBytes`.
///
/// The IO-bound `hTell handle >>= logIsFull` chain in upstream is
/// split here: callers obtain `current_size_bytes` from the
/// platform-specific file-handle inspection, then ask this helper
/// whether to roll.
pub fn log_is_full(current_size_bytes: u64, max_size_in_bytes: u64) -> bool {
    current_size_bytes >= max_size_in_bytes
}

/// Compute which logs to remove given a sorted (oldest-first) list,
/// the maximum age in seconds, the retention count, and the current
/// wall-clock time in Unix-epoch milliseconds. Mirror of upstream's
/// `checkIfThereAreOldLogs` walk (lines 153-172).
///
/// Returns the paths to delete in oldest-first order (so callers
/// can `removeFile` each in sequence). Logs without parseable
/// timestamps are skipped per upstream's
/// `Nothing -> checkOldLogs otherLogs now'` continue-on-malformed
/// fall-through.
pub fn check_if_there_are_old_logs(
    from_oldest_to_newest: &[PathBuf],
    max_age_in_minutes: u64,
    keep_files_num: u32,
    now_ms: i64,
) -> Vec<PathBuf> {
    // Newest N files retained unconditionally.
    let logs_we_have_to_check = logs_to_remove(from_oldest_to_newest, keep_files_num as usize);

    let max_age_in_secs = max_age_in_minutes.saturating_mul(60) as i64;
    let mut to_remove = Vec::new();
    for log in logs_we_have_to_check {
        let Some(ts_ms) = get_timestamp_from_log(log) else {
            // Malformed timestamp — skip per upstream's continue-on-Nothing.
            continue;
        };
        let age_secs = (now_ms - ts_ms) / 1000;
        if age_secs >= max_age_in_secs {
            to_remove.push(log.clone());
        } else {
            // Once we hit a log that's still young, stop scanning —
            // newer logs are by definition also young (mirror of
            // upstream's "oldestLog isn't outdated, so newer ones
            // aren't either" early-exit).
            break;
        }
    }
    to_remove
}

/// Drop the `keep_files_num` newest entries from a sorted (oldest-
/// first) log-path list, returning only the entries old enough to
/// potentially remove. Mirror of upstream
/// `logsWeHaveToCheck = dropEnd (fromIntegral keepFilesNum) fromOldestToNewest`.
pub fn logs_to_remove(from_oldest_to_newest: &[PathBuf], keep_files_num: usize) -> &[PathBuf] {
    let total = from_oldest_to_newest.len();
    if keep_files_num >= total {
        return &[];
    }
    &from_oldest_to_newest[..total - keep_files_num]
}

/// Sort a list of log paths into oldest-first order using the
/// embedded timestamp from each filename. Logs whose names don't
/// parse to a valid timestamp sort last (mirror of upstream's
/// implicit ordering — `sort logs` operates on full paths but
/// timestamp-bearing names sort lexically and chronologically the
/// same way).
pub fn sort_logs_oldest_first<I, P>(logs: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut keyed: Vec<(Option<i64>, PathBuf)> = logs
        .into_iter()
        .map(|p| {
            let path = p.as_ref().to_path_buf();
            let ts = get_timestamp_from_log(&path);
            (ts, path)
        })
        .collect();
    // Stable sort: parseable-timestamp entries first, ordered by
    // ts ascending; unparseable entries last in original order.
    keyed.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    keyed.into_iter().map(|(_, p)| p).collect()
}

/// Status descriptor for the deferred IO-orchestration entry-point.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RunLogsRotatorStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing upstream ports.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `runLogsRotator`.
pub fn run_logs_rotator_status() -> RunLogsRotatorStatus {
    RunLogsRotatorStatus {
        status: "deferred",
        depends_on: "Cardano.Tracer.Utils.{showProblemIfAny, readRegistry} (unported); tracer-trace channel from MetaTrace.hs (unported); super::utils::createOrUpdateEmptyLog (deferred at R390)",
        deferred_round: "R395+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{LogFormat, LogMode, LoggingParams};

    fn lp_filemode(root: &str, format: LogFormat) -> LoggingParams {
        LoggingParams {
            root: PathBuf::from(root),
            mode: LogMode::FileMode,
            format,
        }
    }

    fn lp_journalmode(root: &str, format: LogFormat) -> LoggingParams {
        LoggingParams {
            root: PathBuf::from(root),
            mode: LogMode::JournalMode,
            format,
        }
    }

    #[test]
    fn logging_params_for_files_keeps_only_filemode() {
        let logging = vec![
            lp_filemode("/var/log/a", LogFormat::ForHuman),
            lp_journalmode("/var/log/b", LogFormat::ForMachine),
            lp_filemode("/var/log/c", LogFormat::ForMachine),
        ];
        let filtered = logging_params_for_files(&logging);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|p| p.mode == LogMode::FileMode));
    }

    #[test]
    fn logging_params_for_files_dedups_identical_entries() {
        let logging = vec![
            lp_filemode("/var/log/a", LogFormat::ForHuman),
            lp_filemode("/var/log/a", LogFormat::ForHuman),
            lp_filemode("/var/log/a", LogFormat::ForHuman),
        ];
        let filtered = logging_params_for_files(&logging);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn logging_params_for_files_returns_empty_when_no_filemode() {
        let logging = vec![
            lp_journalmode("/var/log/a", LogFormat::ForHuman),
            lp_journalmode("/var/log/b", LogFormat::ForMachine),
        ];
        let filtered = logging_params_for_files(&logging);
        assert!(filtered.is_empty());
    }

    #[test]
    fn log_is_full_true_when_size_meets_threshold() {
        assert!(log_is_full(1024, 1024));
        assert!(log_is_full(2048, 1024));
    }

    #[test]
    fn log_is_full_false_when_size_below_threshold() {
        assert!(!log_is_full(1023, 1024));
        assert!(!log_is_full(0, 1024));
    }

    #[test]
    fn logs_to_remove_drops_n_newest() {
        let logs = vec![
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("c"),
            PathBuf::from("d"),
            PathBuf::from("e"),
        ];
        // Keep 2 newest → remove first 3.
        let candidates = logs_to_remove(&logs, 2);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0], PathBuf::from("a"));
        assert_eq!(candidates[2], PathBuf::from("c"));
    }

    #[test]
    fn logs_to_remove_keep_more_than_total_returns_empty() {
        let logs = vec![PathBuf::from("a"), PathBuf::from("b")];
        let candidates = logs_to_remove(&logs, 5);
        assert!(candidates.is_empty());
    }

    #[test]
    fn logs_to_remove_keep_equal_to_total_returns_empty() {
        let logs = vec![PathBuf::from("a"), PathBuf::from("b")];
        let candidates = logs_to_remove(&logs, 2);
        assert!(candidates.is_empty());
    }

    #[test]
    fn logs_to_remove_keep_zero_returns_all() {
        let logs = vec![PathBuf::from("a"), PathBuf::from("b")];
        let candidates = logs_to_remove(&logs, 0);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn check_if_there_are_old_logs_removes_old_files() {
        // 3 logs at known timestamps:
        //   2023-11-14 22:13:20 → 1700000000 secs (12 min before "now")
        //   2023-11-14 22:14:20 → 1700000060 secs (11 min before "now")
        //   2023-11-14 22:15:20 → 1700000120 secs (10 min before "now")
        // now = 2023-11-14 22:25:20 → 1700000720 secs
        // max_age = 11 minutes (660 secs)
        // keep_num = 0 → all 3 are candidates
        // Expected: first two removed (age >= 660 secs), third kept
        // (10 min < 11 min). The early-exit on the first young log
        // also stops the scan there.
        let logs = vec![
            PathBuf::from("/log/node-2023-11-14T22-13-20.json"),
            PathBuf::from("/log/node-2023-11-14T22-14-20.json"),
            PathBuf::from("/log/node-2023-11-14T22-15-20.json"),
        ];
        let now_ms = 1_700_000_720_000;
        let to_remove = check_if_there_are_old_logs(&logs, 11, 0, now_ms);
        assert_eq!(to_remove.len(), 2);
        assert!(to_remove[0].ends_with("node-2023-11-14T22-13-20.json"));
        assert!(to_remove[1].ends_with("node-2023-11-14T22-14-20.json"));
    }

    #[test]
    fn check_if_there_are_old_logs_keeps_newest_n() {
        let logs = vec![
            PathBuf::from("/log/node-2020-01-01T00-00-00.json"),
            PathBuf::from("/log/node-2020-01-02T00-00-00.json"),
            PathBuf::from("/log/node-2020-01-03T00-00-00.json"),
        ];
        // now far in the future; all 3 are ancient.
        let now_ms = 2_000_000_000_000;
        // Keep 2 newest → only the oldest one is candidate for removal.
        let to_remove = check_if_there_are_old_logs(&logs, 1, 2, now_ms);
        assert_eq!(to_remove.len(), 1);
        assert!(to_remove[0].ends_with("node-2020-01-01T00-00-00.json"));
    }

    #[test]
    fn check_if_there_are_old_logs_empty_input_returns_empty() {
        let to_remove = check_if_there_are_old_logs(&[], 1, 0, 1_700_000_000_000);
        assert!(to_remove.is_empty());
    }

    #[test]
    fn check_if_there_are_old_logs_skips_unparseable_timestamps() {
        // First entry has a bad timestamp; second has a valid old one.
        let logs = vec![
            PathBuf::from("/log/node-not-a-timestamp.json"),
            PathBuf::from("/log/node-2020-01-01T00-00-00.json"),
        ];
        let now_ms = 2_000_000_000_000;
        let to_remove = check_if_there_are_old_logs(&logs, 1, 0, now_ms);
        // Bad timestamp skipped; second log is genuinely old → 1 to remove.
        assert_eq!(to_remove.len(), 1);
        assert!(to_remove[0].ends_with("node-2020-01-01T00-00-00.json"));
    }

    #[test]
    fn check_if_there_are_old_logs_stops_at_first_young_log() {
        // First log old, second log young: should remove only first.
        let logs = vec![
            PathBuf::from("/log/node-2020-01-01T00-00-00.json"), // old
            PathBuf::from("/log/node-2024-12-01T00-00-00.json"), // young
        ];
        let now_ms = 1_733_011_200_000; // 2024-12-01T00-00-00 in ms
        let to_remove = check_if_there_are_old_logs(&logs, 60 * 24, 0, now_ms);
        assert_eq!(to_remove.len(), 1);
    }

    #[test]
    fn sort_logs_oldest_first_orders_by_timestamp() {
        let unsorted = vec![
            PathBuf::from("/log/node-2024-01-01T00-00-00.json"),
            PathBuf::from("/log/node-2020-01-01T00-00-00.json"),
            PathBuf::from("/log/node-2022-06-15T12-00-00.json"),
        ];
        let sorted = sort_logs_oldest_first(unsorted);
        assert_eq!(sorted.len(), 3);
        assert!(sorted[0].ends_with("node-2020-01-01T00-00-00.json"));
        assert!(sorted[1].ends_with("node-2022-06-15T12-00-00.json"));
        assert!(sorted[2].ends_with("node-2024-01-01T00-00-00.json"));
    }

    #[test]
    fn sort_logs_oldest_first_pushes_unparseable_to_end() {
        let unsorted = vec![
            PathBuf::from("/log/node-not-a-timestamp.json"),
            PathBuf::from("/log/node-2020-01-01T00-00-00.json"),
        ];
        let sorted = sort_logs_oldest_first(unsorted);
        assert!(sorted[0].ends_with("node-2020-01-01T00-00-00.json"));
        assert!(sorted[1].ends_with("node-not-a-timestamp.json"));
    }

    #[test]
    fn run_logs_rotator_status_describes_deferral() {
        let s = run_logs_rotator_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("readRegistry"));
        assert_eq!(s.deferred_round, "R395+");
    }
}
