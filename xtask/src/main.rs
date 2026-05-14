//! `cargo xtask` — workspace developer subcommands.
//!
//! Invoked via the `.cargo/config.toml` alias added in Wave 1 PR 3:
//! `cargo xtask = "run -p xtask --release --"`. The crate intentionally
//! stays std-only-plus-clap-plus-serde so a fresh `cargo xtask` run
//! doesn't pull in fresh transitive deps unrelated to the subcommand
//! being invoked.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate that
//! houses parity-matrix scaffolding + workspace-level dev tasks.
//! Upstream has no equivalent — `cardano-node` uses cabal directly.

#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use eyre::{Context, Result, bail, ensure};

#[derive(Parser)]
#[command(name = "xtask", author, version, about = "Yggdrasil workspace developer subcommands.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Append a row to `docs/strict-mirror-audit.tsv` so a newly-
    /// added Rust file (carrying the `## Naming parity` docstring
    /// stanza ending in `**Strict mirror:** none.`) passes the
    /// strict-mirror CI gate without manual TSV editing.
    ParityAdd {
        /// Workspace-relative path to the new production `.rs` file
        /// (e.g. `crates/node/sync/src/lib.rs`).
        #[arg(long)]
        file: PathBuf,
        /// Optional one-line note for the audit TSV's last column.
        #[arg(long, default_value = "")]
        note: String,
    },

    /// Run the four Python parity validators in sequence
    /// (strict-mirror, parity-matrix, fixture-manifest). Useful as
    /// a pre-push convenience when `just parity-all` isn't on PATH.
    ParityAll,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::ParityAdd { file, note } => parity_add(&file, &note),
        Cmd::ParityAll => parity_all(),
    }
}

/// Append a row to `docs/strict-mirror-audit.tsv`.
///
/// The TSV columns (verified against the existing rows in the
/// allowlist):
///   1. rust_path
///   2. candidates             — basename-derivation key alternatives,
///      or `lib` for crate-root files
///   3. matched_candidate      — `-` when no upstream match
///   4. upstream_hits          — `-` when synthesis
///   5. docstring_parity       — `yes(strict-none)` for synthesis
///   6. initial_verdict        — `no_candidate_match` for synthesis
///   7. final_verdict          — `(c) NO_MIRROR_NEEDS_DOCSTRING ...`
///   8. notes
fn parity_add(file: &Path, note: &str) -> Result<()> {
    let workspace_root = workspace_root()?;
    let audit_path = workspace_root.join("docs/strict-mirror-audit.tsv");

    let file_rel = file
        .strip_prefix(&workspace_root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");

    // Sanity-check that the file actually carries a `## Naming parity`
    // synthesis docstring; xtask should not be used to silence the
    // gate on files that are missing the stanza.
    let abs = workspace_root.join(&file_rel);
    let contents = std::fs::read_to_string(&abs)
        .with_context(|| format!("reading {}", abs.display()))?;
    ensure!(
        contents.contains("## Naming parity"),
        "{file_rel}: missing `## Naming parity` docstring stanza; \
         add the synthesis block before running `cargo xtask parity-add`",
    );
    ensure!(
        contents.contains("**Strict mirror:** none."),
        "{file_rel}: `## Naming parity` block must declare \
         `**Strict mirror:** none.` (this xtask scaffolds rows for \
         synthesis-only files; for upstream-mirroring files no row \
         is needed because the basename match handles them)",
    );

    // Derive the candidates token from the file's basename. The
    // existing allowlist convention is `<stem>|lib` for crate
    // roots and `<stem>|<aliases>` for ordinary files; for the
    // simple lib.rs case we emit `<crate>|lib` if the file is at
    // `crates/.../src/lib.rs` else the bare stem.
    let stem = file_rel
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".rs"))
        .unwrap_or("lib");
    let candidates_token = if stem == "lib" {
        // For a crate-root lib.rs, pick the crate's directory name
        // so the candidate column lines up with the parity-audit
        // basename-derivation convention.
        let crate_name = file_rel
            .split('/')
            .rev()
            .nth(2)
            .unwrap_or("lib")
            .trim_start_matches("yggdrasil-")
            .replace('-', "_");
        format!("{crate_name}|lib")
    } else {
        // Common abbreviated alias on the right of the pipe — the
        // existing audit rows always emit one; just use the stem
        // here. Operators can hand-edit if a better alias exists.
        format!("{stem}|{stem}")
    };

    let note_field = if note.is_empty() {
        "synthesis crate; added via `cargo xtask parity-add`. \
         Update note with the specific scope when reviewing."
            .to_string()
    } else {
        note.to_string()
    };

    let row = format!(
        "{file_rel}\t{candidates}\t-\t-\tyes(strict-none)\tno_candidate_match\t{verdict}\t{note}\n",
        file_rel = file_rel,
        candidates = candidates_token,
        verdict = "(c) NO_MIRROR_NEEDS_DOCSTRING (cargo xtask parity-add)",
        note = note_field,
    );

    let existing = std::fs::read_to_string(&audit_path)
        .with_context(|| format!("reading {}", audit_path.display()))?;
    let new_prefix = format!("{file_rel}\t");
    ensure!(
        !existing.lines().any(|line| line.starts_with(&new_prefix)),
        "{file_rel} already has a row in docs/strict-mirror-audit.tsv \
         — refusing to duplicate; hand-edit the existing row instead",
    );

    // Append, then run the validator to confirm the TSV stays clean.
    // We deliberately append (rather than insert-in-sorted-order)
    // because the existing TSV is not strictly sorted and the
    // validator is order-insensitive.
    std::fs::write(
        &audit_path,
        format!("{existing}{row}"),
    )?;
    println!("[xtask parity-add] appended row for {file_rel}");

    validate_strict_mirror(&workspace_root)
}

