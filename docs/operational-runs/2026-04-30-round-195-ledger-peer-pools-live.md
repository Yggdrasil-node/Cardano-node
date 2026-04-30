## Round 195 — Live ledger-peer-snapshot pool list (Phase A.5)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.5 — replace the empty `bigLedgerPools` placeholder in
`cardano-cli conway query ledger-peer-snapshot` with live data
extracted from yggdrasil's snapshot's `pool_state`, including
each registered pool's relay endpoints.

### Code change

`node/src/local_server.rs`:

- New `encode_ledger_peer_snapshot_v2_for_lsq(snapshot)`
  helper emitting upstream `LedgerPeerSnapshotV2` wire shape:

  ```text
  [outer 2-elem]
    0: discriminator (Word8) = 1
    1: [inner 2-elem]
         0: WithOrigin SlotNo (live tip)
         1: pools (indef-length CBOR list of)
              [AccPoolStake, [PoolStake, NonEmpty Relays]]
  ```

  Per-pool entry:
  - `AccPoolStake` and `PoolStake` are `Rational` newtypes
    encoded as 2-element `[numerator, denominator]` lists.
    Yggdrasil emits `0/1` placeholders pending live active
    stake distribution snapshot integration (Phase A.7+).
  - `NonEmpty Relays` is an **indefinite-length** CBOR list
    (cardano-cli's V2 decoder rejected definite-length here
    at depth 20).

  Per-relay encoding per upstream
  `Ouroboros.Network.PeerSelection.RelayAccessPoint`:
  - Domain (DNS): `[3, 0, port_int, domain_bstr]`
  - IPv4 address: `[3, 1, port_int, ipv4_word32]`
  - IPv6 address: `[3, 2, port_int, ipv6_bytes]`

  Yggdrasil's `PoolRelayAccessPoint { address: String, port:
  u16 }` parses via `String::parse::<IpAddr>` to detect
  IPv4/IPv6; otherwise falls through to Domain.

- `GetLedgerPeerSnapshot` dispatcher arm now calls the helper.

### Discovery: NonEmpty Relays uses indef-length

Initial implementation emitted `NonEmpty Relays` as
definite-length list (via `enc.array(relays.len())`).
cardano-cli rejected with `DeserialiseFailure 20 "expected list
start"`.  Switched to indef-length (`0x9f` start, `0xff` break)
matching the same pattern R189 found for the outer pool list.

### Operational verification

After ~30s of preview sync with `YGG_LSQ_ERA_FLOOR=6` (chain
at slot ~2960):

```
$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{
    "bigLedgerPools": [
        {
            "accumulatedStake": 0,
            "relativeStake": 0,
            "relays": [
                {
                    "address": "preview-node.world.dev.cardano.org",
                    "port": 30002
                }
            ]
        },
        {
            "accumulatedStake": 0,
            "relativeStake": 0,
            "relays": [
                {
                    "address": "preview-node.world.dev.cardano.org",
                    "port": 30002
                }
            ]
        },
        {
            "accumulatedStake": 0,
            "relativeStake": 0,
            "relays": [
                {
                    "address": "preview-node.world.dev.cardano.org",
                    "port": 30002
                }
            ]
        }
    ],
    "slotNo": 2960,
    "version": 2
}
```

**All three preview-registered pools surface with their real
DNS relay endpoints** (`preview-node.world.dev.cardano.org:30002`).
Stake values are 0 placeholders pending Phase A.7 active stake
plumbing.

Regression checks pass:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 2960, "era": "Conway", ... }

$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query spo-stake-distribution --testnet-magic 2 --all-spos
[ ["38f4a58a...", 0, null], ... ]

$ cardano-cli conway query protocol-state --testnet-magic 2
{ "candidateNonce": null, ... }
```

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

Continuing the data-plumbing arc:

1. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser
   (`leadership-schedule` and `kes-period-info` use this
   internally).
2. **Phase A.7** — wire active stake distribution into
   `bigLedgerPools` AccPoolStake / PoolStake fields and
   `spo-stake-distribution` amounts.
3. **Phase A.2** — runtime nonce attach via Arc publish channel.
4. **Phase A.3 OMap proposals** — gov-state proposal entries
   (requires `GovActionState` shape adaptation).
5. **Phase B** — R91 multi-peer livelock.
6. **Phase C/D/E** — sync perf, deep rollback, mainnet rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) — new
  `encode_ledger_peer_snapshot_v2_for_lsq` helper +
  dispatcher arm.
- Upstream reference:
  `Ouroboros.Network.PeerSelection.LedgerPeers.Type.LedgerPeerSnapshotV2`;
  `Ouroboros.Network.PeerSelection.RelayAccessPoint.LedgerRelayAccessPoint`.
- Yggdrasil reference:
  `LedgerStateSnapshot::pool_state`,
  `RegisteredPool::relay_access_points`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-194-stake-distributions-live.md`](2026-04-30-round-194-stake-distributions-live.md).
