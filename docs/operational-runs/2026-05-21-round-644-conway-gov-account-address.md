---
title: "Round 644 Typed Conway GOV AccountAddress variants (A5 Phase-2.5)"
parent: Reference
---

# Round 644 Typed Conway GOV AccountAddress variants (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tags 16 (`ProposalReturnAccountDoesNotExist`)
and 17 (`TreasuryWithdrawalReturnAccountsDoNotExist`) by adding
a shared `show_reward_account` helper and the
`NonEmptyAccountAddress` list carrier. After R644, **4 of 19
Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:213-215,293-296`
  (`ProposalReturnAccountDoesNotExist AccountAddress`,
  `TreasuryWithdrawalReturnAccountsDoNotExist (NonEmpty
  AccountAddress)`; CBOR `Sum` tags 16/17).

## Changes

- Factored out the inline reward-account rendering from
  `NonEmptySetAccountAddress::Display` into a free
  `show_reward_account` helper rendering upstream's stock-derived
  `Show AccountAddress`: `AccountAddress {aaNetworkId =
  <Network>, aaId = <Credential>}`.
- Added `NonEmptyAccountAddress` carrier — a
  `Vec<RewardAccount>` preserving wire order (unlike
  `NonEmptySetAccountAddress`'s BTreeSet). CBOR wire format is a
  plain CBOR array; empty arrays reject at decode time. Display:
  `<head> :| [<tail>...]`.
- Refactored `ConwayGovPredFailure::ProposalReturnAccountDoesNotExist(Vec<u8>)`
  → `ProposalReturnAccountDoesNotExist(RewardAccount)` and
  `TreasuryWithdrawalReturnAccountsDoNotExist(Vec<u8>)` →
  `TreasuryWithdrawalReturnAccountsDoNotExist(NonEmptyAccountAddress)`.
  `from_cbor` decodes tag 16 as `[16, AccountAddress bytes]` and
  tag 17 as `[17, NonEmpty AccountAddress]`.

3 new focused unit tests:
- `_proposal_return_account_tag16` — single AccountAddress
  (key/Mainnet).
- `_treasury_withdrawal_return_accounts_tag17` — NonEmpty with
  one script/Testnet account.
- `_treasury_withdrawal_return_accounts_rejects_empty` —
  NonEmpty empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (303 lib + 4
  doctests + 1 main, +3 new tests vs R643 baseline of 300)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants — typed decoders for `GovAction era`,
  `Voter`, `ProposalProcedure era`, `ProtVer`, `Credential`
  cold/hot committee roles, `StrictMaybe ScriptHash`,
  `StrictMaybe (GovPurposeId 'HardForkPurpose)`, `NonEmptyMap`.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
