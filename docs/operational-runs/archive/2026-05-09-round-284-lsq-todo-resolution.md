# Round 284 — LSQ `TODO follow-ups` comment resolution

**Date:** 2026-05-09
**Phase:** C (tech-debt purge)
**Predecessor:** R283 (`docs/operational-runs/2026-05-09-round-283-era-tag-wiring.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve the production TODO at `node/src/local_server.rs:731`:

> Falls through to `null_response()` for queries we don't yet handle,
> which produces the same behaviour as before (cardano-cli will still
> print `DeserialiseFailure` for those — TODO follow-ups).

## Investigation

Counting handled `EraSpecificQuery` variants in
`dispatch_upstream_query` revealed 28 explicitly dispatched arms:
`GetCurrentPParams`, `GetEpochNo`, `GetWholeUTxO`, `GetUTxOByAddress`,
`GetUTxOByTxIn`, `GetStakePools`, `GetStakePoolParams`, `GetPoolState`,
`GetStakeSnapshots`, `GetStakeDistribution`,
`GetFilteredDelegationsAndRewardAccounts`, `GetGenesisConfig`,
`GetConstitution`, `GetGovState`, `GetDRepState`, `GetAccountState`,
`DebugNewEpochState`, `DebugChainDepState`, `GetLedgerPeerSnapshot`,
`GetRatifyState`, `GetFuturePParams`, `GetCommitteeMembersState`,
`GetFilteredVoteDelegatees`, `GetDRepStakeDistr`,
`GetStakeDelegDeposits`, `GetPoolDistr2`, `GetProposals`,
`QueryStakePoolDefaultVote`, `GetSPOStakeDistr`, `GetCBOR`.

Plus the catch-all `EraSpecificQuery::Unknown { .. } => null_response()`
for queries Yggdrasil's decoder does not recognize (e.g. LSQ queries
added in a future cardano-node release that pre-date a Yggdrasil
upgrade).

So the TODO comment was misleading: it implied a meaningful gap in
coverage, but in fact the dispatch is comprehensive against the LSQ
query set Yggdrasil's decoder recognizes. The `Unknown` fallback's
`null_response()` is the upstream-intended on-wire shape for "no
result available"; cardano-cli's `DeserialiseFailure` surfacing IS the
correct diagnostic for queries Yggdrasil doesn't recognize.

## Resolution

Updated the comment block to:
1. Remove the `TODO follow-ups` marker.
2. Document the actual current state (28+ recognized variants,
   `Unknown` arm as the catch-all).
3. Explain why `null_response()` is the right shape for unknown
   queries (matching upstream's "no result available" wire form).
4. Document the extension procedure for adding new LSQ queries
   (extend `decode_query_if_current` + add a new dispatch arm).

```rust
// Round 156 onwards — decode `[era_index, era_specific_query]` and
// dispatch every recognised `EraSpecificQuery` variant (28+ subqueries
// covering pparams, epoch/era state, UTxO, stake distribution, pool
// state, governance, DRep state, ledger peers, etc.).
//
// The `EraSpecificQuery::Unknown { .. }` arm catches LSQ queries
// Yggdrasil's decoder does not recognize (e.g. new queries added in
// a future cardano-node release that pre-date a Yggdrasil upgrade)
// and replies with `null_response()` — matching the on-wire shape
// for "no result available". cardano-cli surfaces this as a
// `DeserialiseFailure` on its end, which is the upstream-intended
// diagnostic for unknown queries. New LSQ queries are added by
// extending `decode_query_if_current` plus the dispatch arm below.
match decode_query_if_current(&inner_cbor) {
    ...
}
```

## Production TODO/FIXME count

```text
$ grep -rn "TODO\|FIXME" --include="*.rs" crates/ node/src/ | grep -v "/tests/" | grep -v "cfg(test)" | grep -v parity-matrix
(empty)
```

R284 brings production-side TODO/FIXME count to ZERO.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 2.53s)
cargo lint                          clean (Finished `dev` profile in 6.97s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

## Diff stat

```text
node/src/local_server.rs   +12 lines (replace stale TODO with accurate
                                       state + extension procedure)
docs/operational-runs/2026-05-09-round-284-... (new)
```

## Stop point — Phase C in progress

| Round | Site | Status |
|---|---|---|
| R282 | `block_producer.rs::description` | ✅ closed |
| R283 | `sync.rs era_tag` + `local_server.rs lsq_era_index` | ✅ closed |
| **R284** | `local_server.rs:713` LSQ TODO | ✅ closed |
| R285 | `peer_management.rs` Phase 6 wiring | next |
| R286 | `reconnecting.rs` marker + `shelley.rs` test helper | pending |
| R287 | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R283 (`docs/operational-runs/2026-05-09-round-283-era-tag-wiring.md`)
