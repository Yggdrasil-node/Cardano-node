## Round 221 — ChainProvider trait contract: separate `chain_tip` (Tip envelope) from `chain_tip_point` (bare Point)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Type: Trait contract refinement following R220 + tentative-trap rollback fix

### Goal

R220 fixed the server-side ChainSync `Tip` envelope encoding for
`MsgRollForward`/`MsgIntersectFound`/`MsgIntersectNotFound` — the
`tip` slot in those messages now correctly emits the upstream
`Tip` envelope (`[]` or `[point, blockNo]`).

R221 closes a related but distinct gap exposed by R220:
`MsgRollBackward { point, tip }` carries TWO different shapes —
the `point` slot is **bare Point** (the rollback target) while the
`tip` slot is **Tip envelope** (the chain tip).  The R220
`provider.chain_tip()` change made `chain_tip()` return the Tip
envelope, but the tentative-trap rollback path was using
`chain_tip()` for BOTH slots — so the `point` slot now also
emitted the Tip envelope, which is wire-incorrect.

### Diagnosis

The pre-R220 `chainsync_server_rolls_back_after_tentative_trap`
test passed because both `point` and `tip` slots used
`Point::to_cbor_bytes()`.  Post-R220 the test failed:

```
left:  [130, 130, 1, 88, 32, ...]  // [Tip envelope]
right: [130, 1, 88, 32, ...]       // [bare Point]
```

The tentative-trap rollback path at
`node/src/server.rs:684-691`:

```rust
let confirmed_tip = provider.chain_tip();  // post-R220: Tip envelope
cursor = Some(confirmed_tip.clone());      // wrong shape for cursor
server.roll_backward(confirmed_tip.clone(), confirmed_tip).await?;
//                   ^^ wrong: should be bare Point  ^^ correct: Tip envelope
```

### Fix

Add a new method to the `ChainProvider` trait:

```rust
/// Return the current chain tip as CBOR-encoded bare `Point`
/// (`[]` for genesis, `[slot, hash]` for a specific tip).  Used
/// as the `point` slot of `MsgRollBackward` and to seed the
/// chainsync cursor.  Distinct from [`Self::chain_tip`] which
/// returns the upstream `Tip` envelope used at tip-slot positions.
fn chain_tip_point(&self) -> Vec<u8> {
    Point::Origin.to_cbor_bytes()
}
```

Trait-level docs now spell out the contract: every `tip` Vec<u8>
returned MUST be the Tip envelope; `chain_tip_point()` is the
distinct accessor for bare-Point uses.

Production `SharedChainDb` impl returns `db.tip().to_cbor_bytes()`
for `chain_tip_point()`.  The `MockTentativeChainProvider` test
mock implements both.  The tentative-trap rollback path now uses
both:

```rust
let confirmed_tip_point = provider.chain_tip_point();    // bare Point
let confirmed_tip_envelope = provider.chain_tip();       // Tip envelope
cursor = Some(confirmed_tip_point.clone());
server
    .roll_backward(confirmed_tip_point, confirmed_tip_envelope)
    .await?;
```

### Test update

`chainsync_server_rolls_back_after_tentative_trap` updated to
assert both shapes:

```rust
assert_eq!(point, confirmed_point.to_cbor_bytes());      // bare Point
assert_eq!(tip, mock_tip_envelope(confirmed_point));     // Tip envelope
```

### End-to-end verification (preprod, two yggdrasil instances)

Same setup as R220 — A listens on :13041, B `--peer 127.0.0.1:13041`:

```
A inbound: yggdrasil_inbound_connections_accepted=1
           yggdrasil_blocks_synced=499
B sync:    yggdrasil_blocks_synced=250
           yggdrasil_current_slot=94440
           yggdrasil_reconnects=0
B errors:  (empty — no chainsync decode errors)
```

R221 preserves R220's bidirectional P2P parity AND fixes the
rollback wire shape.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
cargo build --release            # clean (31.97 s)
```

### Strategic significance

R221 closes a follow-on bug from R220's trait change.  The two
rounds together establish a clean trait-level invariant:

| Method                | Returns         | Used at                                              |
| --------------------- | --------------- | ---------------------------------------------------- |
| `chain_tip()`         | `Tip` envelope  | tip slot of `MsgRollForward`, `MsgRollBackward`, `MsgIntersectFound`, `MsgIntersectNotFound` |
| `chain_tip_point()`   | bare `Point`    | `point` slot of `MsgRollBackward`; chainsync cursor   |
| `next_header()` `point` | bare `Point`  | `MsgRollForward.point`                               |
| `next_header()` `tip` | `Tip` envelope  | `MsgRollForward.tip`                                 |
| `find_intersect()` `point` | bare `Point` | `MsgIntersectFound.point` (echoed)                |
| `find_intersect()` `tip` | `Tip` envelope | `MsgIntersectFound.tip`                            |

Future implementors of the trait have an unambiguous contract; the
tentative-trap rollback path is now the canonical example of using
both methods together.

### Open follow-ups (unchanged from R220)

1. Phase E.2 — long-running mainnet rehearsal (24 h+).
2. Phase D.1 — deep cross-epoch rollback recovery.
3. Phase D.2 — multi-session peer accounting.
4. Phase E.1 cardano-base — coordinated vendored fixture refresh.
5. (de-prioritised by R217) Phase C.2 pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §3
  (sync robustness — bidirectional P2P parity row).
- Previous round: [R220](2026-04-30-round-220-server-tip-envelope-fix.md).
- Captures: `/tmp/ygg-r221-{a,b}.log` (post-R221 verification).
- Touched files (1):
  - `node/src/server.rs` — new `chain_tip_point` trait method +
    SharedChainDb + MockTentativeChainProvider impls + rollback
    callsite update + test assertion update.
- Upstream reference:
  - `Ouroboros.Network.Protocol.ChainSync.Codec` —
    `MsgRollBackward` carries `(Point, Tip)` (heterogeneous shape
    at the two argument positions).
