//! Notification-engine utility helpers — queue lookup + event push
//! + queue flush + per-group timer control.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Utils.hs.
//!
//! Direct port of upstream's utility helpers. R385 shipped the
//! bounded subset (`addNewEvent`, `getNewEvents`); R387 re-enables
//! the timer-bound entries (`updateNotificationsEvents`,
//! `updateNotificationsPeriods`, `changeTimerState`) now that
//! [`super::timer::Timer`] is fully implemented (R386).
//!
//! Mapping summary:
//!
//! | Upstream                                                     | Yggdrasil                              |
//! |--------------------------------------------------------------|----------------------------------------|
//! | `addNewEvent :: EventsQueues -> EventGroup -> Event -> IO ()` | [`add_new_event`]                      |
//! | `getNewEvents :: EventsQueues -> EventGroup -> IO [Event]`   | [`get_new_events`]                     |
//! | `updateNotificationsEvents :: EventsQueues -> EventGroup -> Bool -> IO ()` | [`update_notifications_events`] |
//! | `updateNotificationsPeriods :: EventsQueues -> EventGroup -> PeriodInSec -> IO ()` | [`update_notifications_periods`] |
//! | `changeTimerState :: (Timer -> IO ()) -> EventsQueues -> EventGroup -> IO ()` | [`change_timer_state`]   |
//! | `initEventsQueues :: ... -> IO EventsQueues`                 | (deferred — see [`init_events_queues_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`initEventsQueues`**: still deferred — depends on Notifications/Send.hs (`makeAndSendNotification`) + DataPointRequestors + tracer-trace channel. Status documented in [`init_events_queues_status`]; downstream callers can reference it programmatically.
//! - **`Cardano.Tracer.MetaTrace.TracerTrace`**: upstream's
//!   `initEventsQueues` writes trace events to a `Trace IO
//!   TracerTrace` channel during initialization. Yggdrasil-side
//!   tracer-trace surface is not yet ported (deferred per the
//!   sister-tools port arc plan).
//! - **`isFullTBQueue` bounded-queue check**: upstream's
//!   `addNewEvent` skips the write if the queue is full
//!   (`unlessM isFullTBQueue ...`). Yggdrasil's
//!   `tokio::sync::mpsc::UnboundedSender` is unbounded — see the
//!   `EventsQueue` carve-out in [`super::types`]'s docstring. The
//!   Rust `add_new_event` therefore never fails on a full queue;
//!   if a future round needs strict bounded-queue semantics, swap
//!   `EventsQueue` to `tokio::sync::mpsc::Receiver<Event>` (bounded)
//!   and observe `try_send` Err(Full) here.

use super::check::EventsSenders;
use super::types::{Event, EventGroup, EventsQueues, PeriodInSec, Timer};

/// Push a new event to the per-group queue. Mirror of upstream
/// `addNewEvent eventsQueues eventGroup event`.
///
/// Returns `true` if the event was successfully sent; `false` if the
/// group has no registered sender (mirror of upstream's silent
/// no-op when `M.lookup eventGroup queues` returns `Nothing`).
///
/// Note: signature takes [`EventsSenders`] (the producer side, added
/// in R381) instead of upstream's [`EventsQueues`]. Upstream's STM
/// `TBQueue` is bidirectional; Yggdrasil splits it across an
/// `mpsc::UnboundedSender` (in `EventsSenders`) + an
/// `mpsc::UnboundedReceiver` (in `EventsQueues`). The producer side
/// is what `addNewEvent` actually needs.
pub async fn add_new_event(senders: &EventsSenders, event_group: EventGroup, event: Event) -> bool {
    let guard = senders.read().await;
    let Some(tx) = guard.get(&event_group) else {
        return false;
    };
    tx.send(event).is_ok()
}

