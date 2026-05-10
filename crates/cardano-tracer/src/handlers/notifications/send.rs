//! Notification-engine send orchestration — drains event queues,
//! formats body text, dispatches to the SMTP layer.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Send.hs.
//!
//! Direct port of upstream's notification-send module. The full
//! surface ships as of R404 — body-formatting helpers from R389
//! plus the orchestration entry-point [`make_and_send_notification`]
//! that drains the per-group queue, filters by last-seen timestamp,
//! resolves node names via a caller-supplied closure, and sends
//! through R403's [`super::email::create_and_send_email`].
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `sendNotification` body-formatting (`preface` + `events`)      | [`format_notification_body`]           |
//! | `formatTS = T.pack . formatTime defaultTimeLocale "%F %T %Z"` | [`format_event_timestamp`]             |
//! | `getNodeName` lookup-with-fallback                             | [`get_node_name`]                      |
//! | `makeAndSendNotification :: ... -> IO ()`                      | [`make_and_send_notification`]         |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Data.Time.Format.formatTime`** with `"%F %T %Z"` produces an
//!   ISO-8601 date + 24-hour time + timezone abbreviation. The Rust
//!   port emits the same shape using a manual format string against
//!   the Unix-epoch-ms Event::time_ms field — no `chrono` dependency
//!   needed since the upstream format is fixed and the timezone is
//!   always reported as `UTC` for parity (upstream's `%Z` resolves
//!   to whatever the system locale is set to; in operational
//!   practice tracer hosts run in UTC).
//! - **`askNodeNameRaw` data-point requestor chain**: per the R398
//!   plan's TracerEnv option (b) decision, [`make_and_send_notification`]
//!   takes a `Fn(&NodeId) -> NodeName` closure rather than coupling
//!   to TracerEnv + DataPointRequestors. Production sites build the
//!   closure from a snapshot of `ConnectedNodesNames` taken
//!   immediately before the call.

use crate::severity::SeverityS;
use crate::types::{NodeId, NodeName};

use super::types::Event;

/// Format the body text for an outgoing notification. Mirror of
/// upstream `sendNotification`'s body-construction (`preface <>
/// events`).
///
/// `node_id_to_name` is the per-node-id lookup the orchestration
/// layer would normally derive from `ConnectedNodesNames` — passed
/// as a slice of pairs so this function stays pure (testable
/// without async machinery).
///
/// Returns the formatted body string. Empty `events` yields an
/// empty string (mirror of upstream's `sendNotification _ [] _ =
/// return ()` early-out — the caller is expected to skip the SMTP
/// send when this returns empty).
pub fn format_notification_body(
    events: &[Event],
    node_id_to_name: &[(NodeId, NodeName)],
) -> String {
    if events.is_empty() {
        return String::new();
    }
    let only_one = events.len() == 1;
    let header_word = if only_one { "event" } else { "events" };
    let mut body = String::new();
    body.push_str("This is a notification from Cardano RTView service.\n");
    body.push('\n');
    body.push_str(&format!("The following {header_word} occurred:\n",));
    body.push('\n');
    for event in events {
        let ts = format_event_timestamp(event.time_ms);
        let node_name = get_node_name(&event.node_id, node_id_to_name);
        let sev = format_severity(event.severity);
        let msg = &event.message;
        body.push_str(&format!("[{ts}] [{node_name}] [{sev}] [{msg}]\n"));
    }
    // Mirror upstream's `T.intercalate nl` — drop the trailing
    // newline so the result joins cleanly with downstream
    // concatenation.
    if body.ends_with('\n') {
        body.pop();
    }
    body
}

/// Format a Unix-epoch-millisecond timestamp as `%F %T UTC`. Mirror
/// of upstream `formatTS = T.pack . formatTime defaultTimeLocale
/// "%F %T %Z"`. Yggdrasil hard-codes the timezone label to `UTC`
/// per the carve-out documented in the module docstring.
pub fn format_event_timestamp(time_ms: i64) -> String {
    let total_secs = time_ms.div_euclid(1000);
    let days = total_secs.div_euclid(86_400);
    let secs_within_day = total_secs.rem_euclid(86_400);
    let h = secs_within_day / 3_600;
    let m = (secs_within_day % 3_600) / 60;
    let s = secs_within_day % 60;
    let (year, month, day) = days_since_epoch_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02} UTC")
}

