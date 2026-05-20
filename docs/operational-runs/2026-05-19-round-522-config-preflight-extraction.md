# Round 522 - config preflight extraction

Date: 2026-05-19

## Goal

Continue stale-placement cleanup inside `crates/node/cardano-node` by moving
pure `NodeConfigFile` preflight checks out of the binary command adapter and
into the config owner crate.

## Changes

- Added `NodeConfigPreflightReport`, `NodeConfigPreflightError`, and
  `node_config_preflight_report` to `crates/node/config`.
- Moved config-only hard failures and warnings into the config crate:
  protocol-version presence, security parameter and KES bounds,
  active-slot coefficient range, genesis/checkpoint hash diagnostics,
  protocol/min-node-version/peer-sharing warnings, governor target sanity,
  checkpoint cadence, `RequiresNetworkMagic`, and trace/metrics settings.
- Kept storage recovery, peer-snapshot loading, and forge-feature credential
  file loading in the node binary command because those require runtime,
  storage, or block-producer dependencies.
- Updated local `AGENTS.md` guidance so future `validate-config` checks land
  in the config crate unless they truly require binary/runtime ownership.

## Verification

- `cargo check -p yggdrasil-node-config`
- `cargo check -p yggdrasil-node`
- `cargo test -p yggdrasil-node-config node_config_preflight_report`
- `cargo test -p yggdrasil-node-config credential`
- `cargo test -p yggdrasil-node validate_config_report_`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`
- `python .claude/scripts/filetree.py check`
- `git diff --check`

All listed checks passed. `git diff --check` emitted only CRLF-normalization
warnings from the current Windows checkout.
