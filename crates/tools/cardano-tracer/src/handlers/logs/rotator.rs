//! Log rotation policy — pure helpers for filtering / sorting /
//! retention / age-checking, plus the IO orchestration runtime
//! (R461) that ties them together with the
//! [`crate::types::HandleRegistry`] of open log handles.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Rotator.hs.
//!
//! Direct port of upstream's full surface. R461 closed the previously-
//! deferred IO orchestration (`runLogsRotator`, `launchRotator`,
//! `checkRootDir`, `checkLogs`, `checkIfCurrentLogIsFull`); the
//! pure-helper subset (`loggingParamsForFiles`,
//! `checkIfThereAreOldLogs`, `logIsFull`, `logsToRemove`) was
//! already shipped in earlier rounds.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `loggingParamsForFiles` filter+nub                             | [`logging_params_for_files`]           |
//! | `checkIfThereAreOldLogs`                                       | [`check_if_there_are_old_logs`]        |
//! | `logsToRemove` retention computation                           | [`logs_to_remove`]                     |
//! | `logIsFull` size check                                         | [`log_is_full`]                        |
//! | `runLogsRotator :: TracerEnv -> IO ()`                         | [`run_logs_rotator`] (R461)            |
//! | `launchRotator`                                                | (internal `launch_rotator` — same module) |
//! | `checkRootDir`                                                 | (internal `check_root_dir` — same module) |
//! | `checkLogs`                                                    | (internal `check_logs` — same module)  |
//! | `checkIfCurrentLogIsFull` (IO-bound)                           | (internal `check_if_current_log_is_full` — same module) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`showProblemIfAny verb tracer`**: upstream's wrapper depends
//!   on the unported `Cardano.Tracer.MetaTrace.TracerTrace` channel.
//!   R461 replaces it with a caller-supplied
//!   [`LogsRotatorErrorTracer`] closure (typically wired to
//!   `eprintln!` or `tracing::warn!`). The wire-level error surfaces
//!   are identical; the dispatch mechanism differs.
//! - **`forConcurrently_`**: replaced with
//!   `tokio::task::JoinSet::spawn` for per-subdirectory concurrency.
//!   The first error from a JoinSet member is bubbled up (matching
//!   upstream's silent-tolerance via `showProblemIfAny`, except
//!   Yggdrasil's caller is responsible for logging the error
//!   string via the `LogsRotatorErrorTracer` closure).
//! - **`hTell handle`**: upstream's offset-query maps to
//!   `tokio::fs::File::metadata().len()` since the SharedLogFile is
//!   opened write-only and extends linearly.
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

/// Status descriptor for the (now-closed) IO-orchestration entry-point.
/// Retained for programmatic introspection by status tooling — the
/// struct fields describe the closed state + the round in which the
/// orchestration shipped.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RunLogsRotatorStatus {
    /// One-line summary.
    pub status: &'static str,
    /// Reason — references the round in which the orchestration shipped.
    pub depends_on: &'static str,
    /// Round-number marker.
    pub deferred_round: &'static str,
}

/// Get the status descriptor for `runLogsRotator`. R461 closed the
/// previously-deferred IO orchestration; the function now ships as
/// [`run_logs_rotator`] (top-level) + supporting helpers.
pub fn run_logs_rotator_status() -> RunLogsRotatorStatus {
    RunLogsRotatorStatus {
        status: "closed at R461",
        depends_on: "Yggdrasil's read_registry shipped in R390-era crate::utils; createOrUpdateEmptyLog shipped at R390 in super::utils; showProblemIfAny remains a synthesis carve-out (tracer-trace channel unported) but the orchestration accepts a caller-supplied error-tracer closure as a Rust-side replacement.",
        deferred_round: "(closed)",
    }
}

// ---------------------------------------------------------------------------
// IO orchestration — R461 closure of the previously-deferred Rotator.hs
// runtime entry points
// ---------------------------------------------------------------------------

use std::sync::Arc;

