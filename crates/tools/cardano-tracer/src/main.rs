//! Binary entry point for the `cardano-tracer` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. R335-pattern minimal binary wrapper that
//! delegates to `yggdrasil_cardano_tracer::run_main()`. The upstream binary's
//! entry point is mirrored at the lib level via the per-tool parser
//! and run-loop dispatch.

fn main() -> std::process::ExitCode {
    yggdrasil_cardano_tracer::run_main()
}
