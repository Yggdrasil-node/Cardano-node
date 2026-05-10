//! Notification-engine record types — email + per-event-group
//! dispatch.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Types.hs.
//!
//! Direct port of the data-record + JSON-instance surface used by
//! upstream's notification engine.
//!
//! Mapping summary:
//!
//! | Upstream                                            | Yggdrasil                              |
//! |-----------------------------------------------------|----------------------------------------|
//! | `data EmailSSL = TLS \| STARTTLS \| NoSSL`           | [`EmailSSL`]                           |
//! | `data EmailSettings = EmailSettings { ... }`        | [`EmailSettings`] (8-field struct)     |
//! | `data EventsSettings = EventsSettings { ... }`      | [`EventsSettings`] (6-field struct)    |
//! | `data Event = Event { ... }`                        | [`Event`] (4-field struct)             |
//! | `data EventGroup = ...`                             | [`EventGroup`] (6-variant enum)        |
//! | `type EventsQueue = TBQueue Event`                  | [`EventsQueue`] (alias around `tokio::sync::mpsc::UnboundedReceiver`) |
//! | `type EventsQueues = TVar (Map EventGroup (EventsQueue, Timer))` | [`EventsQueues`] (alias around `Arc<RwLock<BTreeMap<EventGroup, (EventsQueue, Timer)>>>`) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Cardano.Logging.SeverityS`**: upstream package not vendored
//!   — synthesized in [`crate::severity::SeverityS`] from upstream's
//!   exhaustive pattern matches in `Journal/Systemd.hs` and
//!   `Notifications/Check.hs`.
//! - **`Control.Concurrent.STM.TBQueue`**: replaced with
//!   `tokio::sync::mpsc::UnboundedReceiver<Event>` — the bounded
//!   queue is satisfied at the producer-side via a coordinated
//!   `UnboundedSender::send` which never blocks. Yggdrasil's
//!   tokio-driven runtime makes this the natural pattern; if a
//!   future round needs strict bounded-queue semantics, a
//!   `tokio::sync::mpsc::Receiver<Event>` (bounded) is a one-line
//!   swap.
//! - **`Control.Concurrent.STM.TVar`**: replaced with
//!   `Arc<RwLock<...>>` — same pattern already established by
//!   [`crate::types::ConnectedNodes`] (R371).
//! - **`Cardano.Tracer.Handlers.Notifications.Timer.Timer`**: full
//!   Timer port lands in a future round (Timer.hs has 112 lines
//!   wrapping forkIO/killThread). Until then [`Timer`] is a
//!   placeholder unit struct that documents the deferred fields in
//!   its doc-comment — every concrete Timer-using site in
//!   downstream rounds will swap to the real Timer when it lands.
//! - **`PeriodInSec`**: upstream's `type PeriodInSec = Word32` lives
//!   in `Notifications/Timer.hs`. Yggdrasil promotes the alias to
//!   this types-tier file (no functional change; mirrors upstream's
//!   re-export pattern).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};

use crate::severity::SeverityS;
use crate::types::NodeId;

/// Per-event reaction throttle, in seconds. Mirror of upstream
/// `type PeriodInSec = Word32` from
/// `Cardano.Tracer.Handlers.Notifications.Timer`.
pub type PeriodInSec = u32;

/// Email-server transport-security mode. Mirror of upstream
/// `data EmailSSL = TLS | STARTTLS | NoSSL`. JSON-encoded as the
/// variant name verbatim (matching upstream's `deriving FromJSON`
/// generic encoding).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum EmailSSL {
    /// TLS handshake on connect.
    Tls,
    /// STARTTLS upgrade after plaintext greeting.
    Starttls,
    /// Plaintext connection — discouraged but supported.
    #[default]
    NoSSL,
}

