//! Pure helpers from the upstream `Testnet.Property.Util` module.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Property/Util.hs.

use serde_json::Value;

/// Error returned by [`aeson_object_lookup`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PropertyUtilError {
    /// The caller supplied a JSON value that was not an object.
    #[error("Expected an Aeson Object but got: {0}")]
    ExpectedObject(Value),
}

/// Projectable shape of upstream `integration`: one Hedgehog test.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationPlan {
    /// Number of tests configured by upstream `H.withTests`.
    pub tests: usize,
    /// Workspace names the integration wrapper will use.
    pub workspace_names: Vec<String>,
}

/// Mirror upstream `disableRetries`, with injectable lookup for deterministic tests.
pub fn disable_retries_from_env<F>(lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    lookup("DISABLE_RETRIES").as_deref() == Some("1")
}

/// Mirror upstream `disableRetries` using the real process environment.
pub fn disable_retries() -> bool {
    disable_retries_from_env(|name| std::env::var(name).ok())
}

/// Project upstream `integration` into the stable test-count surface.
pub fn integration_plan() -> IntegrationPlan {
    IntegrationPlan {
        tests: 1,
        workspace_names: Vec::new(),
    }
}

/// Project upstream `integrationWorkspace` into its stable workspace naming surface.
pub fn integration_workspace_plan(workspace_name: impl Into<String>) -> IntegrationPlan {
    IntegrationPlan {
        tests: 1,
        workspace_names: vec![workspace_name.into()],
    }
}

/// Compute the workspace names selected by upstream `integrationRetryWorkspace`.
pub fn integration_retry_workspace_names(
    retries: usize,
    workspace_name: &str,
    disable_retries: bool,
) -> Vec<String> {
    if disable_retries {
        vec![format!("{workspace_name}-no-retries")]
    } else {
        (0..retries)
            .map(|i| format!("{workspace_name}-{i}"))
            .collect()
    }
}

/// Project upstream `integrationRetryWorkspace` into its stable test-count and workspace surface.
pub fn integration_retry_workspace_plan(
    retries: usize,
    workspace_name: &str,
    disable_retries: bool,
) -> IntegrationPlan {
    IntegrationPlan {
        tests: 1,
        workspace_names: integration_retry_workspace_names(
            retries,
            workspace_name,
            disable_retries,
        ),
    }
}

/// Mirror upstream `isLinux` with explicit OS injection for tests.
pub fn is_linux_os(os: &str) -> bool {
    os == "linux"
}

/// Mirror upstream `isLinux` using Rust's target OS constant.
pub fn is_linux() -> bool {
    is_linux_os(std::env::consts::OS)
}

/// Mirror upstream `aesonObjectLookUp`.
pub fn aeson_object_lookup(value: &Value, key: &str) -> Result<Option<Value>, PropertyUtilError> {
    match value {
        Value::Object(map) => Ok(map.get(key).cloned()),
        other => Err(PropertyUtilError::ExpectedObject(other.clone())),
    }
}
