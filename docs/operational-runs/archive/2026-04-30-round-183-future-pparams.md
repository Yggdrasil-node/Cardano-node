## Round 183 — Conway `future-pparams` LSQ dispatcher (tag 33)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Add `GetFuturePParams` (tag 33) so
`cardano-cli conway query future-pparams` decodes end-to-end.
Continues the Conway-governance dispatcher series after R180/R181/R182
(constitution, drep-state, treasury, committee-state).

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetFuturePParams` variant (no
  parameters — singleton query).
- `decode_query_if_current` recognises `(1, 33)`.
- Added `decode_recognises_future_pparams_tag_33` regression
  test pinning the wire form `[1, [33]]` = `0x82 0x01 0x81 0x18 0x21`.

`node/src/local_server.rs`:

- New dispatcher arm emits the response as `Maybe (PParams era)`
  per upstream
  `Cardano.Ledger.Conway.LedgerStateQuery.GetFuturePParams`:
  `Nothing` → `[]` (empty CBOR list `0x80`).  Without a
  queued PParams update ready for next-epoch adoption,
  yggdrasil emits `Nothing` — cardano-cli renders this as
  `"No protocol parameter changes will be enacted at the
  next epoch boundary."`.

### Initial misstep + correction

Round started by emitting the `FuturePParams era` ADT shape
(`Sum NoPParamsUpdate 0` = `[0]` = `0x81 0x00`) per upstream
`Cardano.Ledger.Core.PParams.FuturePParams`.  cardano-cli
rejected with `DeserialiseFailure 4 "expected list len or
indef"` — the underlying `BlockQuery` result type for
`GetFuturePParams` is actually `Maybe (PParams era)` (the
LSQ-facing wrapper), not the `FuturePParams` ADT directly.
Switched to the `Maybe` shape and the response decoded
end-to-end.

### Operational verification

After 15s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.
null
```

Decoded end-to-end through cardano-cli 10.16.

Regression checks (R180/R181/R182 governance queries still
work):

```
$ cardano-cli conway query constitution --testnet-magic 2
{ "anchor": { "dataHash": "ca41a91f...", "url": "ipfs://..." }, ... }

$ cardano-cli conway query drep-state --all-dreps --testnet-magic 2
[]

$ cardano-cli conway query committee-state --testnet-magic 2
{ "committee": {}, "epoch": 0, "threshold": null }

$ cardano-cli conway query treasury --testnet-magic 2
0
```

### Updated cumulative Conway-era query coverage

| Query | Tag | Round | Status |
|---|---|---|---|
| constitution | 23 | R180 | ✓ working |
| gov-state | 24 | R180 dispatcher | body shape pending |
| drep-state | 25 | R180/R181 | ✓ working |
| treasury | 29 | R180 | ✓ working |
| committee-state | 27 | R182 | ✓ working |
| **future-pparams** | **33** | **R183** | **✓ working** |
| stake-pools | 16 | R163/R179 | ✓ working |
| stake-distribution | 37 | R179 | ✓ working |
| pool-state | 19 (GetCBOR) | R172/R179 | ✓ working |
| stake-snapshot | 20 (GetCBOR) | R173/R179 | ✓ working |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4739  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4738 → **4739**.

### Open follow-ups

1. **`gov-state` body shape** — substantial 7-element
   `ConwayGovState` record with `Proposals` tree +
   `DRepPulsingState` cache.
2. Tag 26 `GetDRepStakeDistr`, 30 `GetSPOStakeDistr`,
   31 `GetProposals`, 32 `GetRatifyState`,
   35 `QueryStakePoolDefaultVote` — additional Conway-era
   dispatchers for completeness.
3. Live stake-snapshot plumbing (R163/R173 follow-up).
4. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
5. Apply-batch duration histogram (R169).
6. Multi-session peer accounting (R168 structural).
7. Pipelined fetch + apply (R166).
8. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — `EraSpecificQuery::GetFuturePParams` variant + decoder
  branch + regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  dispatcher arm emitting `Maybe (PParams era) = Nothing` =
  `0x80`.
- Captures: `/tmp/ygg-r183-preview.log` (cardano-cli renders
  the `Nothing` response as the human-readable
  "No protocol parameter changes" message).
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.GetFuturePParams`
  (returns `Maybe (PParams era)` not `FuturePParams era`
  directly);
  `Cardano.Ledger.Core.PParams.FuturePParams` (the ADT used
  internally by ledger state, distinct from the LSQ-facing
  `Maybe`).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-182-committee-members-state.md`](2026-04-30-round-182-committee-members-state.md).