/// Email-server connection + envelope settings. Mirror of upstream
/// `data EmailSettings { esSMTPHost, esSMTPPort, esUsername,
/// esPassword, esSSL, esEmailFrom, esEmailTo, esSubject }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct EmailSettings {
    /// SMTP server host. Upstream: `esSMTPHost :: Text`.
    #[serde(rename = "esSMTPHost")]
    pub smtp_host: String,
    /// SMTP server port. Upstream: `esSMTPPort :: Int`.
    #[serde(rename = "esSMTPPort")]
    pub smtp_port: i32,
    /// SMTP username. Upstream: `esUsername :: Text`.
    #[serde(rename = "esUsername")]
    pub username: String,
    /// SMTP password. Upstream: `esPassword :: Text`.
    #[serde(rename = "esPassword")]
    pub password: String,
    /// Transport-security mode. Upstream: `esSSL :: EmailSSL`.
    #[serde(rename = "esSSL")]
    pub ssl: EmailSSL,
    /// From-address envelope. Upstream: `esEmailFrom :: Text`.
    #[serde(rename = "esEmailFrom")]
    pub email_from: String,
    /// To-address envelope. Upstream: `esEmailTo :: Text`.
    #[serde(rename = "esEmailTo")]
    pub email_to: String,
    /// Subject template. Upstream: `esSubject :: Text`.
    #[serde(rename = "esSubject")]
    pub subject: String,
}

/// Per-event-group enable flag + period-in-seconds. Mirror of
/// upstream `data EventsSettings { evsWarnings, evsErrors,
/// evsCriticals, evsAlerts, evsEmergencies, evsNodeDisconnected }`,
/// where each field is a `(Bool, PeriodInSec)` pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct EventsSettings {
    /// Enable + period for `Warning`-severity events.
    #[serde(rename = "evsWarnings")]
    pub warnings: (bool, PeriodInSec),
    /// Enable + period for `Error`-severity events.
    #[serde(rename = "evsErrors")]
    pub errors: (bool, PeriodInSec),
    /// Enable + period for `Critical`-severity events.
    #[serde(rename = "evsCriticals")]
    pub criticals: (bool, PeriodInSec),
    /// Enable + period for `Alert`-severity events.
    #[serde(rename = "evsAlerts")]
    pub alerts: (bool, PeriodInSec),
    /// Enable + period for `Emergency`-severity events.
    #[serde(rename = "evsEmergencies")]
    pub emergencies: (bool, PeriodInSec),
    /// Enable + period for `NodeDisconnected` events.
    #[serde(rename = "evsNodeDisconnected")]
    pub node_disconnected: (bool, PeriodInSec),
}

/// One trace-event the notification engine has been asked to react
/// to. Mirror of upstream
/// `data Event { evNodeId, evTime, evSeverity, evMessage }`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Event {
    /// Node identifier the event originated from.
    pub node_id: NodeId,
    /// Event timestamp — Unix epoch milliseconds (upstream uses
    /// `UTCTime`; Yggdrasil's value-side carries the same precision
    /// as the [`crate::time::get_time_ms`] wall-clock helper).
    pub time_ms: i64,
    /// Severity level.
    pub severity: SeverityS,
    /// Free-form message text.
    pub message: String,
}

impl Event {
    /// Construct a new `Event` at the given wall-clock time.
    pub fn new(node_id: NodeId, time_ms: i64, severity: SeverityS, message: String) -> Self {
        Event {
            node_id,
            time_ms,
            severity,
            message,
        }
    }
}

/// Event-group dispatch tag. Mirror of upstream
/// `data EventGroup = EventWarnings | EventErrors | EventCriticals |
/// EventAlerts | EventEmergencies | EventNodeDisconnected`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum EventGroup {
    /// `Warning`-severity events.
    EventWarnings,
    /// `Error`-severity events.
    EventErrors,
    /// `Critical`-severity events.
    EventCriticals,
    /// `Alert`-severity events.
    EventAlerts,
    /// `Emergency`-severity events.
    EventEmergencies,
    /// Node-disconnected events.
    EventNodeDisconnected,
}

