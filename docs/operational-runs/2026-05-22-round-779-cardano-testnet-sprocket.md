---
title: "Round 779 cardano-testnet Sprocket + socket-path helpers"
parent: Reference
---

# Round 779 cardano-testnet Sprocket + socket-path helpers

Date: 2026-05-22

## Scope

Completes the cardano-testnet `filepath.rs` port (upstream
`Testnet/Filepath.hs`).

## What shipped

`crates/tools/cardano-testnet/src/filepath.rs`:

- `Sprocket` — a Unix-domain-socket path split into `base` +
  `name`, with `system_name` (the joined full path, mirror of
  upstream `sprocketSystemName`). Mirror of the `Hedgehog.Extras`
  `Sprocket` type, carried locally because `Filepath.hs`'s
  `makeSprocket` produces it.
- `make_tmp_rel_path` — the temporary path relative to its base,
  mirror of upstream `makeTmpRelPath` (a path not under the base is
  returned unchanged).
- `make_socket_dir` — `<tmp-rel-path>/socket`, mirror of upstream
  `makeSocketDir` (reusing `paths::DEFAULT_SOCKET_DIR`).
- `make_sprocket` — the `Sprocket` for a named node, mirror of
  upstream `makeSprocket`.

`filepath.rs` now ports the full `Testnet/Filepath.hs` surface.

3 unit tests cover the base-stripping, the socket directory, and the
`Sprocket` base/name split with `system_name`.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 62 lib (+3 vs R778's
  59), all green.

## Remaining (cardano-testnet)

The era-aware `Start/Types.hs` option records; the
`Testnet/Components/` query/configuration surfaces; the
process-handle harness types (`TestnetNode`, `TestnetRuntime`,
`TestnetKesAgent`).
