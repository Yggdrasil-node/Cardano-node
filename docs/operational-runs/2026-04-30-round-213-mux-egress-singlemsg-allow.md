## Round 213 — Mux egress: allow single payloads larger than `EGRESS_SOFT_LIMIT`

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Builds on: R212 (mainnet operational verification surfaced the
`query utxo --whole-utxo` BearerClosed limitation)

### Goal

Root-cause and fix the R212 known limitation: `query utxo
--whole-utxo --mainnet` failed with `BearerClosed` against an active
yggdrasil mainnet sync.  Other queries (tip, era-history, slot-number,
protocol-parameters, tx-mempool info) worked correctly.

### Diagnosis

Reproduced the BearerClosed deterministically.  Enabled
`YGG_NTC_DEBUG=1` for LSQ wire-trace.  Captured:

```
[ygg-ntc-debug] LSQ send state=StAcquired raw_len=1319561 preview=820481b938a98258204c01c878...
```

Yggdrasil generates the **complete and correct** UTxO response (1.3 MB,
14 505 mainnet AVVM bootstrap entries).  Send fails with
`MuxError::EgressBufferOverflow` because the per-protocol
`egress_limit` check in `ProtocolHandle::send` is over-strict:

```
let current = self.egress_bytes.load(Ordering::Relaxed);
if current + len > self.egress_limit {  // current=0, len=1.3MB, limit=262KB → fires
    return Err(MuxError::EgressBufferOverflow {...});
}
```

`EGRESS_SOFT_LIMIT = 0x3ffff = 262 143 bytes` (~262 KB).  The check
rejects **any single message larger than the limit**, even when the
egress buffer is empty.  This is incorrect:

- Upstream `network-mux`'s `egressSoftBufferLimit` is a
  **back-pressure** threshold on *accumulated* pending bytes — used
  to detect a writer that has fallen behind, not to reject single
  large messages.
- The mux writer fragments payloads into SDUs (12 288 bytes max per
  upstream) at the bearer layer; large single messages are perfectly
  valid and standard for LSQ responses.
- Real-world LSQ responses on mainnet are routinely > 1 MB
  (full UTxO map, gov-state with proposals, ledger-state, etc.).

### Fix

`crates/network/src/mux.rs::ProtocolHandle::send`:

```diff
-if current + len > self.egress_limit {
+if current > self.egress_limit {
     return Err(MuxError::EgressBufferOverflow { ... });
 }
```

The check now fires only when the egress buffer has **already
accumulated** more than the soft limit (i.e. the writer is behind).
A single large send is always allowed when the buffer is empty, which
matches upstream's back-pressure semantic.

Doc-comment on `EGRESS_SOFT_LIMIT` and `ProtocolHandle::send` updated
to clarify the limit semantic.

### Test update

`crates/network/tests/integration.rs::mux_egress_buffer_overflow`
was pinning the **old (buggy) semantic** — asserting that a single
payload > `EGRESS_SOFT_LIMIT` returns `EgressBufferOverflow`.  R213
flips the assertion: a single large payload must succeed when the
buffer is empty, then accumulating sends without a draining reader
eventually trigger back-pressure.  The test now pins the **upstream-
aligned semantic** explicitly.

### Verification — `query utxo --whole-utxo --mainnet` works

Setup:
```
$ rm -rf /tmp/ygg-r213e-mainnet-db /tmp/ygg-r213e-mainnet.sock
$ ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r213e-mainnet-db \
    --socket-path /tmp/ygg-r213e-mainnet.sock \
    --peer 3.135.125.51:3001 &
$ sleep 30
```

Query result:
```
$ cardano-cli query utxo --whole-utxo --mainnet --output-json | python3 -c '
> import json, sys
> d = json.load(sys.stdin)
> print(f"UTxO entry count: {len(d)}")
> total = sum(v["value"]["lovelace"] for v in d.values())
> print(f"Total lovelace: {total:,}")
> print(f"Total ADA: {total / 1_000_000:,.2f}")
> '
UTxO entry count: 14 505
Total lovelace: 31 112 484 745 000 000
Total ADA: 31 112 484 745.00
```

The response is the full mainnet **AVVM bootstrap distribution** —
14 505 entries totaling **31.1 billion ADA**, exactly matching the
mainnet `byron-genesis.json` `avvmDistr` count and the upstream
genesis-utxo formula.

Sample entries:
```
0002e8580d96f4f8e58f66fa57e539d001ae5f7939de0cdd3d29cc9b6c4c1f7f#0
  address: Ae2tdPwUPEZKLbb7iGFGtKuWj1yJEiMK53ovb1HVd6GztJgqJZnuebMbP2Z
  lovelace: 462 146 000 000

000e39c38855652eb099224932a5938fa7ecc24c1499ea1ff83055937644ece5#0
  address: Ae2tdPwUPEZ8iFyHc7bUgDrJVqk22ADTKaqJs6UsDf3ajGSpeUyZcmgbhGG
  ...
```

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 744 passed / 0 failed / 1 ignored
cargo build --release            # clean
```

### Strategic significance

R213 closes the R212 known limitation and proves yggdrasil's mainnet
**operational LSQ surface is now complete** — every cardano-cli query
that worked on testnets also works on mainnet, including the
heavyweight `query utxo --whole-utxo` that returns ~1.3 MB.

The bug was a ~10-line semantic miscoding in the mux back-pressure
check that had been latent since the mux was implemented.  It only
manifested for LSQ responses > 262 KB, which never arose on testnet
operational tests because preview/preprod bootstrap UTxO sets are
much smaller.  R212's mainnet test surfaced it; R213 fixes it.

R213 is the first round in the post-mainnet-sync arc to materially
improve the operator-facing LSQ experience.

### Open follow-ups (unchanged from R212 minus the BearerClosed item)

1. Long-running mainnet sync rehearsal (24 h+) — verify Byron→Shelley
   HFC at slot 4 492 800.
2. Phase A.6 — `GetGenesisConfig` ShelleyGenesis serialiser.
3. Phase C.2 — pipelined fetch+apply (sync rate currently ~3.3 slot/s).
4. Phase D.1 — deep cross-epoch rollback recovery.
5. Phase D.2 — multi-session peer accounting.
6. Phase E.1 cardano-base — coordinated vendored fixture refresh.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §8e (R212 multi-network verification, now updated).
- Previous round: [R212](2026-04-30-round-212-mainnet-cardano-cli-verification.md).
- Captures:
  - `/tmp/ygg-r213c-mainnet.log` (diagnosis with `YGG_NTC_DEBUG=1`).
  - `/tmp/ygg-r213e-mainnet.log` (verification — 14 505 UTxO entries).
- Touched files (3):
  - `crates/network/src/mux.rs` — `ProtocolHandle::send` semantic fix +
    doc comment update.
  - `crates/network/tests/integration.rs::mux_egress_buffer_overflow`
    — test updated to pin the new (upstream-aligned) semantic.
  - Doc updates: this round doc + journal updates.

### Upstream reference

`Ouroboros.Network.Mux.Egress.send`:
```haskell
sendToBearer egress slot payload = do
  bufferUsed <- readTVar (egressPendingBytes slot)
  -- ↓ Note: upstream's check matches yggdrasil's R213 semantic:
  --   only the already-accumulated `bufferUsed` is compared against
  --   the limit; the new `payload` length is not added to the check.
  when (bufferUsed > egressSoftBufferLimit) $
    throwIO EgressBufferOverflow
  -- ... enqueue payload, regardless of its size ...
```

Yggdrasil's R213 fix aligns with this semantic.