impl EventGroup {
    /// Map a [`SeverityS`] to its corresponding [`EventGroup`] (used
    /// by Check.hs::checkCommonErrors when porting in a follow-on
    /// round). Returns `None` for severities that don't trigger
    /// notifications (`Debug` / `Info` / `Notice`).
    pub fn from_severity(severity: SeverityS) -> Option<Self> {
        match severity {
            SeverityS::Warning => Some(EventGroup::EventWarnings),
            SeverityS::Error => Some(EventGroup::EventErrors),
            SeverityS::Critical => Some(EventGroup::EventCriticals),
            SeverityS::Alert => Some(EventGroup::EventAlerts),
            SeverityS::Emergency => Some(EventGroup::EventEmergencies),
            SeverityS::Debug | SeverityS::Info | SeverityS::Notice => None,
        }
    }
}

/// Per-event-group event queue. Mirror of upstream
/// `type EventsQueue = TBQueue Event` — a multi-producer / single-
/// consumer queue. Yggdrasil uses
/// `tokio::sync::mpsc::UnboundedReceiver<Event>` per the carve-out
/// in the module docstring.
pub type EventsQueue = mpsc::UnboundedReceiver<Event>;

/// Placeholder for the notification timer struct. The full Timer
/// surface (forkIO + killThread closures) lands in a future round
/// when `Notifications/Timer.hs` (112 lines) is ported.
///
/// Until then this is a unit-shaped struct so downstream sites can
/// thread `Timer` through the type system without a concrete
/// implementation.
#[derive(Clone, Debug, Default)]
pub struct Timer;

/// Map of [`EventGroup`] → ([`EventsQueue`], [`Timer`]) shared across
/// the notification engine. Mirror of upstream
/// `type EventsQueues = TVar (Map EventGroup (EventsQueue, Timer))`.
/// Yggdrasil uses `Arc<RwLock<BTreeMap<...>>>` per the carve-out.
pub type EventsQueues = Arc<RwLock<BTreeMap<EventGroup, (EventsQueue, Timer)>>>;

