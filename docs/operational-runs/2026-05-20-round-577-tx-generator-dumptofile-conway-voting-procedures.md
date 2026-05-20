---
title: "Round 577 tx-generator DumpToFile Conway VotingProcedures map"
parent: Reference
---

# Round 577 tx-generator DumpToFile Conway VotingProcedures map

Date: 2026-05-20

## Scope

This round lifts the `ctbrVotingProcedures` map boundary in
`show_conway_tx_for_dump`. Previously, any Conway transaction with a
non-empty `voting_procedures` body field would fail
`SubmitMode::DumpToFile` with `does not yet support non-empty
ctbrVotingProcedures`. After this round, voting procedures render as
upstream stock-derived `Show (VotingProcedures era)`:
`VotingProcedures {unVotingProcedures = fromList [(Voter, fromList
[(GovActionId, VotingProcedure)])]}`.

`ctbrProposalProcedures` (the OSet of `ProposalProcedure` records,
each wrapping the 7+-variant `GovAction` sum type) remains on
explicit `TxGenError` boundary pending a dedicated round.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:338-386`
  (`Voter`, `Vote`, `VotingProcedure`, `VotingProcedures`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:162,175-200`
  (`GovActionIx`, `GovActionId`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:667-981`
  (`Url`, `Anchor`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Hashes.hs:162-180`
  (`KeyHash` record-newtype).

## Changes

- Replaced `ensure_empty_voting_procedures` with
  `show_conway_voting_procedures` in `script/core.rs`. Outer-map
  entries iterate `BTreeMap<Voter, ...>` (Rust `Ord Voter` matches
  upstream `Ord` over constructor index + hash bytes), inner-map
  entries iterate `BTreeMap<GovActionId, VotingProcedure>` (matching
  upstream `Ord` over `(TxId, GovActionIx)`).
- Added 6 helpers:
  - `show_conway_vote(vote: Vote)` — `VoteNo` / `VoteYes` /
    `Abstain`.
  - `show_conway_voter(voter: &Voter)` — 5 variants mirroring
    upstream `CommitteeVoter (KeyHashObj (KeyHash {unKeyHash =
    ...}))`, `CommitteeVoter (ScriptHashObj (ScriptHash ...))`,
    `DRepVoter (KeyHashObj ...)`, `DRepVoter (ScriptHashObj ...)`,
    `StakePoolVoter (KeyHash {unKeyHash = ...})`.
  - `show_conway_gov_action_id(id: &GovActionId)` — `GovActionId
    {gaidTxId = TxId {unTxId = SafeHash "<hex>"}, gaidGovActionIx =
    GovActionIx {unGovActionIx = <ix>}}`.
  - `show_conway_voting_procedure(vp: &VotingProcedure)` —
    `VotingProcedure {vProcVote = <Vote>, vProcAnchor = <StrictMaybe
    Anchor>}`.
  - `show_anchor(anchor: &Anchor)` — `Anchor {anchorUrl = Url
    {urlToText = "<text>"}, anchorDataHash = SafeHash "<hex>"}`.
  - `show_url(url: &str)` — `Url {urlToText = "<text>"}` (Rust
    Debug formatter quotes + escapes ASCII URLs the same way
    Haskell `Show Text` does for plain ASCII).
- Updated the Conway body format string to substitute the rendered
  voting-procedures expression.
- Added 5 focused unit tests:
  - `dumptofile_show_conway_vote` — three Vote variants.
  - `dumptofile_show_conway_voter_variants` — all 5 voter variants
    pinned to their exact rendered text.
  - `dumptofile_show_conway_gov_action_id_renders_record_form` —
    record shape with hash + index.
  - `dumptofile_show_conway_voting_procedure_with_and_without_anchor`
    — both `vProcAnchor = SNothing` and `SJust (Anchor {...})`.
  - `dumptofile_show_conway_voting_procedures_empty_and_full` —
    `None` ⇒ `VotingProcedures {unVotingProcedures = fromList []}`,
    single-outer-entry / single-inner-entry case checks the full
    envelope.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (33 tests, +5
  from R576)
- `cargo test -p yggdrasil-tx-generator` (216 lib tests + 5
  CLI/golden, +5 from R576 baseline)

## Remaining

- Render `ctbrProposalProcedures` OSet entries — needs `GovAction`
  Show (7+ variants: ParameterChange, HardForkInitiation,
  TreasuryWithdrawals, NoConfidence, UpdateCommittee, NewConstitution,
  InfoAction) plus `Constitution`, `PrevGovActionId`, and various
  per-variant supporting types.
- Render native reference scripts (`Script::Native`) — needs the
  Timelock Show port.
- Render native scripts and bootstrap witnesses inside the witness
  set.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for byte
  parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
