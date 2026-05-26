//! Process execution helpers for the `cardano-testnet` harness.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/Run.hs.

use crate::filepath::Sprocket;

use serde::de::DeserializeOwned;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Process execution configuration.
///
/// Mirror of upstream `Hedgehog.Extras.Test.Process.ExecConfig` fields used by
/// `Testnet.Process.Run`: an optional full environment and optional cwd.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecConfig {
    /// Full child-process environment when set.
    pub env: Option<Vec<(String, String)>>,
    /// Child-process working directory when set.
    pub cwd: Option<PathBuf>,
}

/// Planned flexible process invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessPlan {
    /// Executable path or name selected for the process.
    pub executable: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Full child-process environment when set.
    pub env: Option<Vec<(String, String)>>,
    /// Child-process working directory when set.
    pub cwd: Option<PathBuf>,
    /// Whether the child should be put in a separate process group.
    pub create_group: bool,
}

/// Captured process output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    /// Process exit code. `None` means the process ended without a normal code.
    pub exit_code: Option<i32>,
    /// Captured stdout as UTF-8-lossy text.
    pub stdout: String,
    /// Captured stderr as UTF-8-lossy text.
    pub stderr: String,
}

/// Error returned by [`exec_flex`].
#[derive(Debug, thiserror::Error)]
pub enum ProcessRunError {
    /// Starting or waiting for the process failed.
    #[error("process IO failed: {0}")]
    Io(#[from] std::io::Error),
    /// Process exited with a non-zero status.
    #[error("process exited with non-zero status {exit_code:?}")]
    NonZeroExit {
        /// Process exit code.
        exit_code: Option<i32>,
        /// Captured stdout.
        stdout: String,
        /// Captured stderr.
        stderr: String,
    },
}

/// Error returned by [`exec_cli_stdout_to_json`].
#[derive(Debug, thiserror::Error)]
pub enum ProcessJsonError {
    /// Process execution failed.
    #[error(transparent)]
    Process(#[from] ProcessRunError),
    /// Stdout was not valid JSON for the requested type.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Resolve upstream `bashPath`.
pub fn bash_path() -> String {
    match std::env::var("BASH_PATH") {
        Ok(value) if !value.is_empty() => value,
        _ => "bash".to_string(),
    }
}

/// Build upstream `mkExecConfig` from a socket and inherited environment.
pub fn mk_exec_config(
    temp_base_abs_path: impl AsRef<Path>,
    sprocket: &Sprocket,
    network_id: i64,
    inherited_env: Vec<(String, String)>,
) -> ExecConfig {
    let mut env = vec![
        (
            "CARDANO_NODE_SOCKET_PATH".to_string(),
            sprocket.system_name(),
        ),
        (
            "CARDANO_NODE_NETWORK_ID".to_string(),
            network_id.to_string(),
        ),
    ];
    env.extend(inherited_env);
    ExecConfig {
        env: Some(env),
        cwd: Some(temp_base_abs_path.as_ref().to_path_buf()),
    }
}

/// Build upstream `mkExecConfigOffline` from inherited environment.
pub fn mk_exec_config_offline(
    temp_base_abs_path: impl AsRef<Path>,
    inherited_env: Vec<(String, String)>,
) -> ExecConfig {
    ExecConfig {
        env: Some(inherited_env),
        cwd: Some(temp_base_abs_path.as_ref().to_path_buf()),
    }
}

/// Prepend environment variables to an existing [`ExecConfig`].
pub fn add_env_vars_to_config(
    exec_config: &ExecConfig,
    new_env_vars: Vec<(String, String)>,
) -> ExecConfig {
    let mut env = new_env_vars;
    env.extend(exec_config.env.clone().unwrap_or_default());
    ExecConfig {
        env: Some(env),
        cwd: exec_config.cwd.clone(),
    }
}

/// Build an inherited environment vector for config constructors.
pub fn current_environment() -> Vec<(String, String)> {
    std::env::vars().collect()
}

/// Build upstream `procFlex` using a caller-supplied environment lookup.
pub fn proc_flex_plan_with_env<I, S, F>(
    exec_config: &ExecConfig,
    pkg_bin: &str,
    env_bin: &str,
    arguments: I,
    env_lookup: F,
) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    let executable = env_lookup(env_bin).unwrap_or_else(|| pkg_bin.to_string());
    ProcessPlan {
        executable,
        args: arguments
            .into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect(),
        env: exec_config.env.clone(),
        cwd: exec_config.cwd.clone(),
        create_group: true,
    }
}

/// Build upstream `procFlex` using the real process environment.
pub fn proc_flex_plan<I, S>(
    exec_config: &ExecConfig,
    pkg_bin: &str,
    env_bin: &str,
    arguments: I,
) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex_plan_with_env(exec_config, pkg_bin, env_bin, arguments, |name| {
        std::env::var(name).ok()
    })
}

/// Build upstream `procCli`.
pub fn proc_cli_plan<I, S>(exec_config: &ExecConfig, arguments: I) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex_plan(exec_config, "cardano-cli", "CARDANO_CLI", arguments)
}

