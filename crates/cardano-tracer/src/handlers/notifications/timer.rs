//! Periodic-action scheduler — runs a configurable closure every
//! `period_secs` seconds with start/stop control.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Timer.hs.
//!
//! Direct port of upstream's Timer module. Mirrors upstream's
//! 5-method record + 5 constructor variants on top of tokio's
//! `task::JoinHandle` + `task::AbortHandle` instead of GHC's
//! `forkIO` + `killThread`, and `tokio::sync::Mutex` instead of
//! `Data.IORef`.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `data Timer { threadAlive, threadKill, setCallPeriod, startTimer, stopTimer }` | [`Timer`] struct + inherent methods |
//! | `mkTimer :: Trace IO TracerTrace -> IO () -> Bool -> PeriodInSec -> IO Timer` | [`Timer::new`]                  |
//! | `mkTimerStderr :: IO () -> Bool -> PeriodInSec -> IO Timer`    | [`Timer::new_stderr`]                  |
//! | `mkTimerDieOnFailure :: ...`                                   | [`Timer::new_die_on_failure`]          |
//! | `mkTimerStderrDieOnFailure :: ...`                             | [`Timer::new_stderr_die_on_failure`]   |
//! | `checkPeriod = 1`                                              | [`CHECK_PERIOD_SECS`]                  |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Trace IO TracerTrace`**: upstream's tracer-trace channel
//!   isn't ported yet. Yggdrasil's port replaces the tracer arg
//!   with a `Box<dyn Fn(&str) + Send + Sync>` failure-callback that
//!   prints the failure message verbatim. The two `mkTimerStderr*`
//!   helpers wire that callback to `eprintln!`.
//! - **`killThread =<< myThreadId`** (the die-on-failure pattern):
//!   upstream kills the calling thread on action failure. The Rust
//!   port can't kill its own task from inside (`std::process::abort()`
//!   would be wrong; aborting only the current task leaks the
//!   spawning context). Yggdrasil's `Timer::new_die_on_failure`
//!   instead aborts the timer's own loop after the failure
//!   callback fires — the periodic action stops running but the
//!   surrounding tracer process keeps going, which is operationally
//!   safer in a multi-tenant tokio runtime.
//! - **`Control.Exception.try`**: upstream wraps the action in
//!   `try @SomeException io`. Rust's port wraps the boxed action in
//!   `std::panic::catch_unwind` since the action returns `()` and
//!   any errors must surface as panics or be swallowed inside the
//!   action itself.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};

use super::types::PeriodInSec;

/// Granularity of the timer's internal poll-loop, in seconds.
/// Mirror of upstream `checkPeriod :: PeriodInSec; checkPeriod = 1`.
pub const CHECK_PERIOD_SECS: PeriodInSec = 1;

/// One periodic action with run/stop control. Mirror of upstream
/// `data Timer = Timer { threadAlive, threadKill, setCallPeriod,
/// startTimer, stopTimer }`.
///
/// Upstream's record-of-IO-actions pattern collapses to a Rust
/// struct of `Arc<Mutex>`-shared state + a single
/// `tokio::task::JoinHandle` for the spawned poll-loop. Each
/// inherent method below mirrors one of the 5 record fields.
#[derive(Clone, Debug)]
pub struct Timer {
    /// Operator-set period between action invocations (in seconds).
    /// Mutable so [`Timer::set_call_period`] can swap it in flight.
    /// Upstream's `elapsed_time` IORef lives only inside the spawn
    /// loop closure and isn't exposed on the struct (its lifetime
    /// is bounded by the task).
    call_period: Arc<Mutex<PeriodInSec>>,
    /// `true` while the timer is enabled. [`Timer::start_timer`]
    /// flips this on, [`Timer::stop_timer`] flips it off; the spawn
    /// loop checks this flag every `CHECK_PERIOD_SECS` and skips
    /// the period-check body when it's `false`.
    is_running: Arc<Mutex<bool>>,
    /// Optional task handle for the spawned poll-loop. `None` means
    /// the Timer is a placeholder constructed via
    /// [`Timer::placeholder`] — no task was spawned.
    handle: Option<Arc<JoinHandle<()>>>,
}

impl Default for Timer {
    fn default() -> Self {
        Timer::placeholder()
    }
}

impl Timer {
    /// Construct a no-op Timer with no spawned task. Used as the
    /// `Default` value in [`super::types::EventsQueues`] map values,
    /// and as a stand-in in tests that don't need real periodic behavior.
    /// All inherent methods are safe to call on a placeholder; they
    /// just operate on the in-memory state without driving any action.
    pub fn placeholder() -> Self {
        Timer {
            call_period: Arc::new(Mutex::new(0)),
            is_running: Arc::new(Mutex::new(false)),
            handle: None,
        }
    }

