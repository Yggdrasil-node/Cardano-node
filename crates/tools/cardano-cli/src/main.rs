//! Standalone `yggdrasil-cardano-cli` binary entry point.
//!
//! Mirrors upstream `cardano-cli/cardano-cli/app/cardano-cli.hs`'s
//! `main = …` body: parse argv via the in-crate parser, run the
//! resulting `Command` via the in-crate run dispatcher, exit with
//! status 0 on success / 1 on any error.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/app/cardano-cli.hs`.
//! R503 (May 2026) ships this binary as the standalone target so
//! `cargo install --path crates/tools/cardano-cli` produces a
//! `yggdrasil-cardano-cli` binary. Today's binary supports the
//! `version` subcommand operationally; `show-upstream-config` and
//! `query-tip` emit structured deferral messages that point
//! operators at the node binary's wrapper (see `run::run_command`
//! for the full deferral rationale per arm). Full subcommand
//! surface migration tracked in `docs/TECH-DEBT.md` under the
//! "yggdrasil-cardano-cli library-only crate has no `[[bin]]`"
//! entry (now operator-visible binary; only the run-time
//! coverage matrix remains to grow).

use std::process::ExitCode;

use yggdrasil_cardano_cli::parser::ParseError;

fn main() -> ExitCode {
    let argv = std::env::args_os();
    let cmd = match yggdrasil_cardano_cli::parser::parse_command(argv) {
        Ok(cmd) => cmd,
        Err(ParseError::Clap(err)) => {
            // clap surfaces `--help` / `--version` through Err with a
            // DisplayHelp / DisplayVersion kind. Conventional handling:
            // print on stdout and exit 0; print other clap errors on
            // stderr and exit 2 (matches clap's own `exit` shorthand
            // and operator muscle-memory from upstream cardano-cli).
            use clap::error::ErrorKind;
            match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    print!("{err}");
                    return ExitCode::from(0);
                }
                _ => {
                    eprint!("{err}");
                    return ExitCode::from(2);
                }
            }
        }
    };
    match yggdrasil_cardano_cli::run::run_command(cmd) {
        Ok(()) => ExitCode::from(0),
        Err(err) => {
            eprintln!("yggdrasil-cardano-cli: {err}");
            ExitCode::from(1)
        }
    }
}
