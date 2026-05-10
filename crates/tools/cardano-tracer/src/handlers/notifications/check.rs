//! Per-event severity dispatch — routes incoming trace events to the
//! correct [`super::types::EventGroup`] queue.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Check.hs.
//!
//! Direct port of the single-function upstream module. For each
//! [`super::types::Event`] of [`crate::severity::SeverityS`] level
//! `Warning`-and-above, dispatch the event onto the appropriate
//! event-group queue. Lower severities (`Debug` / `Info` / `Notice`)
//! are dropped.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                        |
//! |---------------------------------------------------------|----------------------------------|
//! | `checkCommonErrors :: NodeId -> TraceObjectInfo -> EventsQueues -> IO ()` | [`check_common_errors`] |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`TraceObjectInfo` 3-tuple `(msg, sev, ts)`**: upstream's
//!   `TraceObjectInfo` is a tuple of `(message, severity,
//!   timestamp)` extracted from a `TraceObject`. The Rust port
//!   takes those three values as separate parameters since the
//!   upstream `TraceObject` type is not yet ported (carve-out
//!   deferred per [`super::types`]'s docstring).
//! - **`addNewEvent` from `Notifications/Utils`**: upstream's
//!   `addNewEvent eventsQueues eventGroup event` runs in `IO`, looks
//!   up the queue under `eventGroup`, and pushes via
//!   `STM.atomically . writeTBQueue`. The Rust port inlines the
//!   send to avoid the extra Utils.hs port round; once the
//!   `Notifications/Utils.hs` round lands the Rust function will be
//!   refactored to call the Utils helper.

use crate::severity::SeverityS;
use crate::types::NodeId;

use super::types::{Event, EventGroup};

/// Send sender side of the event-group queues — needed because
/// upstream's `EventsQueues` map stores receivers (the consumer
/// side) and the producer side is held alongside it externally.
/// The Rust port keeps the receiver inside [`EventsQueues`] (per
/// [`super::types::EventsQueues`]) and adds this companion
/// `EventsSenders` map for the producer side.
///
/// This auxiliary alias is **not** in upstream — upstream uses STM
/// `TBQueue` which is bidirectional and stored once. The split into
/// receiver+sender is a Rust-side requirement of `tokio::sync::mpsc`
/// where the producer half must be cloned to be sharable. Documented
/// here rather than in `super::types` so the surface tracks the
/// upstream Notifications.Types.hs file 1:1.
pub type EventsSenders = std::sync::Arc<
    tokio::sync::RwLock<
        std::collections::BTreeMap<EventGroup, tokio::sync::mpsc::UnboundedSender<Event>>,
    >,
>;

/// Construct an empty [`EventsSenders`] map.
pub fn new_events_senders() -> EventsSenders {
    std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::BTreeMap::new()))
}