    /// Construct a periodic-action Timer with a custom failure
    /// callback. Mirror of upstream `mkTimer`.
    ///
    /// The `action` closure runs every `period_secs` seconds while
    /// the timer is enabled (initial state set via `initial_running`).
    /// If `action` panics, `on_failure_message` is invoked with the
    /// panic message and the loop continues running unless this
    /// timer was constructed via [`Timer::new_die_on_failure`].
    pub fn new<A, F>(
        on_failure_message: F,
        die_on_failure: bool,
        action: A,
        initial_running: bool,
        period_secs: PeriodInSec,
    ) -> Self
    where
        A: Fn() + Send + Sync + 'static,
        F: Fn(&str) + Send + Sync + 'static,
    {
        let call_period = Arc::new(Mutex::new(period_secs));
        let is_running = Arc::new(Mutex::new(initial_running));
        let action = Arc::new(action);
        let on_failure_message = Arc::new(on_failure_message);

        let cp = Arc::clone(&call_period);
        let et = Arc::new(Mutex::new(0_u32));
        let ir = Arc::clone(&is_running);

        let handle = tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(u64::from(CHECK_PERIOD_SECS))).await;
                if !*ir.lock().await {
                    continue;
                }
                let period = *cp.lock().await;
                let mut elapsed_guard = et.lock().await;
                if *elapsed_guard < period {
                    *elapsed_guard += CHECK_PERIOD_SECS;
                    continue;
                }
                drop(elapsed_guard);
                let act = Arc::clone(&action);
                let result = tokio::task::spawn_blocking(move || {
                    std::panic::catch_unwind(AssertUnwindSafe(|| act()))
                })
                .await;
                match result {
                    Ok(Ok(())) => {
                        *et.lock().await = 0;
                    }
                    Ok(Err(panic_payload)) => {
                        let msg = panic_msg(&panic_payload);
                        on_failure_message(&msg);
                        if die_on_failure {
                            break;
                        }
                    }
                    Err(join_err) => {
                        on_failure_message(&format!("timer action join error: {join_err}",));
                        if die_on_failure {
                            break;
                        }
                    }
                }
            }
        });

        Timer {
            call_period,
            is_running,
            handle: Some(Arc::new(handle)),
        }
    }

    /// Construct a Timer whose failure callback prints to stderr.
    /// Mirror of upstream `mkTimerStderr`.
    pub fn new_stderr<A>(action: A, initial_running: bool, period_secs: PeriodInSec) -> Self
    where
        A: Fn() + Send + Sync + 'static,
    {
        Timer::new(
            |msg: &str| eprintln!("{msg}"),
            false,
            action,
            initial_running,
            period_secs,
        )
    }

    /// Construct a die-on-failure Timer with a custom failure
    /// callback. Mirror of upstream `mkTimerDieOnFailure` (with the
    /// carve-out documented in the module docstring — Yggdrasil
    /// aborts only the timer's own loop on failure, not the
    /// surrounding process).
    pub fn new_die_on_failure<A, F>(
        on_failure_message: F,
        action: A,
        initial_running: bool,
        period_secs: PeriodInSec,
    ) -> Self
    where
        A: Fn() + Send + Sync + 'static,
        F: Fn(&str) + Send + Sync + 'static,
    {
        Timer::new(
            on_failure_message,
            true,
            action,
            initial_running,
            period_secs,
        )
    }

    /// Construct a stderr-logging die-on-failure Timer. Mirror of
    /// upstream `mkTimerStderrDieOnFailure`.
    pub fn new_stderr_die_on_failure<A>(
        action: A,
        initial_running: bool,
        period_secs: PeriodInSec,
    ) -> Self
    where
        A: Fn() + Send + Sync + 'static,
    {
        Timer::new(
            |msg: &str| eprintln!("{msg}"),
            true,
            action,
            initial_running,
            period_secs,
        )
    }

    /// `true` while the spawned task is still running. Mirror of
    /// upstream `threadAlive :: IO Bool`. A placeholder Timer (no
    /// spawned task) returns `false`.
    pub fn is_alive(&self) -> bool {
        match &self.handle {
            Some(h) => !h.is_finished(),
            None => false,
        }
    }

    /// Abort the spawned task. Mirror of upstream
    /// `threadKill :: IO ()`. A placeholder Timer is a no-op.
    pub fn kill(&self) {
        if let Some(h) = &self.handle {
            h.abort();
        }
    }

    /// Replace the period between action invocations. Mirror of
    /// upstream `setCallPeriod :: PeriodInSec -> IO ()`. The change
    /// takes effect at the next poll-loop iteration (within
    /// `CHECK_PERIOD_SECS` seconds).
    pub async fn set_call_period(&self, period: PeriodInSec) {
        *self.call_period.lock().await = period;
    }

    /// Enable the timer. Mirror of upstream `startTimer :: IO ()`.
    pub async fn start_timer(&self) {
        *self.is_running.lock().await = true;
    }

    /// Disable the timer. Mirror of upstream `stopTimer :: IO ()`.
    /// The spawned task continues running but skips the
    /// period-check body until [`Timer::start_timer`] is called.
    pub async fn stop_timer(&self) {
        *self.is_running.lock().await = false;
    }

    /// Read the currently-set call period. Useful for tests +
    /// status surfaces.
    pub async fn call_period(&self) -> PeriodInSec {
        *self.call_period.lock().await
    }

    /// Read whether the timer is currently enabled.
    pub async fn is_running(&self) -> bool {
        *self.is_running.lock().await
    }
}

