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

## Haskell parity rehearsal (extension)

After the runbook §6 / §8 PASS results, an extended rehearsal targeted the
§5 hash-compare cadence by running upstream `cardano-node 10.7.1 (ghc-9.6,
git rev 045bc187a36ef0cbd236db902b85dd8f202fb059)` alongside `yggdrasil-node`
on the same preprod chain.  Both nodes started from genesis; we attempted
`compare_tip_to_haskell.sh` at the moving tip per §5b cadence.  Three
operational findings emerged:

### Finding A: Sync-rate gap blocks moving-tip hash-compare in a single session

Side-by-side syncing on preprod from genesis:

```
ygg=112040 haskell=207860     (2 minutes elapsed)
ygg=114440 haskell=252440
ygg=117040 haskell=307280
ygg=119640 haskell=365760
ygg=122240 haskell=444920
ygg=124840 haskell=498700
ygg=127440 haskell=539240
ygg=129840 haskell=577500
ygg=132040 haskell=620680
ygg=134640 haskell=655140
ygg=137040 haskell=707440
ygg=139440 haskell=769540
ygg=141840 haskell=831120
ygg=144240 haskell=903380     (~10 minutes elapsed)
```

`yggdrasil-node` syncs at ~80 slots/sec from genesis; `cardano-node 10.7.1`
syncs at ~1600 slots/sec — a 20× gap.  Both nodes converge on the same chain
eventually (preprod has finite history), but at this rate `yggdrasil-node`
needs roughly 17 days from genesis to catch the current preprod tip
(~slot 121,000,000) while the Haskell node needs ~6 hours.  The §5 moving-tip
cadence requires both nodes pre-synced to network tip; the §5 sign-off step
therefore needs an out-of-band pre-sync window before the cadence can run.

### Finding B: NtC handshake refuses upstream `cardano-cli` (parity gap, two bugs)

Pointing upstream `cardano-cli 10.16.0.0 query tip` at yggdrasil's NtC
socket reproduces:

```
cardano-cli: HandshakeError (VersionMismatch
  [NodeToClientV_16,NodeToClientV_17,...,NodeToClientV_23]
  [])
```

The empty right-hand list (`[]`) is what cardano-cli reports as the
*server's* supported version table.  Two parity bugs root-cause the
behaviour:

1. **Refuse-payload bug** (fixed this run, Round 145):
   `crates/network/src/ntc_peer.rs::ntc_accept` was calling
   `encode_ntc_refuse_version_mismatch` with the *client's* proposed
   versions echoed back instead of `NTC_SUPPORTED_VERSIONS`.  Per upstream
   `Ouroboros.Network.Protocol.Handshake.Codec`, the `Refuse VersionMismatch`
   payload must carry the *server's* version table so the client can see
   what range to renegotiate against.  Fixed by passing
   `NTC_SUPPORTED_VERSIONS` (V9..V16) and pinning with the new
   `ntc_accept_refuse_payload_carries_server_supported_versions` regression
   test.  Post-fix, the same handshake against an out-of-range client
   would reply with `[V_9..V_16]` instead of `[]`, giving operators a
   real diagnosis.
2. **V16 high-bit encoding** (fixed this run, Round 146):
   captured via `YGG_NTC_DEBUG=1` from a real upstream `cardano-cli`
   handshake — `ProposeVersions raw_len=51 preview=8200a8 19 8010 …`
   showed cardano-cli proposing V16-V23 as `0x8010..=0x8017`.  Per
   upstream `Ouroboros.Network.NodeToClient.Version`, every NtC
   version on the wire carries the `nodeToClientVersionBit = 0x8000`
   high-bit flag to distinguish from NtN versions sharing the same
   handshake table.  yggdrasil's `HandshakeVersion::NTC_V9..=NTC_V16`
   were defined as the logical values `9..=16`, so the matcher in
   `ntc_accept` saw the bit-flagged numbers as foreign and refused.
   Fixed by redefining all 8 constants as `NTC_VERSION_BIT | n` plus a
   new `pub const NTC_VERSION_BIT: u16 = 0x8000` and three regression
   tests pinning the high-bit invariant, the literal `0x8010` for
   V16, and the on-wire decode of cardano-cli's actual 51-byte
   `[0, {0x8010..=0x8017 -> [1, false]}]` payload.

### Finding D: LocalStateQuery wire-format parity (fixed this run, Round 146)

