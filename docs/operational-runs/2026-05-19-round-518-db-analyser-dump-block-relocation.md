# Round 518 - db-analyser dump-block relocation

**Date:** 2026-05-19
**Area:** workspace layout / stale node-crate diagnostics cleanup
**Upstream reference:** the closest upstream surface is the
`db-analyser`/unstable-cardano-tools block-inspection family under
`.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/`.

## Summary

The R251 `dump_block` forensic helper was still packaged as an extra binary
target inside the node binary crate. That made the cleaned node crate look
fatter than the intended thin runtime shell and mixed db-analyser-style
forensics with node startup/orchestration code.

This round moves the helper to:

- `crates/tools/db-analyser/src/bin/dump_block.rs`

The shipped `yggdrasil-node` package now exposes only the node binary target
from `src/main.rs`; the `dump_block` target is owned by the
`yggdrasil-db-analyser` package.

## Guard Updates

`scripts/check-stale-placement.py` now rejects developer diagnostic binaries
under the node binary crate and requires the db-analyser-owned replacement
path. The node and db-analyser `AGENTS.md` files now document the ownership
rule explicitly: forensic ChainDB and immutable-chunk tools belong under
`crates/tools/`, not in the node binary shell.

The strict mirror audit row for `dump_block` moved with the file and remains a
`strict-none` synthesis entry because there is no single upstream Haskell file
with the same role.

## Verification

- `cargo check -p yggdrasil-db-analyser --bins`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python .claude/scripts/filetree.py check`
- `python scripts/check-stale-placement.py --self-test`
- `python scripts/check-stale-placement.py`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `cargo metadata --no-deps --format-version 1`

Metadata evidence after the move:

- `yggdrasil-node`: `src/main.rs` is the only binary target.
- `yggdrasil-db-analyser`: `src/main.rs` provides `db-analyser`, and
  `src/bin/dump_block.rs` provides `dump_block`.

## Remaining Scope

This closes one more stale node-crate placement class. It does not claim full
functional parity with upstream `cardano-node`; the broader parity goal
remains governed by the parity matrix, strict mirror audit, fixture manifest,
and operator runbook gates.
