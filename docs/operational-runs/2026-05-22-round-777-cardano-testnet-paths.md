---
title: "Round 777 cardano-testnet path conventions (paths.rs)"
parent: Reference
---

# Round 777 cardano-testnet path conventions (paths.rs)

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — ports the shared testnet
output-directory path conventions.

## What shipped

`crates/tools/cardano-testnet/src/paths.rs` — new file, port of
upstream `cardano-node/src/Cardano/Node/Testnet/Paths.hs` (the
`Cardano.Node.Testnet.Paths` module, shared by `cardano-testnet` and
consumers of generated testnet configs; placed in the cardano-testnet
crate as its primary consumer):

- `default_node_name` (`node<n>`), `default_node_data_dir`
  (`node-data/node<n>`).
- `default_utxo_key_dir` (`utxo-keys/utxo<n>`) plus the
  `utxo.skey` / `utxo.vkey` / `utxo.addr` paths under it.
- `DEFAULT_SOCKET_DIR` (`socket`), `DEFAULT_SOCKET_NAME` (`sock`),
  `default_socket_path` (`socket/node<n>/sock`).
- `DEFAULT_CONFIG_FILE` (`configuration.yaml`), `default_port_file`
  (`<node-data-dir>/port`).

All pure path-building — Haskell `</>` joins map to `PathBuf::join`.
The `<n>` node index is `i32`, matching `types::NodeId`.
`lib.rs` gains `pub mod paths;`.

4 unit tests pin every path/name against the upstream forms.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 56 lib (+4 vs R776's
  52), all green.

## Remaining (cardano-testnet)

`Testnet/Filepath.hs` (the `TmpAbsolutePath` + `Sprocket` runtime
path helpers); the era-aware `Start/Types.hs` option records; the
`Testnet/Components/` query/configuration surfaces; the
process-handle harness types.
