//! Environment / config-path helpers.
//!
//! Mirrors upstream `Cardano.CLI.Environment` — discovery of
//! `CARDANO_NODE_SOCKET_PATH`, `CARDANO_NODE_NETWORK_ID`, and the
//! upstream-config root directory the cardano-cli reads when invoked
//! without explicit flags.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Environment.hs`.
//! R289 lands the file with a Yggdrasil-specific
//! `resolve_upstream_reference_paths` placeholder. The full helper
//! set (env-var fallback chains, optional override resolution) lands
//! with the per-cluster rounds.

use std::path::PathBuf;

/// Resolve the upstream Haskell-share root directory used by the
/// cardano-cli binary. The fallback chain (R289 stub):
/// 1. Operator-supplied `--upstream-config-root` flag.
/// 2. `CARDANO_NODE_UPSTREAM_CONFIG_ROOT` env var.
/// 3. `/tmp/cardano-tooling/share` default (operator convention).
/// 4. Vendored `node/configuration/<network>/` as last-resort.
///
/// Mirrors upstream `getEnvCli*` resolution from
/// `Cardano.CLI.Environment`.
pub fn resolve_upstream_config_root(override_path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = override_path {
        return path;
    }
    if let Ok(env_path) = std::env::var("CARDANO_NODE_UPSTREAM_CONFIG_ROOT") {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }
    PathBuf::from("/tmp/cardano-tooling/share")
}

/// Resolve the node socket path from the operator flag or
/// `CARDANO_NODE_SOCKET_PATH` env var.
///
/// Mirrors upstream `getEnvSocketPath` from `Cardano.CLI.Environment`.
pub fn resolve_socket_path(override_path: Option<PathBuf>) -> Option<PathBuf> {
    if override_path.is_some() {
        return override_path;
    }
    std::env::var("CARDANO_NODE_SOCKET_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
