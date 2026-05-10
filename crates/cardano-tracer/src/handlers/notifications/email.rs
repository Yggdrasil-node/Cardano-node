//! Email-notification helpers — status-message type + watchdog
//! timeout wrapper + body templating.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Email.hs.
//!
//! Direct port of upstream's notification-engine email module —
//! bounded subset. The actual SMTP-send paths (`createAndSendEmail`
//! / `createAndSendTestEmail` / `sendEmail`) are carved out pending
//! `lettre` (or equivalent SMTP client) workspace dependency
//! approval per `docs/DEPENDENCIES.md`. This round ships the
//! pure-Rust bounded subset that doesn't require an SMTP transport:
//!
//! Mapping summary:
//!
//! | Upstream                                                 | Yggdrasil                              |
//! |----------------------------------------------------------|----------------------------------------|
//! | `type StatusMessage = Text`                              | [`StatusMessage`]                      |
//! | `statusIsOK :: StatusMessage -> Bool`                    | [`status_is_ok`]                       |
//! | `runIOWithWatchdog :: Double -> a -> IO a -> IO a`       | [`run_io_with_watchdog`]               |
//! | `createAndSendTestEmail` body template                   | [`test_notification_body`]             |
//! | `createAndSendEmail` (SMTP send)                         | (carve-out — see [`SmtpSendStatus`])   |
//! | `createAndSendEmail` (SMTP send)                         | (carve-out — see [`SmtpSendStatus`])   |
//! | `sendEmail` SSL dispatch                                 | (carve-out — see [`SmtpSendStatus`])   |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Network.Mail.SMTP` + `Network.Mail.Mime`**: upstream uses
//!   the Haskell `mail` + `smtp-mail` packages for SMTP transport.
//!   The Rust equivalent is `lettre` (~30 transitive deps, MIT,
//!   pure Rust). Adding it requires `docs/DEPENDENCIES.md`
//!   justification per the workspace policy. Until then, the SMTP
//!   send-path is exposed as [`SmtpSendStatus`] — a status
//!   descriptor sites can reference programmatically. Once `lettre`
//!   lands, the actual `createAndSendEmail` + `sendEmail` functions
//!   will be added without changing the rest of this module's
//!   surface (StatusMessage / status_is_ok / run_io_with_watchdog).
//! - **`getAddrInfo` / `user error` Haskell-specific error string
//!   matching**: upstream's `explanation` helper greps the show'd
//!   exception for substrings. The Rust port will surface SMTP
//!   error categories more cleanly via `lettre::error::Error`
//!   variants when the SMTP client is added.

use std::time::Duration;

use tokio::time::timeout;

/// Free-form status message returned by send-attempts. Mirror of
/// upstream `type StatusMessage = Text`. The leading `✓ Yay!` /
/// `✗ Unable to send:` sentinel is preserved verbatim from
/// upstream so [`status_is_ok`] can grep for the same substring.
pub type StatusMessage = String;

/// Conventional success-message constant matching upstream's
/// exact "Yay!"-bearing string. Used by [`status_is_ok`] +
/// downstream sites that want to construct success statuses.
pub const STATUS_SUCCESS: &str = "✓ Yay! Notification is sent.";

/// Conventional timeout-message constant matching upstream's
/// `runIOWithWatchdog ... "✗ Unable to send: timeout"` invocation.
pub const STATUS_TIMEOUT: &str = "✗ Unable to send: timeout";

/// Test-email body template. Mirror of upstream's
/// `body = "This is a test notification from Cardano RTView. ..."`
/// inline string.
pub fn test_notification_body() -> &'static str {
    "This is a test notification from Cardano RTView. Congrats: your email settings are correct!"
}

/// `True` when a status message indicates success. Mirror of
/// upstream `statusIsOK msg = "Yay" \`T.isInfixOf\` msg`.
pub fn status_is_ok(msg: &str) -> bool {
    msg.contains("Yay")
}

/// Run `action` with a timeout. If it doesn't finish within
/// `timeout_secs`, return `timeout_value` instead. Mirror of
/// upstream
/// `runIOWithWatchdog :: Double -> a -> IO a -> IO a`
/// implemented via `Control.Concurrent.Async.race`.
///
/// The `timeout_value` is the value upstream returns when the
/// race is won by the sleep arm — typically a "✗ Unable to send:
/// timeout" StatusMessage.
pub async fn run_io_with_watchdog<F, T>(timeout_secs: f64, timeout_value: T, action: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let dur = Duration::from_secs_f64(timeout_secs);
    timeout(dur, action).await.unwrap_or(timeout_value)
}

/// Status descriptor for the carve-out SMTP send-path. Sites that
/// want to surface the deferral programmatically can call
/// [`smtp_send_status`] and reference the returned struct rather
/// than duplicating the rationale string.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SmtpSendStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing workspace dependency.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the SMTP send-path.
pub fn smtp_send_status() -> SmtpSendStatus {
    SmtpSendStatus {
        status: "deferred",
        depends_on: "lettre crate (or equivalent SMTP client) — pending docs/DEPENDENCIES.md justification",
        deferred_round: "R389+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_success_constant_contains_yay() {
        assert!(STATUS_SUCCESS.contains("Yay"));
    }

    #[test]
    fn status_timeout_constant_does_not_contain_yay() {
        assert!(!STATUS_TIMEOUT.contains("Yay"));
    }

    #[test]
    fn status_is_ok_true_for_success_message() {
        assert!(status_is_ok(STATUS_SUCCESS));
    }

    #[test]
    fn status_is_ok_false_for_timeout_message() {
        assert!(!status_is_ok(STATUS_TIMEOUT));
    }

    #[test]
    fn status_is_ok_false_for_error_message() {
        assert!(!status_is_ok("✗ Unable to send: check SMTP host"));
    }

    #[test]
    fn status_is_ok_substring_match_is_case_sensitive() {
        // Upstream uses T.isInfixOf which is case-sensitive — so a
        // lowercase "yay" should NOT match.
        assert!(!status_is_ok("yay everything is fine"));
    }

    #[test]
    fn test_notification_body_matches_upstream() {
        assert_eq!(
            test_notification_body(),
            "This is a test notification from Cardano RTView. Congrats: your email settings are correct!",
        );
    }

    #[tokio::test]
    async fn run_io_with_watchdog_returns_action_result_when_fast() {
        // Action completes immediately; should pass through.
        let result: i32 = run_io_with_watchdog(10.0, -1, async { 42 }).await;
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn run_io_with_watchdog_returns_timeout_value_when_slow() {
        // Action sleeps longer than the timeout — should return
        // timeout_value. Use a tiny real timeout so the test runs
        // in ~100ms.
        let result: &str = run_io_with_watchdog(0.1, "timed out", async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "completed"
        })
        .await;
        assert_eq!(result, "timed out");
    }

    #[tokio::test]
    async fn run_io_with_watchdog_with_timeout_status_string() {
        // Verify the canonical timeout-status string passes through.
        let result: String = run_io_with_watchdog(0.05, STATUS_TIMEOUT.to_string(), async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            STATUS_SUCCESS.to_string()
        })
        .await;
        assert_eq!(result, STATUS_TIMEOUT);
        assert!(!status_is_ok(&result));
    }

    #[test]
    fn smtp_send_status_describes_deferral() {
        let s = smtp_send_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("lettre"));
        assert_eq!(s.deferred_round, "R389+");
    }
}
