---
title: "Round 650 Typed Conway GOV network-id-mismatch variants (A5 Phase-2.5)"
parent: Reference
---

# Round 650 Typed Conway GOV network-id-mismatch variants (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tags 2 (`ProposalProcedureNetworkIdMismatch`)
and 3 (`TreasuryWithdrawalsNetworkIdMismatch`) — both
network-id-mismatch governance predicates. After R650, **14 of
19 Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:180-181,239-240,265-268`
  (`ProposalProcedureNetworkIdMismatch AccountAddress Network`,
  `TreasuryWithdrawalsNetworkIdMismatch (NonEmptySet
  AccountAddress) Network`; CBOR `Sum` tags 2/3, each `!> To
  acnt !> To nid`).

## Changes

- Refactored `ConwayGovPredFailure::ProposalProcedureNetworkIdMismatch(Vec<u8>)`
  → struct variant `{ account: RewardAccount, network: Network
  }`. `from_cbor` decodes the 3-element envelope `[2,
  AccountAddress bytes, Network]`.
- Refactored `TreasuryWithdrawalsNetworkIdMismatch(Vec<u8>)` →
  struct variant `{ accounts: NonEmptySetAccountAddress,
  network: Network }`. `from_cbor` decodes the 3-element
  envelope `[3, NonEmptySet AccountAddress, Network]`.
- Display: `ProposalProcedureNetworkIdMismatch (<AccountAddress>)
  <Network>` and `TreasuryWithdrawalsNetworkIdMismatch
  (<NonEmptySet>) <Network>` (the AccountAddress / NonEmptySet
  args are parens-wrapped at p=11; the bare Network constructor
  is not).

2 new focused unit tests:
- `_proposal_procedure_network_mismatch_tag2` — single
  AccountAddress + Mainnet.
- `_treasury_withdrawals_network_mismatch_tag3` — NonEmptySet
  with one script/Testnet account + Testnet.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (315 lib + 4
  doctests + 1 main, +2 new tests vs R649 baseline of 313)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (5) — typed decoders for `GovAction
  era` (tags 1 MalformedProposal / 15 ZeroTreasuryWithdrawals),
  `ProposalProcedure era` (tags 8 InvalidPrevGovActionId / 12
  DisallowedProposalDuringBootstrap), `ProtVer` (tag 10
  ProposalCantFollow).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
