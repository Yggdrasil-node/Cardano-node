//! Email-notification helpers — status-message type + watchdog
//! timeout wrapper + body templating.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Email.hs.
//!
//! Direct port of upstream's notification-engine email module. The
//! full surface ships as of R403 — the previously-carved-out SMTP
//! send paths (`createAndSendEmail` / `createAndSendTestEmail` /
//! `sendEmail`) are now wired through the `lettre` 0.11 crate per
//! the R398 plan's D1 audit (rustls-only feature pin; transitive
//! tree clean of `native-tls` / `openssl` / `openssl-sys` per
//! `deny.toml`).
//!
//! Mapping summary:
//!
//! | Upstream                                                 | Yggdrasil                              |
//! |----------------------------------------------------------|----------------------------------------|
//! | `type StatusMessage = Text`                              | [`StatusMessage`]                      |
//! | `statusIsOK :: StatusMessage -> Bool`                    | [`status_is_ok`]                       |
//! | `runIOWithWatchdog :: Double -> a -> IO a -> IO a`       | [`run_io_with_watchdog`]               |
//! | `createAndSendTestEmail` body template                   | [`test_notification_body`]             |
//! | `createAndSendEmail :: EmailSettings -> Text -> IO StatusMessage` | [`create_and_send_email`]   |
//! | `createAndSendTestEmail :: EmailSettings -> IO StatusMessage` | [`create_and_send_test_email`] |
//! | `sendEmail` SSL dispatch (TLS / STARTTLS / NoSSL)        | [`send_email`] (private helper)        |
//! | `explanation` error-string helper                        | [`explain_smtp_error`] (private)       |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Data.Text.Lazy.Builder` Mail body**: upstream builds the
//!   `Mail` value via `simpleMail'`. Yggdrasil's port uses lettre's
//!   `Message::builder()` API which produces a strict-text
//!   equivalent.
//! - **`getAddrInfo` / `user error` Haskell-specific error string
//!   matching**: upstream's `explanation` helper greps the show'd
//!   exception for substrings. The Rust port preserves the same
//!   replacement strings ("check SMTP host" / "check your name,
//!   password or SSL") but operates on the `lettre::Error` Display
//!   string instead of a Haskell `SomeException`.

use std::time::Duration;

use tokio::time::timeout;

use super::types::{EmailSSL, EmailSettings};

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

/// Status descriptor for the previously-carved-out SMTP send-path.
/// Closed at R403 with the lettre 0.11 dep land. Kept around so
/// call sites that previously queried for the status can see the
/// closure round.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SmtpSendStatus {
    /// One-line summary of the closure status.
    pub status: &'static str,
    /// Round at which the SMTP send-path landed.
    pub closed_at_round: &'static str,
}

/// Get the closure-status descriptor for the SMTP send-path. R403
/// closes the carve-out: the actual send functions are
/// [`create_and_send_email`] / [`create_and_send_test_email`] /
/// [`send_email`].
pub fn smtp_send_status() -> SmtpSendStatus {
    SmtpSendStatus {
        status: "closed at R403",
        closed_at_round: "R403",
    }
}

/// Send an email to the configured recipient with the given body.
/// Mirror of upstream
/// `createAndSendEmail :: EmailSettings -> Text -> IO StatusMessage`.
///
/// Wraps the underlying [`send_email`] call in
/// [`run_io_with_watchdog`] (10-second timeout matching upstream's
/// `runIOWithWatchdog 10.0 ...` invocation). Returns
/// [`STATUS_SUCCESS`] on success, an error message on failure, or
/// [`STATUS_TIMEOUT`] if the send didn't complete within 10s.
pub async fn create_and_send_email(settings: &EmailSettings, body_message: &str) -> StatusMessage {
    use lettre::Message;
    let from = format!("Cardano RTView <{}>", settings.email_from);
    let parsed_to = match settings.email_to.parse() {
        Ok(addr) => addr,
        Err(e) => return format!("✗ Unable to send: {e}"),
    };
    let parsed_from = match from.parse() {
        Ok(addr) => addr,
        Err(e) => return format!("✗ Unable to send: {e}"),
    };
    let mail = match Message::builder()
        .to(parsed_to)
        .from(parsed_from)
        .subject(&settings.subject)
        .body(body_message.to_string())
    {
        Ok(m) => m,
        Err(e) => return format!("✗ Unable to send: {e}"),
    };
    run_io_with_watchdog(10.0, STATUS_TIMEOUT.to_string(), send_email(settings, mail)).await
}

