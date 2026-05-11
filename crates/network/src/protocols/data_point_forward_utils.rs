//! DataPointRequestor — STM-style shared state that external
//! context uses to push a "request these data-points" signal to the
//! acceptor loop and receive the reply.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Utils/DataPoint.hs.
//!
//! Filename flattens the upstream directory; carries
//! `DataPointRequestor`, `init_data_point_requestor`, and
//! `ask_for_data_points` mirrors. The forwarder-side (`DataPoint`,
//! `DataPointStore`, `init_data_point_store`, `read_from_store`,
//! `write_to_store`) lives outside this round's scope — those run on
//! the cardano-node side and need the unported
//! `Trace.Forward.Protocol.DataPoint.Forwarder` driver.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data DataPointRequestor = DataPointRequestor { ... }` | [`DataPointRequestor`]                 |
//! | `askDataPoints   :: TVar Bool`                          | (internal — see `wait_for_ask` + `clear_ask_flag`) |
//! | `dataPointsNames :: TVar [DataPointName]`               | (internal — see `wait_for_ask`)        |
//! | `dataPointsReply :: TMVar DataPointValues`              | (internal — see `take_reply` + `put_reply`) |
//! | `initDataPointRequestor :: IO DataPointRequestor`       | [`DataPointRequestor::new`]            |
//! | `askForDataPoints :: ... -> IO DataPointValues`         | [`DataPointRequestor::ask_for_data_points`] |
//! | `tenSeconds`                                            | [`ASK_FOR_DATA_POINTS_TIMEOUT`]        |
//!
//! Carve-outs (synthesis-mirror, NOT strict-mirror):
//!
//! - **STM TVar/TMVar primitives** collapse to a single
//!   `tokio::sync::Mutex<RequestorState>` holding all three fields
//!   plus two `tokio::sync::Notify` channels for cross-task wakes.
//!   The atomic-multi-write semantics carry across: every state
//!   transition is performed inside one critical section.
//! - **`takeTMVar` blocking-take** replaced with
//!   `tokio::time::timeout(ASK_FOR_DATA_POINTS_TIMEOUT, reply_notify.notified())`
//!   matching upstream's `orElse (registerDelay tenSeconds)`
//!   timeout fallback exactly.
//! - **Public field-access pattern** (upstream exposes all three
//!   `TVar`/`TMVar` fields directly via record-pattern destructuring
//!   at the call site `Run/DataPoint/Acceptor.hs:67`) is replaced
//!   with method calls on [`DataPointRequestor`] that take an
//!   internal mutex once each. The acceptor loop reads the fields
//!   in the same logical order; the lock-acquisition cost is
//!   negligible (the loop runs at human-scale data-point request
//!   rates, not per-block or per-tx).
//!
//! Reference: `Trace.Forward.Utils.DataPoint` from the upstream
//! `trace-forward` package.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};

use super::{DataPointName, DataPointValues};

/// Timeout for [`DataPointRequestor::ask_for_data_points`] when no
/// reply arrives. Mirror of upstream's `tenSeconds = 10 * 1000000 :: Int`
/// (10-second microsecond constant fed to `registerDelay`).
pub const ASK_FOR_DATA_POINTS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
struct RequestorState {
    /// Names to request in the next round. External context sets
    /// these via [`DataPointRequestor::ask_for_data_points`].
    ///
    /// Mirror of `dataPointsNames :: TVar [DataPointName]`.
    names: Vec<DataPointName>,

    /// "Ask flag" — `true` when external context has signaled a new
    /// request. Cleared by the acceptor after filling the reply slot.
    ///
    /// Mirror of `askDataPoints :: TVar Bool`.
    ask_flag: bool,

    /// Reply slot — `None` when empty, `Some(values)` when filled by
    /// the acceptor.
    ///
    /// Mirror of `dataPointsReply :: TMVar DataPointValues`.
    reply: Option<DataPointValues>,
}

