//! Persistence layer for the notification engine — save + load
//! email + per-event-group settings to disk.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Notifications/Settings.hs.
//!
//! Direct port of upstream's settings-persistence module. Each
//! helper takes the operator-supplied state-dir directly via
//! [`crate::handlers::system::get_paths_to_notifications_settings`]
//! (which itself takes `Option<&Path>` instead of `&TracerEnv` per
//! the R383 carve-out).
//!
//! Mapping summary:
//!
//! | Upstream                                                                          | Yggdrasil                              |
//! |-----------------------------------------------------------------------------------|----------------------------------------|
//! | `readSavedEmailSettings :: Maybe FilePath -> IO EmailSettings`                    | [`read_saved_email_settings`]          |
//! | `readSavedEventsSettings :: Maybe FilePath -> IO EventsSettings`                  | [`read_saved_events_settings`]         |
//! | `saveEmailSettingsOnDisk :: TracerEnv -> EmailSettings -> IO ()`                  | [`save_email_settings_on_disk`]        |
//! | `saveEventsSettingsOnDisk :: TracerEnv -> EventsSettings -> IO ()`                | [`save_events_settings_on_disk`]       |
//! | `incompleteEmailSettings :: EmailSettings -> Bool`                                | [`incomplete_email_settings`]          |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`TracerEnv`-record-arg**: replaced with `Option<&Path>` per
//!   R383 (full TracerEnv 14-field record port deferred until
//!   Cardano.Logging + Timeseries vendoring).
//! - **`Control.Exception.Extra.try_` + `ignore`**: upstream wraps
//!   `BS.readFile` and `LBS.writeFile` in `try_` (returns `Either
//!   SomeException a`) so any IO failure silently falls back to
//!   defaults / silently no-ops. Rust port mirrors this with
//!   `Result::ok()` (read path) and `let _ = ...` (write path) —
//!   matching upstream's exact behavior of treating a missing /
//!   unwritable settings file as "use defaults" without surfacing
//!   the error.
//! - **Encryption commented out in upstream**: lines 33-39 + 52-53 +
//!   58-69 + 95-98 of upstream's Settings.hs are commented-out
//!   placeholders for an AES256 encryption layer (notes: "Encrypt
//!   JSON-content to avoid saving user's private data in 'plain
//!   mode'"). Yggdrasil mirrors upstream's actual behavior — plain
//!   JSON. If a future round adds encryption, it lands as a separate
//!   port + carve-out close.

use std::path::{Path, PathBuf};

use crate::handlers::system::get_paths_to_notifications_settings;

use super::types::{EmailSSL, EmailSettings, EventsSettings, PeriodInSec};

/// Default per-event-group `(enabled, period_in_sec)` pair upstream
/// uses when no settings file exists yet. Mirror of upstream
/// `defaultState = (False, 1800)`.
pub fn default_events_state() -> (bool, PeriodInSec) {
    (false, 1800)
}

/// Default [`EventsSettings`] mirroring upstream's
/// `defaultSettings` block in `readSavedEventsSettings`.
pub fn default_events_settings() -> EventsSettings {
    let s = default_events_state();
    EventsSettings {
        warnings: s,
        errors: s,
        criticals: s,
        alerts: s,
        emergencies: s,
        node_disconnected: s,
    }
}

/// Default [`EmailSettings`] mirroring upstream's `defaultSettings`
/// block in `readSavedEmailSettings`. Note `smtp_port = -1` is the
/// upstream sentinel for "not configured yet" — paired with
/// [`incomplete_email_settings`] which uses `smtp_host.is_empty()`
/// for the same purpose.
pub fn default_email_settings() -> EmailSettings {
    EmailSettings {
        smtp_host: String::new(),
        smtp_port: -1,
        username: String::new(),
        password: String::new(),
        ssl: EmailSSL::Tls,
        email_from: String::new(),
        email_to: String::new(),
        subject: String::new(),
    }
}