/// Drain all currently-queued events for a group. Mirror of upstream
/// `getNewEvents eventsQueues eventGroup`.
///
/// Returns the events in FIFO order (oldest first), or an empty
/// vector if the group has no registered queue. The receiver-side
/// half of [`EventsQueues`] is consumed via `try_recv` in a loop
/// until the queue is empty (mirror of upstream's
/// `atomically $ flushTBQueue queue`).
pub async fn get_new_events(queues: &EventsQueues, event_group: EventGroup) -> Vec<Event> {
    let mut guard = queues.write().await;
    let Some((rx, _timer)) = guard.get_mut(&event_group) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

/// Apply a per-Timer transform to the timer registered under a
/// given [`EventGroup`] in the [`EventsQueues`] map. Mirror of
/// upstream
/// `changeTimerState :: (Timer -> IO ()) -> EventsQueues -> EventGroup -> IO ()`.
///
/// The closure runs while holding the read-lock on
/// [`EventsQueues`]; it does **not** mutate the map. Upstream's
/// `Timer`-side mutation (start/stop/set_period) operates on
/// internal `Mutex`-shared state that the timer's spawn-loop reads
/// — see [`super::timer::Timer`].
///
/// Returns `true` if the timer was found and the closure ran;
/// `false` if no timer is registered for `event_group`.
pub async fn change_timer_state<F>(
    queues: &EventsQueues,
    event_group: EventGroup,
    setter: F,
) -> bool
where
    F: AsyncFn(&Timer),
{
    let guard = queues.read().await;
    let Some((_rx, timer)) = guard.get(&event_group) else {
        return false;
    };
    setter(timer).await;
    true
}

/// Toggle a per-event-group timer on/off. Mirror of upstream
/// `updateNotificationsEvents queues group True = changeTimerState
/// startTimer queues group; updateNotificationsEvents queues group
/// False = changeTimerState stopTimer queues group`.
pub async fn update_notifications_events(
    queues: &EventsQueues,
    event_group: EventGroup,
    enabled: bool,
) -> bool {
    if enabled {
        change_timer_state(queues, event_group, async |timer| {
            timer.start_timer().await;
        })
        .await
    } else {
        change_timer_state(queues, event_group, async |timer| {
            timer.stop_timer().await;
        })
        .await
    }
}

/// Update the period of a per-event-group timer. Mirror of upstream
/// `updateNotificationsPeriods queues group period =
/// changeTimerState (\`setCallPeriod\` period) queues group`.
pub async fn update_notifications_periods(
    queues: &EventsQueues,
    event_group: EventGroup,
    period: PeriodInSec,
) -> bool {
    change_timer_state(queues, event_group, async |timer| {
        timer.set_call_period(period).await;
    })
    .await
}

/// Status descriptor for the previously-carved-out
/// `initEventsQueues` orchestration. Closed at R405 with R386's
/// Timer + R403's lettre + R404's makeAndSendNotification all
/// landed.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InitEventsQueuesStatus {
    /// One-line summary of the closure status.
    pub status: &'static str,
    /// Round at which the orchestration landed.
    pub closed_at_round: &'static str,
}

/// Get the closure-status descriptor for `initEventsQueues`. R405
/// closes the carve-out: the actual orchestration is
/// [`init_events_queues`].
pub fn init_events_queues_status() -> InitEventsQueuesStatus {
    InitEventsQueuesStatus {
        status: "closed at R405",
        closed_at_round: "R405",
    }
}

