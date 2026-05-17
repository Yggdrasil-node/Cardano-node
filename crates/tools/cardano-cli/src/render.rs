//! cardano-cli output rendering.
//!
//! Mirrors upstream `Cardano.CLI.Render` — the helpers that format
//! `ClientCommand` results (query responses, key files, transaction
//! envelopes) into stdout-friendly bytes. Most upstream renderers
//! use `Aeson` for JSON + `cardano-api` for text-envelope CBOR; the
//! Yggdrasil port uses `serde_json` + the `text-envelope` codec from
//! `crates/node/block-producer/src/lib.rs`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Render.hs`.
//! R289 ships the top-level `render_*` API surface as stubs. The full
//! renderer tree (per-output-type formatters) lands in R290–R295
//! alongside the per-cluster runners that consume them.

use eyre::Result;
use serde_json::Value;

/// Render a JSON value to stdout, pretty-printed.
///
/// Mirrors upstream `renderJson` from `Cardano.CLI.Render`.
pub fn render_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Render a JSON value to a string, pretty-printed.
///
/// Used by tests and integration runners that need to capture the
/// rendered output for byte-equivalence checks against upstream.
pub fn render_json_string(value: &Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

/// Render a plain-text line to stdout.
///
/// Mirrors upstream `renderText` from `Cardano.CLI.Render`.
pub fn render_text(line: &str) {
    println!("{line}");
}