/// Build upstream `procNode`.
pub fn proc_node_plan<I, S>(exec_config: &ExecConfig, arguments: I) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex_plan(exec_config, "cardano-node", "CARDANO_NODE", arguments)
}

/// Build upstream `procKESAgent`.
pub fn proc_kes_agent_plan<I, S>(exec_config: &ExecConfig, arguments: I) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex_plan(exec_config, "kes-agent", "KES_AGENT", arguments)
}

/// Build upstream `procSubmitApi`.
pub fn proc_submit_api_plan<I, S>(exec_config: &ExecConfig, arguments: I) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex_plan(
        exec_config,
        "cardano-submit-api",
        "CARDANO_SUBMIT_API",
        arguments,
    )
}

/// Build upstream `procChairman` using a caller-supplied environment lookup.
pub fn proc_chairman_plan_with_env<I, S, F>(
    exec_config: &ExecConfig,
    arguments: I,
    env_lookup: F,
) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    let args = std::iter::once("run".to_string())
        .chain(arguments.into_iter().map(|arg| arg.as_ref().to_string()))
        .collect::<Vec<_>>();
    proc_flex_plan_with_env(
        exec_config,
        "cardano-node-chairman",
        "CARDANO_NODE_CHAIRMAN",
        args,
        env_lookup,
    )
}

/// Build upstream `procChairman`.
pub fn proc_chairman_plan<I, S>(exec_config: &ExecConfig, arguments: I) -> ProcessPlan
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_chairman_plan_with_env(exec_config, arguments, |name| std::env::var(name).ok())
}

/// Run a process, returning exit code, stdout, and stderr without failing on
/// non-zero exit.
pub fn exec_flex_any<I, S>(
    exec_config: &ExecConfig,
    pkg_bin: &str,
    env_bin: &str,
    arguments: I,
) -> Result<ProcessOutput, std::io::Error>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let plan = proc_flex_plan(exec_config, pkg_bin, env_bin, arguments);
    run_process_plan(&plan)
}

/// Run a process, returning stdout and treating non-zero exit as an error.
pub fn exec_flex<I, S>(
    exec_config: &ExecConfig,
    pkg_bin: &str,
    env_bin: &str,
    arguments: I,
) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let output = exec_flex_any(exec_config, pkg_bin, env_bin, arguments)?;
    match output.exit_code {
        Some(0) => Ok(output.stdout),
        exit_code => Err(ProcessRunError::NonZeroExit {
            exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        }),
    }
}

/// Run upstream `execCli'`.
pub fn exec_cli<I, S>(exec_config: &ExecConfig, arguments: I) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_flex(exec_config, "cardano-cli", "CARDANO_CLI", arguments)
}

/// Run upstream `execCli_`, discarding stdout on success.
pub fn exec_cli_unit<I, S>(exec_config: &ExecConfig, arguments: I) -> Result<(), ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_cli(exec_config, arguments).map(|_| ())
}

/// Run upstream `execCliAny`.
pub fn exec_cli_any<I, S>(
    exec_config: &ExecConfig,
    arguments: I,
) -> Result<ProcessOutput, std::io::Error>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_flex_any(exec_config, "cardano-cli", "CARDANO_CLI", arguments)
}

/// Run upstream `execCreateScriptContext'`.
pub fn exec_create_script_context<I, S>(
    exec_config: &ExecConfig,
    arguments: I,
) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_flex(
        exec_config,
        "create-script-context",
        "CREATE_SCRIPT_CONTEXT",
        arguments,
    )
}

/// Run upstream `execCliStdoutToJson`.
pub fn exec_cli_stdout_to_json<T, I, S>(
    exec_config: &ExecConfig,
    arguments: I,
) -> Result<T, ProcessJsonError>
where
    T: DeserializeOwned,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Ok(serde_json::from_str(&exec_cli(exec_config, arguments)?)?)
}

/// Run upstream `execKESAgentControl`.
pub fn exec_kes_agent_control<I, S>(
    exec_config: &ExecConfig,
    arguments: I,
) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_flex(
        exec_config,
        "kes-agent-control",
        "KES_AGENT_CONTROL",
        arguments,
    )
}

/// Run upstream `execKESAgentControl_`, discarding stdout on success.
pub fn exec_kes_agent_control_unit<I, S>(
    exec_config: &ExecConfig,
    arguments: I,
) -> Result<(), ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    exec_kes_agent_control(exec_config, arguments).map(|_| ())
}

/// Start a planned process without waiting for completion.
pub fn initiate_process(plan: &ProcessPlan) -> Result<Child, std::io::Error> {
    command_from_plan(plan).spawn()
}

fn command_from_plan(plan: &ProcessPlan) -> Command {
    let mut command = Command::new(&plan.executable);
    command.args(plan.args.iter().map(AsRef::<OsStr>::as_ref));
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = &plan.env {
        command.env_clear();
        command.envs(env.iter().map(|(key, value)| (key, value)));
    }
    configure_process_group(&mut command, plan.create_group);
    command
}