/// Shared coordination state between external context (which wants
/// to ask for data-points) and the acceptor loop (which actually
/// drives the protocol).
///
/// Mirror of upstream's `data DataPointRequestor = DataPointRequestor
/// { askDataPoints, dataPointsNames, dataPointsReply }` record. The
/// three STM primitives collapse into a single `tokio::sync::Mutex`
/// holding all three fields atomically + two `Notify` channels for
/// the cross-task wakes.
///
/// Clones share the same underlying state (Arc-based). This matches
/// upstream's STM-record semantics where the record itself is just a
/// triple of mutable references.
#[derive(Clone)]
pub struct DataPointRequestor {
    inner: Arc<DataPointRequestorInner>,
}

struct DataPointRequestorInner {
    state: Mutex<RequestorState>,
    /// Wake signal for external → acceptor-loop. Mirror of the
    /// `readTVar askDataPoints >>= check` STM-retry-until-true
    /// pattern.
    ask_notify: Notify,
    /// Wake signal for acceptor-loop → external. Mirror of the
    /// `takeTMVar dataPointsReply` STM-blocking-take pattern.
    reply_notify: Notify,
}

impl std::fmt::Debug for DataPointRequestor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataPointRequestor")
            .field("inner", &"<Arc<DataPointRequestorInner>>")
            .finish()
    }
}

impl Default for DataPointRequestor {
    fn default() -> Self {
        Self::new()
    }
}

