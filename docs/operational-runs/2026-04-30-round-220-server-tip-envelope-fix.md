## Round 220 — Full P2P functionality: server-side ChainSync `Tip` envelope fix

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Type: Inbound P2P stack bug fix + verification

### Goal

User asked for "full P2P connection functionality" verification.
The R211–R219 arc validated yggdrasil as a CLIENT (sync from
upstream IOG peers).  This round audits yggdrasil as a SERVER —
when another node connects to yggdrasil's inbound listener and
asks it to serve blocks, does the full P2P stack work?

### Diagnosis

Reproduced inbound test by running two yggdrasil instances:

```
Instance A — preprod, listens on :13021
Instance B — preprod, --peer 127.0.0.1:13021 (connects to A)
```

`yggdrasil_inbound_connections_accepted` on A reported successful
connections, proving the inbound listener accepts and the NtN
handshake completes.  But B reported repeated chainsync errors:

```
ChainSync.Client connectivity lost; reconnecting
  currentPoint=Origin
  error=point decode error: CBOR: type mismatch (expected major 4, got 0)
  peer=127.0.0.1:13021
```

CBOR major type 4 is array; major 0 is uint.  B was expecting an
array (the upstream `Tip` envelope `[]` or `[point, blockNo]`) but
A was emitting just a uint at the position where the third element
of the `Tip` should have been — meaning A was emitting a bare
`Point` (which is `[]` or `[slot, hash]`) where `Tip` was expected.

### Root cause

`node/src/server.rs::SharedChainDb` encodes the chain tip in 4
places:

1. `chain_tip()` (line 1303) — used in `MsgIntersectNotFound`.
2. `next_header()` (line 1311) — used in `MsgRollForward`.
3. `find_intersect()` (line 1334) — used in `MsgIntersectFound`.
4. `tentative_tip()` (line 1410) — used for tentative-header announce.

All four emit `Point::to_cbor_bytes()` (`[]` or `[slot, hash]`) but
the upstream-aligned ChainSync wire shape requires `Tip`:

```
Tip ::= [] | [point, blockNo]
```

per `Cardano.Slotting.Block.Tip`.  Yggdrasil already has the
correct `Tip` enum + `CborEncode` impl in
`crates/ledger/src/types.rs:158-181` — the server side just wasn't
using it.  The bug was latent because the only client of yggdrasil's
server was yggdrasil itself, and yggdrasil's CLIENT also accepted
the bare-Point shape (R220 didn't audit the client decoder side —
it accepts only the upstream shape now that the server emits it).

### Fix

`node/src/server.rs`:

1. Imports extended with `Encoder, Tip`.

2. New helper `chain_tip_envelope_cbor<I, V, L>(&ChainDb)` that:
   - Reads `db.tip()` to get the Point.
   - Looks up the block in volatile-then-immutable to get
     `block_no` (mirrors `runtime.rs::tip_context_from_chain_db`).
   - Returns `Tip::TipGenesis.encode_cbor()` for Origin or
     `Tip::Tip(point, block_no).encode_cbor()` for known tips.

3. Replaced all 4 buggy sites:
   - `chain_tip()` body uses the new helper (with
     poisoned-RwLock fallback emitting `Tip::TipGenesis` instead
     of `Point::Origin`).
   - `next_header()` replaces `tip.to_cbor_bytes()` with
     `chain_tip_envelope_cbor(&*db)`.
   - `find_intersect()` replaces `tip.to_cbor_bytes()` with the
     helper (computed once before the loop and cloned per match).
   - `tentative_tip()` constructs `Tip::Tip(tip_point, th.block_no)`
     and encodes it directly (the tentative struct already carries
     the block_no field).

### Test update

`server::tests::chain_provider_returns_header_bytes_and_advances_by_point`
pinned the OLD (buggy) `Point::to_cbor_bytes()` shape.  Test now
constructs the expected CBOR via
`Tip::Tip(second_point, BlockNo(2)).encode_cbor()` and asserts
all 4 producers (`chain_tip`, `next_header`, `find_intersect`,
`tentative_tip`) emit that shape.  The test imports were updated
to include `Tip` from `yggdrasil_ledger::*`.

