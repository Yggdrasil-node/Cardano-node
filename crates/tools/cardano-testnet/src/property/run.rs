//! Pure and injectable helpers from upstream `Testnet.Property.Run`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Property/Run.hs.
//!
//! The actual `runTestnet` / `testnetProperty` body depends on
//! Hedgehog/Tasty resource management and an intentional infinite keepalive.
//! This module ports the stable Rust-observable parts of that surface:
//! user-provided environment mode, `testnetProperty` workspace planning,
//! OS-ignore dispositions, and the startup message rendered after a testnet
//! runtime has been captured.

use crate::runtime_types::{TestnetRuntime, spo_nodes};

use std::path::{Path, PathBuf};

/// Workspace name used by upstream `integrationWorkspace "testnet"`.
pub const TESTNET_WORKSPACE_NAME: &str = "testnet";

/// Upstream keepalive sleep used by both `runTestnet` and `forkAndRunTestnet`.
pub const KEEPALIVE_DELAY_MICROS: u64 = 10_000_000;

/// Whether the user supplied an existing testnet environment path.
///
/// Mirror of upstream `data UserProvidedEnv`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserProvidedEnv {
    /// No user-provided environment; upstream creates a fresh workspace.
    NoUserProvidedEnv,
    /// User-provided environment path from the `--node-env` flag.
    UserProvidedEnv(PathBuf),
}

impl UserProvidedEnv {
    /// Borrow the user-provided workspace path when one exists.
    pub fn workspace_hint(&self) -> Option<&Path> {
        match self {
            UserProvidedEnv::NoUserProvidedEnv => None,
            UserProvidedEnv::UserProvidedEnv(path) => Some(path.as_path()),
        }
    }
}

/// Deterministic projection of upstream `testnetProperty` setup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestnetPropertyPlan {
    /// Workspace branch selected by `UserProvidedEnv`.
    pub workspace: TestnetPropertyWorkspace,
    /// Keepalive delay used by the resource-holding thread.
    pub keepalive_delay_micros: u64,
    /// Upstream intentionally fails after `runTn` to force the report body.
    pub intentional_failure_after_run: bool,
}

impl TestnetPropertyPlan {
    /// The `H.note_` text emitted for user-provided environments.
    pub fn note(&self) -> Option<String> {
        match &self.workspace {
            TestnetPropertyWorkspace::IntegrationWorkspace { .. } => None,
            TestnetPropertyWorkspace::UserProvided { output_dir, action } => {
                Some(format!("{} {}", action.note_prefix(), output_dir.display()))
            }
        }
    }
}

/// Workspace branch selected by upstream `testnetProperty`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TestnetPropertyWorkspace {
    /// `NoUserProvidedEnv` uses `integrationWorkspace "testnet"`.
    IntegrationWorkspace {
        /// Workspace name passed to `integrationWorkspace`.
        workspace_name: String,
    },
    /// `UserProvidedEnv` uses the absolute user output directory.
    UserProvided {
        /// Absolute user output directory.
        output_dir: PathBuf,
        /// Filesystem action selected from `doesDirectoryExist`.
        action: UserProvidedEnvAction,
    },
}

/// Filesystem action selected for a user-provided testnet environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserProvidedEnvAction {
    /// Existing directory; upstream notes `Reusing <path>`.
    ReuseExisting,
    /// Missing directory; upstream creates it and notes `Created <path>`.
    CreateDirectory,
}

impl UserProvidedEnvAction {
    fn note_prefix(self) -> &'static str {
        match self {
            UserProvidedEnvAction::ReuseExisting => "Reusing",
            UserProvidedEnvAction::CreateDirectory => "Created",
        }
    }
}

/// Project the `NoUserProvidedEnv` branch of upstream `testnetProperty`.
pub fn no_user_provided_env_testnet_property_plan() -> TestnetPropertyPlan {
    TestnetPropertyPlan {
        workspace: TestnetPropertyWorkspace::IntegrationWorkspace {
            workspace_name: TESTNET_WORKSPACE_NAME.to_string(),
        },
        keepalive_delay_micros: KEEPALIVE_DELAY_MICROS,
        intentional_failure_after_run: true,
    }
}

/// Project the `UserProvidedEnv` branch of upstream `testnetProperty`.
pub fn user_provided_env_testnet_property_plan(
    abs_user_output_dir: impl Into<PathBuf>,
    dir_exists: bool,
) -> TestnetPropertyPlan {
    let action = if dir_exists {
        UserProvidedEnvAction::ReuseExisting
    } else {
        UserProvidedEnvAction::CreateDirectory
    };
    TestnetPropertyPlan {
        workspace: TestnetPropertyWorkspace::UserProvided {
            output_dir: abs_user_output_dir.into(),
            action,
        },
        keepalive_delay_micros: KEEPALIVE_DELAY_MICROS,
        intentional_failure_after_run: true,
    }
}

