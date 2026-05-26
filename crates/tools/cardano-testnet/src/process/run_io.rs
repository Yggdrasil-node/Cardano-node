//! RIO-oriented process planning helpers for the `cardano-testnet` harness.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-testnet/src/Testnet/Process/RunIO.hs.

use super::run::{self, ExecConfig, ProcessOutput, ProcessPlan, ProcessRunError};
use crate::filepath::Sprocket;

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Errors returned while resolving RunIO process plans.
#[derive(Debug, thiserror::Error)]
pub enum RunIoError {
    /// Reading the plan file or probing the filesystem failed.
    #[error("process RunIO IO failed: {0}")]
    Io(#[from] std::io::Error),
    /// The Cabal plan JSON could not be parsed.
    #[error("cannot decode plan in {path}: {source}")]
    PlanJson {
        /// Plan file path.
        path: PathBuf,
        /// JSON parse error.
        source: serde_json::Error,
    },
    /// The plan file was not found.
    #[error(
        "could not find plan.json in the path: {path}. Please run \"cabal build {pkg}\" if you are working with sources. Otherwise define {binary_env} and have it point to the executable you want."
    )]
    MissingPlanJson {
        /// Expected plan file path.
        path: PathBuf,
        /// Package/executable name.
        pkg: String,
        /// Environment variable override.
        binary_env: String,
    },
    /// The matching component was present but lacked a bin-file key.
    #[error("missing \"bin-file\" key in plan component for exe:{pkg} in the plan in: {path}")]
    MissingBinFile {
        /// Plan file path.
        path: PathBuf,
        /// Package/executable name.
        pkg: String,
    },
    /// The plan did not contain the requested executable component.
    #[error(
        "cannot find \"component-name\" key with the value \"exe:{pkg}\" in the plan in: {path}"
    )]
    MissingComponent {
        /// Plan file path.
        path: PathBuf,
        /// Package/executable name.
        pkg: String,
    },
}

/// Default process config used by upstream RunIO helpers.
pub fn default_exec_config() -> ExecConfig {
    ExecConfig::default()
}

/// Build upstream RunIO `mkExecConfig` using the real inherited environment.
pub fn mk_exec_config(
    temp_base_abs_path: impl AsRef<Path>,
    sprocket: &Sprocket,
    network_id: i64,
) -> ExecConfig {
    run::mk_exec_config(
        temp_base_abs_path,
        sprocket,
        network_id,
        run::current_environment(),
    )
}

/// Resolve `planJsonFile` with explicit inputs for deterministic tests.
pub fn plan_json_file_from_env(
    cabal_builddir: Option<&str>,
    current_dir: impl AsRef<Path>,
) -> Result<PathBuf, std::io::Error> {
    match cabal_builddir {
        Some(build_dir) => Ok(PathBuf::from("..").join(build_dir).join("cache/plan.json")),
        None => find_default_plan_json_file_from(current_dir),
    }
}

/// Find the nearest `dist-newstyle/cache/plan.json` walking upward.
pub fn find_default_plan_json_file_from(
    current_dir: impl AsRef<Path>,
) -> Result<PathBuf, std::io::Error> {
    let mut dir = current_dir.as_ref().to_path_buf();
    loop {
        let candidate = dir.join("dist-newstyle/cache/plan.json");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Ok(PathBuf::from("dist-newstyle/cache/plan.json"));
        }
    }
}

/// Add the platform executable suffix exactly like upstream `addExeSuffix`.
pub fn add_exe_suffix(path: &str) -> String {
    if path.ends_with(".exe") {
        path.to_string()
    } else {
        format!("{path}{EXE_SUFFIX}")
    }
}

#[cfg(windows)]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(windows))]
const EXE_SUFFIX: &str = "";

/// Resolve a binary from an environment override or a Cabal plan JSON file.
pub fn bin_flex_with_plan<F>(
    pkg: &str,
    binary_env: &str,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<String, RunIoError>
where
    F: Fn(&str) -> Option<String>,
{
    match env_lookup(binary_env) {
        Some(env_bin) => Ok(env_bin),
        None => bin_dist_from_plan_json(pkg, binary_env, plan_json_file),
    }
}

/// Resolve a binary using the real environment and default plan discovery.
pub fn bin_flex(pkg: &str, binary_env: &str) -> Result<String, RunIoError> {
    let plan_json_file = plan_json_file_from_env(
        std::env::var("CABAL_BUILDDIR").ok().as_deref(),
        std::env::current_dir()?,
    )?;
    bin_flex_with_plan(
        pkg,
        binary_env,
        |name| std::env::var(name).ok(),
        plan_json_file,
    )
}

/// Consult a Cabal `plan.json` and return the executable path for `pkg`.
pub fn bin_dist_from_plan_json(
    pkg: &str,
    binary_env: &str,
    plan_json_file: impl AsRef<Path>,
) -> Result<String, RunIoError> {
    let path = plan_json_file.as_ref();
    if !path.exists() {
        return Err(RunIoError::MissingPlanJson {
            path: path.to_path_buf(),
            pkg: pkg.to_string(),
            binary_env: binary_env.to_string(),
        });
    }

    let raw = std::fs::read_to_string(path)?;
    let plan: Value = serde_json::from_str(&raw).map_err(|source| RunIoError::PlanJson {
        path: path.to_path_buf(),
        source,
    })?;
    let needle = format!("exe:{pkg}");
    let components = plan
        .get("install-plan")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let component =
        find_component(components, &needle).ok_or_else(|| RunIoError::MissingComponent {
            path: path.to_path_buf(),
            pkg: pkg.to_string(),
        })?;
    let bin_file = component
        .get("bin-file")
        .and_then(Value::as_str)
        .ok_or_else(|| RunIoError::MissingBinFile {
            path: path.to_path_buf(),
            pkg: pkg.to_string(),
        })?;

    Ok(add_exe_suffix(bin_file))
}

fn find_component<'a>(components: &'a [Value], needle: &str) -> Option<&'a Value> {
    for component in components {
        if component
            .get("component-name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == needle)
        {
            return Some(component);
        }
        if let Some(nested) = component.get("components").and_then(Value::as_array)
            && let Some(found) = find_component(nested, needle)
        {
            return Some(found);
        }
    }
    None
}

