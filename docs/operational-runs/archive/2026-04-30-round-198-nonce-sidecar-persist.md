## Round 198 — Sync-side persist for `nonce_state` (Phase A.2 final)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.2 final slice — wire the sync-runtime persist call so
`<storage_dir>/nonce_state.cbor` is written at every checkpoint
landing.  Combined with R197's read-side load, this delivers
**live nonces in `cardano-cli conway query protocol-state`**.

### Code change

`node/src/sync.rs`:

- New `persist_nonce_state_sidecar(checkpoint_outcome,
  storage_dir, state)` helper that's a no-op unless the
  outcome is `Persisted` AND `storage_dir` is set.  Encodes
  `NonceEvolutionState` via R197's CBOR codec and calls
  `yggdrasil_storage::save_nonce_state`.  Imports `Path`
  alongside the existing `PathBuf`.
- The helper is invoked from the chaindb apply path
  (run_verified_sync_service_chaindb) right after
  `apply_nonce_evolution_to_progress` updates the state.

`node/src/runtime.rs`:

- Three reconnecting/runtime apply sites (run_reconnecting_*,
  resume_reconnecting_*) updated with the same persist logic
  inline (right after `record_verified_batch_progress`).
- Inline construction reads `applied.checkpoint_outcome` (now
  available after R196), `nonce_state` (already in scope), and
  `tracking.ocert_persist_dir` (re-used as the storage_dir).

### Operational verification

After ~30s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ ls -la /tmp/ygg-r198-preview-db/
-rw-r--r-- 1 vscode vscode  114 Apr 30 07:29 nonce_state.cbor
-rw-r--r-- 1 vscode vscode  218 Apr 30 07:29 ocert_counters.cbor
```

**Both sidecars are now persisted.**

```
$ cardano-cli conway query protocol-state --testnet-magic 2
{
    "candidateNonce": "81b581640709619dab8decf3114bdde7dff26e975be18f7ec5ca17c9de288a41",
    "epochNonce": null,
    "evolvingNonce": "81b581640709619dab8decf3114bdde7dff26e975be18f7ec5ca17c9de288a41",
    "labNonce": "0f5d06e7a71ee248e2ecc585d929fe50200fbe88e3eea76b5c19ab300ee3b31c",
    "lastEpochBlockNonce": null,
    "lastSlot": 4960,
    "oCertCounters": {
        "0e0b11e80d958732e587585d30978d683a061831d1b753878f549d05": 0,
        "10257f6d3bae913514bdc96c9170b3166bf6838cca95736b0e418426": 0,
        "7c54a168c731f2f44ced620f3cca7c2bd90731cab223d5167aa994e6": 0,
        "82a02922f10105566b70366b07c758c8134fa91b3d8ae697dfa5e8e0": 0,
        "c44bc2f3cc7e98c0f227aa399e4035c33c0d775a0985875fff488e20": 0,
        "e302198135fb5b00bfe0b9b5623426f7cf03179ab7ba75f945d5b79b": 0,
        "ebe606e22d932d51be2c1ce87e7d7e4c9a7d1f7df4a5535c29e23d22": 0,
        ...
    }
}
```

**`protocol-state` now surfaces live nonces and OCert
counters** instead of placeholders:
- `candidateNonce` and `evolvingNonce` are real Blake2b-256
  hashes computed from VRF outputs of applied blocks.
- `labNonce` is the most recent block's prev-hash-derived
  nonce.
- `epochNonce` and `lastEpochBlockNonce` remain `null` because
  preview is still in epoch 0 (no epoch transition has fired).
- `oCertCounters` includes every block-issuing pool that has
  produced a block — the OCert validation path is wired to
  populate counters on inbound block validation.

Regression checks pass: gov-state / ratify-state /
ledger-peer-snapshot / spo-stake-distribution / future-pparams
all continue to work.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean (one useless-conversion fix)
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

The Phase A.2 nonce arc is complete.  Next slices:

1. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser
   (queries: `leadership-schedule` / `kes-period-info` use it
   internally).
2. **Phase A.7** — active stake distribution amounts in
   `spo-stake-distribution` and `bigLedgerPools` (epoch
   boundary-driven).
3. **Phase A.3 OMap proposals** — gov-state proposal entries
   (requires `GovActionState` shape adaptation from yggdrasil's
   reduced 4-field to upstream's 7-field record).
4. **Phase B** — R91 multi-peer dispatch storage livelock.
5. **Phase C/D/E** — sync perf, deep cross-epoch rollback,
   mainnet rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/sync.rs`](node/src/sync.rs) — new
  `persist_nonce_state_sidecar` helper +
  `run_verified_sync_service_chaindb` invocation;
  [`node/src/runtime.rs`](node/src/runtime.rs) — inline
  persist at 3 reconnecting-runtime apply sites.
- Upstream reference:
  `Ouroboros.Consensus.Protocol.Praos.PraosState.csCounters`
  (OCert counters);
  `Cardano.Protocol.TPraos.API.ChainDepState` (nonce evolution
  + OCert counters as one persistent record).
- Yggdrasil persistence: `nonce_state.cbor` and
  `ocert_counters.cbor` sidecars under `<storage_dir>/`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-197-nonce-sidecar-codec.md`](2026-04-30-round-197-nonce-sidecar-codec.md).
