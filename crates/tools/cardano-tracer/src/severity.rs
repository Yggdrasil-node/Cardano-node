//! Trace-event severity ladder used by the cardano-tracer notification
//! filter and metrics emitter.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side synthesis of the `Cardano.Logging.SeverityS` type
//! that the upstream `cardano-tracer` imports from the
//! `trace-dispatcher` package. The upstream package is **not**
//! vendored at `.reference-haskell-cardano-node/`, so this file is a
//! carved-out synthesis: its variant set is recovered from the
//! exhaustive pattern matches in upstream's Systemd journal sink
//! (`cardano-tracer/src/Cardano/Tracer/Handlers/Logs/Journal/Systemd.hs::mkPriority`)
//! and the notification dispatch arms in
//! `cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Check.hs`.
//!
//! The 8 variants form the standard syslog severity ladder
//! (RFC 5424 §6.2.1) — the upstream `Data.Aeson.ToJSON` instance
//! emits each variant's name verbatim (Debug / Info / Notice /
//! Warning / Error / Critical / Alert / Emergency).
//!
//! When `trace-dispatcher` is eventually vendored this file should
//! be retired in favour of a strict 1:1 port at the equivalent path.

use serde::{Deserialize, Serialize};

/// Trace-event severity, mirroring the standard syslog ladder.
/// Synthesis stand-in for upstream `Cardano.Logging.SeverityS`.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Default, Serialize, Deserialize,
)]
pub enum SeverityS {
    /// Routine debug output (verbose).
    #[default]
    Debug,
    /// Informational message.
    Info,
    /// Notable condition, no action required.
    Notice,
    /// Warning — operator should investigate.
    Warning,
    /// Error — recoverable failure.
    Error,
    /// Critical condition — service degradation.
    Critical,
    /// Alert — operator action required.
    Alert,
    /// Emergency — system unusable.
    Emergency,
}

impl SeverityS {
    /// Numeric severity (RFC 5424 §6.2.1) — Debug is `7`, Emergency
    /// is `0`. Useful for ordering severity levels low → high.
    pub fn syslog_code(self) -> u8 {
        match self {
            SeverityS::Emergency => 0,
            SeverityS::Alert => 1,
            SeverityS::Critical => 2,
            SeverityS::Error => 3,
            SeverityS::Warning => 4,
            SeverityS::Notice => 5,
            SeverityS::Info => 6,
            SeverityS::Debug => 7,
        }
    }

    /// `true` when this severity is `Warning` or higher (i.e. one of
    /// the levels the cardano-tracer notification engine reacts to
    /// in `Check.hs::checkCommonErrors`).
    pub fn is_notification_worthy(self) -> bool {
        matches!(
            self,
            SeverityS::Warning
                | SeverityS::Error
                | SeverityS::Critical
                | SeverityS::Alert
                | SeverityS::Emergency,
        )
    }

    /// Reverse of [`SeverityS::syslog_code`] — parse a syslog-code
    /// `u8` back into a `SeverityS` variant. Used by R437's
    /// CBOR-decode path to round-trip the severity field.
    /// Returns `None` for codes outside the canonical 0-7 range.
    pub fn from_syslog_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(SeverityS::Emergency),
            1 => Some(SeverityS::Alert),
            2 => Some(SeverityS::Critical),
            3 => Some(SeverityS::Error),
            4 => Some(SeverityS::Warning),
            5 => Some(SeverityS::Notice),
            6 => Some(SeverityS::Info),
            7 => Some(SeverityS::Debug),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_debug() {
        assert_eq!(SeverityS::default(), SeverityS::Debug);
    }

    #[test]
    fn syslog_code_matches_rfc_5424() {
        assert_eq!(SeverityS::Emergency.syslog_code(), 0);
        assert_eq!(SeverityS::Alert.syslog_code(), 1);
        assert_eq!(SeverityS::Critical.syslog_code(), 2);
        assert_eq!(SeverityS::Error.syslog_code(), 3);
        assert_eq!(SeverityS::Warning.syslog_code(), 4);
        assert_eq!(SeverityS::Notice.syslog_code(), 5);
        assert_eq!(SeverityS::Info.syslog_code(), 6);
        assert_eq!(SeverityS::Debug.syslog_code(), 7);
    }

    #[test]
    fn ord_orders_severities_least_to_most_severe() {
        // Derived Ord on the enum follows declaration order: Debug
        // (lowest) → Emergency (highest). This is the opposite of
        // the syslog code (which has Emergency = 0).
        assert!(SeverityS::Debug < SeverityS::Info);
        assert!(SeverityS::Info < SeverityS::Notice);
        assert!(SeverityS::Notice < SeverityS::Warning);
        assert!(SeverityS::Warning < SeverityS::Error);
        assert!(SeverityS::Error < SeverityS::Critical);
        assert!(SeverityS::Critical < SeverityS::Alert);
        assert!(SeverityS::Alert < SeverityS::Emergency);
    }

    #[test]
    fn is_notification_worthy_true_for_warning_and_above() {
        assert!(SeverityS::Warning.is_notification_worthy());
        assert!(SeverityS::Error.is_notification_worthy());
        assert!(SeverityS::Critical.is_notification_worthy());
        assert!(SeverityS::Alert.is_notification_worthy());
        assert!(SeverityS::Emergency.is_notification_worthy());
    }

    #[test]
    fn is_notification_worthy_false_for_info_and_below() {
        assert!(!SeverityS::Debug.is_notification_worthy());
        assert!(!SeverityS::Info.is_notification_worthy());
        assert!(!SeverityS::Notice.is_notification_worthy());
    }

    #[test]
    fn serializes_as_variant_name_string() {
        assert_eq!(
            serde_json::to_value(SeverityS::Warning).expect("serializes"),
            serde_json::json!("Warning"),
        );
        assert_eq!(
            serde_json::to_value(SeverityS::Emergency).expect("serializes"),
            serde_json::json!("Emergency"),
        );
    }

    #[test]
    fn round_trips_through_serde_json() {
        for sev in [
            SeverityS::Debug,
            SeverityS::Info,
            SeverityS::Notice,
            SeverityS::Warning,
            SeverityS::Error,
            SeverityS::Critical,
            SeverityS::Alert,
            SeverityS::Emergency,
        ] {
            let json = serde_json::to_string(&sev).expect("serializes");
            let back: SeverityS = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, sev);
        }
    }
}