fn run_process_plan(plan: &ProcessPlan) -> Result<ProcessOutput, std::io::Error> {
    let mut command = command_from_plan(plan);
    let output = command.output()?;
    Ok(ProcessOutput {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Execute a precomputed [`ProcessPlan`].
pub fn exec_process_plan(plan: &ProcessPlan) -> Result<ProcessOutput, std::io::Error> {
    run_process_plan(plan)
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command, create_group: bool) {
    use std::os::unix::process::CommandExt;
    if create_group {
        command.process_group(0);
    }
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command, create_group: bool) {
    use std::os::windows::process::CommandExt;
    if create_group {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command, _create_group: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filepath::Sprocket;

    #[test]
    fn exec_config_helpers_match_upstream_env_and_cwd_shape() {
        let sprocket = Sprocket {
            base: "/tmp/testnet".to_string(),
            name: "socket/node-1".to_string(),
        };
        let inherited = vec![
            ("PATH".to_string(), "/bin".to_string()),
            ("OTHER".to_string(), "value".to_string()),
        ];

        let config = mk_exec_config("/tmp/testnet", &sprocket, 42, inherited.clone());

        assert_eq!(config.cwd, Some(std::path::PathBuf::from("/tmp/testnet")));
        assert_eq!(
            config.env,
            Some(vec![
                (
                    "CARDANO_NODE_SOCKET_PATH".to_string(),
                    "/tmp/testnet/socket/node-1".to_string(),
                ),
                ("CARDANO_NODE_NETWORK_ID".to_string(), "42".to_string()),
                ("PATH".to_string(), "/bin".to_string()),
                ("OTHER".to_string(), "value".to_string()),
            ])
        );

        let offline = mk_exec_config_offline("/tmp/offline", inherited);
        assert_eq!(offline.cwd, Some(std::path::PathBuf::from("/tmp/offline")));
        assert_eq!(
            offline.env,
            Some(vec![
                ("PATH".to_string(), "/bin".to_string()),
                ("OTHER".to_string(), "value".to_string()),
            ])
        );

        let extended = add_env_vars_to_config(
            &offline,
            vec![("CARDANO_NODE_NETWORK_ID".to_string(), "7".to_string())],
        );
        assert_eq!(
            extended.env,
            Some(vec![
                ("CARDANO_NODE_NETWORK_ID".to_string(), "7".to_string()),
                ("PATH".to_string(), "/bin".to_string()),
                ("OTHER".to_string(), "value".to_string()),
            ])
        );
    }

    #[test]
    fn flexible_process_plans_prefer_env_binary_and_preserve_config() {
        let config = ExecConfig {
            env: Some(vec![("A".to_string(), "B".to_string())]),
            cwd: Some(std::path::PathBuf::from("work")),
        };

        let plan = proc_flex_plan_with_env(
            &config,
            "cardano-cli",
            "CARDANO_CLI",
            ["query", "tip"],
            |name| (name == "CARDANO_CLI").then(|| "bin/cardano-cli".to_string()),
        );

        assert_eq!(plan.executable, "bin/cardano-cli");
        assert_eq!(plan.args, vec!["query", "tip"]);
        assert_eq!(plan.env, config.env);
        assert_eq!(plan.cwd, config.cwd);
        assert!(plan.create_group);

        let chairman =
            proc_chairman_plan_with_env(&ExecConfig::default(), ["--timeout", "10"], |_| None);
        assert_eq!(chairman.executable, "cardano-node-chairman");
        assert_eq!(chairman.args, vec!["run", "--timeout", "10"]);

        let create_script = proc_flex_plan_with_env(
            &ExecConfig::default(),
            "create-script-context",
            "CREATE_SCRIPT_CONTEXT",
            ["--help"],
            |_| None,
        );
        assert_eq!(create_script.executable, "create-script-context");
        assert_eq!(create_script.args, vec!["--help"]);
    }

    #[test]
    fn exec_flex_any_runs_process_and_captures_output() {
        let current_exe = std::env::current_exe().expect("current test binary path");
        let output = exec_flex_any(
            &ExecConfig::default(),
            current_exe.to_string_lossy().as_ref(),
            "IGNORED_TEST_BINARY",
            ["--help"],
        )
        .expect("runs test binary help");

        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.contains("Usage") || output.stdout.contains("USAGE"));
        assert!(output.stderr.is_empty());

        #[cfg(windows)]
        let plan = ProcessPlan {
            executable: "cmd".to_string(),
            args: vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
            env: None,
            cwd: None,
            create_group: true,
        };
        #[cfg(not(windows))]
        let plan = ProcessPlan {
            executable: "true".to_string(),
            args: Vec::new(),
            env: None,
            cwd: None,
            create_group: true,
        };
        let mut child = initiate_process(&plan).expect("spawns planned process");
        assert!(child.wait().expect("waits for planned process").success());
    }
}