/// A test-tree branch selected by upstream's OS ignore helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyDisposition {
    /// The property should run normally.
    Run {
        /// Upstream property name.
        property_name: String,
    },
    /// The property should be reported as ignored.
    Ignored(IgnoredProperty),
}

impl PropertyDisposition {
    /// The upstream `resultShortDescription` when this property is ignored.
    pub fn ignored_reason(&self) -> Option<&str> {
        match self {
            PropertyDisposition::Run { .. } => None,
            PropertyDisposition::Ignored(ignored) => {
                Some(ignored.result_short_description.as_str())
            }
        }
    }
}

/// Stable projection of upstream `ignoreOn`'s `testPassed` result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IgnoredProperty {
    /// Upstream property name.
    pub property_name: String,
    /// The `testPassed` reason text.
    pub reason: String,
    /// The Tasty `resultShortDescription`.
    pub result_short_description: String,
}

/// Mirror upstream `ignoreOn`.
pub fn ignore_on(os: impl Into<String>, property_name: impl Into<String>) -> IgnoredProperty {
    let reason = format!("IGNORED on {}", os.into());
    IgnoredProperty {
        property_name: property_name.into(),
        result_short_description: reason.clone(),
        reason,
    }
}

/// Mirror upstream `disabled`.
pub fn disabled(property_name: impl Into<String>) -> IgnoredProperty {
    ignore_on("Disabled", property_name)
}

/// Mirror upstream `ignoreOnWindows` with injectable OS predicate.
pub fn ignore_on_windows(
    property_name: impl Into<String>,
    is_windows: bool,
) -> PropertyDisposition {
    let property_name = property_name.into();
    if is_windows {
        PropertyDisposition::Ignored(ignore_on("Windows", property_name))
    } else {
        PropertyDisposition::Run { property_name }
    }
}

/// Mirror upstream `ignoreOnWindows` using Rust's target OS.
pub fn ignore_on_windows_current(property_name: impl Into<String>) -> PropertyDisposition {
    ignore_on_windows(property_name, cfg!(windows))
}

/// Mirror upstream `ignoreOnMac` with injectable `System.Info.os`.
pub fn ignore_on_mac(property_name: impl Into<String>, sys_os: &str) -> PropertyDisposition {
    let property_name = property_name.into();
    if is_macos_os(sys_os) {
        PropertyDisposition::Ignored(ignore_on("MacOS", property_name))
    } else {
        PropertyDisposition::Run { property_name }
    }
}

/// Mirror upstream `ignoreOnMac` using Rust's target OS name.
pub fn ignore_on_mac_current(property_name: impl Into<String>) -> PropertyDisposition {
    let sys_os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    ignore_on_mac(property_name, sys_os)
}

/// Mirror upstream `ignoreOnMacAndWindows` with injectable OS name.
pub fn ignore_on_mac_and_windows(
    property_name: impl Into<String>,
    sys_os: &str,
) -> PropertyDisposition {
    let property_name = property_name.into();
    if is_macos_os(sys_os) || is_windows_os(sys_os) {
        PropertyDisposition::Ignored(ignore_on("MacOS and Windows", property_name))
    } else {
        PropertyDisposition::Run { property_name }
    }
}

/// Mirror upstream `ignoreOnMacAndWindows` using Rust's target OS name.
pub fn ignore_on_mac_and_windows_current(property_name: impl Into<String>) -> PropertyDisposition {
    let sys_os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "windows"
    } else {
        std::env::consts::OS
    };
    ignore_on_mac_and_windows(property_name, sys_os)
}

/// Mirror upstream `SYS.os == "darwin"`.
pub fn is_macos_os(sys_os: &str) -> bool {
    sys_os == "darwin" || sys_os == "macos"
}

fn is_windows_os(sys_os: &str) -> bool {
    matches!(sys_os, "windows" | "mingw32" | "win32")
}

/// Render the operator-facing message printed after `runTestnet` captures a runtime.
pub fn render_running_testnet_message(runtime: &TestnetRuntime) -> String {
    let mut message = format!(
        "Please disregard the message above implying a failure.\n\n\
         Testnet is running with config file {}\n",
        runtime.configuration_file.display()
    );

    match spo_nodes(runtime).first() {
        Some(node) => {
            message.push_str(&format!(
                "Logs of the SPO node can be found at {}\n\n\
                 To interact with the testnet using cardano-cli, you might want to set:\n\n\
                   export CARDANO_NODE_SOCKET_PATH={}\n\
                   export CARDANO_NODE_NETWORK_ID={}\n",
                node.node_stdout.display(),
                node.node_sprocket.system_name(),
                runtime.testnet_magic
            ));
        }
        None => {
            message.push_str("\nFailed to find any SPO node in the testnet\n\n");
        }
    }

    message.push_str("Type CTRL-C to exit.\n");
    message
}

/// Render the post-check branch of upstream `runTestnet`.
pub fn render_run_testnet_result(runtime: Option<&TestnetRuntime>) -> String {
    match runtime {
        Some(runtime) => render_running_testnet_message(runtime),
        None => "Failed to start testnet.\n".to_string(),
    }
}