impl DataPointRequestor {
    /// Construct a fresh requestor. Mirror of upstream's
    /// `initDataPointRequestor :: IO DataPointRequestor` (which
    /// creates a `TVar False`, a `TVar []`, and an empty `TMVar`).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DataPointRequestorInner {
                state: Mutex::new(RequestorState::default()),
                ask_notify: Notify::new(),
                reply_notify: Notify::new(),
            }),
        }
    }

    /// External-context API: ask the acceptor to request the listed
    /// data-points, wait up to [`ASK_FOR_DATA_POINTS_TIMEOUT`] for
    /// the reply, and return it (or an empty `Vec` on timeout).
    ///
    /// Mirror of upstream's
    /// `askForDataPoints :: DataPointRequestor -> [DataPointName] ->
    /// IO DataPointValues`:
    /// 1. `askForDataPoints _ [] = return []` — empty-name short-
    ///    circuit (preserved).
    /// 2. Set `dataPointsNames` + `askDataPoints` flag atomically.
    /// 3. Block on `takeTMVar dataPointsReply` with a 10-second
    ///    `registerDelay` timeout fallback.
    pub async fn ask_for_data_points(&self, names: Vec<DataPointName>) -> DataPointValues {
        // Upstream: `askForDataPoints _ [] = return []`.
        if names.is_empty() {
            return Vec::new();
        }

        // Acquire a `notified` future *before* mutating state — this
        // is the canonical tokio "register-then-check" pattern that
        // guarantees we don't miss a wake-up if `put_reply` runs
        // between our state update and the await below.
        let reply_notified = self.inner.reply_notify.notified();
        tokio::pin!(reply_notified);

        // Set names + ask flag + clear any stale reply atomically.
        // Mirror of:
        //   atomically $ do
        //     modifyTVar' dataPointsNames $ const dpNames
        //     modifyTVar' askDataPoints $ const True
        {
            let mut state = self.inner.state.lock().await;
            state.names = names;
            state.ask_flag = true;
            state.reply = None;
        }
        // Wake the acceptor loop.
        self.inner.ask_notify.notify_one();

        // Wait for reply or timeout. Mirror of:
        //   maxTimer <- registerDelay tenSeconds
        //   atomically $
        //     takeTMVar dataPointsReply
        //     `orElse`
        //     (readTVar maxTimer >>= check >> return [])
        match tokio::time::timeout(ASK_FOR_DATA_POINTS_TIMEOUT, reply_notified).await {
            Ok(()) => {
                let mut state = self.inner.state.lock().await;
                state.reply.take().unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Acceptor-loop API: block until external context calls
    /// [`Self::ask_for_data_points`] and return the names list.
    ///
    /// Mirror of upstream's `acceptorActions` block:
    ///   atomically $ readTVar askDataPoints >>= check
    ///   dpNames' <- readTVarIO dataPointsNames
    ///
    /// Does NOT clear the ask flag — the acceptor clears it
    /// implicitly via [`Self::put_reply`] after the next reply is
    /// in. (This matches the upstream invariant: a single ask-flag
    /// raise gets one round-trip of request/reply.)
    pub async fn wait_for_ask(&self) -> Vec<DataPointName> {
        loop {
            // Register first — see canonical tokio pattern.
            let notified = self.inner.ask_notify.notified();
            tokio::pin!(notified);
            {
                let state = self.inner.state.lock().await;
                if state.ask_flag {
                    return state.names.clone();
                }
            }
            notified.await;
        }
    }

    /// Acceptor-loop API: fill the reply slot if `values` is
    /// non-empty, then clear the ask flag and wake the external
    /// caller.
    ///
    /// Mirror of upstream's
    /// ```haskell
    /// unless (null replyWithDataPoints) $ atomically $ do
    ///   putTMVar dataPointsReply replyWithDataPoints
    ///   modifyTVar' askDataPoints $ const False
    /// ```
    /// The empty-reply short-circuit preserves upstream's
    /// "skip the initial dummy round-trip" optimization (the first
    /// acceptor-loop call sends `MsgDataPointsRequest []` to
    /// establish the channel; the empty reply is discarded).
    pub async fn put_reply(&self, values: DataPointValues) {
        if values.is_empty() {
            return;
        }
        {
            let mut state = self.inner.state.lock().await;
            state.reply = Some(values);
            state.ask_flag = false;
        }
        self.inner.reply_notify.notify_one();
    }

    /// Test-only: read the current ask flag without consuming it.
    /// Public-but-undocumented to support integration tests that
    /// need to observe handoff state from the acceptor side.
    #[doc(hidden)]
    pub async fn debug_ask_flag(&self) -> bool {
        self.inner.state.lock().await.ask_flag
    }

    /// Test-only: read the current names list without consuming
    /// it.
    #[doc(hidden)]
    pub async fn debug_names(&self) -> Vec<DataPointName> {
        self.inner.state.lock().await.names.clone()
    }
}

/// Free-function alias mirroring upstream's
/// `initDataPointRequestor :: IO DataPointRequestor`. Operationally
/// equivalent to [`DataPointRequestor::new`]; provided so call sites
/// reading like the upstream code (`initDataPointRequestor`) can be
/// ported verbatim.
pub fn init_data_point_requestor() -> DataPointRequestor {
    DataPointRequestor::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::DataPointValue;

    #[tokio::test]
    async fn new_requestor_starts_unsignalled() {
        let req = DataPointRequestor::new();
        assert!(!req.debug_ask_flag().await);
        assert!(req.debug_names().await.is_empty());
    }

    #[tokio::test]
    async fn init_data_point_requestor_alias_matches_new() {
        let req = init_data_point_requestor();
        assert!(!req.debug_ask_flag().await);
        assert!(req.debug_names().await.is_empty());
    }

    #[tokio::test]
    async fn ask_for_empty_names_short_circuits() {
        // Mirror of upstream `askForDataPoints _ [] = return []`.
        // The 10-second timeout must NOT fire — the call returns
        // immediately.
        let req = DataPointRequestor::new();
        let start = std::time::Instant::now();
        let reply = req.ask_for_data_points(vec![]).await;
        assert!(reply.is_empty());
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "empty-name ask must return immediately, took {:?}",
            start.elapsed()
        );
        // Ask flag must NOT be set (no acceptor wake-up needed).
        assert!(!req.debug_ask_flag().await);
    }

    #[tokio::test]
    async fn ask_sets_flag_and_names_visible_to_acceptor() {
        let req = DataPointRequestor::new();
        let req_clone = req.clone();
        // Run the ask in a background task — it'll wait for the
        // reply we don't provide, so we just inspect the in-flight
        // state from the test thread.
        let ask_task = tokio::spawn(async move {
            req_clone
                .ask_for_data_points(vec![
                    DataPointName::new("node-info"),
                    DataPointName::new("tip"),
                ])
                .await
        });
        // Yield to let the task acquire the mutex + raise the flag.
        for _ in 0..10 {
            tokio::task::yield_now().await;
            if req.debug_ask_flag().await {
                break;
            }
        }
        assert!(req.debug_ask_flag().await);
        let names = req.debug_names().await;
        assert_eq!(
            names,
            vec![DataPointName::new("node-info"), DataPointName::new("tip")]
        );
        // Drop the ask task (it would otherwise wait 10s for a reply).
        ask_task.abort();
        let _ = ask_task.await;
    }

    #[tokio::test]
    async fn wait_for_ask_blocks_until_external_signal() {
        let req = DataPointRequestor::new();
        let req_clone = req.clone();
        // Start the acceptor's wait_for_ask BEFORE external asks —
        // it must block until external context asks.
        let acceptor_task = tokio::spawn(async move { req_clone.wait_for_ask().await });
        // Make sure the acceptor is blocked.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!acceptor_task.is_finished(), "acceptor must block");

        // Now signal from external.
        let req_clone2 = req.clone();
        tokio::spawn(async move {
            req_clone2
                .ask_for_data_points(vec![DataPointName::new("ledger")])
                .await
        });

        // Acceptor task should now wake up with the names.
        let names = tokio::time::timeout(Duration::from_secs(1), acceptor_task)
            .await
            .expect("acceptor did not wake")
            .expect("acceptor task panicked");
        assert_eq!(names, vec![DataPointName::new("ledger")]);
    }

    #[tokio::test]
    async fn wait_for_ask_returns_immediately_if_flag_already_set() {
        // If external context raises the flag BEFORE the acceptor
        // ever calls wait_for_ask, the call must return immediately
        // (don't block waiting for a notify that already fired).
        let req = DataPointRequestor::new();
        let req_clone = req.clone();
        // Spawn the ask first — it blocks waiting for a reply that
        // never comes, but it leaves the flag raised.
        let ask_task = tokio::spawn(async move {
            req_clone
                .ask_for_data_points(vec![DataPointName::new("ready")])
                .await
        });
        // Wait until the flag is up.
        for _ in 0..50 {
            tokio::task::yield_now().await;
            if req.debug_ask_flag().await {
                break;
            }
        }
        assert!(req.debug_ask_flag().await);

        // Now wait_for_ask must return immediately.
        let start = std::time::Instant::now();
        let names = tokio::time::timeout(Duration::from_millis(500), req.wait_for_ask())
            .await
            .expect("wait_for_ask blocked unexpectedly");
        assert!(start.elapsed() < Duration::from_millis(100));
        assert_eq!(names, vec![DataPointName::new("ready")]);

        // Clean up the ask task.
        ask_task.abort();
        let _ = ask_task.await;
    }

    #[tokio::test]
    async fn put_reply_wakes_external_caller() {
        let req = DataPointRequestor::new();
        let req_clone = req.clone();
        // External asks.
        let ask_task = tokio::spawn(async move {
            req_clone
                .ask_for_data_points(vec![DataPointName::new("k")])
                .await
        });
        // Wait until the flag is up + acceptor would normally read
        // the names + send the request.
        for _ in 0..50 {
            tokio::task::yield_now().await;
            if req.debug_ask_flag().await {
                break;
            }
        }
        // Acceptor fills the reply.
        req.put_reply(vec![(
            DataPointName::new("k"),
            Some(DataPointValue::new(b"v".to_vec())),
        )])
        .await;

        // External caller must wake up with the reply.
        let reply = tokio::time::timeout(Duration::from_secs(1), ask_task)
            .await
            .expect("ask did not wake")
            .expect("ask task panicked");
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0].0, DataPointName::new("k"));
        assert_eq!(
            reply[0].1.as_ref().expect("Just"),
            &DataPointValue::new(b"v".to_vec())
        );
        // Ask flag must be cleared.
        assert!(!req.debug_ask_flag().await);
    }

    #[tokio::test]
    async fn put_reply_empty_is_noop() {
        // Empty reply: ask flag stays set, no reply_notify wake.
        let req = DataPointRequestor::new();
        let req_clone = req.clone();
        let _ask_task = tokio::spawn(async move {
            req_clone
                .ask_for_data_points(vec![DataPointName::new("k")])
                .await
        });
        for _ in 0..50 {
            tokio::task::yield_now().await;
            if req.debug_ask_flag().await {
                break;
            }
        }
        // Empty reply must NOT wake the caller or clear the flag.
        req.put_reply(vec![]).await;
        // Give the system a chance to react if there's a bug.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            req.debug_ask_flag().await,
            "empty reply must not clear ask flag"
        );
        // _ask_task drops, aborting it.
    }

    #[test]
    fn ask_for_data_points_timeout_constant_matches_upstream() {
        // Mirror of upstream `tenSeconds = 10 * 1000000 :: Int`
        // (10-second microsecond constant fed to `registerDelay`).
        assert_eq!(ASK_FOR_DATA_POINTS_TIMEOUT, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn ask_with_short_circuit_timeout_via_external_no_reply() {
        // Verify that ask_for_data_points eventually returns empty
        // when no reply ever arrives — but use a wrapper timeout to
        // bound the test (we don't want to wait 10s).
        //
        // We can't reach the 10-second internal timeout without
        // `tokio::time::pause` (which requires the `test-util`
        // feature, not enabled in this workspace). Instead, verify
        // that an outer `timeout(50ms, ask)` succeeds with
        // `Err(elapsed)` — proving the ask is genuinely blocking on
        // the reply rather than racing.
        let req = DataPointRequestor::new();
        let result = tokio::time::timeout(
            Duration::from_millis(50),
            req.ask_for_data_points(vec![DataPointName::new("never")]),
        )
        .await;
        // The ask is blocked waiting for reply → outer timeout fires.
        assert!(result.is_err(), "ask should block on reply");
    }

    #[tokio::test]
    async fn full_round_trip_sequence() {
        // Exercise the full external↔acceptor handoff for one round.
        let req = DataPointRequestor::new();
        let req_external = req.clone();
        let req_acceptor = req.clone();

        // External side: ask + await reply.
        let external = tokio::spawn(async move {
            req_external
                .ask_for_data_points(vec![DataPointName::new("a"), DataPointName::new("b")])
                .await
        });

        // Acceptor side: wait for ask, get names, simulate the
        // forwarder reply.
        let acceptor = tokio::spawn(async move {
            let names = req_acceptor.wait_for_ask().await;
            // Pretend the forwarder returned: a -> Some([0x01]),
            // b -> None.
            let reply = names
                .into_iter()
                .map(|n| {
                    if n.as_str() == "a" {
                        (n, Some(DataPointValue::new(vec![0x01])))
                    } else {
                        (n, None)
                    }
                })
                .collect::<Vec<_>>();
            req_acceptor.put_reply(reply).await;
        });

        let reply = tokio::time::timeout(Duration::from_secs(1), external)
            .await
            .expect("external did not wake")
            .expect("external panicked");
        acceptor.await.expect("acceptor panicked");

        assert_eq!(reply.len(), 2);
        assert_eq!(reply[0].0, DataPointName::new("a"));
        assert_eq!(
            reply[0].1.as_ref().expect("Just"),
            &DataPointValue::new(vec![0x01])
        );
        assert_eq!(reply[1].0, DataPointName::new("b"));
        assert!(reply[1].1.is_none());
        // Ask flag cleared after delivery.
        assert!(!req.debug_ask_flag().await);
    }

    #[tokio::test]
    async fn clone_shares_state_across_tasks() {
        // Multiple clones must observe the same underlying state.
        let req = DataPointRequestor::new();
        let a = req.clone();
        let b = req.clone();
        let req_third = req.clone();

        let external = tokio::spawn(async move {
            a.ask_for_data_points(vec![DataPointName::new("shared")])
                .await
        });

        let acceptor = tokio::spawn(async move {
            let names = b.wait_for_ask().await;
            assert_eq!(names, vec![DataPointName::new("shared")]);
            b.put_reply(vec![(
                DataPointName::new("shared"),
                Some(DataPointValue::new(vec![0xFF])),
            )])
            .await;
        });

        let reply = tokio::time::timeout(Duration::from_secs(1), external)
            .await
            .expect("external did not wake")
            .expect("external panicked");
        acceptor.await.expect("acceptor panicked");

        assert_eq!(reply.len(), 1);
        // Verify the third clone sees the cleared flag too.
        assert!(!req_third.debug_ask_flag().await);
    }
}
