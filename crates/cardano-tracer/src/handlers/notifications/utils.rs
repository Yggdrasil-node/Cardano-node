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

/// Status struct describing why [`init_events_queues`]-equivalent
/// is not yet available — exposed as a public type so downstream
/// callers can reference it in their own deferred-work tracking
/// without duplicating the rationale string.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InitEventsQueuesStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the upstream module dependency.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `initEventsQueues`. Used
/// by sites that want to surface the deferral programmatically
/// (e.g. when wiring up a partial cardano-tracer runtime that needs
/// to skip the notification-engine init path).
pub fn init_events_queues_status() -> InitEventsQueuesStatus {
    InitEventsQueuesStatus {
        status: "deferred",
        depends_on: "super::types::Timer (upstream Notifications/Timer.hs)",
        deferred_round: "R385+",
    }
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
    fn init_events_queues_status_describes_deferral() {
        let s = init_events_queues_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("Timer"));
        assert_eq!(s.deferred_round, "R385+");
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
