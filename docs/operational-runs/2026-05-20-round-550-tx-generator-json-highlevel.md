# Round 550 - tx-generator json_highlevel execution

Date: 2026-05-20
Area: sister-tools / `crates/tools/tx-generator`

## Summary

- Wired upstream `Benchmarking.Command.runCommand` `JsonHL` flow in the
  Rust `run` entry point.
- `json_highlevel FILE` now parses or discovers high-level config,
  applies `--nodeConfig` / `--cardano-tracer` overrides, prints initial
  and final option snapshots, runs a `quickTestPlutusDataOrDie`-style
  datum/redeemer preflight, compiles with `compileOptions`, and passes
  the generated script to `run_script`.
- The explicit `version` subcommand now emits the same version fixture
  as top-level `--version`.

## Remaining Boundaries

- `StartProtocol` still stops at the explicit
  `mkConsensusProtocol` / genesis runtime boundary; high-level command
  execution now reaches that boundary instead of stopping after
  compilation.
- `SecureGenesis`, `Benchmark`, exact `DumpToFile`, `RoundRobin`,
  `OneOf`, `selftest`, and Plutus pre-execution / script-spend
  integrity remain open tx-generator slices.

## Validation

```text
cargo fmt --all -- --check
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
