# xtask — workspace developer subcommands

## Scope

Wave 9 PR 27. Lives at the workspace root (not under `crates/`) so
the path-based dep is unambiguous and the binary name `xtask` is
what `.cargo/config.toml`'s `cargo xtask = "run -p xtask --release --"`
resolves.

Current subcommands:

- `parity-add --file <path>` — appends a row to
  `docs/strict-mirror-audit.tsv` for a newly-added synthesis Rust
  file. Validates the file actually carries the
  `## Naming parity ... **Strict mirror:** none.` stanza before
  appending, then re-runs `scripts/check-strict-mirror.py
  --fail-on-violation` to confirm the audit stays clean.
- `parity-all` — runs all three Python parity validators in
  sequence (strict-mirror, parity-matrix, fixture-manifest).

## Rules — Non-Negotiable

- **Minimal dependencies.** Std + clap + eyre + serde + serde_json
  only. xtask is invoked frequently during development; pulling in
  a fat transitive graph slows every `cargo xtask` run.
- **Idempotent.** Subcommands MUST be safe to re-run. `parity-add`
  refuses to duplicate an existing row.
- **No code generation that bypasses gates.** xtask scaffolds rows
  that match the existing audit format; it does NOT silence
  validator failures or generate parity-matrix entries without
  human review.

## Naming parity

Synthesis crate. The `main.rs` docstring carries the
`## Naming parity` stanza.

## R-arc tracking

Wave 9 PR 27.