After the V16 handshake fix, upstream `cardano-cli query tip` reached
yggdrasil's LocalStateQuery server but immediately tore down the bearer
with `BearerClosed "<socket: 11> closed when reading data"`.
`YGG_NTC_DEBUG=1` traced two further parity bugs in
`crates/network/src/protocols/local_state_query.rs`:

1. **Bytes-string-wrapped point/query/result payloads.**  yggdrasil
   encoded the `point` argument of `MsgAcquire` / `MsgReAcquire` and
   the payloads of `MsgQuery` / `MsgResult` via `enc.bytes(...)`,
   wrapping the inner CBOR in a CBOR major-type-2 byte string.
   Upstream `Ouroboros.Network.Protocol.LocalStateQuery.Codec` writes
   them as INLINE structured CBOR (no wrapper).  yggdrasil's
   `dec.bytes()` decode of cardano-cli's inline-encoded acquire then
   returned a type-mismatch error and tore down the bearer.  Fixed
   by switching encode to `enc.raw(point_cbor)` and decode to
   `dec.raw_value()`; new
   `acquire_point_wire_bytes_are_inline_not_byte_string_wrapped`
   regression test pins the exact wire bytes (`0x82 0x00 <inline>`
   vs the pre-fix `0x82 0x00 0x58 <len> <bytes>`).

2. **`MsgAcquireVolatileTip` tag mismatch.**  yggdrasil mapped this
   variant to wire tag 9 (encode AND decode); upstream uses tag 8.
   `cardano-cli` sends `[8]` (`0x81 0x08`); yggdrasil rejected it as
   `unknown LocalStateQuery message tag: 8` and tore down the
   connection.  yggdrasil's own client+server happened to round-trip
   with each other on tag 9, masking the bug until upstream traffic
   exposed it.  Fixed; new
   `acquire_volatile_tip_wire_tag_matches_upstream_canonical_tag_8`
   regression test pins the exact 2-byte wire payload `[0x81, 0x08]`.

After both fixes, the handshake + acquire + query round-trip
succeeds end-to-end; upstream `cardano-cli` reaches the result-decode
phase before failing with `DeserialiseFailure 2 "expected list len or
indef"`.  That last error reflects the next layer of parity work
(upstream HardForkBlock query/result codec — see Finding E below).

### Finding E: HardForkBlock query/result codec (open)

After the Round 146 wire-level fixes, upstream `cardano-cli query tip`
sends yggdrasil a query payload `82 03 82 00 82 02 81 01` —
`[3, [0, [2, [1]]]]` — which is the upstream era-aware
`HardForkQuery (QueryIfCurrent ConwayEra (BlockQuery GetTip))` shape.
yggdrasil's `BasicLocalQueryDispatcher` services queries via a
flat tag-table (0..=23) and returns a simple result envelope.
Upstream `cardano-cli` then fails to decode the result as
`HardForkBlock ... ServerHasAgency (SingQuerying)
(DeserialiseFailure 2 "expected list len or indef")` because the
result is missing the era-wrapper structure.

Closing this gap requires implementing the full upstream
`Ouroboros.Consensus.HardFork.Combinator.Ledger.Query` codec —
roughly 1000+ lines of structured codec including era selection,
`BlockQuery` vs `QueryAnytime` opcode dispatch, and per-era result
envelopes.  Documented as a major follow-up slice; the building
blocks (`MultiEraBlock`, `Era::all()`, era-specific ledger snapshots)
are already in place.



### Finding C: `compare_tip_to_haskell.sh` silently exits 1 on missing JSON keys

The runbook's helper script ran under `set -euo pipefail` and called
`extract_field` (a `grep | head | sed | tr` pipeline) for `block` and
`epoch` fields — which yggdrasil's `cardano-cli query-tip` JSON does NOT
emit (`{tip: {hash, origin, slot}}`).  When `grep -oE` finds no match,
pipefail propagates the failure and `set -e` exits the whole script
without reaching the `[info]` summary print or the divergence-snapshot
block.  Operators saw exit-1 with no output and no snapshot dir — a
silent failure that masked any real divergence diagnosis.

Fixed in this run: `extract_field` now captures the `grep` output via
`raw="$(... || true)"` and short-circuits on empty.  Empty fields render
as blanks in the comparison summary instead of exiting the script.
The success/divergence printout now fires on every run.

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
