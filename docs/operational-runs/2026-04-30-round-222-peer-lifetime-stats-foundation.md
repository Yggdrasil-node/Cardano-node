## Round 222 — Phase D.2 first slice: PeerLifetimeStats foundation

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: D.2 (multi-session peer accounting — first slice)

### Goal

Phase D.2 calls for refactoring the governor's peer-state to track
**lifetime** statistics independently of session-keyed state, so an
operator dashboard can distinguish "this peer just connected for
the first time" from "this peer has churned 5 times in the past
hour".  The session-keyed `failures` map decays / resets via
`record_success`; the lifetime counter must accumulate
monotonically across reconnects.

R222 is the **first slice**: the parallel-tracking shadow data
structure + accessor methods + regression test.  It does NOT yet
wire update points into the runtime — those connections come in
follow-up rounds, gated by the structural foundation here.

### Implementation

**New struct** in [`crates/network/src/governor.rs`](../../crates/network/src/governor.rs):

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerLifetimeStats {
    pub sessions: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub successful_handshakes: u32,
    pub failures_total: u32,
    pub first_seen: Option<Instant>,
    pub last_seen: Option<Instant>,
}
```

Each field is documented with rationale and upstream parallels
(see the rustdoc on `PeerLifetimeStats`).

**New field** on `GovernorState`:

```rust
pub lifetime_stats: BTreeMap<SocketAddr, PeerLifetimeStats>,
```

Documented contract: distinct from session-keyed state.  Survives
`record_success` and other session-level resets.  Upstream
parallel: long-lived `KnownPeers.knownPeerInfo` map keyed by
`PeerAddr` from
`Ouroboros.Network.PeerSelection.State.KnownPeers`.

**Three accessor methods** on `GovernorState`:

| Method                                              | Effect                                                     |
| --------------------------------------------------- | ---------------------------------------------------------- |
| `record_lifetime_session_started(peer)`             | bump `sessions`, `successful_handshakes`; set `first_seen` (idempotent), `last_seen` |
| `record_lifetime_session_failure(peer)`             | bump `failures_total`; update `last_seen`                  |
| `record_lifetime_traffic(peer, bytes_in, bytes_out)`| accumulate byte counts; update `last_seen` (no-op if peer entry absent) |

Plus a read-only accessor `lifetime_stats_for(peer) -> Option<&PeerLifetimeStats>`.

### Regression test

`lifetime_stats_accumulate_across_simulated_reconnects` pins the
accumulation contract:

1. Initial state: no entry for the peer.
2. First simulated session: handshake → 1024B in / 256B out → failure.
   - sessions=1, handshakes=1, bytes=1024/256, failures_total=1.
3. Session-keyed reset (`record_failure` + `record_success`): MUST NOT
   touch lifetime stats.  Asserted that `lifetime_stats_for(&peer)`
   is byte-equal before and after the reset.
4. Second simulated session: handshake → 2048B in / 512B out (no failure).
   - sessions=2, handshakes=2, bytes accumulated to 3072/768,
     failures_total still=1.
   - first_seen unchanged from session 1.
   - last_seen advanced.

This test is the contract pin: any future regression that resets
the lifetime stats on session boundaries fails this assertion.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 746 passed / 0 failed / 1 ignored
                                 # (R221 4745 + R222 +1 new test)
```

### Strategic significance

Phase D.2's full scope is "refactor governor's peer state to track
lifetime stats independently of session-state".  R222 lays the
foundation: the data structure is in place, the contract is
documented in rustdoc, and a regression test pins the
accumulation invariant.  Subsequent rounds can wire concrete
update points (handshake completion, mux abort, mini-protocol
byte accounting) without re-litigating the data-model design.

### Open follow-ups (Phase D.2 follow-up slices)

1. **Wire `record_lifetime_session_started`** at the point where
   the NtN handshake completes and the runtime promotes a peer
   from `PeerCold/PeerCooling` to `PeerWarm`.  Likely sites:
   `node/src/server.rs::run_inbound_accept_loop` (inbound) and
   `node/src/runtime.rs::registry_mark_bootstrap_hot` (outbound).
2. **Wire `record_lifetime_session_failure`** at handshake failure
   / mux abort sites.
3. **Wire `record_lifetime_traffic`** from BlockFetch /
   ChainSync / TxSubmission2 byte-accounting paths (the
   `BlockFetchInstrumentation::note_success` already tracks
   per-peer bytes; thread that into the lifetime stats).
4. **Expose lifetime stats via `/metrics`** as
   `yggdrasil_peer_lifetime_sessions{peer="…"}`,
   `yggdrasil_peer_lifetime_bytes_in{peer="…"}`, etc.  This is
   the ultimate observability win: an operator can see real
   churn metrics independent of the live session.

Other deferred items (unchanged from R221):
- Phase D.1 deep cross-epoch rollback recovery.
- Phase E.1 cardano-base coordinated fixture refresh.
- Phase E.2 24h+ mainnet sync rehearsal.
- (de-prioritised by R217) Phase C.2 pipelined fetch+apply.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md)
  step D.2.
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6
  (cumulative phase status — D.2 row should now read "1 of N
  slices done").
- Previous round: [R221](2026-04-30-round-221-chainprovider-tip-point-split.md).
- Touched files (1):
  - `crates/network/src/governor.rs` — new `PeerLifetimeStats`
    struct, new `lifetime_stats` field on `GovernorState`, three
    accessor methods, regression test.
- Upstream reference:
  - `Ouroboros.Network.PeerSelection.State.KnownPeers.knownPeerInfo`
    (per-peer info map persisted across status transitions).