/// Look up a node-id's display name with fallback to the id's
/// underlying string. Mirror of upstream's `getNodeName` inline
/// helper inside `sendNotification`.
pub fn get_node_name(node_id: &NodeId, node_id_to_name: &[(NodeId, NodeName)]) -> String {
    node_id_to_name
        .iter()
        .find_map(|(id, name)| (id == node_id).then(|| name.clone()))
        .unwrap_or_else(|| node_id.as_str().to_string())
}

/// Render a [`SeverityS`] as upstream's `showT sev` would — by
/// taking the variant's name and packing it as text.
fn format_severity(severity: SeverityS) -> &'static str {
    match severity {
        SeverityS::Debug => "Debug",
        SeverityS::Info => "Info",
        SeverityS::Notice => "Notice",
        SeverityS::Warning => "Warning",
        SeverityS::Error => "Error",
        SeverityS::Critical => "Critical",
        SeverityS::Alert => "Alert",
        SeverityS::Emergency => "Emergency",
    }
}

/// Convert a count of days since 1970-01-01 to (year, month, day).
/// Adapted from the standard "civil from days" algorithm by Howard
/// Hinnant (public domain) — used here to avoid a chrono dependency
/// for the bounded format-timestamp helper.
fn days_since_epoch_to_ymd(days: i64) -> (i32, u32, u32) {
    // Shift epoch to 0000-03-01 (the start of a leap-cycle).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

/// Status descriptor for the previously-carved-out
/// `makeAndSendNotification` orchestration. Closed at R404 with the
/// R403 lettre land + R389 body-formatting subset already in place.
/// Kept around so call sites that previously queried for the status
/// can see the closure round.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MakeAndSendNotificationStatus {
    /// One-line summary of the closure status.
    pub status: &'static str,
    /// Round at which the orchestration landed.
    pub closed_at_round: &'static str,
}

/// Get the closure-status descriptor for `makeAndSendNotification`.
/// R404 closes the carve-out: the actual orchestration is
/// [`make_and_send_notification`].
pub fn make_and_send_notification_status() -> MakeAndSendNotificationStatus {
    MakeAndSendNotificationStatus {
        status: "closed at R404",
        closed_at_round: "R404",
    }
}

use std::sync::Arc;

use super::types::{EmailSettings, EventGroup, EventsQueues};