/// Initialize the per-event-group queues + senders + timers for a
/// running cardano-tracer instance. Mirror of upstream
/// `initEventsQueues`.
///
/// On entry:
/// 1. Reads `EmailSettings` from disk via R384's
///    [`super::settings::read_saved_email_settings`].
/// 2. If the email config is incomplete (no SMTP host configured),
///    returns empty queues + senders — the notification engine is
///    effectively disabled. Mirror of upstream's
///    `if incompleteEmailSettings emailSettings then pure [] else ...`.
/// 3. Otherwise reads `EventsSettings` from disk via
///    [`super::settings::read_saved_events_settings`] and creates 6
///    per-group queues, each backed by a [`super::types::Timer`]
///    (R386) that periodically calls
///    [`super::send::make_and_send_notification`] (R404).
///
/// `state_dir` is the operator-supplied state directory; passed
/// through to the settings loaders.
///
/// `node_name_resolver` resolves `NodeId` → `NodeName` for the
/// notification body. Per R398's plan option (b), this is a closure
/// rather than coupling to TracerEnv. Production sites build it from
/// a snapshot of `ConnectedNodesNames`.
///
/// Returns the populated `(EventsQueues, EventsSenders)` pair.
/// Empty pair when the email config is incomplete.
pub async fn init_events_queues<F>(
    state_dir: Option<&std::path::Path>,
    node_name_resolver: F,
) -> (EventsQueues, super::check::EventsSenders)
where
    F: Fn(&crate::types::NodeId) -> crate::types::NodeName + Clone + Send + Sync + 'static,
{
    use super::check::new_events_senders;
    use super::send::make_and_send_notification;
    use super::settings::{
        incomplete_email_settings, read_saved_email_settings, read_saved_events_settings,
    };
    use super::types::{Timer, new_events_queues};
    use std::sync::Arc;

    let email_settings = read_saved_email_settings(state_dir);
    let queues = new_events_queues();
    let senders = new_events_senders();

    if incomplete_email_settings(&email_settings) {
        return (queues, senders);
    }

    let events_settings = read_saved_events_settings(state_dir);
    let last_time = Arc::new(tokio::sync::Mutex::new(0_i64));

    let groups: [(EventGroup, (bool, u32)); 6] = [
        (EventGroup::EventWarnings, events_settings.warnings),
        (EventGroup::EventErrors, events_settings.errors),
        (EventGroup::EventCriticals, events_settings.criticals),
        (EventGroup::EventAlerts, events_settings.alerts),
        (EventGroup::EventEmergencies, events_settings.emergencies),
        (
            EventGroup::EventNodeDisconnected,
            events_settings.node_disconnected,
        ),
    ];

    for (group, (initial_running, period_secs)) in groups {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

        // Each timer's periodic action calls
        // make_and_send_notification on the corresponding group's
        // queue. Clone all owned state so the closure can run
        // independently.
        let resolver_for_timer = node_name_resolver.clone();
        let settings_for_timer = email_settings.clone();
        let last_time_for_timer = Arc::clone(&last_time);
        let queues_for_timer = Arc::clone(&queues);
        let group_for_timer = group;

        let timer_action = move || {
            // Spawn an async task — Timer's action signature is
            // `Fn() + Send + Sync + 'static`, but
            // make_and_send_notification is async.
            let resolver = resolver_for_timer.clone();
            let settings = settings_for_timer.clone();
            let lt = Arc::clone(&last_time_for_timer);
            let queues_inner = Arc::clone(&queues_for_timer);
            tokio::spawn(async move {
                let _ = make_and_send_notification(
                    &settings,
                    &queues_inner,
                    group_for_timer,
                    &lt,
                    move |id| resolver(id),
                )
                .await;
            });
        };

        let timer = Timer::new_stderr(timer_action, initial_running, period_secs);

        senders.write().await.insert(group, tx);
        queues.write().await.insert(group, (rx, timer));
    }

    (queues, senders)
}

#[cfg(test)]
mod tests {
    use super::super::check::new_events_senders;
    use super::super::types::{Timer, new_events_queues};
    use super::*;
    use crate::severity::SeverityS;
    use crate::types::NodeId;
    use tokio::sync::mpsc;

    fn sample_event(group_severity: SeverityS, msg: &str) -> Event {
        Event::new(
            NodeId::new("node-spo-1"),
            1_700_000_000_000,
            group_severity,
            msg.to_string(),
        )
    }

    #[tokio::test]
    async fn add_new_event_returns_true_when_group_registered() {
        let senders = new_events_senders();
        let queues = new_events_queues();
        // Register the warnings group on both sides.
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(EventGroup::EventWarnings, tx);
        queues
            .write()
            .await
            .insert(EventGroup::EventWarnings, (rx, Timer::placeholder()));

        let routed = add_new_event(
            &senders,
            EventGroup::EventWarnings,
            sample_event(SeverityS::Warning, "blockfetch lag"),
        )
        .await;
        assert!(routed);
    }

    #[tokio::test]
    async fn add_new_event_returns_false_when_group_not_registered() {
        let senders = new_events_senders();
        let routed = add_new_event(
            &senders,
            EventGroup::EventErrors,
            sample_event(SeverityS::Error, "fail"),
        )
        .await;
        assert!(!routed);
    }

