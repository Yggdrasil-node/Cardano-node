//! Notification-engine utility helpers — queue lookup + event push
//! + queue flush.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Utils.hs.
//!
//! Direct port of upstream's bounded utility helpers
//! (`addNewEvent` + `getNewEvents`) plus stub-and-defer markers for
//! the timer-bound entries (`initEventsQueues`,
//! `updateNotificationsEvents`, `updateNotificationsPeriods`,
//! `changeTimerState`) that need [`super::types::Timer`]'s full
//! implementation before they can ship.
//!
//! Mapping summary:
//!
//! | Upstream                                                     | Yggdrasil                              |
//! |--------------------------------------------------------------|----------------------------------------|
//! | `addNewEvent :: EventsQueues -> EventGroup -> Event -> IO ()` | [`add_new_event`]                      |
//! | `getNewEvents :: EventsQueues -> EventGroup -> IO [Event]`   | [`get_new_events`]                     |
//! | `initEventsQueues :: ... -> IO EventsQueues`                 | (deferred — see [`init_events_queues_status`]) |
//! | `updateNotificationsEvents :: EventsQueues -> EventGroup -> Bool -> IO ()` | (deferred) |
//! | `updateNotificationsPeriods :: EventsQueues -> EventGroup -> PeriodInSec -> IO ()` | (deferred) |
//! | `changeTimerState :: (Timer -> IO ()) -> EventsQueues -> EventGroup -> IO ()` | (deferred) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`initEventsQueues`**: depends on the full
//!   [`super::types::Timer`] surface (forkIO + killThread closures + setCallPeriod) which lands in a future round per the parity-matrix `remaining_work` list. Stub status documented in [`init_events_queues_status`].
//! - **`updateNotificationsEvents` / `updateNotificationsPeriods` /
//!   `changeTimerState`**: same Timer dependency.
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
use super::types::{Event, EventGroup, EventsQueues};

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
}
