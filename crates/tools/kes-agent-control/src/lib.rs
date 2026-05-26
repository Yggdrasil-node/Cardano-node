#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `kes-agent-control`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `kes-agent-control` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R355 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `cli/ControlMain.hs` section                | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `data CommonOptions` + per-subcommand option types   | `types.rs`                   |
//! | `pCommonOptions` + `pProgramOptions` + per-subcommand parsers | `parser.rs`         |
//! | `humanFriendlyControlTracer` / status reporting      | `tracer.rs` (pending)        |
//! | `runGenKey` / `runQueryKey` / `runDropKey` / etc     | per-subcommand `.rs` (pending)|
//! | `Cardano.KESAgent.Processes.ControlClient` socket    | `control_client.rs` (pending) |
//! | `main`                                               | `main.rs`                    |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod status;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R362 wires the typed parser dispatcher end-to-end; R370 layers
/// in environment-variable threading: the CLI-derived
/// [`types::ProgramOptions`] inherits env-derived defaults from
/// `KES_AGENT_CONTROL_PATH` / `KES_AGENT_CONTROL_RETRY_INTERVAL` /
/// `KES_AGENT_CONTROL_RETRY_ATTEMPTS` before being handed off to
/// [`run`]. `--help` and `--version` short-circuit with byte-
/// equivalent upstream output.
///
/// Resolution order (mirrors upstream's WithCommonOptions threading):
/// 1. CLI-derived options (highest priority).
/// 2. Environment-derived options.
/// 3. [`types::CommonOptions::defaults`] (lowest priority — fills
///    in any field still unset).
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cli_options = match parser::parse_args(&argv) {
        Ok(opts) => opts,
        Err(parser::ParseError::HelpRequested) => {
            let _ = std::io::stdout().write_all(parser::HELP_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(parser::ParseError::VersionRequested) => {
            let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            return ExitCode::FAILURE;
        }
    };

    // R370: layer env-derived options + defaults under the CLI overrides.
    let env_options = types::CommonOptions::from_env();
    let resolved_common = env_options.merge(types::CommonOptions::defaults());
    let program_options = cli_options.with_common_options(resolved_common);

    match run(&program_options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R362 wires argv → [`types::ProgramOptions`] dispatch. The actual
/// per-subcommand ControlClient socket I/O is deferred to a
/// follow-on round gated on the R444+ kes-agent daemon/socket work:
/// the server-side socket protocol must be byte-equivalent before
/// ControlClient can be verified safely.
///
/// R440 surfaces the deferral via the [`RunError`] enum +
/// [`status::control_client_status`] introspection helper rather
/// than a raw `eyre::eyre!` string. Callers can match on the
/// specific subcommand for programmatic dispatch.
pub fn run(program_options: &types::ProgramOptions) -> eyre::Result<()> {
    let subcommand = match program_options {
        types::ProgramOptions::RunGenKey(_) => status::Subcommand::GenStagedKey,
        types::ProgramOptions::RunQueryKey(_) => status::Subcommand::ExportStagedVkey,
        types::ProgramOptions::RunDropStagedKey(_) => status::Subcommand::DropStagedKey,
        types::ProgramOptions::RunInstallKey(_) => status::Subcommand::InstallKey,
        types::ProgramOptions::RunDropKey(_) => status::Subcommand::DropKey,
        types::ProgramOptions::RunGetInfo(_) => status::Subcommand::Info,
    };
    Err(RunError::SubcommandSocketIoDeferred { subcommand }.into())
}

/// Errors from the kes-agent-control `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// ControlClient socket I/O for the selected subcommand is
    /// deferred. Mirror of upstream
    /// `Cardano.KESAgent.Processes.ControlClient` — connects to
    /// a running kes-agent daemon over its Unix-domain socket and
    /// drives the per-subcommand runner. Yggdrasil's port is
    /// gated on the R444+ kes-agent daemon/socket follow-on; the
    /// server-side socket protocol must be byte-equivalent before
    /// ControlClient can be verified safely.
    #[error(
        "yggdrasil-kes-agent-control: ControlClient socket I/O for `{subcommand}' deferred — \
         gated on the R444+ kes-agent daemon/socket follow-on (see crates/tools/kes-agent-control/src/status.rs::\
         control_client_status for the full deferral rationale)."
    )]
    SubcommandSocketIoDeferred {
        /// The subcommand the operator invoked.
        subcommand: status::Subcommand,
    },
}