fn parity_all() -> Result<()> {
    let workspace_root = workspace_root()?;
    let scripts = [
        ("strict-mirror", "scripts/check-strict-mirror.py", &["--fail-on-violation"][..]),
        ("parity-matrix", "scripts/check-parity-matrix.py", &[][..]),
        ("fixture-manifest", "scripts/check-fixture-manifest.py", &[][..]),
    ];
    for (label, script, args) in scripts {
        let script_path = workspace_root.join(script);
        if !script_path.is_file() {
            bail!("missing validator: {}", script_path.display());
        }
        let status = Command::new("python3")
            .arg(&script_path)
            .args(args)
            .current_dir(&workspace_root)
            .status()
            .with_context(|| format!("invoking python3 {}", script_path.display()))?;
        ensure!(status.success(), "{label} validator exited with {status}");
        println!("[xtask parity-all] {label}: clean");
    }
    Ok(())
}

fn validate_strict_mirror(workspace_root: &Path) -> Result<()> {
    let script = workspace_root.join("scripts/check-strict-mirror.py");
    let status = Command::new("python3")
        .arg(&script)
        .arg("--fail-on-violation")
        .current_dir(workspace_root)
        .status()
        .with_context(|| format!("invoking python3 {}", script.display()))?;
    ensure!(
        status.success(),
        "strict-mirror validator failed after parity-add — revert the \
         appended row and either rename the file to match an upstream \
         basename or rewrite the `## Naming parity` block",
    );
    Ok(())
}

/// Walk parent directories from CARGO_MANIFEST_DIR to find the
/// workspace root (the directory containing `Cargo.lock`).
fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::current_dir())?;
    let mut dir = manifest_dir.as_path();
    loop {
        if dir.join("Cargo.lock").is_file() {
            return Ok(dir.to_path_buf());
        }
        dir = dir
            .parent()
            .ok_or_else(|| eyre::eyre!("could not locate workspace root (Cargo.lock) from {}", manifest_dir.display()))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_root_locates_cargo_lock() {
        let root = workspace_root().expect("workspace root");
        assert!(root.join("Cargo.lock").is_file());
        assert!(root.join("Cargo.toml").is_file());
    }
}
