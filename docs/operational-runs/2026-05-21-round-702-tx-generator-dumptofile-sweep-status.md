---
title: "Round 702 tx-generator DumpToFile sweep status + blocker flag (A4)"
parent: Reference
---

# Round 702 tx-generator DumpToFile sweep status + blocker flag (A4)

Date: 2026-05-21

## Scope

Records the completed R692-R701 tx-generator `DumpToFile`
`Show (Tx)` simple-field rendering sweep in
`crates/tools/tx-generator/AGENTS.md`, and flags the remaining
gated fields — naming the precise reference data needed before
`auxiliary_data` can be ported with parity confidence.

## Rationale

R692-R701 rendered every scalar / hash / set / map tx-body
field that was previously gated by an `ensure_absent` /
`ensure_empty_or_absent` rejection. The three remaining gated
fields are not all gaps:

- `certificates` / `update` — never populated by the
  tx-generator benchmark path; their gates are defensive.
- `auxiliary_data` — *is* reachable (the `NtoM` size-padding
  path emits `{ uint => TxMetaBytes }` metadata via
  `mkMetadata`), so its gate is a real gap.

Porting `auxiliary_data` needs the exact upstream `Show` output
for the `MemoBytes`-wrapped `ShelleyTxAuxData` / `AlonzoTxAuxData`
and for `Metadatum` — shapes that cannot be guessed without a
forensic reference. Per the `parity-plan` skill ("Do not edit
code on assumption … surface the blocker with the missing
reference data named explicitly"), this round records the
blocker rather than guessing a `Show` string.

## Changes (doc-only)

- `crates/tools/tx-generator/AGENTS.md` — updated the `Status`
  line to `post-R701`; added a "Current Functional Surface"
  entry summarising the R692-R701 sweep; added a
  "DumpToFile remaining-work blocker" entry naming the missing
  artifact (an upstream golden `Show (Tx)` for a Shelley tx
  carrying metadata, or a vendored `cardano-ledger` test vector
  for `Show (ShelleyTxAuxData)`).

No source change — `crates/tools/tx-generator/AGENTS.md` only.

## Validation

- `cargo fmt --all -- --check` — green.
- `check-strict-mirror.py --fail-on-violation` — 0 violations.
- `cargo check-all` / `cargo lint` / `cargo test-all` —
  unaffected (doc-only edit).

## Remaining (A4) — blocked

- `auxiliary_data` (`stAuxData` / `atAuxData`) rendering —
  blocked pending forensic upstream `Show` reference data for
  `ShelleyTxAuxData` / `AlonzoTxAuxData` / `Metadatum`.
- `certificates` / `update` — defensive gates; not reachable
  from the tx-generator benchmark path.