    #[tokio::test]
    async fn get_new_events_drains_all_queued_events_in_fifo_order() {
        let senders = new_events_senders();
        let queues = new_events_queues();
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(EventGroup::EventErrors, tx);
        queues
            .write()
            .await
            .insert(EventGroup::EventErrors, (rx, Timer::placeholder()));

        // Push 3 events, then drain.
        for i in 0..3 {
            let routed = add_new_event(
                &senders,
                EventGroup::EventErrors,
                sample_event(SeverityS::Error, &format!("event-{i}")),
            )
            .await;
            assert!(routed);
        }

        let drained = get_new_events(&queues, EventGroup::EventErrors).await;
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].message, "event-0");
        assert_eq!(drained[1].message, "event-1");
        assert_eq!(drained[2].message, "event-2");
    }

    #[tokio::test]
    async fn get_new_events_returns_empty_when_group_not_registered() {
        let queues = new_events_queues();
        let drained = get_new_events(&queues, EventGroup::EventEmergencies).await;
        assert!(drained.is_empty());
    }

    #[tokio::test]
    async fn get_new_events_returns_empty_when_queue_is_empty() {
        let senders = new_events_senders();
        let queues = new_events_queues();
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(EventGroup::EventCriticals, tx);
        queues
            .write()
            .await
            .insert(EventGroup::EventCriticals, (rx, Timer::placeholder()));

        let drained = get_new_events(&queues, EventGroup::EventCriticals).await;
        assert!(drained.is_empty());
    }

    #[tokio::test]
    async fn get_new_events_after_drain_yields_empty() {
        let senders = new_events_senders();
        let queues = new_events_queues();
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(EventGroup::EventAlerts, tx);
        queues
            .write()
            .await
            .insert(EventGroup::EventAlerts, (rx, Timer::placeholder()));

        let _routed = add_new_event(
            &senders,
            EventGroup::EventAlerts,
            sample_event(SeverityS::Alert, "first"),
        )
        .await;

        let first_drain = get_new_events(&queues, EventGroup::EventAlerts).await;
        assert_eq!(first_drain.len(), 1);

        // Second drain should be empty.
        let second_drain = get_new_events(&queues, EventGroup::EventAlerts).await;
        assert!(second_drain.is_empty());
    }

    #[test]
    fn init_events_queues_status_describes_closure() {
        let s = init_events_queues_status();
        assert_eq!(s.status, "closed at R405");
        assert_eq!(s.closed_at_round, "R405");
    }

    #[tokio::test]
    async fn init_events_queues_returns_empty_when_email_incomplete() {
        // No state-dir → falls back to default EmailSettings which
        // is incomplete → empty queues.
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-init-events-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).expect("tempdir");
        let (queues, senders) = init_events_queues(Some(&tmp), |id| id.as_str().to_string()).await;
        assert!(queues.read().await.is_empty());
        assert!(senders.read().await.is_empty());
    }

    #[tokio::test]
    async fn change_timer_state_returns_false_for_unregistered_group() {
        let queues = new_events_queues();
        let invoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let invoked_for_closure = std::sync::Arc::clone(&invoked);
        let result = change_timer_state(&queues, EventGroup::EventErrors, async move |_t| {
            invoked_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await;
        assert!(!result);
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn change_timer_state_runs_closure_for_registered_group() {
        let queues = new_events_queues();
        let (_, rx) = mpsc::unbounded_channel::<Event>();
        queues
            .write()
            .await
            .insert(EventGroup::EventWarnings, (rx, Timer::placeholder()));

        let invoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let invoked_for_closure = std::sync::Arc::clone(&invoked);
        let result = change_timer_state(&queues, EventGroup::EventWarnings, async move |_t| {
            invoked_for_closure.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await;
        assert!(result);
        assert!(invoked.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn update_notifications_events_starts_and_stops_timer() {
        let queues = new_events_queues();
        let timer = Timer::new(
            |_msg: &str| {},
            false,
            || {},
            false, // initially stopped
            10,
        );
        let (_, rx) = mpsc::unbounded_channel::<Event>();
        queues
            .write()
            .await
            .insert(EventGroup::EventErrors, (rx, timer.clone()));

        // Initially the timer is not running.
        assert!(!timer.is_running().await);

        // Enable.
        let enabled = update_notifications_events(&queues, EventGroup::EventErrors, true).await;
        assert!(enabled);
        assert!(timer.is_running().await);

        // Disable.
        let disabled = update_notifications_events(&queues, EventGroup::EventErrors, false).await;
        assert!(disabled);
        assert!(!timer.is_running().await);

        timer.kill();
    }

    #[tokio::test]
    async fn update_notifications_events_returns_false_for_unregistered_group() {
        let queues = new_events_queues();
        let result = update_notifications_events(&queues, EventGroup::EventAlerts, true).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn update_notifications_periods_swaps_call_period_in_flight() {
        let queues = new_events_queues();
        let timer = Timer::new(|_msg: &str| {}, false, || {}, false, 10);
        let (_, rx) = mpsc::unbounded_channel::<Event>();
        queues
            .write()
            .await
            .insert(EventGroup::EventCriticals, (rx, timer.clone()));

        assert_eq!(timer.call_period().await, 10);

        let updated = update_notifications_periods(&queues, EventGroup::EventCriticals, 60).await;
        assert!(updated);
        assert_eq!(timer.call_period().await, 60);

        timer.kill();
    }

    #[tokio::test]
    async fn update_notifications_periods_returns_false_for_unregistered_group() {
        let queues = new_events_queues();
        let result = update_notifications_periods(&queues, EventGroup::EventEmergencies, 30).await;
        assert!(!result);
    }
}
