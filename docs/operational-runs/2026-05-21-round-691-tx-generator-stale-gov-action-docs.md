---
title: "Round 691 Correct stale tx-generator GovAction renderer doc comments (A4)"
parent: Reference
---

# Round 691 Correct stale tx-generator GovAction renderer doc comments (A4)

Date: 2026-05-21

## Scope

Documentation-accuracy pass over the Conway `DumpToFile`
`Show (Tx)` GovAction renderer in
`crates/tools/tx-generator/src/script/core.rs`.

## Rationale

Three doc comments / messages claimed the Conway governance
renderer was incomplete when it is not:

- `show_conway_gov_action` renders all seven `GovAction`
  variants (`InfoAction`, `NoConfidence`, `HardForkInitiation`,
  `NewConstitution`, `ParameterChange`, `TreasuryWithdrawals`,
  `UpdateCommittee`) — but its doc comment claimed "The 3
  complex variants ... return a typed `TxGenError` until their
  internal types gain Show ports."
- `show_conway_proposal_procedures` no longer rejects any
  `GovAction` variant — but its doc claimed it "Rejects
  ProposalProcedures carrying `GovAction` variants whose
  rendering is not yet ported."
- `show_conway_pparams_update`'s rejection only fires for
  Shelley-era-only fields that have no Conway `PParamsUpdate`
  representation (a malformed input), yet the message read
  "renderer does not yet support" — implying a missing port.

## Changes (comment + message-string only)

- Rewrote the `show_conway_gov_action` doc comment to state all
  seven variants render, and that the only `ParameterChange`
  rejection path is a malformed Shelley-era-only field.
- Rewrote the `show_conway_proposal_procedures` doc comment.
- Reworded the `show_conway_pparams_update` rejection message
  from "Conway Show(Tx) renderer does not yet support
  non-empty ParameterChange fields" to "Conway ParameterChange
  carries field(s) with no Conway PParamsUpdate
  representation".

No behavior change — the rejection still fires for the same
inputs; only its wording (and the surrounding doc comments) is
corrected. No test asserts the old message string.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (239 lib + 5 main —
  unchanged)

## Remaining (A4)

- tx-generator unmirrored upstream `.hs` files (`NodeToNode`,
  `OuroborosImports`, `Tracer`, `ProtocolParameters`,
  `Setup/NodeConfig`, `Setup/SigningKey`) — large arc.
