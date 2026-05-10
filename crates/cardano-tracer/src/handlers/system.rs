//! Path-resolution helpers — locate per-tracer state + config
//! directories on the host filesystem, with XDG fallback.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/System.hs.
//!
//! Direct port of upstream's path-resolution module, mapping
//! `TracerEnv`-taking helpers to state-dir-arg variants so the port
//! stays self-contained while the upstream `TracerEnv` 14-field
//! record is still pending.
//!
//! Mapping summary:
//!
//! | Upstream                                                                          | Yggdrasil                                  |
//! |-----------------------------------------------------------------------------------|--------------------------------------------|
//! | `getPathsToNotificationsSettings :: Maybe FilePath -> IO (FilePath, FilePath)`    | [`get_paths_to_notifications_settings`]    |
//! | `getPathToChartsConfig :: TracerEnv -> IO FilePath`                               | [`get_path_to_charts_config`]              |
//! | `getPathToThemeConfig :: TracerEnv -> IO FilePath`                                | [`get_path_to_theme_config`]               |
//! | `getPathToLogsLiveViewFontConfig :: TracerEnv -> IO FilePath`                     | [`get_path_to_logs_live_view_font_config`] |
//! | `getPathToChartColorsDir :: TracerEnv -> IO FilePath`                             | [`get_path_to_chart_colors_dir`]           |
//! | `getPathToBackupDir :: TracerEnv -> IO FilePath`                                  | [`get_path_to_backup_dir`]                 |
//! | `rtViewRootDir`                                                                   | [`RT_VIEW_ROOT_DIR`]                       |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`Cardano.Tracer.Environment.TracerEnv`**: upstream's
//!   path-resolution helpers take a [`TracerEnv`] record and pluck
//!   `teStateDir :: !(Maybe FilePath)` out of it. The full
//!   `TracerEnv` 14-field record is pending (R382+ remaining-work
//!   list — depends on Cardano.Logging + Timeseries vendoring).
//!   Yggdrasil's port takes `Option<&Path>` directly so the helpers
//!   are usable now; once `TracerEnv` is ported, thin wrappers can
//!   pluck `te_state_dir` and call into these lower-level helpers.
//! - **`System.Directory.XdgDirectory`**: upstream uses GHC's
//!   `getXdgDirectory` which consults `$XDG_CONFIG_HOME` /
//!   `$XDG_DATA_HOME` and falls back to platform-specific defaults.
//!   The Rust port replicates the Linux/Unix subset (the cardano-tracer
//!   binary is Unix-only in practice — operators run it on the same
//!   hosts as the node binary): reads `$XDG_CONFIG_HOME` with
//!   fallback to `$HOME/.config`, and `$XDG_DATA_HOME` with fallback
//!   to `$HOME/.local/share`.

use std::path::{Path, PathBuf};

/// Sub-directory under XDG-config or XDG-data where the cardano-tracer
/// stashes its persistent state. Mirror of upstream
/// `rtViewRootDir = "cardano-rt-view"`.
pub const RT_VIEW_ROOT_DIR: &str = "cardano-rt-view";

/// XDG-base-dir kind — `Config` resolves to `$XDG_CONFIG_HOME` (or
/// `$HOME/.config`); `Data` resolves to `$XDG_DATA_HOME` (or
/// `$HOME/.local/share`). Mirror of upstream
/// `System.Directory.XdgDirectory` minus the cache + state variants
/// (cardano-tracer uses only Config + Data).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum XdgKind {
    /// `$XDG_CONFIG_HOME` (default `$HOME/.config`).
    Config,
    /// `$XDG_DATA_HOME` (default `$HOME/.local/share`).
    Data,
}

/// Get the cardano-tracer's per-XDG-kind state directory. Mirror of
/// upstream `getStateDir`.
///
/// If the caller supplies `Some(state_dir)` via the
/// `--state-dir`/`teStateDir` plumbing, that path is used verbatim
/// regardless of `XdgKind`. Otherwise the XDG fallback applies (per
/// the carve-out documented in the module docstring).
///
/// Returns the resolved directory **without** creating it. Callers
/// that need the directory to exist should use one of the inherent
/// `getPathTo*` helpers, which call
/// `std::fs::create_dir_all` on the resolved path.
pub fn get_state_dir(state_dir: Option<&Path>, xdg: XdgKind) -> PathBuf {
    if let Some(path) = state_dir {
        return path.to_path_buf();
    }
    xdg_dir_with_fallback(xdg)
}

