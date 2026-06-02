---
title: 'R332: bech32 CLI parser + byte-equivalent --help/--version'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-332-bech32-cli-parser/
---

# Round 332 — bech32 CLI parser + byte-equivalent `--help` / `--version`

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R331`](2026-05-09-round-331-bech32-skeleton.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.1 round 2 of 4.

## Summary

R332 lands the bech32 binary's CLI parser surface with byte-equivalent
`--help` and `--version` output captured from the upstream
`bech32 1.1.10` binary. Concrete encode/decode dispatch still
returns the R332 sentinel until R333 lands; everything else is
parity-correct as of this round.

## Approach

clap can't match optparse-applicative's exact help-text format
byte-for-byte (different conventions for flag rendering, ANSI
escape codes, multi-line option descriptions). For deployment-
ready 100% parity, the byte-equivalent help text is mandatory —
SPO automation scripts grep `bech32 --help` for specific lines
and would break on rendering differences.

R332 therefore captures upstream's exact `--help` and `--version`
output as fixture files
(`crates/bech32/tests/fixtures/upstream-{help,version}.txt`) and
embeds them at compile time via `include_str!`. The runtime
help-printing path (`parser::HELP_TEXT`, `parser::VERSION_TEXT`)
and the golden tests both read the same fixtures, so the source
of truth is the fixture file and there's no risk of escape-encoding
errors.

Argument-parsing logic itself uses simple manual scanning — the
upstream CLI surface is just one optional positional (`PREFIX`)
plus the standard `-h`/`--help`/`-v`/`--version` flags, so a clap
derive layer would be pure overhead.

## Diff inventory

| Path | Change |
|---|---|
| `crates/bech32/tests/fixtures/upstream-help.txt` | New fixture — captured from `.reference-haskell-cardano-node/install/bin/bech32 --help` (984 bytes including ANSI escape codes for "To"/"From" underline + bold inline shell-prompt examples). |
| `crates/bech32/tests/fixtures/upstream-version.txt` | New fixture — captured from `.../bech32 --version` (`1.1.10\n`, 7 bytes). |
| `crates/bech32/src/parser.rs` | New module: `Args` struct (single optional `prefix: Option<String>`); `ParseError` enum (HelpRequested / VersionRequested / UnknownFlag / TooManyPositionals); `parse_args()` function; `HELP_TEXT` / `VERSION_TEXT` constants from `include_str!` of the fixtures. 10 unit tests. |
| `crates/bech32/src/lib.rs` | `pub mod parser;` added. `run()` now delegates to `run_with(args)` after parsing argv. New `run_with(args: parser::Args) -> eyre::Result<()>` returns the R332 sentinel. |
| `crates/bech32/src/main.rs` | Replaced eyre-result wrapper with `ExitCode` returning. Added `--help` / `--version` early-exit handling that writes the fixture bytes to stdout (exit 0) before the run dispatch. Unknown-flag and over-positional errors → stderr + exit 1. |
| `crates/bech32/Cargo.toml` | Added `thiserror = { workspace = true }` dep (needed by the new `ParseError` derive). |
| `crates/bech32/tests/cli_help_golden.rs` | New integration test file. 6 golden tests pinning byte-equivalence of `--help` (long+short flag) + `--version` (long+short flag) + unknown-flag rejection + R332 sentinel propagation. |
| `docs/parity-matrix.json` | `sister-tool.bech32` advanced: next_milestone `R332 → R333`; 5 new `implemented_evidence` rows. |
| `docs/operational-runs/2026-05-09-round-332-bech32-cli-parser.md` | This round-doc. |

## Verification

```text
$ cargo test -p yggdrasil-bech32
running 6 tests in tests/cli_help_golden.rs
test help_long_flag_matches_upstream_byte_for_byte ... ok
test help_short_flag_matches_upstream_byte_for_byte ... ok
test version_long_flag_matches_upstream_byte_for_byte ... ok
test version_short_flag_matches_upstream_byte_for_byte ... ok
test unknown_flag_exits_non_zero ... ok
test no_args_returns_r332_sentinel_until_r333 ... ok
test result: ok. 6 passed; 0 failed

running 10 tests in src/parser.rs (parser unit tests)
... all 10 passed

$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s

$ cargo test --workspace --all-features
passed: 4872  failed: 0    (was 4856 → +16 new tests)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 20 entries validated
```

## Drop-in deployment evidence

```text
$ diff <(.reference-haskell-cardano-node/install/bin/bech32 --help) \
       <(target/debug/bech32 --help)
(empty diff — byte-equivalent)

$ diff <(.reference-haskell-cardano-node/install/bin/bech32 --version) \
       <(target/debug/bech32 --version)
(empty diff — byte-equivalent)
```

Operators can swap `bech32 --help` for `target/release/bech32 --help`
in any documentation or automation script without observing any
byte-level difference. The encode/decode dispatch (R333) is the
last gate before swap-readiness.

## Closure criterion

- Fixture files captured from upstream `bech32 1.1.10` binary
  (help + version); embedded into runtime via `include_str!`.
- Parser module declares `Args` / `ParseError` / `parse_args()`.
- Main binary uses `ExitCode` return type with proper exit codes
  (0 for help/version, 1 for unknown flag / sentinel).
- 6 golden tests pin byte-equivalence (help/version × long/short
  flag + unknown flag + sentinel).
- 10 unit tests cover the parse_args() surface (every arg-shape
  variant has an explicit test).
- Workspace test count: 4,856 → 4,872 (+16).
- All 5 cargo gates + 3 CI parity validators clean.

All six are met.

## Out of scope (R333-R334 next steps)

- **R333 — Concrete encode/decode**:
  - Replace placeholder types in `lib.rs` (`DataPartPlaceholder`,
    `HumanReadablePartPlaceholder`, error enums) with real
    implementations using the `bech32` crate (workspace dep
    landed at R330).
  - Implement `run_with(args)`: read stdin, dispatch on
    `args.prefix` to either encode (Some(prefix)) or decode (None).
  - Round-trip test against upstream test vectors at
    `.reference-haskell-cardano-node/deps/bech32/bech32/test/`.
  - Wire `node/dev/scripts/run-tools.sh bech32` end-to-end.
- **R334 — Closeout**: CHANGELOG entry; AGENTS.md operational
  guide refresh; parity-matrix transition `partial → verified_11_0_1`.

After R334, `bech32` becomes the first sister tool with full
deployment-ready 100% parity to upstream.