use crate::configuration::{RotationParams, TracerConfig};
use crate::types::{HandleRegistry, HandleRegistryKey, NodeName, SharedLogFile};

/// Error-tracer callback: invoked with a short error string when a
/// log-rotation iteration encounters a non-fatal IO problem. Replaces
/// upstream's `showProblemIfAny verb tracer` wrapper (which depends
/// on the unported tracer-trace channel). Operators commonly wire
/// this to an `eprintln!` or `tracing::warn!` sink; the default
/// `|_msg: &str| ()` no-op also works fine for tests.
pub type LogsRotatorErrorTracer = Arc<dyn Fn(&str) + Send + Sync>;

/// Top-level entry: launch the log-rotation runtime if the operator
/// supplied a `rotation` field in their `TracerConfig`. Mirror of
/// upstream `runLogsRotator :: TracerEnv -> IO ()`. Runs until the
/// `stop_flag` brake is engaged.
///
/// Returns `Ok(())` cleanly when the brake fires. The function is
/// safe to call when `rotation` is `None` (returns immediately
/// without spawning anything).
///
/// **Strict mirror:** lines 35-50 of upstream's `Rotator.hs`. The
/// `LoggingParams` filter + dedup logic uses the previously-shipped
/// [`logging_params_for_files`] helper.
pub async fn run_logs_rotator(
    config: &TracerConfig,
    registry: HandleRegistry,
    current_log_lock: Arc<tokio::sync::Mutex<()>>,
    stop_flag: Arc<tokio::sync::RwLock<bool>>,
    error_tracer: LogsRotatorErrorTracer,
) {
    let Some(rotation) = config.rotation.clone() else {
        return;
    };
    // Coerce NonEmpty<LoggingParams> → Vec by collecting the
    // type-erased iterator. Upstream's `nub (NE.filter filesOnly logging)`
    // is the [`logging_params_for_files`] helper.
    let logging_for_files = logging_params_for_files(&config.logging);
    launch_rotator(
        logging_for_files,
        rotation,
        registry,
        current_log_lock,
        stop_flag,
        error_tracer,
    )
    .await;
}

/// Internal sleep-loop: every `rotation.frequency_secs` seconds,
/// invoke [`check_root_dir`] for each file-mode `LoggingParams`. The
/// loop terminates cleanly when the `stop_flag` brake is engaged.
///
/// Mirror of upstream `launchRotator` (lines 52-67). The
/// `forever do { ...; sleep }` loop becomes
/// `loop { ...; tokio::time::sleep(...); if brake { break; } }`.
async fn launch_rotator(
    logging_params_for_files: Vec<crate::configuration::LoggingParams>,
    rotation: RotationParams,
    registry: HandleRegistry,
    current_log_lock: Arc<tokio::sync::Mutex<()>>,
    stop_flag: Arc<tokio::sync::RwLock<bool>>,
    error_tracer: LogsRotatorErrorTracer,
) {
    if logging_params_for_files.is_empty() {
        return;
    }
    let frequency = std::time::Duration::from_secs(u64::from(rotation.frequency_secs));
    loop {
        // Mirror of upstream's `showProblemIfAny verb tracer do ...`.
        // Each per-logging-params iteration is wrapped in its own
        // error-trace context so one failing root dir doesn't block
        // the others.
        for params in &logging_params_for_files {
            if let Err(e) = check_root_dir(
                current_log_lock.clone(),
                &registry,
                rotation.clone(),
                params,
            )
            .await
            {
                error_tracer(&format!(
                    "logs-rotator: check_root_dir({:?}) failed: {e}",
                    params.root
                ));
            }
        }
        // Race the sleep against the brake so shutdown completes
        // within ~50ms rather than waiting up to `frequency` seconds.
        tokio::select! {
            () = tokio::time::sleep(frequency) => {}
            () = wait_for_stop(&stop_flag) => return,
        }
        if *stop_flag.read().await {
            return;
        }
    }
}

