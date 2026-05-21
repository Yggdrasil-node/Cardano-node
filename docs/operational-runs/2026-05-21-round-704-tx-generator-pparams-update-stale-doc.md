---
title: "Round 704 Correct stale show_conway_pparams_update doc comment (A4)"
parent: Reference
---

# Round 704 Correct stale show_conway_pparams_update doc comment (A4)

Date: 2026-05-21

## Scope

Corrects the stale doc comment on `show_conway_pparams_update`
in `crates/tools/tx-generator/src/script/core.rs`.

## Rationale

The doc comment claimed "Non-empty updates return a typed
`TxGenError` naming the first set field whose per-type Show is
not yet ported" and listed `Prices` / `CostModels` /
`PoolVotingThresholds` etc. as "each need dedicated Show
ports". This is stale: `show_conway_pparams_update` renders all
30 Conway `ConwayPParams` fields at their typed values (the
per-type Show ports landed in earlier rounds). The only
rejection path is a malformed input — a
`ProtocolParameterUpdate` setting a Shelley-era-only field
(`d`, `extra_entropy`, `protocol_version`, `min_utxo_value`)
with no Conway `PParamsUpdate` representation.

This is the same stale-doc class corrected in R688 / R691 /
R703.

## Changes (doc-comment only)

- Rewrote the `show_conway_pparams_update` doc comment to state
  all 30 fields render, and that the sole rejection path is the
  malformed Shelley-era-only-field input.

No behavior change.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (249 lib + 5 main —
  unchanged; comment-only)

## Remaining (A4)

- `auxiliary_data` DumpToFile rendering — blocked on the
  `primitive`-package `Show ByteArray` (see R703).
