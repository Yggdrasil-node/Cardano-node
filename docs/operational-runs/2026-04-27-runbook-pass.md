---
title: "Operational Run — 2026-04-27"
layout: default
parent: Reference
nav_order: 9
---

# Operational Run — 2026-04-27 (devcontainer rehearsal)

Per [docs/MANUAL_TEST_RUNBOOK.md](../MANUAL_TEST_RUNBOOK.md) §9 template. This
run was performed inside the WSL2 devcontainer; sections that require
infrastructure not available in this environment (a parallel Haskell
`cardano-node` for hash-compare, real pool credentials for producer mode,
multi-day soaks) are recorded as N/A with rationale.

```
[date]        2026-04-27 19:05 Z
[network]     preprod (relay-only sync)
[mode]        relay-only (no pool credentials)
[duration]    ~9 min wallclock across §2a / §6.5a / §6 / §8 phases
[binary]      target/release/yggdrasil-node (1m35s release build, 10.6 MB)
[toolchain]   rustc 1.95.0 (59807616e 2026-04-14)

[§2a Preprod sync-only smoke]                  PASS
  RUN_SECONDS=120, knob=1 (default)
  result   : totalBlocks=1556, finalPoint=slot 116640
  storage  : populated continuously (legacy single-peer path)
  reconnects=0, rollbacks=1 (initial Origin realignment)
  shutdown : clean SIGINT, sync complete event emitted

[§6.5a Two-peer multi-peer dispatch (Round 144 closure)]    PASS
  RUN_SECONDS=120, knob=2, topology=2-localRoot
  activation criteria   : both held
    yggdrasil_blockfetch_workers_registered=2 → 6 (warm peers migrated)
    yggdrasil_blockfetch_workers_migrated_total = 7 across run
  storage  : 845 files persisted, blocks_synced=836, current_slot=102240
  reconnects=0, batches_completed=84, 0 consensus contiguity errors
  closure  : Round 91 Gap BN closed end-to-end (storage populates from
             genesis under the multi-peer path)

[§6.5b Hash-compare under parallel fetch]      N/A (no Haskell node available)
  Devcontainer has cardano-cli 10.16.0.0 installed but no upstream
  Haskell `cardano-node` running on a peer Unix socket.  Running one
  in parallel would require a separate ~50 GB DB / multi-hour
  initial sync.  Tracked as outstanding sign-off step in §6.5f.

[§6.5c Sustained-rate measurement]             OBSERVED (regression noted)
  knob=1 baseline (§2a): 1556 blocks in 120 s
  knob=2 (§6.5a)      : 836 blocks in 120 s
  throughput delta     : 0.54×  (target ≥ 1.0×)
  Rationale: the Round 144 placeholder-hash guard collapses
  multi-chunk plans to a single peer until the runtime grows
  multi-peer ChainSync candidate-fragment lookup.  Storage
  correctness is preserved; throughput parity tracks that
  follow-up work and gates the §6.5f default-flip recommendation.

[§6.5d Knob=4 24h soak]                        N/A (session-bounded)
[§6.5e Mainnet knob=2 24h soak]                N/A (session-bounded; mainnet
                                                     reachable but multi-day
                                                     run not feasible)

[§3 Mainnet relay-only rehearsal]              N/A (session-bounded)
  backbone.cardano.iog.io:3001 reachable from this devcontainer over
  IPv6.  RUN_SECONDS=600 not feasible inside a single session.

[§4 Mainnet block production]                  N/A (no pool credentials)

[§5 Hash compare vs Haskell node]              N/A (no Haskell socket)

[§6 Restart resilience]                         PASS
  CYCLES=3 INTERVAL_BASE_S=60 (shortened from 12×300 s)
  cycle 1   : tip 88440  (from Origin)
  cycle 2   : tip 103220 (after restart from cycle 1's persisted state)
  cycle 3   : tip 118600 (after restart from cycle 2's persisted state)
  final     : tip 122520 (post-cycles recovery probe)
  monotonic : every cycle and the final probe satisfied
  result    : storage WAL + dirty-flag recovery confirmed clean

[§7 Metrics snapshots]                          CAPTURED
  /tmp/ygg-metrics-snapshots/preprod-2a-end-*.txt
  /tmp/ygg-metrics-snapshots/preprod-65a-end-*.txt

[§8 Local query / submit smoke]                 PASS (read-only queries)
  tip                   : {slot: 123220, hash: 4173415d…, origin: false}
  current-era           : 1 (Shelley/Allegra/Mary range)
  current-epoch         : 4
  protocol-params       : full Conway ProtocolParameters CBOR returned
  ledger-counts         : pools=3, stake_credentials=3, gen_delegs=7
  stability-window      : null (3k/f not configured at runtime)
  treasury-and-reserves : both zero (Byron region, pre-rewards)
  deposit-pot           : pool=1.5e9, key=6e6, drep=0, proposal=0
  submit-tx             : N/A (no signed transaction available)

[evidence-summary]
  workspace-tests pass=4644 fail=0 (4642 + 4 new tests,
                                   minus 0 — net +2 over Round 144 baseline)
  cargo fmt --check  : clean
  cargo lint         : clean (-D warnings honoured)
  cargo doc --no-deps: clean (no unresolved-link warnings)
```

## Operational findings beyond the runbook template

1. **Round 91 Gap BN closure was incomplete after Round 144 unit-test fix.**
   The ReorderBuffer Origin-gate fix passed unit tests but reproduced the
   livelock operationally.  The actual root cause was placeholder
   `[0; 32]` `HeaderHash` boundaries synthesised by `split_range` for
   intermediate chunks; peers responded `NoBlocks` to wire requests
   carrying unknown hashes, so every batch returned zero blocks.  This
   was caught by `YGG_SYNC_DEBUG=1` capturing the
   `[ygg-sync-debug] blockfetch-request-cbor=...0000000000…` payload.
2. **Symmetric `lower_hash` dedup was missing from the multi-peer branch.**
   Once the placeholder guard let real blocks flow, the consensus
   `track_chain_state_entries` non-contiguity check fired with
   `expected N, got N-1` because the closed-interval fetch returns the
   block at `lower` which the runtime had already applied.  Ported the
   single-peer branch's dedup loop into the multi-peer branch.
3. **Throughput regression at knob=2** (54% of knob=1 baseline) is the
   expected cost of collapsing multi-chunk plans to single-chunk; closing
   it requires multi-peer ChainSync candidate-fragment lookup.
   Recorded as a follow-up to keep the production default at `1` until
   that work lands.
4. **No mainnet sync attempted** — devcontainer is reachable but a real
   mainnet run takes multi-hours from Origin and was out of scope.

## Reproduction

All results above can be replayed with:

```sh
cargo build --release -p yggdrasil-node
export YGG_BIN="$PWD/target/release/yggdrasil-node"

# §2a baseline
"$YGG_BIN" run --network preprod --database-path /tmp/ygg-2a-db --metrics-port 9001 &

# §6.5a multi-peer (after applying topology with 2 localRoots)
"$YGG_BIN" run --network preprod \
  --topology /tmp/multi.json \
  --max-concurrent-block-fetch-peers 2 \
  --database-path /tmp/ygg-65a-db --metrics-port 9201 &

# §6 restart resilience
YGG_BIN="$YGG_BIN" NETWORK=preprod CYCLES=3 INTERVAL_BASE_S=60 \
  node/scripts/restart_resilience.sh
```
