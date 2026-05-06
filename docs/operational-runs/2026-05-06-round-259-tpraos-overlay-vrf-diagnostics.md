## Round 259 — TPraos active-overlay VRF diagnostic enrichment

Date: 2026-05-06
Branch: main
Type: Diagnostic instrumentation (no behaviour change)

### Goal

Restore slot/era/epoch-nonce context to the trace surface when an
active-overlay TPraos block fails VRF verification. Prior to this
round, the active-overlay path returned `SyncError::Consensus(
ConsensusError::InvalidVrfProof)` directly, which the runtime tracer
formats as the bare `consensus error: invalid VRF proof` — discarding
slot, era, and epoch nonce context that the non-overlay
(Praos pool-leader) path already includes via `SyncError::VrfVerification`.

R249 surfaced this gap on preprod at slot 429460:

```
[Error] Node.Sync ... error=consensus error: invalid VRF proof primaryPeer=3.79.79.217:3001
```

No slot, no era, no nonce — making downstream root-causing impossible
without re-running the sync with patched code.

### Change

`node/src/sync.rs::verify_block_vrf_with_genesis_delegate` now returns
`Result<bool, ConsensusError>` (was `Result<bool, SyncError>`). The
single call site in `advance_ledger_with_epoch_boundary` wraps the
error with `SyncError::VrfVerification { slot, era, epoch_nonce, source }`,
matching the diagnostic surface area the non-overlay path emits.

After this change, an active-overlay VRF failure produces:

```
VRF verification failed at slot 429460 in shelley using epoch nonce <nonce>: invalid VRF proof
```