/// Send the canonical test notification. Mirror of upstream
/// `createAndSendTestEmail`. Convenience wrapper around
/// [`create_and_send_email`] with [`test_notification_body`].
pub async fn create_and_send_test_email(settings: &EmailSettings) -> StatusMessage {
    create_and_send_email(settings, test_notification_body()).await
}

/// Send a pre-built `lettre::Message` via the configured transport.
/// Mirror of upstream `sendEmail`. Dispatches on
/// [`EmailSettings::ssl`] to pick the matching SMTP transport
/// constructor (TLS / STARTTLS / NoSSL).
async fn send_email(settings: &EmailSettings, mail: lettre::Message) -> StatusMessage {
    use lettre::transport::smtp::AsyncSmtpTransport;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncTransport, Tokio1Executor};

    let creds = Credentials::new(settings.username.clone(), settings.password.clone());
    let transport_result: Result<
        AsyncSmtpTransport<Tokio1Executor>,
        lettre::transport::smtp::Error,
    > = match settings.ssl {
        EmailSSL::Tls => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host).map(|builder| {
                builder
                    .credentials(creds)
                    .port(settings.smtp_port as u16)
                    .build()
            })
        }
        EmailSSL::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(
            &settings.smtp_host,
        )
        .map(|builder| {
            builder
                .credentials(creds)
                .port(settings.smtp_port as u16)
                .build()
        }),
        EmailSSL::NoSSL => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &settings.smtp_host,
        )
        .credentials(creds)
        .port(settings.smtp_port as u16)
        .build()),
    };
    let transport = match transport_result {
        Ok(t) => t,
        Err(e) => return format!("✗ Unable to send: {}", explain_smtp_error(&e.to_string())),
    };
    match transport.send(mail).await {
        Ok(_) => STATUS_SUCCESS.to_string(),
        Err(e) => format!("✗ Unable to send: {}", explain_smtp_error(&e.to_string())),
    }
}

/// Mirror of upstream's `explanation` helper: greps the show'd
/// exception for known patterns and returns a friendlier message.
/// Yggdrasil's port operates on the lettre error string but
/// preserves upstream's exact replacement text for byte-equivalent
/// status output.
fn explain_smtp_error(msg: &str) -> String {
    if msg.contains("getAddrInfo") || msg.contains("dns") || msg.contains("DNS") {
        "check SMTP host".to_string()
    } else if msg.contains("user error")
        || msg.contains("authentication")
        || msg.contains("unauthorized")
    {
        "check your name, password or SSL".to_string()
    } else {
        msg.to_string()
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
    fn smtp_send_status_describes_closure() {
        let s = smtp_send_status();
        assert_eq!(s.status, "closed at R403");
        assert_eq!(s.closed_at_round, "R403");
    }

    #[test]
    fn create_and_send_email_returns_send_failure_for_invalid_host() {
        // Use a synthetic settings block pointing at an unreachable
        // host so the send fails fast. The dispatch should produce
        // a "✗ Unable to send" prefix without panicking.
        let settings = EmailSettings {
            smtp_host: "nonexistent.invalid.test.example".to_string(),
            smtp_port: 587,
            username: "user".to_string(),
            password: "pass".to_string(),
            ssl: EmailSSL::Starttls,
            email_from: "from@example.com".to_string(),
            email_to: "to@example.com".to_string(),
            subject: "test".to_string(),
        };
        // run with extremely short timeout to avoid hanging on DNS.
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt")
            .block_on(async {
                run_io_with_watchdog(
                    0.5,
                    STATUS_TIMEOUT.to_string(),
                    create_and_send_email(&settings, "body"),
                )
                .await
            });
        // Either timeout or unable-to-send; both indicate the send
        // path executed without panicking.
        assert!(!status_is_ok(&result));
        assert!(result.starts_with('✗'));
    }
}
