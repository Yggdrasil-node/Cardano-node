---
title: "Round 770 A1 closeout — remove the inert ntc feature flag"
parent: Reference
---

# Round 770 A1 closeout — remove the inert ntc feature flag

Date: 2026-05-22

## Scope

Closes roadmap item **A1 — Feature-flag gating**. The dmq-node
runtime sub-arc's decomposable surface having been completed
(R758-R768), A1 was the remaining Category-A ("executable now")
roadmap item.

## What shipped

`crates/network/Cargo.toml`:

- Removed the inert `[features]` block (`default = ["ntc"]`,
  `ntc = []`). `yggdrasil-network/ntc` carried 0 `#[cfg]` sites — a
  pure no-op flag. Wiring it would have scattered
  `#[cfg(feature = "ntc")]` across the `yggdrasil-network` NtC module
  tree (`local_state_query_*`, `local_tx_submission_*`,
  `local_tx_monitor_*`, `ntc_peer`), the `yggdrasil-node-ntc-server`
  crate, and `cardano-cli`'s LocalStateQuery surface; since
  `cargo lint-no-default` builds the whole workspace with
  `--no-default-features`, a partial gating breaks it. For a niche
  relay-only build that omits the local socket, that cost is not
  justified. Removal matches the R591 (`ntn`) / R592
  (`yggdrasil-ledger/plutus`) inert-flag precedent and the
  no-decorative-feature-flags rule.

`crates/node/cardano-node/src/commands/validate_config.rs`:

- Fixed a pre-existing `--no-default-features` regression —
  `BlockProducerCredentialStatus` and
  `ensure_block_producer_credential_policy` were imported
  unconditionally but used only inside the `#[cfg(feature = "forge")]`
  `load_configured_block_producer_credentials`. The import is now
  `forge`-gated, so `cargo lint-no-default` is green.

`docs/COMPLETION_ROADMAP.md` (A1 → ✅ COMPLETE) and `docs/TECH-DEBT.md`
(the Wave 3/5 inert-flags entry → all removed) updated.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo lint-no-default` — green (A1's exit criterion; was red
  before R770 due to the `validate_config.rs` regression).

## Outcome

No inert feature flags remain in the workspace. A1 is closed.
