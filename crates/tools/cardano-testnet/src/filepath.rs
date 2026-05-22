//! cardano-testnet runtime temporary-path helpers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Filepath.hs.
//!
//! The path helpers return `String` (Haskell `FilePath`) rather than
//! `PathBuf` so the trailing-separator forms produced by
//! `addTrailingPathSeparator` survive — `PathBuf` normalises trailing
//! separators away. This slice ports `TmpAbsolutePath`,
//! `makeTmpBaseAbsPath`, and `makeLogDir`; the `Sprocket`-valued
//! `makeTmpRelPath` / `makeSocketDir` / `makeSprocket` land with the
//! testnet-harness rounds.

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
}