Both the slot, era, and the underlying `ConsensusError` discriminant
are surfaced — distinguishing `InvalidVrfProof` (proof bytes don't
verify against the input), `WrongGenesisColdKey` (issuer doesn't match
the expected genesis delegate), and `VrfKeyMismatch` (the genesis
delegate's VRF key hash doesn't match the block's VRF vkey hash).

### Why this matters for R253

R253 (preprod TPraos overlay VRF gap, slot ~429460) is currently
blocked on the lack of forensic evidence. The next preprod sync run
will produce one of three actionable error surfaces:

- `WrongGenesisColdKey { expected, actual }` — Yggdrasil's overlay
  schedule selected the wrong genesis delegate index. Compare against
  upstream `lookupInOverlaySchedule` for the same `(first_slot, d,
  active_slot_coeff)` triple.
- `VrfKeyMismatch { expected, actual }` — Yggdrasil decoded the gen_delegs
  map with a corrupted VRF key. Compare gen_delegs map content against
  the Shelley genesis JSON and any subsequent
  `MoveInstantaneousReward`/`UpdateProposal` that updates delegations.
- `InvalidVrfProof` (preserved discriminant) — overlay classification
  + delegate selection are both correct, but the VRF input bytes or
  proof verification differs from upstream. Compare `vrf_input(slot,
  epoch_nonce, mode, VrfUsage::Leader)` byte assembly vs upstream
  `mkSeed seedL slot eta0`.

Whichever branch fires, the next sync's trace will pinpoint which
of the three failure modes preprod hits, replacing days of forensic
guesswork with a single grep.

### Direct upstream-parity check landed

While the diagnostic enrichment was being added, this round also did
a direct byte-for-byte comparison of yggdrasil's overlay
classification against upstream
`Cardano.Protocol.TPraos.Rules.Overlay::classifyOverlaySlot`, using
the exact preprod gen_delegs map from
`.reference-haskell-cardano-node/install/share/preprod/shelley-genesis.json`
(7 genesis delegates, `decentralisationParam=1`,
`activeSlotsCoeff=0.05`). The pin test
`tpraos_overlay_matches_upstream_classifyoverlayslot_preprod_429460_window`
in `node/src/sync.rs` verifies five active overlay slots
(429460, 429480, 429500, 429520, 429540) and four NonActive slots
(429461, 429470, 429479, 429481, 429499) match upstream's
`(position/ascInv) mod length(gkeys)` selection exactly.

Additionally a manual byte-trace verified:

- `is_overlay_slot` matches upstream `Cardano.Ledger.Slot.isOverlaySlot`
  (same `step(s) < step(s+1)` predicate with `step(x) = ceiling(x*d)`).
- `tpraos_vrf_seed` matches upstream
  `Cardano.Protocol.TPraos.BHeader.mkSeed`:
  - both build `slot_be8 || nonce_bytes_or_empty`
  - both `Blake2b256` hash that
  - both XOR with the tag's `mkNonceFromNumber` hash
    (`Blake2b256(word64BE(0|1))`)
- `tpraos_seed_tag_hash` matches upstream `mkNonceFromNumber n` =
  `Nonce(Blake2b256(word64BE(n)))`.

**Conclusion:** the R249 preprod failure is not in overlay
classification, `is_overlay_slot`, or the `mkSeed`/seed-tag
construction. R253's root-cause investigation can skip those layers
and focus on:

1. **gen_delegs activation timing** — does
   `effective_gen_delegs()` return the upstream-correct map at slot
   429460+? Specifically, has `maybe_activate_pending_shelley_genesis`
   been triggered by the first Shelley-family block, AND has any
   subsequent `GenesisDelegateCert` cert that updates
   `_dsFutureGenDelegs`/`_dsGenDelegs` been applied? See
   `crates/ledger/src/state.rs::effective_gen_delegs`
   (line 3480) and `maybe_activate_pending_shelley_genesis` (line 6440).
2. **Block header VRF parsing** — are `block_issuer_vkey`,
   `block_vrf_vkey`, `s.header.body.leader_vrf.proof`, and
   `s.header.body.nonce_vrf.proof` decoded from the on-wire bytes the
   same way upstream `bhBody` parses them? Likely candidate for
   off-by-one or endian flip on `prev_hash`/issuer counter fields.
3. **Ed25519 / IETF VRF draft compatibility** — does
   `yggdrasil_crypto::vrf::VrfVerificationKey::verify` produce
   byte-identical output to upstream `Cardano.Crypto.VRF.Praos.verify`?
   The yggdrasil crypto crate ships its own pure-Rust IETF draft03/13
   implementation; a one-bit divergence in the curve point arithmetic
   or hash-to-curve cofactor handling would surface exactly as
   `InvalidVrfProof` on real-network proofs while passing synthetic
   test vectors.

(1) and (2) are diagnostic-bounded: the next preprod sync's trace will
disambiguate via the `WrongGenesisColdKey` / `VrfKeyMismatch` /
`InvalidVrfProof` discriminant enriched in this round. (3) requires
test-vector comparison against upstream's draft-13 implementation
(`deps/cardano-base/cardano-crypto-praos/src/Cardano/Crypto/VRF/Praos.hs`).

### Testing

The change is a return-type refactor with semantic preservation. The
test suite passes unchanged (4 903 / 0). No new tests required because
the existing `tpraos_overlay_schedule_*` tests already cover the
classification logic, and the VRF verification helpers
(`verify_leader_proof_output`, `verify_nonce_proof`) keep their
existing crypto-layer test coverage in `crates/consensus/src/praos.rs`.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 903 passed, 0 failed
```

### Operator-facing notes

This is purely a diagnostic enrichment. No behaviour change. Operators
running preprod sync past slot ~429460 will see the failure trace
include slot/era/nonce context if the gap reproduces. Pair the trace
with the captured failing-block CBOR (via `node/src/bin/dump_block.rs`
or db-analyser) to feed R253's root-cause investigation.

### References

- R248 (preview overlay-VRF fix): `2026-05-02-round-248-tpraos-overlay-vrf.md`
- R249 (preprod gap surfaced): `2026-05-05-round-249-preprod-vrf-failure-slot-429460.log`
- Upstream: `Cardano.Protocol.TPraos.Rules.Overlay::pbftVrfChecks`
- Upstream: `Cardano.Protocol.TPraos.BHeader::seedL` / `seedEta` / `mkSeed`
