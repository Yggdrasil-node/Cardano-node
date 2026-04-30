## Round 197 — `NonceEvolutionState` CBOR codec + sidecar load (Phase A.2 next)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.2 next slice — extend the R196 sidecar plumbing to
also load persisted `NonceEvolutionState` from
`<storage_dir>/nonce_state.cbor` and surface live nonces in
`cardano-cli conway query protocol-state`.  Mirrors R196's
read-first-write-later pattern: ship the codec + storage
helpers + read-side load now, defer the sync-side persist call
to a follow-up.

### Code change

`crates/consensus/src/nonce.rs`:

- New `CborEncode` and `CborDecode` impls for
  `NonceEvolutionState`.  6-element CBOR list:
  `[evolving, candidate, epoch, prev_hash, lab, current_epoch]`.
  Each `Nonce` uses upstream `Cardano.Ledger.Crypto.Nonce` wire
  shape (`NeutralNonce → [0]`, `Nonce h → [1, h]`).  Local
  helpers `encode_nonce` / `decode_nonce` factor the per-field
  encoding.

`crates/storage/src/ocert_sidecar.rs`:

- New `NONCE_STATE_FILENAME = "nonce_state.cbor"` constant.
- New `nonce_sidecar_path(dir)` private helper.
- New `save_nonce_state(dir, encoded)` / `load_nonce_state(dir)`
  helpers, mirroring the existing OCert sidecar atomic-write
  contract.

`crates/storage/src/lib.rs`:

- Re-exports `NONCE_STATE_FILENAME`, `save_nonce_state`,
  `load_nonce_state` alongside existing OCert sidecar exports.

`node/src/local_server.rs`:

- `attach_chain_dep_state_from_sidecar` now also calls
  `load_nonce_state` and decodes the persisted
  `NonceEvolutionState`.  Maps yggdrasil's 5-nonce shape into
  upstream's 6-nonce `PraosState`:
  - `evolving_nonce`           → `praosStateEvolvingNonce`
  - `candidate_nonce`          → `praosStateCandidateNonce`
  - `epoch_nonce`              → `praosStateEpochNonce`
  - `prev_hash_nonce`          → `praosStateLastEpochBlockNonce`
  - `lab_nonce`                → `praosStateLabNonce`
  - `previous_epoch_nonce`     → Neutral (yggdrasil doesn't
    track this distinctly from the active epoch nonce).
- Both sidecars (OCert + nonce) are independently optional;
  any combination of present/absent files is handled.  Missing
  or undecodeable files leave the corresponding field at the
  `ChainDepStateContext::default()` neutral.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query protocol-state --testnet-magic 2
{
    "candidateNonce": null,
    "epochNonce": null,
    "evolvingNonce": null,
    "labNonce": null,
    "lastEpochBlockNonce": null,
    "lastSlot": 1960,
    "oCertCounters": {}
}

$ ls /tmp/ygg-r197-preview-db/
immutable
ledger
ocert_counters.cbor
volatile
```

`nonce_state.cbor` is not yet present in the storage directory
because the sync-side persist call is deferred to a follow-up
round.  When it lands, the same read path will surface live
nonces in `protocol-state` with no further encoder changes.
The read-side gracefully returns the neutral default when the
file is missing.

Regression checks pass: gov-state / ratify-state /
ledger-peer-snapshot / spo-stake-distribution continue to work.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

1. **Sync-side persist for nonce_state**: at the same
   checkpoint-write site that produces `ocert_counters.cbor`
   (after `apply_nonce_evolution_to_progress`), encode
   `nonce_state` via the new `CborEncode` impl and call
   `yggdrasil_storage::save_nonce_state(dir, &encoded)`.  The
   call sites are in `node/src/sync.rs` (line ~2087) and
   `node/src/runtime.rs` (line ~5001 / 5553).
2. **OCert validate-and-update wiring** in the verified-sync
   apply path so `ocert_counters.cbor` accumulates real
   per-pool counters (R196 follow-up).
3. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
4. **Phase A.7** — active stake distribution amounts.
5. **Phase A.3 OMap proposals** — gov-state proposal entries.
6. **Phase B** — R91 multi-peer livelock.
7. **Phase C/D/E** — sync perf, deep rollback, mainnet rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`crates/consensus/src/nonce.rs`](crates/consensus/src/nonce.rs)
  — new CBOR codec for `NonceEvolutionState` (6-element list);
  [`crates/storage/src/ocert_sidecar.rs`](crates/storage/src/ocert_sidecar.rs)
  — new `save_nonce_state` / `load_nonce_state` helpers;
  [`crates/storage/src/lib.rs`](crates/storage/src/lib.rs)
  — re-exports;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  extended `attach_chain_dep_state_from_sidecar` to load
  nonce sidecar.
- Upstream reference:
  `Ouroboros.Consensus.Protocol.Praos.PraosState` (8-element
  record, of which 6 are nonce fields + opcert counters);
  `Cardano.Ledger.Crypto.Nonce`.
- Yggdrasil reference: `NonceEvolutionState` in
  `crates/consensus/src/nonce.rs` (5 nonce fields + epoch).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-196-ocert-sidecar-load.md`](2026-04-30-round-196-ocert-sidecar-load.md).