/// `True` when no SMTP host has been configured. Mirror of upstream
/// `incompleteEmailSettings emailSettings = T.null $ esSMTPHost emailSettings`.
pub fn incomplete_email_settings(settings: &EmailSettings) -> bool {
    settings.smtp_host.is_empty()
}

/// Read saved email settings from disk, falling back to
/// [`default_email_settings`] on any IO or parse error. Mirror of
/// upstream `readSavedEmailSettings`.
pub fn read_saved_email_settings(state_dir: Option<&Path>) -> EmailSettings {
    let Ok((email_path, _)) = get_paths_to_notifications_settings(state_dir) else {
        return default_email_settings();
    };
    read_json_or_default(&email_path, default_email_settings)
}

/// Read saved events settings from disk, falling back to
/// [`default_events_settings`] on any IO or parse error. Mirror of
/// upstream `readSavedEventsSettings`.
pub fn read_saved_events_settings(state_dir: Option<&Path>) -> EventsSettings {
    let Ok((_, events_path)) = get_paths_to_notifications_settings(state_dir) else {
        return default_events_settings();
    };
    read_json_or_default(&events_path, default_events_settings)
}

fn read_json_or_default<T, F>(path: &Path, fallback: F) -> T
where
    T: serde::de::DeserializeOwned,
    F: FnOnce() -> T,
{
    let Ok(bytes) = std::fs::read(path) else {
        return fallback();
    };
    serde_json::from_slice::<T>(&bytes).unwrap_or_else(|_| fallback())
}

/// Save email settings to disk. Mirror of upstream
/// `saveEmailSettingsOnDisk` (with TracerEnv-arg → Option<&Path>
/// per R383). IO errors are silently ignored mirroring upstream's
/// `ignore do ...` wrapper.
pub fn save_email_settings_on_disk(state_dir: Option<&Path>, settings: &EmailSettings) {
    let Ok((email_path, _)) = get_paths_to_notifications_settings(state_dir) else {
        return;
    };
    let _ = save_json(&email_path, settings);
}

/// Save events settings to disk. Mirror of upstream
/// `saveEventsSettingsOnDisk`. IO errors are silently ignored.
pub fn save_events_settings_on_disk(state_dir: Option<&Path>, settings: &EventsSettings) {
    let Ok((_, events_path)) = get_paths_to_notifications_settings(state_dir) else {
        return;
    };
    let _ = save_json(&events_path, settings);
}

