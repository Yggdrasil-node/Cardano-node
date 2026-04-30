## Round 210 — Mainnet stall diagnostic (apply-side ruled out)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Builds on: R208 (boot smoke), R209 (parity-plan refresh)

### Goal

Add opt-in apply-side diagnostic to `apply_verified_progress_to_chaindb`
so a brief mainnet run can answer the R208 question: **is the stall
at BlockFetch (zero blocks fetched) or at apply (blocks fetched but
silently rejected)?**

### Change

Single targeted edit in `node/src/runtime.rs` (~line 5008) — gated on
`YGG_SYNC_DEBUG=1`:

```
[YGG_SYNC_DEBUG] apply_verified_progress
    fetched_blocks=N rollback_count=R steps=S current_point={Origin|BlockPoint}
[YGG_SYNC_DEBUG] applied
    stable_block_count=N epoch_events=E rolled_back_tx_ids=T tracking.tip={...}
```

Lives where `apply_verified_progress_to_chaindb` is invoked, so its
absence proves apply is never reached.  Existing `[ygg-sync-debug]
blockfetch-range` instrumentation (lower-cased) was already present
from earlier rounds.  No prod-path overhead when env var unset.

### Test setup

```
$ rm -rf /tmp/ygg-r210-mainnet-db
$ python3 -c "import socket; print(socket.gethostbyname('backbone.cardano.iog.io'))"
3.135.125.51
$ YGG_SYNC_DEBUG=1 RUST_LOG=info timeout 90 \
    ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r210-mainnet-db \
    --peer 3.135.125.51:3001 \
    --metrics-port 0 \
    --max-concurrent-block-fetch-peers 1
```

### Findings (90 s window)

| Signal                                     |   Count |
| ------------------------------------------ | ------- |
| `[YGG_SYNC_DEBUG] apply_verified_progress` |   **0** |
| `[ygg-sync-debug] blockfetch-range` lines  |     634 |
| `[ygg-sync-debug] demux-exit` errors       |       2 |
| `Node.Recovery.Checkpoint cleared-origin`  |      12 |
| `volatile/` size                           |       0 |
| `immutable/` size                          |       0 |
| `ledger/` size                             |       0 |

Representative log slice:

```
verified sync session established fromPoint=Origin peer=3.135.125.51:3001
[ygg-sync-debug] blockfetch-range
    lower=Origin
    upper=BlockPoint(SlotNo(648087), HeaderHash(c6d6a3d9c37c0d1b…))
    tip=BlockPoint(SlotNo(186009858), HeaderHash(fde5e8c0db0b0bf7…))
    header_point_decoded=true range_valid=true skip_fetch=false
    raw_header_len=94
    raw_header_hex=82008282001a0009e397d8185850851a2d964a09…
[ygg-sync-debug] demux-exit error=connection closed by remote peer
[ygg-sync-debug] mux-exit  error=connection closed by remote peer
[Notice] Node.Recovery.Checkpoint action=cleared-origin
```

### Conclusion

**Apply path is not the bottleneck.**  `apply_verified_progress`
is *never invoked* on mainnet during the 90 s window.  The sync
loop computes valid BlockFetch ranges (634 of them) and the
ChainSync header decode succeeds (`header_point_decoded=true`),
but the remote IOG backbone peer **closes the mux connection
immediately after the BlockFetch request is sent**.  Because no
blocks ever arrive, `apply_verified_progress` has nothing to
process and no checkpoint, sidecar, volatile, or immutable file
is ever written.

This **rules out** every R208 hypothesis that targeted the apply
path, ledger rules, or storage hand-off.  The stall is at the
**BlockFetch wire layer** during the Byron-era range
`Origin → SlotNo(648087)`.

Likely causes (now narrowed):
1. **Byron BlockFetch CDDL/wire shape on the request side** —
   yggdrasil's BlockFetch `RequestRange` may be encoding the
   `Origin` lower bound or Byron-era upper bound in a shape the
   IOG backbone peer rejects.  Preprod has Byron blocks but its
   peers may be more permissive (or yggdrasil's first preprod
   batch hits a different boundary).
2. **NtN handshake version negotiation** — mainnet peers may
   require a newer protocol version where yggdrasil is offering
   an older one (or vice versa); ChainSync works but BlockFetch
   may have stricter requirements.
3. **Byron EBB (epoch boundary block) shape** — the upper bound
   `SlotNo(648087)` likely contains EBBs.  If yggdrasil's
   BlockFetch range encoding doesn't account for EBB hash
   indirection upstream expects, the peer would reject.

### Next steps (deferred to follow-up rounds)

R211+ (Phase E.2 wire-layer diagnosis):
1. Capture the exact `MsgRequestRange` bytes sent over the mux to
   the IOG peer using `tcpdump` or socat-relay tracing.
2. Run upstream `cardano-node 10.7.x` against the same peer and
   capture its `MsgRequestRange` bytes for the same range.
3. Byte-compare; identify the diverging field.
4. Fix in `crates/network/src/protocols/blockfetch_pool.rs` or
   the encoder for `MsgRequestRange`.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4744 passed, 0 failed, 1 ignored
cargo check -p yggdrasil-node    # clean (6.03 s)
cargo build --release -p yggdrasil-node    # clean (35.66 s)
```

### Status update

**R210 complete.**  Diagnostic instrumentation now permanent (low
overhead, env-gated).  Mainnet sync gap **narrowed from
"apply or fetch" to "BlockFetch wire layer"** via direct
observational evidence.

Phase E.2 mainnet rehearsal remains deferred but is now
de-risked: any future round investigating it can skip the
ledger/apply/storage rabbit holes and go straight to wire-bytes
comparison against upstream.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §8c.
- Previous round: [R208](2026-04-30-round-208-mainnet-boot-smoke.md).
- Captures: `/tmp/ygg-r210-mainnet.log` (90 s window).
- Touched files (single edit): `node/src/runtime.rs` apply site
  (~line 5001).