/// Convenience constructor for an empty [`EventsQueues`].
pub fn new_events_queues() -> EventsQueues {
    Arc::new(RwLock::new(BTreeMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_email_settings() -> EmailSettings {
        EmailSettings {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: "alerts@example.com".to_string(),
            password: "hunter2".to_string(),
            ssl: EmailSSL::Starttls,
            email_from: "Cardano Tracer <alerts@example.com>".to_string(),
            email_to: "operator@example.com".to_string(),
            subject: "Cardano node alert".to_string(),
        }
    }

    #[test]
    fn email_ssl_default_is_no_ssl() {
        assert_eq!(EmailSSL::default(), EmailSSL::NoSSL);
    }

    #[test]
    fn email_ssl_serializes_as_variant_name() {
        assert_eq!(
            serde_json::to_value(EmailSSL::Tls).expect("serializes"),
            serde_json::json!("Tls"),
        );
        assert_eq!(
            serde_json::to_value(EmailSSL::Starttls).expect("serializes"),
            serde_json::json!("Starttls"),
        );
        assert_eq!(
            serde_json::to_value(EmailSSL::NoSSL).expect("serializes"),
            serde_json::json!("NoSSL"),
        );
    }

    #[test]
    fn email_settings_round_trip_through_json() {
        let original = sample_email_settings();
        let json = serde_json::to_string(&original).expect("serializes");
        let back: EmailSettings = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, original);
    }

    #[test]
    fn email_settings_uses_upstream_field_names() {
        let json = serde_json::to_value(sample_email_settings()).expect("serializes");
        // All keys carry upstream's "es"-prefix camelCase form.
        assert!(json.get("esSMTPHost").is_some());
        assert!(json.get("esSMTPPort").is_some());
        assert!(json.get("esUsername").is_some());
        assert!(json.get("esPassword").is_some());
        assert!(json.get("esSSL").is_some());
        assert!(json.get("esEmailFrom").is_some());
        assert!(json.get("esEmailTo").is_some());
        assert!(json.get("esSubject").is_some());
    }

    #[test]
    fn events_settings_default_zeroes_all_pairs() {
        let s = EventsSettings::default();
        assert_eq!(s.warnings, (false, 0));
        assert_eq!(s.errors, (false, 0));
        assert_eq!(s.criticals, (false, 0));
        assert_eq!(s.alerts, (false, 0));
        assert_eq!(s.emergencies, (false, 0));
        assert_eq!(s.node_disconnected, (false, 0));
    }

    #[test]
    fn events_settings_uses_upstream_field_names() {
        let s = EventsSettings {
            warnings: (true, 30),
            errors: (true, 15),
            criticals: (true, 5),
            alerts: (false, 1),
            emergencies: (false, 1),
            node_disconnected: (true, 60),
        };
        let json = serde_json::to_value(s).expect("serializes");
        assert!(json.get("evsWarnings").is_some());
        assert!(json.get("evsErrors").is_some());
        assert!(json.get("evsCriticals").is_some());
        assert!(json.get("evsAlerts").is_some());
        assert!(json.get("evsEmergencies").is_some());
        assert!(json.get("evsNodeDisconnected").is_some());
        assert_eq!(json["evsWarnings"], serde_json::json!([true, 30]));
    }

    #[test]
    fn event_constructor_round_trips() {
        let node = NodeId::new("node-spo-7");
        let event = Event::new(
            node.clone(),
            1_700_000_000_123,
            SeverityS::Error,
            "BlockFetch peer disconnected".to_string(),
        );
        assert_eq!(event.node_id, node);
        assert_eq!(event.time_ms, 1_700_000_000_123);
        assert_eq!(event.severity, SeverityS::Error);
        assert!(event.message.contains("BlockFetch"));
    }

    #[test]
    fn event_group_from_severity_dispatches_to_correct_group() {
        assert_eq!(
            EventGroup::from_severity(SeverityS::Warning),
            Some(EventGroup::EventWarnings),
        );
        assert_eq!(
            EventGroup::from_severity(SeverityS::Error),
            Some(EventGroup::EventErrors),
        );
        assert_eq!(
            EventGroup::from_severity(SeverityS::Critical),
            Some(EventGroup::EventCriticals),
        );
        assert_eq!(
            EventGroup::from_severity(SeverityS::Alert),
            Some(EventGroup::EventAlerts),
        );
        assert_eq!(
            EventGroup::from_severity(SeverityS::Emergency),
            Some(EventGroup::EventEmergencies),
        );
    }

    #[test]
    fn event_group_from_severity_returns_none_for_low_severities() {
        assert!(EventGroup::from_severity(SeverityS::Debug).is_none());
        assert!(EventGroup::from_severity(SeverityS::Info).is_none());
        assert!(EventGroup::from_severity(SeverityS::Notice).is_none());
    }

    #[test]
    fn event_group_ordering_matches_declaration_order() {
        // Ord derived from declaration order.
        assert!(EventGroup::EventWarnings < EventGroup::EventErrors);
        assert!(EventGroup::EventErrors < EventGroup::EventCriticals);
        assert!(EventGroup::EventCriticals < EventGroup::EventAlerts);
        assert!(EventGroup::EventAlerts < EventGroup::EventEmergencies);
        assert!(EventGroup::EventEmergencies < EventGroup::EventNodeDisconnected);
    }

    #[tokio::test]
    async fn new_events_queues_starts_empty() {
        let queues = new_events_queues();
        let guard = queues.read().await;
        assert!(guard.is_empty());
    }

    #[tokio::test]
    async fn events_queues_can_register_a_group() {
        let queues = new_events_queues();
        let (_tx, rx) = mpsc::unbounded_channel::<Event>();
        let timer = Timer;
        {
            let mut guard = queues.write().await;
            guard.insert(EventGroup::EventErrors, (rx, timer));
        }
        let guard = queues.read().await;
        assert_eq!(guard.len(), 1);
        assert!(guard.contains_key(&EventGroup::EventErrors));
    }
}