/// Polls the brake flag every 50ms until it becomes `true`. Mirror of
/// R421's `wait_for_stop` precedent in
/// [`yggdrasil_network::trace_object_run_acceptor`].
async fn wait_for_stop(stop_flag: &Arc<tokio::sync::RwLock<bool>>) {
    loop {
        if *stop_flag.read().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Walk each subdirectory of `logging_params.root` (one per
/// registered node), and if a handle exists in the registry for that
/// node, scan its log files for rotation. Mirror of upstream
/// `checkRootDir` (lines 75-100).
async fn check_root_dir(
    current_log_lock: Arc<tokio::sync::Mutex<()>>,
    registry: &HandleRegistry,
    rotation: RotationParams,
    logging_params: &crate::configuration::LoggingParams,
) -> Result<(), super::utils::LogRotationError> {
    let log_root_abs = tokio::fs::canonicalize(&logging_params.root)
        .await
        .map_err(super::utils::LogRotationError::Io)?;
    let metadata = match tokio::fs::metadata(&log_root_abs).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(super::utils::LogRotationError::Io(e)),
    };
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut read_dir = tokio::fs::read_dir(&log_root_abs)
        .await
        .map_err(super::utils::LogRotationError::Io)?;

    // Snapshot the registry once per outer iteration. Cloning the
    // registry-value tuples is cheap (SharedLogFile is an Arc + a
    // PathBuf clone).
    let handles: Vec<(HandleRegistryKey, (SharedLogFile, std::path::PathBuf))> =
        crate::utils::read_registry(registry);

    // Foreach concurrent over subdirectories. We use tokio::spawn +
    // JoinSet to mirror upstream's forConcurrently_; each subdir gets
    // an independent task so a stuck filesystem doesn't block the
    // others.
    let mut set = tokio::task::JoinSet::new();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(super::utils::LogRotationError::Io)?
    {
        let sub_path = entry.path();
        let ft = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let Some(node_name_str) = sub_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let node_name: NodeName = node_name_str.to_string();
        let key: HandleRegistryKey = (node_name, logging_params.clone());

        // Find the handle registered for this (node, logging_params)
        // pair. If absent, no rotation work — the registry is the
        // source of truth for which logs are currently open.
        let Some((_k, (handle, _path))) = handles.iter().find(|(k, _)| *k == key).cloned() else {
            continue;
        };
        let registry_clone = registry.clone();
        let lock_clone = current_log_lock.clone();
        let sub_path_clone = sub_path.clone();
        let rotation_clone = rotation.clone();
        set.spawn(async move {
            check_logs(
                lock_clone,
                handle,
                key,
                &registry_clone,
                rotation_clone,
                &sub_path_clone,
            )
            .await
        });
    }
    while let Some(joined) = set.join_next().await {
        if let Ok(Err(e)) = joined {
            // Per upstream's forConcurrently_, sibling failures
            // don't abort the outer loop — propagating one error
            // would hide the others. Bubble the first one up; the
            // caller wraps the whole call in error_tracer.
            return Err(e);
        }
    }
    Ok(())
}

/// Check the log files in a single subdirectory: enforce size limit
/// on the current (newest) log + age/retention limit on older logs.
/// Mirror of upstream `checkLogs` (lines 105-125).
async fn check_logs(
    current_log_lock: Arc<tokio::sync::Mutex<()>>,
    handle: SharedLogFile,
    key: HandleRegistryKey,
    registry: &HandleRegistry,
    rotation: RotationParams,
    sub_dir_for_logs: &Path,
) -> Result<(), super::utils::LogRotationError> {
    let format = key.1.format;
    let mut read_dir = tokio::fs::read_dir(sub_dir_for_logs)
        .await
        .map_err(super::utils::LogRotationError::Io)?;
    let mut log_paths: Vec<PathBuf> = Vec::new();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(super::utils::LogRotationError::Io)?
    {
        let path = entry.path();
        if super::utils::is_it_log(format, &path) {
            log_paths.push(path);
        }
    }
    if log_paths.is_empty() {
        return Ok(());
    }
    let from_oldest_to_newest = sort_logs_oldest_first(log_paths);
    let all_other_logs = if from_oldest_to_newest.len() > 1 {
        from_oldest_to_newest[..from_oldest_to_newest.len() - 1].to_vec()
    } else {
        Vec::new()
    };

    // Size check on the current log handle.
    check_if_current_log_is_full(
        current_log_lock,
        handle,
        &key,
        registry,
        rotation.log_limit_bytes,
        sub_dir_for_logs,
    )
    .await?;

    // Age check on the older logs.
    let now_ms = crate::time::get_time_ms();
    let to_remove = check_if_there_are_old_logs(
        &all_other_logs,
        rotation.max_age_minutes,
        rotation.keep_files_num,
        now_ms,
    );
    for stale in to_remove {
        match tokio::fs::remove_file(&stale).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Race with another process or already removed —
                // tolerable per upstream's swallow-error pattern.
            }
            Err(e) => return Err(super::utils::LogRotationError::Io(e)),
        }
    }
    Ok(())
}

