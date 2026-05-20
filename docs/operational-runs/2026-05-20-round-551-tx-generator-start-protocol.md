# Round 551 - tx-generator StartProtocol env wiring

Date: 2026-05-20
Area: sister-tools / `crates/tools/tx-generator`

## Summary

- Wired upstream `Benchmarking.Script.Action.startProtocol` side
  effects in the Rust action interpreter.
- `StartProtocol` now loads the node config with JSON/YAML fallback via
  `yggdrasil-node-config`, rejects non-Cardano protocol configs, sets
  protocol and genesis carriers, derives upstream-shaped
  `Testnet NetworkMagic` env state, and initializes benchmark tracers.
- `json_highlevel FILE` now advances beyond the old
  `mkConsensusProtocol` / genesis sentinel and reaches the next
  concrete script/runtime boundary.

## Remaining Boundaries

- The protocol/genesis values are config-derived Rust carriers until the
  remaining consensus/genesis transaction slices consume full runtime
  structures.
- `SecureGenesis`, Plutus pre-execution / script-spend integrity,
  Benchmark submission, exact `DumpToFile`, `RoundRobin`, `OneOf`, and
  `selftest` remain open tx-generator slices.

## Validation

```text
cargo fmt --all -- --check
cargo test -p yggdrasil-tx-generator --lib script::action
cargo test -p yggdrasil-tx-generator --lib
cargo clippy -p yggdrasil-tx-generator --all-targets
python scripts/check-parity-matrix.py
python scripts/check-strict-mirror.py --fail-on-violation
python scripts/check-stale-placement.py
python .claude/scripts/filetree.py check
cargo check-all
cargo lint
cargo test-all
```