/// Route an incoming event to its event-group queue. Mirror of
/// upstream `checkCommonErrors :: NodeId -> TraceObjectInfo ->
/// EventsQueues -> IO ()`. Returns `true` if the event was routed
/// to a queue, `false` if the severity was below `Warning` (and
/// therefore dropped, mirroring upstream's catch-all `_ -> return ()`).
///
/// Parameters mirror upstream's tuple destructuring of
/// `TraceObjectInfo (msg, sev, ts)`.
pub async fn check_common_errors(
    node_id: NodeId,
    message: String,
    severity: SeverityS,
    time_ms: i64,
    senders: &EventsSenders,
) -> bool {
    let Some(group) = EventGroup::from_severity(severity) else {
        return false;
    };
    let event = Event::new(node_id, time_ms, severity, message);
    let guard = senders.read().await;
    let Some(tx) = guard.get(&group) else {
        // No queue registered for this group — drop the event but
        // count it as not-routed for the caller's reporting.
        return false;
    };
    tx.send(event).is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::types::{EventsQueue, Timer, new_events_queues};
    use super::*;
    use tokio::sync::mpsc;

    /// Helper: register one event-group queue and return both the
    /// senders + receivers handles.
    async fn register_group(
        group: EventGroup,
        queues: &super::super::types::EventsQueues,
        senders: &EventsSenders,
    ) -> EventsQueue {
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(group, tx);
        // Stash a placeholder receiver+timer in the EventsQueues map
        // for parity with how upstream's EventsQueues holds
        // `(EventsQueue, Timer)` pairs. The actual rx returned to
        // the test is fresh — we don't store the same one.
        let (_, fresh_rx) = mpsc::unbounded_channel::<Event>();
        queues
            .write()
            .await
            .insert(group, (fresh_rx, Timer::placeholder()));
        rx
    }

    fn sample_node() -> NodeId {
        NodeId::new("node-spo-7")
    }

    #[tokio::test]
    async fn check_common_errors_routes_warning_to_event_warnings_queue() {
        let queues = new_events_queues();
        let senders = new_events_senders();
        let mut rx = register_group(EventGroup::EventWarnings, &queues, &senders).await;

        let routed = check_common_errors(
            sample_node(),
            "blockfetch lag".to_string(),
            SeverityS::Warning,
            1_700_000_000_000,
            &senders,
        )
        .await;
        assert!(routed);

        let event = rx.recv().await.expect("queue receives event");
        assert_eq!(event.node_id, sample_node());
        assert_eq!(event.severity, SeverityS::Warning);
        assert_eq!(event.message, "blockfetch lag");
        assert_eq!(event.time_ms, 1_700_000_000_000);
    }

    #[tokio::test]
    async fn check_common_errors_routes_each_high_severity_to_correct_group() {
        let queues = new_events_queues();
        let senders = new_events_senders();
        let mut rx_err = register_group(EventGroup::EventErrors, &queues, &senders).await;
        let mut rx_crit = register_group(EventGroup::EventCriticals, &queues, &senders).await;
        let mut rx_alert = register_group(EventGroup::EventAlerts, &queues, &senders).await;
        let mut rx_em = register_group(EventGroup::EventEmergencies, &queues, &senders).await;

        for (sev, label) in [
            (SeverityS::Error, "err"),
            (SeverityS::Critical, "crit"),
            (SeverityS::Alert, "alt"),
            (SeverityS::Emergency, "em"),
        ] {
            let routed =
                check_common_errors(sample_node(), label.to_string(), sev, 0, &senders).await;
            assert!(routed, "{sev:?} should route");
        }

        assert_eq!(rx_err.recv().await.expect("err").severity, SeverityS::Error);
        assert_eq!(
            rx_crit.recv().await.expect("crit").severity,
            SeverityS::Critical,
        );
        assert_eq!(
            rx_alert.recv().await.expect("alt").severity,
            SeverityS::Alert,
        );
        assert_eq!(
            rx_em.recv().await.expect("em").severity,
            SeverityS::Emergency,
        );
    }

    #[tokio::test]
    async fn check_common_errors_drops_low_severity_events() {
        let queues = new_events_queues();
        let senders = new_events_senders();
        // Register a Warnings queue but the event severity is Info.
        let mut rx = register_group(EventGroup::EventWarnings, &queues, &senders).await;

        let routed = check_common_errors(
            sample_node(),
            "blockfetch advanced".to_string(),
            SeverityS::Info,
            0,
            &senders,
        )
        .await;
        assert!(!routed);

        // Verify the receiver got nothing — try_recv returns Empty.
        match rx.try_recv() {
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            other => panic!("expected empty queue, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_common_errors_drops_debug_and_notice_severities() {
        let queues = new_events_queues();
        let senders = new_events_senders();
        let _rx = register_group(EventGroup::EventWarnings, &queues, &senders).await;

        for sev in [SeverityS::Debug, SeverityS::Info, SeverityS::Notice] {
            let routed =
                check_common_errors(sample_node(), "low".to_string(), sev, 0, &senders).await;
            assert!(!routed, "{sev:?} should not route");
        }
    }

    #[tokio::test]
    async fn check_common_errors_returns_false_when_group_not_registered() {
        let senders = new_events_senders();
        // No groups registered.

        let routed = check_common_errors(
            sample_node(),
            "warn".to_string(),
            SeverityS::Warning,
            0,
            &senders,
        )
        .await;
        assert!(!routed);
    }

    #[tokio::test]
    async fn new_events_senders_starts_empty() {
        let senders = new_events_senders();
        assert!(senders.read().await.is_empty());
    }
}
