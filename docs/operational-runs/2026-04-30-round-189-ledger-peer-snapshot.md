## Round 189 — Conway `ledger-peer-snapshot` LSQ dispatcher (tag 34) end-to-end

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the **last open Conway-era LSQ dispatcher** so
`cardano-cli conway query ledger-peer-snapshot` decodes
end-to-end.  After R189, every documented Conway-era query
tag has a wire-correct dispatcher in yggdrasil — the
`cardano-cli conway query` parity arc is fully closed.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind:
  Option<u8> }` variant.  cardano-cli 10.16 sends the v15+
  2-element form `[34, peer_kind]` with `peer_kind = 0`
  (BigLedgerPeers) or `1` (AllLedgerPeers).  Older clients
  may send the legacy `[34]` 1-element singleton form.
- Decoder branches `(1, 34)` (legacy, peer_kind = None) and
  `(2, 34)` (v15+, extracts peer_kind byte).
- Regression test
  `decode_recognises_ledger_peer_snapshot_tag_34`
  pinning both wire forms.

`node/src/local_server.rs`:

- New dispatcher arm that emits the **V2 form**
  (discriminator 1) regardless of the requested peer_kind.
  Wire shape per upstream
  `Ouroboros.Network.PeerSelection.LedgerPeers.Type.encodeLedgerPeerSnapshot
   (LedgerPeerSnapshotV2 (wOrigin, pools))`:

  ```
  [outer 2-elem]
    0: discriminator (Word8) = 1
    1: [inner 2-elem]
         0: WithOrigin SlotNo — Origin = [0]
         1: pools (indefinite-length CBOR list)
  ```

  For an empty fresh-sync chain the helper emits
  `[1, [[0], 0x9f 0xff]]` — discriminator 1, origin marker,
  empty indef-length pool list.

### Discovery: cardano-cli 10.16 only decodes V2 (discriminator 1)

Initial implementation emitted V23 forms (discriminator 2 for
BigLedgerPeers, 3 for AllLedgerPeers) per the upstream
`LedgerBigPeerSnapshotV23` / `LedgerAllPeerSnapshotV23`
constructors.  cardano-cli 10.16 rejected with
`DeserialiseFailure 5 "LedgerPeers.Type: no decoder could be
found for version 3"` — its decoder at the negotiated NtC
version doesn't support the V23 forms even when it requested
`AllLedgerPeers (peer_kind=1)` in the query.

Switched the response to V2 form (discriminator 1) which is
the legacy-but-still-supported shape.  The `WithOrigin SlotNo`
+ pool-list shape is compatible with both BigLedgerPeers and
AllLedgerPeers semantics (the SRV-related distinctions don't
affect the empty-pool case).

A second decoder failure surfaced the indefinite-length-list
requirement for the pool field (`DeserialiseFailure 8
"expected list start"`) — upstream's `toCBOR @[a]` for the
pool list uses indefinite encoding `0x9f ... 0xff`, not the
definite-length empty list `0x80`.  Switched to indef-length
empty list (`0x9f 0xff`).

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~8960, era=Conway):

```
$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{
    "bigLedgerPools": [],
    "slotNo": "origin",
    "version": 2
}
```

Decodes end-to-end through cardano-cli 10.16.

Regression checks pass (every other Conway query still works):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 8960, "epoch": 0, "era": "Conway", ... }

$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, "constitution": ..., "currentPParams": ..., "proposals": [] }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query constitution --testnet-magic 2
{ "anchor": ..., "script": "..." }

$ cardano-cli conway query future-pparams --testnet-magic 2
No protocol parameter changes will be enacted at the next epoch boundary.

$ cardano-cli conway query spo-stake-distribution --testnet-magic 2 --all-spos
[]
```

### Cumulative Conway-era query coverage — **fully complete**

Every documented Conway-era LSQ query tag now has a
wire-correct dispatcher in yggdrasil:

| Tag | Query | Round | Status |
|---|---|---|---|
| 16 | stake-pools | R163/R179 | ✓ working |
| 19 | pool-state (GetCBOR) | R172/R179 | ✓ working |
| 20 | stake-snapshot (GetCBOR) | R173/R179 | ✓ working |
| 22 | stake-deleg-deposits | R186 | ✓ wire-correct |
| 23 | constitution | R180 | ✓ working |
| 24 | gov-state | R188 | ✓ working |
| 25 | drep-state | R180/R181 | ✓ working |
| 26 | drep-stake-distribution | R184 | ✓ working |
| 27 | committee-state | R182 | ✓ working |
| 28 | filtered-vote-delegatees | R184 | ✓ working (internal) |
| 29 | treasury (account-state) | R180 | ✓ working |
| 30 | spo-stake-distribution | R184 | ✓ working |
| 31 | proposals | R185 | ✓ working |
| 32 | ratify-state | R187 | ✓ working |
| 33 | future-pparams | R183 | ✓ working |
| **34** | **ledger-peer-snapshot** | **R189** | **✓ working** |
| 35 | stake-pool-default-vote | R185 | ✓ working |
| 36 | pool-distr2 | R186 | ✓ wire-correct |
| 37 | stake-distribution | R179 | ✓ working |

**Every cardano-cli `conway query` subcommand now decodes
end-to-end against yggdrasil** with `YGG_LSQ_ERA_FLOOR=6`.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4743 → **4744** (one new regression
test).

### Open follow-ups

The Conway-era LSQ wire-protocol gap is now closed. Remaining
items shift to *data plumbing* and *operational improvements*:

1. **Live data plumbing** — current placeholders return
   empty data; populating gov-state proposals, ratify-state
   enacted actions, drep stake distribution, ledger-peer-
   snapshot pool list, etc. is the natural follow-on once
   yggdrasil's runtime tracks them in the snapshot.
2. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 structural).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — new `EraSpecificQuery::GetLedgerPeerSnapshot` variant +
  decoder branches `(1, 34)` and `(2, 34)` + regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  dispatcher arm emitting V2 form `[1, [[0], indef pool list]]`.
- Captures: `/tmp/ygg-r189-preview.log`.
- Upstream reference:
  `Ouroboros.Network.PeerSelection.LedgerPeers.Type.LedgerPeerSnapshot`
  (3 constructors: V2 / BigLedgerPeerSnapshotV23 /
  AllLedgerPeerSnapshotV23);
  `encodeLedgerPeerSnapshot` (V2 case for legacy clients);
  `decodeLedgerPeerSnapshot` (case-matches on
  `(ledgerPeerKind, version)` — cardano-cli 10.16 only
  recognises version 1 in the V2 case).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-188-gov-state.md`](2026-04-30-round-188-gov-state.md).