/// Drain the per-event-group queue, format an email body, and send
/// it via [`super::email::create_and_send_email`]. Mirror of
/// upstream `makeAndSendNotification`.
///
/// `last_time_ms` is shared mutable state (`Arc<tokio::sync::Mutex<i64>>`)
/// tracking the timestamp of the most recent event already
/// processed; only events with `time_ms > last_time_ms` are
/// included in the outgoing notification. Mirror of upstream's
/// `TVar UTCTime lastTime` semantics.
///
/// `node_name_resolver` is a closure that maps each `NodeId` to a
/// `NodeName`. Mirror of upstream's `askNodeNameRaw` chain — kept
/// as a closure rather than coupled to TracerEnv per the R398 plan's
/// option (b) decision.
///
/// Returns the [`super::email::StatusMessage`] from the SMTP send
/// (or [`super::email::STATUS_SUCCESS`] when the empty-events
/// short-circuit fires).
pub async fn make_and_send_notification<F>(
    email_settings: &EmailSettings,
    queues: &EventsQueues,
    event_group: EventGroup,
    last_time_ms: &Arc<tokio::sync::Mutex<i64>>,
    node_name_resolver: F,
) -> super::email::StatusMessage
where
    F: Fn(&NodeId) -> NodeName,
{
    use super::email::{STATUS_SUCCESS, create_and_send_email};
    use super::utils::get_new_events;

    let events = get_new_events(queues, event_group).await;
    if events.is_empty() {
        return STATUS_SUCCESS.to_string();
    }

    // Filter to only events newer than the last-recorded timestamp
    // (mirror of upstream's `filter (\(Event _ ts _ _) -> ts > lastEventTime)`).
    let last_seen = *last_time_ms.lock().await;
    let new_events: Vec<_> = events
        .iter()
        .filter(|e| e.time_ms > last_seen)
        .cloned()
        .collect();
    if new_events.is_empty() {
        return STATUS_SUCCESS.to_string();
    }

    // Build the (NodeId, NodeName) pair list using the resolver.
    let unique_ids: Vec<NodeId> = {
        let mut seen = std::collections::HashSet::new();
        let mut ids = Vec::new();
        for event in &new_events {
            if seen.insert(event.node_id.clone()) {
                ids.push(event.node_id.clone());
            }
        }
        ids
    };
    let id_name_pairs: Vec<(NodeId, NodeName)> = unique_ids
        .iter()
        .map(|id| (id.clone(), node_name_resolver(id)))
        .collect();

    let body = format_notification_body(&new_events, &id_name_pairs);

    // Update last-time-ms to the maximum timestamp among new events
    // (mirror of upstream's `updateLastTime $ maximum tss`).
    let max_ts = new_events
        .iter()
        .map(|e| e.time_ms)
        .max()
        .unwrap_or(last_seen);
    *last_time_ms.lock().await = max_ts;

    create_and_send_email(email_settings, &body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(severity: SeverityS, msg: &str, time_ms: i64) -> Event {
        Event::new(
            NodeId::new("node-spo-1"),
            time_ms,
            severity,
            msg.to_string(),
        )
    }

    #[test]
    fn format_event_timestamp_unix_epoch_renders_canonical_string() {
        // Unix epoch (0 ms) → 1970-01-01 00:00:00 UTC.
        assert_eq!(format_event_timestamp(0), "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn format_event_timestamp_known_value_renders_correctly() {
        // 1700000000000 ms = 2023-11-14 22:13:20 UTC.
        assert_eq!(
            format_event_timestamp(1_700_000_000_000),
            "2023-11-14 22:13:20 UTC",
        );
    }

    #[test]
    fn format_event_timestamp_handles_y2038_threshold() {
        // 2_147_483_648 seconds = 2038-01-19 03:14:08 UTC (one
        // second past i32 max).
        assert_eq!(
            format_event_timestamp(2_147_483_648_000),
            "2038-01-19 03:14:08 UTC",
        );
    }

    #[test]
    fn get_node_name_returns_name_when_registered() {
        let node = NodeId::new("node-7");
        let map = vec![
            (NodeId::new("node-3"), "alpha".to_string()),
            (NodeId::new("node-7"), "beta".to_string()),
        ];
        assert_eq!(get_node_name(&node, &map), "beta");
    }

    #[test]
    fn get_node_name_falls_back_to_node_id_when_unregistered() {
        let node = NodeId::new("node-99");
        let map = vec![(NodeId::new("node-7"), "beta".to_string())];
        assert_eq!(get_node_name(&node, &map), "node-99");
    }

    #[test]
    fn get_node_name_falls_back_when_map_is_empty() {
        let node = NodeId::new("node-x");
        let map: Vec<(NodeId, NodeName)> = Vec::new();
        assert_eq!(get_node_name(&node, &map), "node-x");
    }

    #[test]
    fn format_notification_body_empty_events_returns_empty() {
        assert_eq!(format_notification_body(&[], &[]), "");
    }

    #[test]
    fn format_notification_body_single_event_uses_singular_event_word() {
        let events = vec![sample_event(
            SeverityS::Warning,
            "blockfetch lag",
            1_700_000_000_000,
        )];
        let body = format_notification_body(&events, &[]);
        assert!(body.contains("The following event occurred:"));
        assert!(!body.contains("The following events occurred:"));
        assert!(body.contains("[Warning]"));
        assert!(body.contains("[blockfetch lag]"));
        assert!(body.contains("[2023-11-14 22:13:20 UTC]"));
    }

    #[test]
    fn format_notification_body_multiple_events_uses_plural_events_word() {
        let events = vec![
            sample_event(SeverityS::Error, "fail-1", 1_700_000_000_000),
            sample_event(SeverityS::Critical, "fail-2", 1_700_000_001_000),
        ];
        let body = format_notification_body(&events, &[]);
        assert!(body.contains("The following events occurred:"));
        assert!(!body.contains("The following event occurred:"));
        assert!(body.contains("[Error]"));
        assert!(body.contains("[Critical]"));
        assert!(body.contains("[fail-1]"));
        assert!(body.contains("[fail-2]"));
    }

    #[test]
    fn format_notification_body_uses_node_name_when_available() {
        let events = vec![sample_event(SeverityS::Warning, "msg", 1_700_000_000_000)];
        let map = vec![(NodeId::new("node-spo-1"), "alpha-pool".to_string())];
        let body = format_notification_body(&events, &map);
        assert!(body.contains("[alpha-pool]"));
        assert!(!body.contains("[node-spo-1]"));
    }

    #[test]
    fn format_notification_body_falls_back_to_node_id_when_unregistered() {
        let events = vec![sample_event(SeverityS::Warning, "msg", 1_700_000_000_000)];
        let body = format_notification_body(&events, &[]);
        assert!(body.contains("[node-spo-1]"));
    }

    #[test]
    fn format_notification_body_starts_with_canonical_preface() {
        let events = vec![sample_event(SeverityS::Warning, "msg", 0)];
        let body = format_notification_body(&events, &[]);
        assert!(body.starts_with("This is a notification from Cardano RTView service.\n"));
    }

    #[test]
    fn format_notification_body_no_trailing_newline() {
        let events = vec![sample_event(SeverityS::Warning, "msg", 0)];
        let body = format_notification_body(&events, &[]);
        assert!(!body.ends_with('\n'));
    }

    #[test]
    fn make_and_send_notification_status_describes_closure() {
        let s = make_and_send_notification_status();
        assert_eq!(s.status, "closed at R404");
        assert_eq!(s.closed_at_round, "R404");
    }

    #[tokio::test]
    async fn make_and_send_notification_short_circuits_on_empty_queue() {
        use super::super::types::{EmailSSL, EmailSettings, new_events_queues};
        use std::sync::Arc;

        let queues = new_events_queues();
        let last_time = Arc::new(tokio::sync::Mutex::new(0_i64));
        let settings = EmailSettings {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: "u".to_string(),
            password: "p".to_string(),
            ssl: EmailSSL::Starttls,
            email_from: "from@example.com".to_string(),
            email_to: "to@example.com".to_string(),
            subject: "x".to_string(),
        };
        let result = make_and_send_notification(
            &settings,
            &queues,
            EventGroup::EventErrors,
            &last_time,
            |id| id.as_str().to_string(),
        )
        .await;
        // Empty queue → success short-circuit.
        assert!(result.contains("Yay"));
        // last_time unchanged.
        assert_eq!(*last_time.lock().await, 0);
    }

    #[tokio::test]
    async fn make_and_send_notification_skips_events_older_than_last_seen() {
        use super::super::check::new_events_senders;
        use super::super::types::{EmailSSL, EmailSettings, Timer, new_events_queues};
        use std::sync::Arc;
        use tokio::sync::mpsc;

        let queues = new_events_queues();
        let senders = new_events_senders();
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        senders.write().await.insert(EventGroup::EventErrors, tx);
        queues
            .write()
            .await
            .insert(EventGroup::EventErrors, (rx, Timer::placeholder()));

        // Push an event with timestamp 100, but set last_time to
        // 200. The event should be filtered out → empty new_events
        // → success short-circuit (no SMTP send).
        let _ = senders.read().await.get(&EventGroup::EventErrors).map(|s| {
            s.send(Event::new(
                NodeId::new("node-1"),
                100,
                crate::severity::SeverityS::Error,
                "old".to_string(),
            ))
            .expect("send");
        });
        let last_time = Arc::new(tokio::sync::Mutex::new(200_i64));
        let settings = EmailSettings {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: "u".to_string(),
            password: "p".to_string(),
            ssl: EmailSSL::Starttls,
            email_from: "from@example.com".to_string(),
            email_to: "to@example.com".to_string(),
            subject: "x".to_string(),
        };
        let result = make_and_send_notification(
            &settings,
            &queues,
            EventGroup::EventErrors,
            &last_time,
            |id| id.as_str().to_string(),
        )
        .await;
        // All events filtered → success short-circuit.
        assert!(result.contains("Yay"));
        // last_time unchanged.
        assert_eq!(*last_time.lock().await, 200);
    }

    #[test]
    fn format_severity_returns_variant_name_for_each_kind() {
        assert_eq!(format_severity(SeverityS::Debug), "Debug");
        assert_eq!(format_severity(SeverityS::Info), "Info");
        assert_eq!(format_severity(SeverityS::Notice), "Notice");
        assert_eq!(format_severity(SeverityS::Warning), "Warning");
        assert_eq!(format_severity(SeverityS::Error), "Error");
        assert_eq!(format_severity(SeverityS::Critical), "Critical");
        assert_eq!(format_severity(SeverityS::Alert), "Alert");
        assert_eq!(format_severity(SeverityS::Emergency), "Emergency");
    }
}