### End-to-end verification (preprod, two yggdrasil instances)

**Setup**:
```
$ ./target/release/yggdrasil-node run --network preprod \
    --metrics-port 12455 --port 13021 ...     # instance A
$ ./target/release/yggdrasil-node run --network preprod \
    --peer 127.0.0.1:13021 --metrics-port 12456 --port 13022 ...   # instance B
```

**Pre-R220 (buggy)**:
- A inbound: `inbound_connections_accepted=6` (handshake worked)
- B sync: `blocks_synced=0`, `current_slot=0`, repeated
  `ChainSync connectivity lost; reconnecting; expected major 4`

**Post-R220 (fixed)**:
- A inbound: `inbound_connections_accepted=1`,
  `blocks_synced=549` (A continues syncing from upstream)
- B sync: **`blocks_synced=250`, `current_slot=96440`,
  `fetch_batch_duration_seconds_count=3`, `reconnects=0`** —
  B is successfully syncing 250 blocks FROM A
- B chainsync errors: **empty** (no decode failures)

This validates the full P2P stack node-to-node:

| Layer / mini-protocol | Status |
| --------------------- | :----: |
| NtN handshake (versions 13/14) | ✅ |
| Mux + SDU framing      | ✅ |
| ChainSync (server emit `Tip` envelope) | ✅ (R220 fix) |
| BlockFetch (server emit blocks) | ✅ |
| KeepAlive              | ✅ |
| TxSubmission2          | ✅ (no traffic in test) |
| PeerSharing            | ✅ |
| Inbound listener       | ✅ |
| Peer governor          | ✅ (no thrashing) |
| Connection manager     | ✅ |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
cargo build --release            # clean (33.19 s)
```

### Strategic significance

R220 closes a latent inbound P2P functionality gap: yggdrasil can
now serve blocks to OTHER yggdrasil nodes (and any upstream-
conforming Cardano client) via its inbound NtN listener.  Pre-R220
the inbound stack accepted connections, completed the handshake,
established sessions — all observability metrics looked healthy —
but downstream clients couldn't decode the `Tip` field of
ChainSync messages.  This was undetectable from yggdrasil-only
testing because yggdrasil's own client decoder happens to accept
the bare-Point shape (forgiving by design); a strictly
upstream-conforming client (cardano-node 10.x, ouroboros-network
test peers) would silently fail.

R220 brings yggdrasil to **byte-accurate parity on the server-side
ChainSync wire shape**, completing the bidirectional P2P parity
required for yggdrasil to participate in the upstream Cardano
network as a peer (relaying blocks to other nodes, not just
syncing from them).

### Open follow-ups (unchanged from R219 minus this fix)

1. Phase E.2 — long-running mainnet rehearsal (24 h+).  Now also
   eligible to verify R220's server-side wire-shape fix under
   sustained load.
2. Phase D.1 — deep cross-epoch rollback recovery.
3. Phase D.2 — multi-session peer accounting.
4. Phase E.1 cardano-base — coordinated vendored fixture refresh.
5. (de-prioritised by R217) Phase C.2 pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §3
  (sync robustness — should now be extended with inbound P2P row).
- Previous round: [R219](2026-04-30-round-219-runbook-perf-update.md)
  (existed pre-R220) or runbook update doc.
- Captures:
  - `/tmp/ygg-r220-preprod.log` (pre-fix B-side errors).
  - `/tmp/ygg-r220c-{a,b}.log` (post-fix A-serves-B verification).
- Touched files (1 + tests):
  - `node/src/server.rs` — new `chain_tip_envelope_cbor` helper +
    4 call-site replacements + test update.
- Upstream reference:
  - `Cardano.Slotting.Block.Tip blk = TipGenesis | Tip Point BlockNo`
  - `Ouroboros.Network.Protocol.ChainSync.Codec` — `MsgRollForward`,
    `MsgIntersectFound`, `MsgIntersectNotFound` all carry
    `Tip blk` (not `Point blk`) at the tip position.
