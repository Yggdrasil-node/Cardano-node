#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `dmq-node`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `dmq-node` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R356 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                            | Yggdrasil `.rs`              |
//! |-----------------------------------------------------------|------------------------------|
//! | `src/DMQ/Configuration/CLIOptions.hs` + `Configuration.hs` (CLI shape) | `types.rs`         |
//! | `src/DMQ/Configuration/CLIOptions.hs::parseCLIOptions`    | `parser.rs`                  |
//! | `src/DMQ/Configuration.hs::readConfigurationFile`         | `config_file.rs` (pending)   |
//! | `src/DMQ/Configuration/Topology.hs`                       | `topology.rs` (pending)      |
//! | `src/DMQ/NodeToNode.hs` + `NodeToClient.hs`               | `mux/{ntn,ntc}.rs` (pending; via crates/network) |
//! | `src/DMQ/Diffusion/*`                                     | `diffusion/*.rs` (pending)   |
//! | `src/DMQ/Tracer.hs`                                       | `tracer.rs` (pending)        |
//! | `app/Main.hs`                                             | `main.rs`                    |

use std::io::Write;
use std::process::ExitCode;

pub mod configuration;
pub mod parser;
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
/// R361 lands the parser → resolve → run() chain; the actual
/// Diffusion/NodeKernel/PeerSelection wiring lands at R357+ per the
/// per-tool roadmap. Until then, this returns a sentinel error
/// describing what's missing.
pub fn run(config: &types::Configuration) -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-dmq-node: Diffusion/NodeKernel/PeerSelection wiring \
         not yet implemented (R361 ships argv → Configuration validation; \
         later rounds wire the diffusion layer per the per-tool roadmap). \
         Resolved config: host={}:{}, local_socket={}, config_file={}, \
         topology_file={}, cardano_socket={}, cardano_magic={}, dmq_magic={}.",
        config.host_addr,
        config.port_number,
        config.local_address.as_path().display(),
        config.config_file.display(),
        config.topology_file.display(),
        config.cardano_node_socket.display(),
        config.cardano_network_magic.0,
        config.network_magic.0,
    ))
}
