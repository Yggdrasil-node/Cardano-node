//! Binary entry point for the `kes-agent-control` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. R335-pattern minimal binary wrapper that
//! delegates to `yggdrasil_kes_agent_control::run_main()`. The upstream binary's
//! entry point is mirrored at the lib level via the per-tool parser
//! and run-loop dispatch.

fn main() -> std::process::ExitCode {
    yggdrasil_kes_agent_control::run_main()
}
