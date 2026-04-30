## Round 196 — OCert counter sidecar load (Phase A.2 partial)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Wire the read-side path for live PraosState data: at LSQ
snapshot acquisition time, load the persisted OCert counter
sidecar (`ocert_counters.cbor`) from the storage directory and
attach the values to the `LedgerStateSnapshot` via the R192
`ChainDepStateContext` channel.  This is **Phase A.2 partial**
— covers OCert counters; nonces remain a follow-up because the
sync runtime doesn't yet persist them.

### Code change

`node/src/local_server.rs`:

- New `attach_chain_dep_state_from_sidecar(snapshot,
  storage_dir)` helper:
  - Calls `yggdrasil_storage::load_ocert_counters(dir)` to read
    the `ocert_counters.cbor` sidecar produced by
    `update_ledger_checkpoint_after_progress`.
  - Decodes via `OcertCounters::decode_cbor` (existing
    `CborDecode` impl in `crates/consensus/src/opcert.rs`).
  - Translates `OcertCounters::iter()` entries into the
    `ChainDepStateContext::opcert_counters` map.
  - Calls `snapshot.with_chain_dep_state(ctx)` to attach the
    context.
- `acquire_snapshot` accepts an optional `storage_dir:
  Option<&Path>`, calls the new helper after recovery.
- `run_local_state_query_session` accepts `storage_dir:
  Option<PathBuf>` and threads it to each `acquire_snapshot`
  call (Acquire and ReAcquire).
- `run_local_client_session` accepts `storage_dir:
  Option<PathBuf>` and forwards to the LSQ session task.
- `run_local_accept_loop` accepts `storage_dir:
  Option<PathBuf>` and threads it through to each spawned
  client session.
- `run_local_client_session` annotated with
  `#[allow(clippy::too_many_arguments)]` (now 9 params,
  matching the existing `run_local_accept_loop` annotation).

`node/src/main.rs`:

- Constructs `ntc_storage_dir = Some(storage_dir.clone())` from
  the loaded `node_config.storage_dir` and passes to
  `run_local_accept_loop`.

`node/tests/local_ntc.rs`:

- Both `run_local_accept_loop` test call sites updated to pass
  `None` for `storage_dir` (in-memory test fixtures don't have
  a real storage directory).

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

$ ls -la /tmp/ygg-r196-preview-db/ocert_counters.cbor
-rw-r--r-- 1 vscode vscode 1 Apr 30 07:04 ocert_counters.cbor
$ stat -c "size=%s" /tmp/ygg-r196-preview-db/ocert_counters.cbor
size=1
```

The sidecar **is being loaded and decoded successfully** — its
single byte (`0xa0` empty CBOR map) round-trips through
`load_ocert_counters` → `OcertCounters::decode_cbor` →
`ChainDepStateContext::opcert_counters` → JSON `{}`.

`oCertCounters: {}` reflects the persisted state correctly:
yggdrasil's verified-sync flow doesn't currently invoke the
OpCert counter validation path on inbound blocks (the
`OcertCounters::validate_and_update` call site exists in the
header-validation path used during block production but not in
the verified-sync block apply path).  Once the sync layer
populates counters, the same plumbing will surface them
without further code changes.

Regression checks pass for all other LSQ queries:

```
$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{ "bigLedgerPools": [{...3 pools with relays...}], ... }
```

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-fmt fix)
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

Continuing the data-plumbing arc:

1. **Phase A.2 — full nonce attach**: yggdrasil's sync runtime
   tracks `NonceEvolutionState` mutably but doesn't yet persist
   to a sidecar.  Add `nonce_state.cbor` persistence at
   checkpoint write time mirroring the OCert counter pattern,
   then load alongside in `attach_chain_dep_state_from_sidecar`.
2. **OCert counter population in verified-sync apply path**:
   yggdrasil's verified-sync block apply doesn't invoke
   `OcertCounters::validate_and_update`.  Once that's wired,
   the sidecar will accumulate real per-pool counters and
   `protocol-state` will surface them.
3. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
4. **Phase A.7** — active stake distribution amounts in
   spo-stake-distribution and ledger-peer-snapshot AccPoolStake.
5. **Phase A.3 OMap proposals** — gov-state proposal entries.
6. **Phase B** — R91 multi-peer livelock.
7. **Phase C/D/E** — sync perf, deep rollback, mainnet rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) — new
  `attach_chain_dep_state_from_sidecar` helper +
  `acquire_snapshot` / `run_local_state_query_session` /
  `run_local_client_session` / `run_local_accept_loop` updated;
  [`node/src/main.rs`](node/src/main.rs) — passes
  `node_config.storage_dir` to the accept loop;
  [`node/tests/local_ntc.rs`](node/tests/local_ntc.rs) — test
  call sites updated.
- Upstream reference:
  `Ouroboros.Consensus.Protocol.Praos.PraosState.csCounters`
  (per-pool monotonic OpCert sequence-number tracker).
- Yggdrasil reference: `OcertCounters` in
  `crates/consensus/src/opcert.rs`;
  `yggdrasil_storage::{save,load}_ocert_counters` in
  `crates/storage/src/ocert_sidecar.rs`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-195-ledger-peer-pools-live.md`](2026-04-30-round-195-ledger-peer-pools-live.md).