/// Build upstream `procFlex'` with explicit env lookup and plan file.
pub fn proc_flex_with_plan<I, S, F>(
    exec_config: &ExecConfig,
    pkg: &str,
    binary_env: &str,
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<ProcessPlan, RunIoError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    let executable = bin_flex_with_plan(pkg, binary_env, env_lookup, plan_json_file)?;
    Ok(ProcessPlan {
        executable,
        args: arguments
            .into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect(),
        env: exec_config.env.clone(),
        cwd: exec_config.cwd.clone(),
        create_group: true,
    })
}

/// Run upstream `execFlexAny'` with explicit env lookup and plan file.
pub fn exec_flex_any_with_plan<I, S, F>(
    exec_config: &ExecConfig,
    pkg: &str,
    binary_env: &str,
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<ProcessOutput, RunIoError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    let plan = proc_flex_with_plan(
        exec_config,
        pkg,
        binary_env,
        arguments,
        env_lookup,
        plan_json_file,
    )?;
    lift_io_annotated(run::exec_process_plan(&plan))
}

/// Run upstream `execFlex'` with explicit env lookup and plan file.
pub fn exec_flex_with_plan<I, S, F>(
    exec_config: &ExecConfig,
    pkg: &str,
    binary_env: &str,
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    let output = exec_flex_any_with_plan(
        exec_config,
        pkg,
        binary_env,
        arguments,
        env_lookup,
        plan_json_file,
    )
    .map_err(|err| match err {
        RunIoError::Io(io) => ProcessRunError::Io(io),
        other => ProcessRunError::Io(std::io::Error::other(other)),
    })?;
    match output.exit_code {
        Some(0) => Ok(output.stdout),
        exit_code => Err(ProcessRunError::NonZeroExit {
            exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        }),
    }
}

/// Run upstream `execCli'` with explicit env lookup and plan file.
pub fn exec_cli_with_plan<I, S, F>(
    exec_config: &ExecConfig,
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<String, ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    exec_flex_with_plan(
        exec_config,
        "cardano-cli",
        "CARDANO_CLI",
        arguments,
        env_lookup,
        plan_json_file,
    )
}

/// Run upstream `execCli_` with explicit env lookup and plan file.
pub fn exec_cli_unit_with_plan<I, S, F>(
    exec_config: &ExecConfig,
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<(), ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    exec_cli_with_plan(exec_config, arguments, env_lookup, plan_json_file).map(|_| ())
}

/// Run upstream `execKesAgentControl_` with explicit env lookup and plan file.
pub fn exec_kes_agent_control_unit_with_plan<I, S, F>(
    arguments: I,
    env_lookup: F,
    plan_json_file: impl AsRef<Path>,
) -> Result<(), ProcessRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: Fn(&str) -> Option<String>,
{
    exec_flex_with_plan(
        &default_exec_config(),
        "kes-agent-control",
        "KES_AGENT_CONTROL",
        arguments,
        env_lookup,
        plan_json_file,
    )
    .map(|_| ())
}

/// Build upstream `procFlex` using the real environment and default plan discovery.
pub fn proc_flex<I, S>(
    exec_config: &ExecConfig,
    pkg: &str,
    binary_env: &str,
    arguments: I,
) -> Result<ProcessPlan, RunIoError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let plan_json_file = plan_json_file_from_env(
        std::env::var("CABAL_BUILDDIR").ok().as_deref(),
        std::env::current_dir()?,
    )?;
    proc_flex_with_plan(
        exec_config,
        pkg,
        binary_env,
        arguments,
        |name| std::env::var(name).ok(),
        plan_json_file,
    )
}

/// Build upstream `procNode`.
pub fn proc_node<I, S>(arguments: I) -> Result<ProcessPlan, RunIoError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex(
        &default_exec_config(),
        "cardano-node",
        "CARDANO_NODE",
        arguments,
    )
}

/// Build upstream `procKesAgent`.
pub fn proc_kes_agent<I, S>(arguments: I) -> Result<ProcessPlan, RunIoError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    proc_flex(&default_exec_config(), "kes-agent", "KES_AGENT", arguments)
}

/// Annotate an IO result with the RunIO error carrier.
pub fn lift_io_annotated<T>(result: Result<T, std::io::Error>) -> Result<T, RunIoError> {
    result.map_err(RunIoError::Io)
}
