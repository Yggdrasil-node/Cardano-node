---
title: "Round 682 Typed ContextError ProposalProceduresFieldNotSupported (A5 Phase-2.5)"
parent: Reference
---

# Round 682 Typed ContextError ProposalProceduresFieldNotSupported (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types `ContextError` tag 13
(`ProposalProceduresFieldNotSupported`) to carry a typed
`Vec<ProposalProcedure>`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxInfo.hs:181,239`
  (`ProposalProceduresFieldNotSupported (OSet.OSet
  (ProposalProcedure era))`; CBOR `Sum` tag 13).

## Changes

- Refactored `ContextError::ProposalProceduresFieldNotSupported(Vec<u8>)`
  → `ProposalProceduresFieldNotSupported(Vec<ProposalProcedure>)`.
- `ContextError::from_decoder` special-cases tag 13: decodes the
  `OSet ProposalProcedure` — a tag-258-tolerant CBOR array of
  proposal procedures (via `ProposalProcedure::from_decoder`).
- Display: `ProposalProceduresFieldNotSupported (fromList
  [<ProposalProcedure>, ...])`.

1 new focused unit test:
- `context_error_decodes_proposal_procedures_field` — a
  `ContextError` tag 13 with a one-element proposal-procedure
  set, asserting the typed `ProposalProcedure` render.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (349 lib + 4
  doctests + 1 main, +1 new test vs R681 baseline of 348)

## Remaining (A5 Phase-2.5+)

- `ContextError` raw variants — tag 8 `BabbageContextError`,
  tag 12 `VotingProceduresFieldNotSupported`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
