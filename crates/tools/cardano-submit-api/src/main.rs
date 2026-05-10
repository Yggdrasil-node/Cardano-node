//! Binary entry point for the `cardano-submit-api` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/app/Main.hs. The Rust
//! `main.rs` is the canonical 1:1 mirror of upstream
//! `cardano-submit-api/app/Main.hs` — the executable entry point
//! that parses command-line arguments, loads the config file, and
//! starts the web + tracing + metrics servers.
//!
//! R335 ships this skeleton wrapper with byte-equivalent
//! `--help` / `--version` handling. R336-R342 land the concrete
//! REST + Web + Metrics dispatch.

fn main() -> std::process::ExitCode {
    yggdrasil_cardano_submit_api::run_main()
}
