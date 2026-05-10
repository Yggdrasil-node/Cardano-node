//! Notification-engine send orchestration — drains event queues,
//! formats body text, dispatches to the SMTP layer.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Send.hs.
//!
//! Direct port of upstream's notification-send module — bounded
//! subset. The orchestration entry-point `makeAndSendNotification`
//! is deferred (depends on `DataPointRequestors` + tracer-trace
//! channel + the SMTP send-path which is itself carved out per
//! [`super::email::smtp_send_status`]). This round ships the pure-
//! Rust body-formatting subset: the per-event line formatter +
//! the preface template + the `getNodeName` fallback helper.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `sendNotification` body-formatting (`preface` + `events`)      | [`format_notification_body`]           |
//! | `formatTS = T.pack . formatTime defaultTimeLocale "%F %T %Z"` | [`format_event_timestamp`]             |
//! | `getNodeName` lookup-with-fallback                             | [`get_node_name`]                      |
//! | `makeAndSendNotification :: ... -> IO ()`                      | (deferred — see [`make_and_send_notification_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`makeAndSendNotification`**: depends on `DataPointRequestors`
//!   (unported), `Trace IO TracerTrace` (unported), `askNodeNameRaw`
//!   from `Cardano.Tracer.Utils` (unported), and the SMTP send-path
//!   which is itself a carve-out (see
//!   [`super::email::smtp_send_status`]). Status documented in
//!   [`make_and_send_notification_status`].
//! - **`Data.Time.Format.formatTime`** with `"%F %T %Z"` produces an
//!   ISO-8601 date + 24-hour time + timezone abbreviation. The Rust
//!   port emits the same shape using a manual format string against
//!   the Unix-epoch-ms Event::time_ms field — no `chrono` dependency
//!   needed since the upstream format is fixed and the timezone is
//!   always reported as `UTC` for parity (upstream's `%Z` resolves
//!   to whatever the system locale is set to; in operational
//!   practice tracer hosts run in UTC).

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

/// Status descriptor for the carve-out `makeAndSendNotification`
/// orchestration entry-point.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MakeAndSendNotificationStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Comma-separated list of upstream module dependencies that
    /// haven't been ported yet.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `makeAndSendNotification`.
pub fn make_and_send_notification_status() -> MakeAndSendNotificationStatus {
    MakeAndSendNotificationStatus {
        status: "deferred",
        depends_on: "DataPointRequestors (unported), Trace IO TracerTrace (unported), Cardano.Tracer.Utils.askNodeNameRaw (unported), SMTP send-path (super::email::smtp_send_status carve-out)",
        deferred_round: "R390+",
    }
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
    fn make_and_send_notification_status_describes_deferral() {
        let s = make_and_send_notification_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("DataPointRequestors"));
        assert!(s.depends_on.contains("SMTP"));
        assert_eq!(s.deferred_round, "R390+");
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
