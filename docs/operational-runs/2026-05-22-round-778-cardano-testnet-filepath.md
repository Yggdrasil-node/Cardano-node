---
title: "Round 778 cardano-testnet temporary-path helpers (filepath.rs)"
parent: Reference
---

# Round 778 cardano-testnet temporary-path helpers (filepath.rs)

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — opens `filepath.rs`, the port of
upstream `Testnet/Filepath.hs`.

## What shipped

`crates/tools/cardano-testnet/src/filepath.rs` — new file
(`filepath.rs` basename-mirrors `Filepath.hs`):

- `TmpAbsolutePath` — a runtime temporary (output) directory path,
  mirror of upstream `newtype TmpAbsolutePath`. Upstream's `IsString`
  / `Display` instances are reproduced by `From<&str>` /
  `From<String>` and `std::fmt::Display`.
- `make_tmp_base_abs_path` — the parent directory with a trailing
  separator, mirror of upstream
  `makeTmpBaseAbsPath = addTrailingPathSeparator . takeDirectory`.
- `make_log_dir` — `<tmp>/logs/`, mirror of upstream `makeLogDir`.

The helpers return `String` (Haskell `FilePath`) so the
trailing-separator forms survive — `PathBuf` normalises them away.
`lib.rs` gains `pub mod filepath;`.

3 unit tests cover construction/`Display`, the base-path parent +
trailing slash, and `make_log_dir` (including the no-double-slash
case for a trailing-slash input).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt; `filepath.rs` basename-mirrors
  `Filepath.hs`).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 59 lib (+3 vs R777's
  56), all green.

## Remaining (cardano-testnet `Filepath.hs`)

The `Sprocket`-valued helpers — `makeTmpRelPath`, `makeSocketDir`,
`makeSprocket` — and the `Sprocket` socket-path type land with the
testnet-harness rounds.