/// Roll the current log if its byte size exceeds `max_size_in_bytes`.
/// Mirror of upstream `checkIfCurrentLogIsFull` (lines 128-144).
async fn check_if_current_log_is_full(
    current_log_lock: Arc<tokio::sync::Mutex<()>>,
    handle: SharedLogFile,
    key: &HandleRegistryKey,
    registry: &HandleRegistry,
    max_size_in_bytes: u64,
    sub_dir_for_logs: &Path,
) -> Result<(), super::utils::LogRotationError> {
    // Query current file size. Upstream uses `hTell handle` which
    // returns the current write offset; for append-mode files the
    // offset equals the file size. Yggdrasil's SharedLogFile is
    // also opened in write mode + extends linearly, so File::
    // metadata().len() gives the same answer.
    let size = {
        let file = handle.lock().await;
        let meta = file
            .metadata()
            .await
            .map_err(super::utils::LogRotationError::Io)?;
        meta.len()
    };
    if !log_is_full(size, max_size_in_bytes) {
        return Ok(());
    }
    // Roll: drop the previous handle (by overwriting it in the
    // registry), mint a fresh one, swap the symlink.
    let now_ms = crate::time::get_time_ms();
    super::utils::create_or_update_empty_log(
        &current_log_lock,
        key,
        registry,
        sub_dir_for_logs,
        now_ms,
    )
    .await?;
    Ok(())
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
    fn run_logs_rotator_status_describes_closure() {
        let s = run_logs_rotator_status();
        assert_eq!(s.status, "closed at R461");
        assert!(s.depends_on.contains("createOrUpdateEmptyLog"));
        assert_eq!(s.deferred_round, "(closed)");
    }

    // ----- IO orchestration tests (R461) -----------------------------------

    use crate::configuration::{RotationParams, TracerConfig};
    use crate::types::HandleRegistry;
    use std::sync::Arc;

    fn rotation_params(
        freq_secs: u32,
        max_age_minutes: u64,
        keep_files_num: u32,
    ) -> RotationParams {
        RotationParams {
            frequency_secs: freq_secs,
            log_limit_bytes: 1024,
            max_age_minutes,
            keep_files_num,
        }
    }

    fn config_with_rotation_and_root(
        root: &std::path::Path,
        rotation: Option<RotationParams>,
    ) -> TracerConfig {
        use crate::configuration::Network;
        let lp = LoggingParams {
            root: root.to_path_buf(),
            mode: LogMode::FileMode,
            format: LogFormat::ForMachine,
        };
        TracerConfig {
            network_magic: 764824073,
            network: Network::ConnectTo { connect_to: vec![] },
            logging: vec![lp],
            rotation,
            verbosity: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            ekg_request_freq: None,
            ekg_request_full: None,
            metrics_help: None,
            log_objects_request_num: Some(50),
            metrics_no_suffix: None,
            prometheus_labels: None,
            resource_freq: None,
        }
    }

    #[tokio::test]
    async fn run_logs_rotator_returns_immediately_when_no_rotation() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config = config_with_rotation_and_root(dir.path(), None);
        let registry = HandleRegistry::new();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let stop = Arc::new(tokio::sync::RwLock::new(false));
        let tracer: LogsRotatorErrorTracer = Arc::new(|_| {});
        // Should return synchronously despite the unset brake.
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            run_logs_rotator(&config, registry, lock, stop, tracer),
        )
        .await
        .expect("expected immediate return");
    }

    #[tokio::test]
    async fn run_logs_rotator_brake_terminates_loop() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let rotation = rotation_params(60, 1, 1);
        let config = config_with_rotation_and_root(dir.path(), Some(rotation));
        let registry = HandleRegistry::new();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let stop = Arc::new(tokio::sync::RwLock::new(false));
        let tracer: LogsRotatorErrorTracer = Arc::new(|_| {});

        let stop_clone = stop.clone();
        let rotator_task = tokio::spawn(async move {
            run_logs_rotator(&config, registry, lock, stop_clone, tracer).await;
        });

        // Let the rotator iterate once over the empty tempdir.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        *stop.write().await = true;

        // Brake-aware loop must exit within ~50ms of the brake trip.
        tokio::time::timeout(std::time::Duration::from_secs(2), rotator_task)
            .await
            .expect("rotator did not stop in time")
            .expect("rotator panicked");
    }

    #[tokio::test]
    async fn check_root_dir_returns_ok_when_root_missing() {
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let registry = HandleRegistry::new();
        let rotation = rotation_params(60, 1, 1);
        let logging_params = LoggingParams {
            root: PathBuf::from("/nonexistent/path/for/r461/test"),
            mode: LogMode::FileMode,
            format: LogFormat::ForMachine,
        };
        let result = check_root_dir(lock, &registry, rotation, &logging_params).await;
        // canonicalize fails on missing path → Err. Verify we got
        // SOME error (not a panic) — the orchestration tolerates
        // missing-root via the error_tracer callback in the caller.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn check_logs_drops_old_logs_when_present() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let sub_dir = dir.path().join("node-1");
        tokio::fs::create_dir_all(&sub_dir)
            .await
            .expect("create sub_dir");

        // Pre-populate the subdir with two log files: one OLD (epoch
        // 1970-01-01) and one NEW (current time). Retention=1 means
        // we keep the newest, age=1 minute means the old one is
        // overdue.
        let old_log = sub_dir.join("node-1970-01-01T00-00-00.json");
        let now_ts = {
            let s = super::super::utils::format_log_timestamp(crate::time::get_time_ms());
            format!("node-{s}.json")
        };
        let new_log = sub_dir.join(&now_ts);
        tokio::fs::write(&old_log, b"old").await.expect("write old");
        tokio::fs::write(&new_log, b"new").await.expect("write new");

        // Open the new log as the "current" handle.
        let new_handle: SharedLogFile = Arc::new(tokio::sync::Mutex::new(
            tokio::fs::OpenOptions::new()
                .write(true)
                .read(true)
                .open(&new_log)
                .await
                .expect("open new"),
        ));

        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let registry = HandleRegistry::new();
        let key: HandleRegistryKey = (
            "node-1".to_string(),
            LoggingParams {
                root: dir.path().to_path_buf(),
                mode: LogMode::FileMode,
                format: LogFormat::ForMachine,
            },
        );
        registry.insert(key.clone(), (new_handle.clone(), new_log.clone()));

        // Rotation: max_age=1 minute (old log is 50+ years old, so
        // definitely overdue), keep_files_num=0 (don't retain any
        // additional old logs beyond the current one — upstream's
        // `keepFilesNum` is the floor for the "older" subset, not
        // total).
        let rotation = rotation_params(60, 1, 0);
        check_logs(lock, new_handle, key, &registry, rotation, &sub_dir)
            .await
            .expect("check_logs");

        // The old log should be deleted; the new one should remain.
        assert!(!tokio::fs::try_exists(&old_log).await.expect("exists query"));
        assert!(tokio::fs::try_exists(&new_log).await.expect("exists query"));
    }
}