fn xdg_dir_with_fallback(xdg: XdgKind) -> PathBuf {
    match xdg {
        XdgKind::Config => {
            xdg_dir_with_env_lookup(xdg, |k| std::env::var_os(k).map(PathBuf::from), home_dir)
        }
        XdgKind::Data => {
            xdg_dir_with_env_lookup(xdg, |k| std::env::var_os(k).map(PathBuf::from), home_dir)
        }
    }
}

/// Test-friendly helper: resolves the XDG dir for `xdg` using the
/// supplied env-var lookup + home-dir lookup closures rather than
/// the live process environment. Used by the unit tests below.
pub fn xdg_dir_with_env_lookup<E, H>(xdg: XdgKind, env_lookup: E, home_lookup: H) -> PathBuf
where
    E: Fn(&str) -> Option<PathBuf>,
    H: Fn() -> Option<PathBuf>,
{
    let (env_var, home_suffix) = match xdg {
        XdgKind::Config => ("XDG_CONFIG_HOME", ".config"),
        XdgKind::Data => ("XDG_DATA_HOME", ".local/share"),
    };
    if let Some(path) = env_lookup(env_var)
        && !path.as_os_str().is_empty()
    {
        return path;
    }
    home_lookup()
        .map(|home| home.join(home_suffix))
        .unwrap_or_else(|| PathBuf::from(home_suffix))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Get the path to the cardano-tracer's config directory (with
/// directory creation if missing). Mirror of upstream
/// `getPathToConfigDir`.
pub fn get_path_to_config_dir(state_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    let configured = get_state_dir(state_dir, XdgKind::Config).join(RT_VIEW_ROOT_DIR);
    std::fs::create_dir_all(&configured)?;
    Ok(configured)
}

/// Resolve `(email_path, events_path)` for the notification-engine
/// settings sink. Mirror of upstream
/// `getPathsToNotificationsSettings :: Maybe FilePath -> IO (FilePath, FilePath)`.
pub fn get_paths_to_notifications_settings(
    state_dir: Option<&Path>,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let config_dir = get_path_to_config_dir(state_dir)?;
    let notify_dir = config_dir.join("notifications");
    std::fs::create_dir_all(&notify_dir)?;
    Ok((notify_dir.join("email"), notify_dir.join("events")))
}

fn get_path_to_named_config(
    state_dir: Option<&Path>,
    config_name: &str,
) -> std::io::Result<PathBuf> {
    let config_dir = get_path_to_config_dir(state_dir)?;
    Ok(config_dir.join(config_name))
}

/// Path to the charts-config file. Mirror of upstream
/// `getPathToChartsConfig`.
pub fn get_path_to_charts_config(state_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    get_path_to_named_config(state_dir, "charts")
}

/// Path to the theme-config file. Mirror of upstream
/// `getPathToThemeConfig`.
pub fn get_path_to_theme_config(state_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    get_path_to_named_config(state_dir, "theme")
}

/// Path to the live-view-font-config file. Mirror of upstream
/// `getPathToLogsLiveViewFontConfig`.
pub fn get_path_to_logs_live_view_font_config(
    state_dir: Option<&Path>,
) -> std::io::Result<PathBuf> {
    get_path_to_named_config(state_dir, "llvFontSize")
}

/// Path to the chart-colors directory. Mirror of upstream
/// `getPathToChartColorsDir`.
pub fn get_path_to_chart_colors_dir(state_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    let config_dir = get_path_to_config_dir(state_dir)?;
    let colors_dir = config_dir.join("color");
    std::fs::create_dir_all(&colors_dir)?;
    Ok(colors_dir)
}

/// Path to the backup directory. Mirror of upstream
/// `getPathToBackupDir`.
pub fn get_path_to_backup_dir(state_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    let data_dir = get_state_dir(state_dir, XdgKind::Data).join(RT_VIEW_ROOT_DIR);
    let backup_dir = data_dir.join("backup");
    std::fs::create_dir_all(&backup_dir)?;
    Ok(backup_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_env(_: &str) -> Option<PathBuf> {
        None
    }

    fn fake_home() -> Option<PathBuf> {
        Some(PathBuf::from("/home/operator"))
    }

    fn no_home() -> Option<PathBuf> {
        None
    }

    #[test]
    fn rt_view_root_dir_is_canonical_string() {
        assert_eq!(RT_VIEW_ROOT_DIR, "cardano-rt-view");
    }

    #[test]
    fn get_state_dir_uses_supplied_path_for_config() {
        let dir = get_state_dir(Some(Path::new("/tmp/op-state")), XdgKind::Config);
        assert_eq!(dir, PathBuf::from("/tmp/op-state"));
    }

    #[test]
    fn get_state_dir_uses_supplied_path_for_data() {
        let dir = get_state_dir(Some(Path::new("/tmp/op-state")), XdgKind::Data);
        // Even with Data kind, an explicit path overrides XDG.
        assert_eq!(dir, PathBuf::from("/tmp/op-state"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_uses_xdg_config_home_when_set() {
        let env_lookup = |key: &str| {
            if key == "XDG_CONFIG_HOME" {
                Some(PathBuf::from("/custom/cfg"))
            } else {
                None
            }
        };
        let dir = xdg_dir_with_env_lookup(XdgKind::Config, env_lookup, fake_home);
        assert_eq!(dir, PathBuf::from("/custom/cfg"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_uses_xdg_data_home_when_set() {
        let env_lookup = |key: &str| {
            if key == "XDG_DATA_HOME" {
                Some(PathBuf::from("/custom/data"))
            } else {
                None
            }
        };
        let dir = xdg_dir_with_env_lookup(XdgKind::Data, env_lookup, fake_home);
        assert_eq!(dir, PathBuf::from("/custom/data"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_falls_back_to_home_for_config() {
        let dir = xdg_dir_with_env_lookup(XdgKind::Config, empty_env, fake_home);
        assert_eq!(dir, PathBuf::from("/home/operator/.config"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_falls_back_to_home_for_data() {
        let dir = xdg_dir_with_env_lookup(XdgKind::Data, empty_env, fake_home);
        assert_eq!(dir, PathBuf::from("/home/operator/.local/share"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_handles_missing_home_with_relative_fallback() {
        let dir = xdg_dir_with_env_lookup(XdgKind::Config, empty_env, no_home);
        // Without HOME, we get the bare suffix (relative path).
        assert_eq!(dir, PathBuf::from(".config"));
    }

    #[test]
    fn xdg_dir_with_env_lookup_treats_empty_xdg_var_as_unset() {
        let env_lookup = |key: &str| {
            if key == "XDG_CONFIG_HOME" {
                Some(PathBuf::from(""))
            } else {
                None
            }
        };
        let dir = xdg_dir_with_env_lookup(XdgKind::Config, env_lookup, fake_home);
        assert_eq!(dir, PathBuf::from("/home/operator/.config"));
    }

    #[test]
    fn get_paths_to_notifications_settings_creates_directory_and_returns_pair() {
        let tmp = tempdir();
        let (email, events) =
            get_paths_to_notifications_settings(Some(&tmp)).expect("paths resolve");
        assert!(email.ends_with("notifications/email"));
        assert!(events.ends_with("notifications/events"));
        let parent = email.parent().expect("notification dir has parent");
        assert!(parent.exists(), "notifications/ subdir must be created");
        assert!(parent.is_dir());
    }

    #[test]
    fn get_path_to_charts_config_uses_named_config() {
        let tmp = tempdir();
        let path = get_path_to_charts_config(Some(&tmp)).expect("resolves");
        assert!(path.ends_with("charts"));
        assert!(path.starts_with(&tmp));
    }

    #[test]
    fn get_path_to_theme_config_uses_named_config() {
        let tmp = tempdir();
        let path = get_path_to_theme_config(Some(&tmp)).expect("resolves");
        assert!(path.ends_with("theme"));
    }

    #[test]
    fn get_path_to_logs_live_view_font_config_uses_named_config() {
        let tmp = tempdir();
        let path = get_path_to_logs_live_view_font_config(Some(&tmp)).expect("resolves");
        assert!(path.ends_with("llvFontSize"));
    }

    #[test]
    fn get_path_to_chart_colors_dir_creates_color_subdir() {
        let tmp = tempdir();
        let path = get_path_to_chart_colors_dir(Some(&tmp)).expect("resolves");
        assert!(path.ends_with("color"));
        assert!(path.exists());
        assert!(path.is_dir());
    }

    #[test]
    fn get_path_to_backup_dir_creates_backup_subdir() {
        let tmp = tempdir();
        let path = get_path_to_backup_dir(Some(&tmp)).expect("resolves");
        assert!(path.ends_with("backup"));
        assert!(path.exists());
        assert!(path.is_dir());
    }

    #[test]
    fn get_path_to_config_dir_creates_root_dir_if_missing() {
        let tmp = tempdir();
        let path = get_path_to_config_dir(Some(&tmp)).expect("resolves");
        assert_eq!(path, tmp.join(RT_VIEW_ROOT_DIR));
        assert!(path.exists());
        assert!(path.is_dir());
    }

    /// Allocate a unique tempdir under `std::env::temp_dir()`. Tests
    /// run in parallel, so each gets its own root.
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
            "yggdrasil-cardano-tracer-system-test-{pid}-{nanos}-{id}",
        ));
        std::fs::create_dir_all(&path).expect("create tempdir root");
        path
    }
}
