//! cardano-testnet runtime temporary-path helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Filepath.hs.
//!
//! The path helpers return `String` (Haskell `FilePath`) rather than
//! `PathBuf` so the trailing-separator forms produced by
//! `addTrailingPathSeparator` survive — `PathBuf` normalises trailing
//! separators away. Ports the full `Filepath.hs` surface:
//! `TmpAbsolutePath`, `Sprocket`, and the `makeTmpBaseAbsPath` /
//! `makeLogDir` / `makeTmpRelPath` / `makeSocketDir` / `makeSprocket`
//! helpers. `Sprocket` is the `Hedgehog.Extras` socket-path type,
//! carried locally (it is referenced by `Filepath.hs`'s
//! `makeSprocket` signature).

use std::path::Path;

/// A runtime temporary (output) directory path.
///
/// Mirror of upstream `newtype TmpAbsolutePath` (`Testnet/Filepath.hs`).
/// Upstream derives `IsString` (string-literal construction) and a
/// `Display` instance — reproduced here by `From<&str>` / `From<String>`
/// and `std::fmt::Display`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmpAbsolutePath(pub String);

impl TmpAbsolutePath {
    /// Borrow the inner path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TmpAbsolutePath {
    fn from(s: &str) -> TmpAbsolutePath {
        TmpAbsolutePath(s.to_string())
    }
}

impl From<String> for TmpAbsolutePath {
    fn from(s: String) -> TmpAbsolutePath {
        TmpAbsolutePath(s)
    }
}

impl std::fmt::Display for TmpAbsolutePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Append a trailing `/` to a path if it lacks one.
///
/// Mirror of upstream `System.FilePath.addTrailingPathSeparator` for
/// the Unix path separator.
fn add_trailing_path_separator(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn join_unix_file_path(base: &str, name: &str) -> String {
    let base_is_root = base.starts_with('/') && base.trim_matches('/').is_empty();
    let base = base.trim_end_matches('/');
    let name = name.trim_start_matches('/');
    if base_is_root {
        return if name.is_empty() {
            "/".to_string()
        } else {
            format!("/{name}")
        };
    }
    match (base.is_empty(), name.is_empty()) {
        (true, true) => String::new(),
        (true, false) => name.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}/{name}"),
    }
}

/// The base (parent) directory of a temporary path, with a trailing
/// separator.
///
/// Mirror of upstream
/// `makeTmpBaseAbsPath = addTrailingPathSeparator . takeDirectory`.
pub fn make_tmp_base_abs_path(tmp: &TmpAbsolutePath) -> String {
    let parent = Path::new(&tmp.0)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or(&tmp.0);
    add_trailing_path_separator(parent)
}

/// The log directory of a temporary path — `<tmp>/logs/`.
///
/// Mirror of upstream
/// `makeLogDir = addTrailingPathSeparator . (</> "logs")`.
pub fn make_log_dir(tmp: &TmpAbsolutePath) -> String {
    add_trailing_path_separator(&format!("{}/logs", tmp.0.trim_end_matches('/')))
}

/// A Unix-domain-socket path, split into a base directory and a name.
///
/// Mirror of upstream `Sprocket` (from
/// `Hedgehog.Extras.Stock.IO.Network.Sprocket` — a base path plus a
/// relative name; the full socket path is their join). Carried
/// locally because `Filepath.hs`'s `makeSprocket` produces it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sprocket {
    /// The base (absolute) directory.
    pub base: String,
    /// The socket name — a path relative to `base`.
    pub name: String,
}

impl Sprocket {
    /// The full socket path — `base` joined with `name`.
    ///
    /// Mirror of upstream `sprocketSystemName`.
    pub fn system_name(&self) -> String {
        join_unix_file_path(&self.base, &self.name)
    }
}

/// The temporary path relative to its base directory.
///
/// Mirror of upstream
/// `makeTmpRelPath fp = makeRelative (makeTmpBaseAbsPath fp) fp` — a
/// path not under the base is returned unchanged.
pub fn make_tmp_rel_path(tmp: &TmpAbsolutePath) -> String {
    let base = make_tmp_base_abs_path(tmp);
    Path::new(&tmp.0)
        .strip_prefix(&base)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| tmp.0.clone())
}

/// The socket directory of a temporary path —
/// `<tmp-rel-path>/socket`.
///
/// Mirror of upstream `makeSocketDir fp = makeTmpRelPath fp </>
/// defaultSocketDir`.
pub fn make_socket_dir(tmp: &TmpAbsolutePath) -> String {
    join_unix_file_path(&make_tmp_rel_path(tmp), crate::paths::DEFAULT_SOCKET_DIR)
}

/// The [`Sprocket`] for a named node within a temporary path.
///
/// Mirror of upstream `makeSprocket tmpAbsPath node = Sprocket
/// (makeTmpBaseAbsPath tmpAbsPath) (makeSocketDir tmpAbsPath </> node)`.
pub fn make_sprocket(tmp: &TmpAbsolutePath, node: &str) -> Sprocket {
    Sprocket {
        base: make_tmp_base_abs_path(tmp),
        name: join_unix_file_path(&make_socket_dir(tmp), node),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmp_absolute_path_constructs_and_displays() {
        let from_str: TmpAbsolutePath = "/tmp/testnet-abc".into();
        let from_string: TmpAbsolutePath = String::from("/tmp/testnet-abc").into();
        assert_eq!(from_str, from_string);
        assert_eq!(from_str.as_str(), "/tmp/testnet-abc");
        assert_eq!(format!("{from_str}"), "/tmp/testnet-abc");
    }

    #[test]
    fn make_tmp_base_abs_path_is_parent_with_trailing_slash() {
        let tmp: TmpAbsolutePath = "/tmp/testnet-abc/run".into();
        assert_eq!(make_tmp_base_abs_path(&tmp), "/tmp/testnet-abc/");
    }

    #[test]
    fn make_log_dir_appends_logs_with_trailing_slash() {
        let tmp: TmpAbsolutePath = "/tmp/testnet-abc/run".into();
        assert_eq!(make_log_dir(&tmp), "/tmp/testnet-abc/run/logs/");
        // A trailing slash on the input is not doubled.
        let tmp_slash: TmpAbsolutePath = "/tmp/run/".into();
        assert_eq!(make_log_dir(&tmp_slash), "/tmp/run/logs/");
    }

    #[test]
    fn make_tmp_rel_path_strips_the_base() {
        let tmp: TmpAbsolutePath = "/tmp/testnet-abc/run".into();
        assert_eq!(make_tmp_rel_path(&tmp), "run");
    }

    #[test]
    fn make_socket_dir_is_rel_path_plus_socket() {
        let tmp: TmpAbsolutePath = "/tmp/testnet-abc/run".into();
        assert_eq!(make_socket_dir(&tmp), "run/socket");
    }

    #[test]
    fn make_sprocket_splits_base_and_name() {
        let tmp: TmpAbsolutePath = "/tmp/testnet-abc/run".into();
        let s = make_sprocket(&tmp, "node0");
        assert_eq!(s.base, "/tmp/testnet-abc/");
        assert_eq!(s.name, "run/socket/node0");
        assert_eq!(s.system_name(), "/tmp/testnet-abc/run/socket/node0");
    }

    #[test]
    fn sprocket_system_name_preserves_unix_root_base() {
        let s = Sprocket {
            base: "/".to_string(),
            name: "run/socket/node0".to_string(),
        };
        assert_eq!(s.system_name(), "/run/socket/node0");
    }
}
