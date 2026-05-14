## Round 262 — R253 final narrowing: TPraos epoch nonce evolution

Date: 2026-05-06
Branch: main
Type: Forensic capture from live preprod sync (no behaviour change)

### Goal

Run a fresh preprod sync with R259's enriched VRF diagnostic active,
capture the failure, and use the new error discriminant to narrow R253
to its final root-cause candidate.

### Captured failure

Started from genesis with `--peer 3.79.79.217:3001`, knob=2 default
multi-peer, fresh DB. Sync progressed cleanly through Byron and into
Shelley. Failure trace:

```
[Error] Node.Sync error=VRF verification failed at slot 432000 in Shelley
        using epoch nonce Hash([202, 23, 16, 80, 171, 175, 28, 6,
                                140, 76, 59, 167, 27, 63, 178, 195,
                                248, 203, 86, 127, 190, 178, 163, 89,
                                4, 182, 55, 157, 11, 187, 78, 148]):
        invalid VRF proof
```

Decoded:

- **Slot**: 432000 (in preprod's first Shelley epoch — epoch 4 label,
  offset 345600 from Shelley boundary)
- **Era**: Shelley (TPraos)
- **Active epoch nonce**: `0xca171050abaf1c068c4c3ba71b3fb2c3f8cb567fbeb2a35904b6379d0bbb4e94`
- **Discriminant**: `InvalidVrfProof`

The discriminant is **NOT** `WrongGenesisColdKey` or `VrfKeyMismatch`
— so:

1. Issuer's cold key passed `Blake2b224(issuer_vkey) == delegation.delegate`.
2. Issuer's VRF key passed `Blake2b256(vrf_vkey) == delegation.vrf`.

In other words, **yggdrasil correctly identified the genesis delegate
that issued this block AND correctly extracted the delegate's VRF
public key from the block header.** The actual VRF proof verification
is what fails.

### Static checks now ruled out (this round)

- ✅ Header VRF proof field length: `ShelleyVrfCert` decoder reads
  `[output_bytes, 80_byte_proof]` per upstream CDDL `vrf_cert = [bytes,
  bytes .size 80]`. R261 confirmed the 80-byte ECVRF Praos draft03
  expectation. (`crates/ledger/src/eras/shelley.rs:1148-1182`)
- ✅ VRF mode dispatch: `verify_block_vrf_with_genesis_delegate` calls
  `verify_leader_proof_output(..., VrfMode::TPraos)` — the IETF draft03
  path tested against the upstream `vrf_ver03_*` corpus.
  (`node/src/sync.rs:4602`)
- ✅ Cold-key + VRF-key byte extraction: works correctly (the trace
  did NOT fire `WrongGenesisColdKey` or `VrfKeyMismatch`).

### Remaining candidate (narrowed to one)

The cumulative ruled-out surface is now:

- R259 — overlay classification matches upstream byte-for-byte (preprod
  fixture test).
- R261 — JSON-parse of preprod `genDelegs` map is byte-correct.
- R261 — VRF crypto matches upstream byte-for-byte across all
  `vrf_ver03_*` and `vrf_ver13_*` corpora.
- R261 — `tpraos_vrf_seed(slot, eta, usage)` matches upstream `mkSeed`
  byte-for-byte.
- R262 (this round) — Shelley `ShelleyVrfCert` decoder consumes the
  spec-correct 80-byte proof; cold key + VRF key extraction work.

What's left: **the active epoch nonce η_e supplied to the VRF
verifier at slot 432000 differs from what upstream Haskell would
compute at the same slot.**

`Blake2b256(word64BE 0) = 81e47a19e6b29b0a65b9591762ce5143ed30d0261e5d24a3201752506b20f15c`
(upstream `mkNonceFromNumber 0` initial-nonce). Yggdrasil's η at slot
432000 is `0xca171050…` — clearly evolved (not bootstrap), but the
question is whether the evolution path yggdrasil ran matches upstream
`applyChainTickRule` + `applyTickRule` + `ticknStateOf` exactly.

### Next steps for R253 closure

R253 root-cause is now bounded to the TPraos epoch-nonce evolution
path. The remaining work needs upstream-reference comparison:

1. **Identify the divergence slot**: the first slot at which
   yggdrasil's η differs from upstream's. Run both nodes side-by-side
   on the same chain prefix; the first divergent η reveals which
   nonce-evolution rule diverged.
2. **Code surface** to focus: `apply_nonce_evolution` in
   `node/src/sync.rs:4628`, plus the upstream rules
   `Cardano.Ledger.Shelley.Rules.Tickn::TICKN` and
   `Cardano.Protocol.TPraos.Rules.Tickn::ticknTransition` at
   `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Tickn.hs`.
3. **Possible hypothesis** (untested): the candidate-nonce freeze at
   the randomness stabilization window (`4k/f` slots into the epoch,
   = 172800 slots for preprod) may fire at the wrong slot in
   yggdrasil. For preprod's first Shelley epoch, the freeze should be
   at slot 86400 + 172800 = 259200; if yggdrasil computes that
   boundary differently, the η for epoch 5+ would be wrong. But
   slot 432000 is still in epoch 4, so the active η at 432000 is the
   value computed at the Byron→Shelley transition, not from a freeze.
   Most likely divergence point: how yggdrasil computes the active
   η_4 at the Byron→Shelley boundary itself.
4. **Forensic harness available**: `.reference-haskell-cardano-node/install/bin/cardano-node`
   can sync preprod in parallel; `.../install/bin/db-analyser
   --analyse-from 0 --num-blocks-to-process 100` against the resulting
   ChainDB can dump the per-epoch η values upstream computes for
   comparison.

### What this round shipped

- **Forensic data captured** — first preprod sync since R259 with
  the diagnostic enrichment active. R259's
  `SyncError::VrfVerification` wrapper produced exactly the
  bounded discriminator the advisor predicted.
- **Three more sub-candidates ruled out** — proof field length,
  VRF mode dispatch, cold/VRF key extraction.
- **R253 narrowed to one final surface** — TPraos epoch nonce
  evolution, with concrete code-search starting point and
  upstream-comparison harness identified.

The R253 close-out is now operator-time work (run upstream Haskell
in parallel, compare η values) plus a code-side fix once the
divergence slot is identified.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 845 passed, 0 failed
```

### References

- R259 diagnostic enrichment: `2026-05-06-round-259-tpraos-overlay-vrf-diagnostics.md`
- R261 sub-candidate narrowing: `2026-05-06-round-261-r253-narrowing.md`
- Yggdrasil nonce evolution: `node/src/sync.rs:4628::apply_nonce_evolution`
- Upstream rules: `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Tickn.hs`
- Captured failing trace: `/tmp/ygg-r261-preprod/out.log`
