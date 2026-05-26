# Round 825 - cardano-testnet Command payload wiring

## Scope

Advance the `cardano-testnet` sister-tool from raw passthrough command
payloads to the typed upstream-shaped parser records already ported in
R818-R823.

## Findings

- Upstream `Parsers.Run.CardanoTestnetCommands` carries concrete
  payloads:
  `StartCardanoTestnet CardanoTestnetCliOptions`,
  `CreateTestnetEnv CardanoTestnetCreateEnvOptions`, and
  `GetVersion VersionOptions`.
- Yggdrasil still returned `PassthroughArgs` for all three variants,
  leaving the documented "Command payload wiring" gap open even though
  `opts_testnet` and `opts_create_testnet` already produced the typed
  records.
- The upstream `version` subcommand is not runtime/era gated; it calls
  `runVersionOptions` directly.

## Changes

- `parser::Command::Cardano` now carries `CardanoTestnetCliOptions`.
- `parser::Command::CreateEnv` now carries
  `CardanoTestnetCreateEnvOptions`.
- `parser::Command::Version` now carries `VersionOptions`.
- `parse_args` threads the subcommand tail through `opts_testnet` /
  `opts_create_testnet`.
- `run()` dispatches `Command::Version` to the same captured
  byte-equivalent version banner as `--version`.
- Living status docs now mark the remaining cardano-testnet deferral as
  runtime / era-genesis / Process harness execution for `cardano` and
  `create-env`, not parser payload wiring.

## Validation

- Red: `cargo test -p yggdrasil-cardano-testnet parser::tests::cardano_subcommand_captures_passthrough_args --lib`
  failed because `Command` still carried `PassthroughArgs` and
  `VersionOptions` did not exist.
- Green: `cargo test -p yggdrasil-cardano-testnet parser::tests::cardano_subcommand_carries_typed_options --lib`
  passed after typed payload wiring.
- Red: `cargo test -p yggdrasil-cardano-testnet version_subcommand_matches_upstream`
  failed because `cardano-testnet version` still returned the deferral
  error path.
- Green: `cargo test -p yggdrasil-cardano-testnet version_subcommand_matches_upstream`
  passed after `run()` dispatched `Command::Version`.
- Green: `cargo test -p yggdrasil-cardano-testnet` passed with 94 lib
  tests and 3 CLI golden tests.
- Green: `cargo fmt --all -- --check`.
- Green: `cargo check-all`.
- Red/green: `cargo lint` initially rejected a
  `VersionOptions::default()` test for a unit struct; removing the
  unnecessary `Default` derive and direct default construction restored
  a green lint gate.
- Test inventory:
  `cargo test-all -- --list --format terse | Select-String -Pattern ': test$' | Measure-Object | Select-Object -ExpandProperty Count`
  returned `7218`.
- Green: `cargo test-all` passed with no failures and the expected 3
  ignored `yggdrasil_node_tracer` doctests.
- Green: `python scripts/check-stale-placement.py --self-test`.
- Green: `python scripts/check-stale-placement.py`.
- Green: `python scripts/check-doc-status-headers.py --self-test`.
- Green: `python scripts/check-doc-status-headers.py`.
- Green: `python scripts/check-parity-matrix.py` validated 22 entries
  against the 11.0.1 reference tag.
- Green: `python -m py_compile scripts/check-stale-placement.py scripts/check-doc-status-headers.py scripts/check-parity-matrix.py .claude/scripts/filetree.py`.
- Green: `git diff --check`; output was limited to the expected
  LF-to-CRLF working-copy warnings.
- Filetree: `python .claude/scripts/filetree.py check` first reported
  the new R825 operational note and stale accepted metadata, then
  `python .claude/scripts/filetree.py accept-current` refreshed the
  accepted snapshot and a follow-up check was clean.

## Remaining risk

This round closes the parser payload slice only. Full `cardano-testnet`
parity still requires the runtime/process harness, era-genesis builders,
and end-to-end upstream comparison evidence.