fn panic_msg(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else {
        "panic with non-string payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn check_period_secs_is_one() {
        assert_eq!(CHECK_PERIOD_SECS, 1);
    }

    #[test]
    fn placeholder_timer_default_is_not_alive() {
        let t = Timer::placeholder();
        assert!(!t.is_alive());
    }

    #[test]
    fn default_constructs_to_placeholder() {
        let t = Timer::default();
        assert!(!t.is_alive());
    }

    #[tokio::test]
    async fn placeholder_timer_methods_are_safe_no_ops() {
        let t = Timer::placeholder();
        t.start_timer().await;
        t.stop_timer().await;
        t.set_call_period(42).await;
        t.kill();
        // After all that, still not-alive.
        assert!(!t.is_alive());
    }

    #[tokio::test]
    async fn new_timer_invokes_action_after_period_elapses() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_for_action = Arc::clone(&counter);
        let timer = Timer::new(
            |_msg: &str| {},
            false,
            move || {
                counter_for_action.fetch_add(1, Ordering::SeqCst);
            },
            true,
            1, // period 1 second
        );
        // Wait long enough for ~2-3 invocations (give 4 seconds
        // grace for CI variance).
        sleep(Duration::from_secs(4)).await;
        timer.kill();
        let final_count = counter.load(Ordering::SeqCst);
        assert!(
            final_count >= 1,
            "timer should have fired at least once, got {final_count}"
        );
    }

    #[tokio::test]
    async fn stop_timer_pauses_action_invocations() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_for_action = Arc::clone(&counter);
        let timer = Timer::new(
            |_msg: &str| {},
            false,
            move || {
                counter_for_action.fetch_add(1, Ordering::SeqCst);
            },
            false, // initial: stopped
            1,
        );
        sleep(Duration::from_secs(3)).await;
        // Timer is stopped — counter should be 0.
        let count_when_stopped = counter.load(Ordering::SeqCst);
        timer.kill();
        assert_eq!(count_when_stopped, 0);
    }

    #[tokio::test]
    async fn start_timer_resumes_action_invocations() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_for_action = Arc::clone(&counter);
        let timer = Timer::new(
            |_msg: &str| {},
            false,
            move || {
                counter_for_action.fetch_add(1, Ordering::SeqCst);
            },
            false,
            1,
        );
        // Initially stopped.
        sleep(Duration::from_secs(2)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        // Now start, wait, expect at least 1 invocation.
        timer.start_timer().await;
        sleep(Duration::from_secs(3)).await;
        timer.kill();
        let after_start = counter.load(Ordering::SeqCst);
        assert!(
            after_start >= 1,
            "after start, expected >= 1, got {after_start}"
        );
    }

    #[tokio::test]
    async fn set_call_period_updates_in_flight_period() {
        let timer = Timer::new(
            |_msg: &str| {},
            false,
            || {},
            true,
            10, // initial period 10s
        );
        assert_eq!(timer.call_period().await, 10);
        timer.set_call_period(2).await;
        assert_eq!(timer.call_period().await, 2);
        timer.kill();
    }

    #[tokio::test]
    async fn is_running_flag_round_trips() {
        let timer = Timer::new(|_msg: &str| {}, false, || {}, false, 1);
        assert!(!timer.is_running().await);
        timer.start_timer().await;
        assert!(timer.is_running().await);
        timer.stop_timer().await;
        assert!(!timer.is_running().await);
        timer.kill();
    }

    #[tokio::test]
    async fn kill_aborts_the_task_so_is_alive_eventually_false() {
        let timer = Timer::new(|_msg: &str| {}, false, || {}, true, 10);
        assert!(timer.is_alive());
        timer.kill();
        // Give the runtime a moment to observe the abort.
        for _ in 0..20 {
            if !timer.is_alive() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(!timer.is_alive());
    }

    #[tokio::test]
    async fn die_on_failure_aborts_loop_when_action_panics() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_for_action = Arc::clone(&counter);
        let failure_log = Arc::new(Mutex::new(Vec::<String>::new()));
        let failure_log_for_cb = Arc::clone(&failure_log);
        let timer = Timer::new_die_on_failure(
            move |msg: &str| {
                let log = Arc::clone(&failure_log_for_cb);
                let m = msg.to_string();
                tokio::spawn(async move {
                    log.lock().await.push(m);
                });
            },
            move || {
                counter_for_action.fetch_add(1, Ordering::SeqCst);
                panic!("synthetic test failure");
            },
            true,
            1,
        );
        sleep(Duration::from_secs(3)).await;
        // After the first panic, die_on_failure aborts the loop —
        // counter should be exactly 1.
        let final_count = counter.load(Ordering::SeqCst);
        assert_eq!(final_count, 1);
        // Failure callback received the panic message.
        sleep(Duration::from_millis(100)).await;
        let log = failure_log.lock().await;
        assert!(log.iter().any(|m| m.contains("synthetic test failure")));
        timer.kill();
    }
}