fn save_json<T: serde::Serialize>(path: &PathBuf, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Allocate a unique tempdir under `std::env::temp_dir()`. Same
    /// tempdir helper as in [`crate::handlers::system`]'s tests; not
    /// shared via a common test-utils module to keep the file mirror
    /// 1:1 with upstream's `Settings.hs` having no test-utility
    /// imports.
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-cardano-tracer-settings-test-{pid}-{nanos}-{id}",
        ));
        std::fs::create_dir_all(&path).expect("create tempdir root");
        path
    }

    #[test]
    fn default_events_state_matches_upstream() {
        assert_eq!(default_events_state(), (false, 1800));
    }

    #[test]
    fn default_events_settings_uses_default_state_for_all_groups() {
        let s = default_events_settings();
        let expected = (false, 1800);
        assert_eq!(s.warnings, expected);
        assert_eq!(s.errors, expected);
        assert_eq!(s.criticals, expected);
        assert_eq!(s.alerts, expected);
        assert_eq!(s.emergencies, expected);
        assert_eq!(s.node_disconnected, expected);
    }

    #[test]
    fn default_email_settings_matches_upstream_sentinels() {
        let s = default_email_settings();
        assert!(s.smtp_host.is_empty());
        assert_eq!(s.smtp_port, -1);
        assert!(s.username.is_empty());
        assert!(s.password.is_empty());
        assert_eq!(s.ssl, EmailSSL::Tls);
        assert!(s.email_from.is_empty());
        assert!(s.email_to.is_empty());
        assert!(s.subject.is_empty());
    }

    #[test]
    fn incomplete_email_settings_true_for_default() {
        assert!(incomplete_email_settings(&default_email_settings()));
    }

    #[test]
    fn incomplete_email_settings_false_when_smtp_host_set() {
        let mut s = default_email_settings();
        s.smtp_host = "smtp.example.com".to_string();
        assert!(!incomplete_email_settings(&s));
    }

    #[test]
    fn read_saved_email_settings_falls_back_to_default_when_no_file() {
        let tmp = tempdir();
        let read = read_saved_email_settings(Some(&tmp));
        assert_eq!(read, default_email_settings());
    }

    #[test]
    fn read_saved_events_settings_falls_back_to_default_when_no_file() {
        let tmp = tempdir();
        let read = read_saved_events_settings(Some(&tmp));
        assert_eq!(read, default_events_settings());
    }

    #[test]
    fn read_saved_email_settings_falls_back_on_unparsable_json() {
        let tmp = tempdir();
        // Pre-populate the email file with garbage.
        let (email_path, _) =
            get_paths_to_notifications_settings(Some(&tmp)).expect("paths resolve");
        std::fs::write(&email_path, b"not valid json {{{").expect("write garbage");
        let read = read_saved_email_settings(Some(&tmp));
        assert_eq!(read, default_email_settings());
    }

    #[test]
    fn save_then_read_email_settings_round_trips() {
        let tmp = tempdir();
        let mut original = default_email_settings();
        original.smtp_host = "smtp.example.com".to_string();
        original.smtp_port = 587;
        original.username = "alerts@example.com".to_string();
        original.email_from = "Cardano Tracer <alerts@example.com>".to_string();
        original.email_to = "operator@example.com".to_string();
        original.subject = "Cardano alert".to_string();
        original.ssl = EmailSSL::Starttls;
        save_email_settings_on_disk(Some(&tmp), &original);
        let read = read_saved_email_settings(Some(&tmp));
        assert_eq!(read, original);
    }

    #[test]
    fn save_then_read_events_settings_round_trips() {
        let tmp = tempdir();
        let original = EventsSettings {
            warnings: (true, 30),
            errors: (true, 15),
            criticals: (true, 5),
            alerts: (false, 1),
            emergencies: (true, 1),
            node_disconnected: (true, 60),
        };
        save_events_settings_on_disk(Some(&tmp), &original);
        let read = read_saved_events_settings(Some(&tmp));
        assert_eq!(read, original);
    }

    #[test]
    fn save_email_settings_creates_notifications_subdir() {
        let tmp = tempdir();
        let settings = default_email_settings();
        save_email_settings_on_disk(Some(&tmp), &settings);
        let (email_path, _) =
            get_paths_to_notifications_settings(Some(&tmp)).expect("paths resolve");
        assert!(email_path.exists());
    }

    #[test]
    fn save_events_settings_creates_notifications_subdir() {
        let tmp = tempdir();
        let settings = default_events_settings();
        save_events_settings_on_disk(Some(&tmp), &settings);
        let (_, events_path) =
            get_paths_to_notifications_settings(Some(&tmp)).expect("paths resolve");
        assert!(events_path.exists());
    }

    #[test]
    fn save_overwrites_previous_email_settings_file() {
        let tmp = tempdir();
        let mut s1 = default_email_settings();
        s1.smtp_host = "first.example.com".to_string();
        save_email_settings_on_disk(Some(&tmp), &s1);

        let mut s2 = default_email_settings();
        s2.smtp_host = "second.example.com".to_string();
        save_email_settings_on_disk(Some(&tmp), &s2);

        let read = read_saved_email_settings(Some(&tmp));
        assert_eq!(read.smtp_host, "second.example.com");
    }
}
