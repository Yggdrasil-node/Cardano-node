//! Binary entry point for the `bech32` deployable.
//!
//! ## Naming parity
//!
//! **Strict mirror:** bech32/app/Main.hs. The Rust `main.rs` is the
//! canonical 1:1 mirror of upstream `bech32/bech32/app/Main.hs` —
//! the executable entry point that parses command-line arguments
//! (`HRP` + base16 input on stdin), dispatches to the encoder /
//! decoder, and prints the resulting Bech32 / base16 string.
//!
//! R331 ships this skeleton wrapper; R332 lands the optparse-applicative-
//! equivalent CLI parser (clap) and R333 lands the concrete
//! encode/decode dispatch.

fn main() -> eyre::Result<()> {
    yggdrasil_bech32::run()
}
