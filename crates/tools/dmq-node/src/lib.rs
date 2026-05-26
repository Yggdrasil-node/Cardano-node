#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `dmq-node`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell for the `dmq-node`
//! sister-tool crate plus local Rust module surfaces that mirror the vendored
//! DMQ protocol, inbound-governor, NodeKernel, and mux-bundle structure.
//!
//! Layout mapping (post-R816):
//!
//! | Upstream `.hs`                                            | Yggdrasil `.rs`              |
//! |-----------------------------------------------------------|------------------------------|
//! | `src/DMQ/Configuration/CLIOptions.hs` + `Configuration.hs` (CLI shape) | `types.rs`         |
//! | `src/DMQ/Configuration/CLIOptions.hs::parseCLIOptions`    | `parser.rs`                  |
//! | `src/DMQ/Configuration.hs::readConfigurationFile`         | `configuration.rs`           |
//! | `src/DMQ/Configuration/Topology.hs`                       | `topology.rs`                |
//! | `src/DMQ/NodeToNode.hs` + `NodeToClient.hs`               | `node_to_node.rs` + `node_to_client.rs` |
//! | `src/DMQ/Protocol/*`                                      | `protocol/*.rs`              |
//! | `src/DMQ/Diffusion/*` + `NodeKernel.hs`                   | `diffusion.rs`, `registry.rs`, `peer_sharing.rs`, `delta_q.rs`, `keep_alive.rs` |
//! | `src/DMQ/Tracer.hs`                                       | `tracer.rs` (pending)        |
//! | `app/Main.hs`                                             | `main.rs`                    |

use std::io::Write;
use std::process::ExitCode;

pub mod configuration;
pub mod delta_q;
pub mod diffusion;
pub mod inbound_v2;
pub mod keep_alive;
pub mod mempool;
pub mod node_to_client;
pub mod node_to_node;
pub mod parser;
pub mod peer_sharing;
pub mod policy;
pub mod protocol;
pub mod registry;
pub mod sig_submission_v2;
pub mod status;
pub mod topology;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R361 wires the typed parser surface end-to-end:
/// - `--help` / `-h` → emits the upstream-byte-equivalent HELP_TEXT and
///   exits 0.
/// - `--version` / `-v` (in-grammar switch — flips
///   `args.show_version`) → emits VERSION_TEXT and exits 0.
/// - parse error → emits the error to stderr and exits non-zero.
/// - parse success without `--version` → resolves the partial config
///   to a full [`types::Configuration`] and hands off to [`run`].
pub fn run_main() -> ExitCode {
    // Wave 8 PR 23: initialise the workspace tracing subscriber so
    // this binary emits Haskell-Katip JSON logs identical to
    // yggdrasil-node. Idempotent: a second call is a no-op.
    let _ = yggdrasil_telemetry::init_subscriber(&yggdrasil_telemetry::TracingConfig::default());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parser::parse_args(&argv) {
        Ok(args) => args,
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

    if args.show_version == Some(true) {
        let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
        return ExitCode::SUCCESS;
    }

    // R369: if --configuration-file was supplied, load it and merge
    // CLI-derived overrides on top before resolving to the fully-
    // applied Configuration.
    let config = match configuration::resolve_configuration(args) {
        Ok(c) => c,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            return ExitCode::FAILURE;
        }
    };
    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R361/R369 wire the parser + configuration chain. R717-R816 ship
/// the DMQ protocol, inbound governor, NodeKernel helper records, and
/// NtN/NtC mux bundles. The remaining concrete behavior is the final
/// `run()` event loop that binds sockets, performs handshakes, starts
/// mux drivers, and supervises the protocol tasks; until then, this
/// returns a sentinel error describing the missing assembly.
pub fn run(config: &types::Configuration) -> eyre::Result<()> {
    Err(RunError::DiffusionWiringDeferred {
        host: format!("{}:{}", config.host_addr, config.port_number),
        local_socket: config.local_address.as_path().display().to_string(),
        config_file: config.config_file.display().to_string(),
        topology_file: config.topology_file.display().to_string(),
        cardano_socket: config.cardano_node_socket.display().to_string(),
        cardano_magic: config.cardano_network_magic.0,
        dmq_magic: config.network_magic.0,
    }
    .into())
}

/// Errors from the dmq-node `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Diffusion / NodeKernel / PeerSelection wiring is deferred.
    /// Mirror of upstream's DMQ.Node.{Diffusion, Run, NodeKernel}
    /// — the R717-R816 component arc is shipped, but the final
    /// event-loop assembly remains pending.
    #[error(
        "yggdrasil-dmq-node: Diffusion / NodeKernel / PeerSelection wiring deferred (see \
         crates/tools/dmq-node/src/status.rs::diffusion_wiring_status for the full deferral \
         rationale). Resolved CLI: host={host}, local_socket={local_socket}, \
         config_file={config_file}, topology_file={topology_file}, \
         cardano_socket={cardano_socket}, cardano_magic={cardano_magic}, \
         dmq_magic={dmq_magic}."
    )]
    DiffusionWiringDeferred {
        /// Host:port the dmq-node would bind to.
        host: String,
        /// Local Unix socket path.
        local_socket: String,
        /// Operator-supplied config-file path.
        config_file: String,
        /// Operator-supplied topology-file path.
        topology_file: String,
        /// Cardano-node socket path for upstream connection.
        cardano_socket: String,
        /// Cardano-network magic.
        cardano_magic: u32,
        /// DMQ-specific network magic.
        dmq_magic: u32,
    },
}
