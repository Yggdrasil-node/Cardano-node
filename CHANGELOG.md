# Changelog

All notable changes to Yggdrasil are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Operational-parity arc on top of v0.2.0 — Rounds 144 → 208.
Highlights: full cardano-cli 10.16 query parity at preprod (Shelley
era) and preview (Alonzo era), **every Conway-era LSQ subcommand**
(constitution, gov-state, drep-state, drep-stake-distribution,
committee-state, treasury, spo-stake-distribution, proposals,
ratify-state, future-pparams, stake-pool-default-vote,
ledger-peer-snapshot) decoding end-to-end, multi-round
sync-speed and apply-correctness fixes.  Workspace tests:
4 640 (v0.2.0) → **4 744 passing, 0 failing**.

### Added

- **`cardano-cli query` end-to-end parity at preprod (Shelley)
  and preview (Alonzo)**.  All 11 working cardano-cli
  operations — `tip`, `protocol-parameters` (Shelley/Alonzo/
  Babbage/Conway shapes), `era-history`, `slot-number`,
  `utxo --whole-utxo`, `utxo --address X`, `utxo --tx-in T#i`,
  `tx-mempool info` / `next-tx` / `tx-exists`, `submit-tx` —
  decode end-to-end against yggdrasil's NtC socket.
  Verified Rounds 144–164.
- **`YGG_LSQ_ERA_FLOOR=N` env var (Round 178).**  Operator
  opt-in floor on the LSQ-reported era so cardano-cli's
  client-side Babbage+ gate can be bypassed on partial-sync
  chains.  With `YGG_LSQ_ERA_FLOOR=6` the era-gated queries
  (`stake-pools`, `stake-distribution`, `pool-state`,
  `stake-snapshot`, `stake-address-info`) become reachable
  without waiting for the natural Babbage hard-fork.
- **Conway-era LSQ queries (Rounds 180–189) — complete.**
  Every `cardano-cli conway query` subcommand decodes
  end-to-end against yggdrasil:
  `constitution`, `gov-state` (R188, full 7-element
  `ConwayGovState`),
  `drep-state --all-dreps`, `drep-stake-distribution`,
  `committee-state`, `treasury` (via `GetAccountState`),
  `spo-stake-distribution`, `proposals`, `ratify-state` (R187,
  real EnactState with constitution + 31-element PParams +
  treasury), `future-pparams` (R183, `Maybe Nothing`),
  `stake-pool-default-vote`, `ledger-peer-snapshot` (R189,
  V2 form `{"bigLedgerPools": [], "slotNo": "origin",
  "version": 2}`).  Constitution returns real Conway data
  from the chain; the rest return correct empty/placeholder
  shapes for fresh-sync chains.  R184 surfaced a 3-call flow
  inside `query spo-stake-distribution`: SPOStakeDistr (tag
  30) → GetCBOR(GetPoolState) (9→19) →
  GetFilteredVoteDelegatees (tag 28); all three dispatchers
  added in one round.  **The Conway-era LSQ wire-protocol
  gap is now closed entirely.**
- **`yggdrasil_current_era` Prometheus gauge (Round 169)**
  reports the wire era ordinal (`0=Byron, 1=Shelley, …,
  6=Conway`) of the latest applied block.
- **Per-era applied-block counters (Round 170)** —
  `yggdrasil_blocks_byron`, `…_shelley`, `…_allegra`, `…_mary`,
  `…_alonzo`, `…_babbage`, `…_conway` Prometheus counters
  let dashboards graph the share of blocks applied per era
  during a long sync.

### Changed

- **Default `--batch-size` 10 → 30 → 50** (Rounds 165, 166).
  Out-of-the-box preprod sync improves from ~5 blocks/sec at
  the original default to ~14 blocks/sec at the new default
  by amortising per-batch overhead and unblocking the
  initial-sync rollback fast path.  Past 50 the throughput
  plateaus on peer-side fetch latency.
- **Initial-sync rollback fast path** (Round 166) skips the
  heavy `recover_ledger_state_chaindb` replay when the
  rollback target is `Origin` and the base ledger state is
  empty, letting the boundary-aware forward-apply path fire
  epoch transitions correctly.
- **LSQ era-specific tag table re-corrected (Round 179)** —
  R163's tag numbers for `GetStakePools` (was 13, upstream
  is 16), `GetStakePoolParams` (was 14, upstream is 17),
  `GetPoolState` (was 17, upstream is 19), `GetStakeSnapshots`
  (was 18, upstream is 20) are now aligned with cardano-node
  10.7.x's `Ouroboros.Consensus.Shelley.Ledger.Query
  .encodeShelleyQuery`.  Bug masked R163-R178 because
  cardano-cli's client-side era gate refused to send these
  queries.

### Fixed

- **Mid-sync rollback epoch fixup (Round 167)** — when
  `recover_ledger_state` replays the volatile suffix via
  `apply_block` (no boundary detection), `current_epoch` is
  now patched post-recovery to match the recovered tip's
  slot.  Prevents PPUP validation errors on cross-epoch
  rollback.
- **`yggdrasil_active_peers` metric reported 0 during active
  sync** (Round 168).  Bootstrap sync peer is now marked
  `PeerHot` in the registry at session establishment and
  demoted at teardown so `/metrics` reflects the actual
  active session.  Round 175 added cooling at the missed
  `KeepAlive`-failure and session-switching mux-abort sites.
- **Era blockage end-to-end fix (Round 179)**.  Three
  independent bugs unblocked: (1) wrong tag numbers
  (R163-R178); (2) `cardano-cli query stake-distribution`
  uses tag 37 `GetStakeDistribution2` (post-Conway no-VRF
  variant) returning `[map, NonZero Coin]` not bare map;
  (3) `query pool-state` and `query stake-snapshot` use tag
  9 `GetCBOR` wrapper.  All five era-gated queries now
  decode end-to-end against cardano-cli 10.16 with
  `YGG_LSQ_ERA_FLOOR=6`.
- **Decoder strictness (Rounds 174, 176)** — five CBOR
  set-decoder helpers (`decode_pool_hash_set`,
  `decode_stake_credential_set`, `decode_address_set`,
  `decode_txin_set`, `decode_maybe_pool_hash_set`) now
  enforce CIP-21 tag 258 strictly and `Maybe Nothing`
  shortcut requires bare `null` (`0xf6`) rather than any
  CBOR major-7 byte.  Pre-fix malformed payloads were
  silently mis-parsed.
- **`encode_filtered_delegations_and_rewards` correctness
  (Round 177)** — three independent bugs: non-deterministic
  HashSet iteration order, O(N·M) inner search per
  credential, and reward-account lookup mis-matched on hash
  bytes alone (stripping AddrKey-vs-Script discriminator).
  Fixed via sort-then-iterate, `BTreeMap::get`, and
  `find_account_by_credential` (full credential match).
- **`DrepState` LSQ map shape (Round 181)** —
  `GetDRepState` now emits a CBOR map (`encCBOR @(Map a b)`)
  instead of the storage-format array-of-pairs.  cardano-cli
  no longer rejects with `expected map len or indef`.

### Operational notes

- The R178 `YGG_LSQ_ERA_FLOOR` env var is opt-in and
  documented; default behaviour is unchanged.
- The R179 tag-table correction is the major user-visible
  unblocker.  Operators on partial-sync chains (preprod /
  preview before reaching natural Babbage) can now exercise
  the full Conway governance query surface.
- Sync default `--batch-size 50` is safe (boundary-aware
  apply path); legacy operators wanting the old behaviour
  can pass `--batch-size 10` explicitly.


## [0.2.0] - 2026-04-27

Operational-parity, byzantine-path, and recovery-correctness release on
top of v0.1.0.  Highlights: full byzantine-path audit closure
(Rounds 87 / 88 / 89), multi-peer BlockFetch dispatch wiring (with
Round 90 closing the session-handoff `RollbackPointNotFound` crash),
zero-copy `Block.raw_cbor` clone via `Arc<[u8]>` (F-2), single-shot
`BlockTxRawSpans` cache shared by the eviction + apply + ledger-advance
consumers (F-1), sealed `ShelleyCompatibleSubmittedTx` /
`AlonzoCompatibleSubmittedTx` invariants (Q-1), `cargo fmt --check`
enforcement in CI, and a self-contained devcontainer that pre-installs
the upstream IntersectMBO Haskell `cardano-cli` + `cardano-node`
binaries for the §5 / §6.5b operator rehearsals.

Workspace tests: 4 210 (v0.1.0) → **4 640 passing, 0 failing**.

The Round 91 multi-peer storage-persistence livelock (Gap BN) remains
open and is documented as a Known Issue below.  The production default
`max_concurrent_block_fetch_peers = 1` keeps the legacy single-peer
path active until that closes.

### Fixed

- **Fee-validation parity bug at the preprod Byron→Shelley boundary
  (slot ~518 460).** Previously `*_block_to_block` in
  `node/src/sync.rs` re-serialised typed `ShelleyTxBody` /
  `ShelleyWitnessSet` to compute `tx_size`, which produced
  byte-canonical CBOR that did not always match the on-wire encoding
  the block author chose (definite vs indefinite length, set vs
  array, integer-width canonicalisation).  The 10-byte drift was
  enough to shift `min_fee = 44 · txSize + 155 381` past the declared
  fee on a real preprod transaction (440 lovelace gap; ~0.2 %).  Fix:
  new helper `yggdrasil_ledger::extract_block_tx_byte_spans` walks
  the outer block CBOR and returns the on-wire byte spans for every
  `transaction_body` / `transaction_witness_set`; the four era
  converters (`shelley`/`alonzo`/`babbage`/`conway`) now take
  `raw_block_bytes: &[u8]` and use those spans for `tx.body`,
  `tx.witnesses`, and `tx_id` hashing.  `TypedSyncStep::RollForward`
  and `MultiEraSyncStep::RollForward` thread raw bytes alongside the
  typed values, sourced from the existing
  `BlockFetchClient::request_range_collect_points_raw_with` API.
  4 new regression tests in `crates/ledger/src/cbor.rs` exercise the
  helper, including a deliberately mismatched indefinite-length-array
  case that proves on-wire byte preservation.  Surfaced in the
  2026-04-27 operational quality-check pass; details in
  [`docs/REAL_PREPROD_POOL_VERIFICATION.md`](docs/REAL_PREPROD_POOL_VERIFICATION.md).

### Changed

- **Submitted-tx invariant hardening (`Q-1`).**  The `raw_body` and
  `raw_cbor` fields on `ShelleyCompatibleSubmittedTx<TxBody>` and
  `AlonzoCompatibleSubmittedTx<TxBody>` were demoted from `pub` to
  `pub(crate)` to prevent external code from mutating `body` and
  silently desyncing the canonical-bytes invariant that `tx_id()` and
  fee `tx_size` rely on.  New public read accessors `raw_body() ->
  &[u8]` and `raw_cbor() -> &[u8]` replace direct field access.
  External code that previously read these fields directly must now
  use the accessors; external constructors (struct literals) must use
  the existing `::new(...)` constructors instead.

- **Authoritative `tx_id` derivation centralised on `raw_body`.**
  `MultiEraSubmittedTx::Shelley` now wraps
  `ShelleyCompatibleSubmittedTx<ShelleyTxBody>` (preserving the on-
  wire `raw_body` / `raw_cbor` byte spans, like every other era arm
  already did), and `MultiEraSubmittedTx::tx_id()` delegates uniformly
  to each variant's `tx.tx_id()`.  Three ledger-side validation sites
  in `crates/ledger/src/state.rs` switched from
  `tx.body.to_cbor_bytes()` / `tx.to_cbor_bytes().len()` to
  `tx.raw_body` / `tx.raw_cbor.len()`, removing one O(n) re-encode +
  alloc per submitted transaction in the mempool admission and apply
  paths.  New regression test
  `shelley_submitted_tx_id_uses_on_wire_bytes_not_re_encoded` in
  `crates/ledger/tests/integration/shelley.rs` decodes a deliberately
  non-canonical Shelley tx (over-long `uint64` for `fee`) and verifies
  `tx_id() == hash(raw_body) ≠ hash(body.to_cbor_bytes())`, locking in
  the on-wire-byte contract against future regressions.

### Performance

- **One-shot `BlockTxRawSpans` cache on `MultiEraSyncStep::RollForward`.**
  Span extraction is now performed exactly once per block at sync-step
  construction (`node::sync::extract_spans_per_block`) and shared by
  all three roll-forward consumers (mempool eviction via
  `extract_tx_ids`, volatile-store apply via
  `apply_multi_era_step_to_volatile`, and ledger-state advance via
  `advance_ledger_state_with_progress`).  Before this change, every
  confirmed block triggered three independent
  `yggdrasil_ledger::extract_block_tx_byte_spans` walks of the same
  CBOR; the cache cuts that to one.  Implementation:
  `MultiEraSyncStep::RollForward` gained a parallel
  `block_spans: Vec<BlockTxRawSpans>` field; new public
  `*_block_to_block_with_spans` variants for Shelley / Alonzo /
  Babbage / Conway / multi-era consume pre-extracted spans;
  `extract_tx_ids` signature changed from `(block, &[u8])` to
  `(block, Option<&BlockTxRawSpans>)`; the closure passed to
  `for_each_roll_forward_block` now receives spans alongside the
  block and raw bytes.  The three Alonzo-family `*_with_spans`
  helpers (60 lines each, identical modulo era tag and typed block
  struct) are generated by a single `alonzo_family_block_to_block_with_spans!`
  macro to keep the duplication-eliminated.  Test count grew by 1
  (the L-1 fixture above); workspace remains green at 4 636 passing.
- **Zero-copy `Block.raw_cbor` cloning (`F-2`).**  `Block.raw_cbor:
  Option<Vec<u8>>` switched to `Option<Arc<[u8]>>`.  Storage's per-
  block clone (volatile-DB `prefix_up_to`, immutable-DB `suffix_after`,
  `chain_db.append_block`) and the per-apply assignment in
  `node/src/sync.rs::apply_multi_era_step_to_volatile` are now atomic
  refcount bumps instead of full ~80 KB heap copies for typical Conway
  blocks.  The BlockFetch trait boundary (`BlockProvider::get_block_range`
  -> `Vec<Vec<u8>>`) still pays one `Arc::to_vec()` at re-serve time,
  so the net win is one fewer alloc per block per re-serve.  On-disk
  CBOR encoding is unchanged: `serde/rc` is now enabled workspace-wide
  and `Arc<[u8]>` serializes to the same RFC 8949 byte-string as
  `Vec<u8>`.  New regression test `block_raw_cbor_arc_serde_round_trip`
  in `crates/storage/tests/integration.rs` locks the byte-equivalence.
- **CI now enforces `cargo fmt --all -- --check`.**  Previously the
  workflow installed `rustfmt` but never ran it; format drift could
  reach `main` undetected.

### Documentation

- **CI-gate prose alignment.**  `CLAUDE.md`, `docs/CONTRIBUTING.md`, and
  `docs/code-audit.md` now list all four CI gates
  (`fmt --all -- --check`, `check-all`, `test-all`, `lint`) — previously
  three files claimed only the trio (`check-all` / `test-all` / `lint`)
  even though `cargo fmt --check` has been a CI step since iteration 1.
- **Arithmetic conventions documented in `crates/ledger/AGENTS.md`.**
  Audit pass over the 164 `saturating_*` call sites across 11 ledger
  files confirmed each is bounded by validated protocol parameters,
  total-ADA-supply caps, or fixed parser depth.  The convention
  (`checked_*` for value-preservation paths surfacing
  `LedgerError::ValueOverflow`; `saturating_*` everywhere the upper
  bound is upstream-enforced) is now codified in the crate AGENTS.md
  with a pointer to the canonical rationale at
  [`crates/ledger/src/fees.rs:14-22`](crates/ledger/src/fees.rs).
- **Round 84 parity-audit-history entry.**  `docs/PARITY_SUMMARY.md`
  records the Q-1 / F-2 closure with anchored upstream references.

### Known Issues

- **§6.5a multi-peer dispatch — `ChainState` advances but `volatile`
  storage stays empty (Round 91 Gap BN, OPEN).**  After Round 90
  closed the hard-crash path, the same §6.5a rehearsal reveals that
  multi-peer dispatch advances the in-memory chain (`from_point` at
  ~slot 102 240) but **persists no blocks to `volatile/` /
  `immutable/` / `ledger/`** — the per-peer `FetchWorkerPool`
  reassembly is not feeding into `apply_multi_era_step_to_volatile`.
  The Round 90 realignment now keeps the node alive across this
  livelock (5 successful handoffs + 0 crashes confirmed on the
  2026-04-27 90-second rehearsal), but the node re-syncs from Origin
  on every session handoff, so it never reaches a stable steady-
  state.  Investigation entry points:
  `node/src/sync.rs::dispatch_range_with_tentative`,
  `execute_multi_peer_blockfetch_plan`, the reorder-buffer →
  apply-step seam.  Production default
  `max_concurrent_block_fetch_peers = 1` MUST stay until this also
  closes.

### Fixed

- **§6.5a multi-peer dispatch — session-handoff `RollbackPointNotFound`
  crash (Round 90 Gap BM).**  With
  `--max-concurrent-block-fetch-peers 2` and ≥ 3 `localRoots`, the
  multi-peer BlockFetch worker pool activates correctly
  (`yggdrasil_blockfetch_workers_registered = 3`,
  `_migrated_total = 3`) but within ~30 s of preprod sync the
  governor's `Net.PeerSelection: switching sync session to
  higher-tip hot peer` path triggered a reconnect, the re-established
  session resumed from `fromPoint=BlockPoint(N, H)`, and
  `roll_backward` on the in-memory `ChainState` returned
  `RollbackPointNotFound { slot: N, hash: H }` — crashing the node.
  Not the Round 88 fresh-restart bug — `ChainState` was the same
  in-memory object across the reconnect loop, but `from_point` had
  advanced past whatever the volatile store actually held (e.g.,
  `from_point` at slot 102 240 vs storage tip at Origin, observed
  live).  Fix: at the top of every reconnect-loop iteration in both
  `run_reconnecting_verified_sync_service_chaindb_inner` and
  `run_reconnecting_verified_sync_service_shared_chaindb_inner`,
  re-seed `chain_state` from the volatile DB AND realign
  `from_point` to `chain_state.tip()` — emitting
  `Net.PeerSelection: realigning from_point to volatile storage tip
  before reconnect` whenever they differ.  This makes the resume
  self-consistent regardless of what diverged in the prior session:
  the next peer's `RollBackward(from_point)` confirmation always
  finds the target.  Verified end-to-end on the 2026-04-27 §6.5a
  rehearsal — 5 realignments handled cleanly + 0 crashes over
  1 m 31 s, was crashing at 30 s pre-fix.  Forensic log:
  `/tmp/ygg-multi-peer-rollback-crash-2026-04-27.log`.

### Added

- **CLI override for `max_concurrent_block_fetch_peers`.**  New
  `--max-concurrent-block-fetch-peers <N>` flag on the `run` subcommand,
  matching the existing override pattern for `--peer`, `--port`,
  `--metrics-port`.  Lets the §6.5 multi-peer BlockFetch rehearsal
  flip the knob without editing the vendored config files; replaces
  the previously-documented (but unimplemented)
  `NODE_CONFIG_OVERRIDE_max_concurrent_block_fetch_peers` env-var
  pattern in the runbook.
- **Devcontainer setup for the full operator-rehearsal toolchain.**
  `.devcontainer/devcontainer.json` now declares the Rust 1.95.0
  feature, common-utils feature, port forwards for `3001` (NtN) +
  `9001/9099/9101` (metrics), VSCode extensions
  (`rust-analyzer`, `vadimcn.vscode-lldb`,
  `tamasfe.even-better-toml`), and a `postCreateCommand` that runs
  `node/scripts/install_haskell_cardano_node.sh` to fetch the
  upstream IntersectMBO Haskell `cardano-node` + `cardano-cli`
  binaries (10.7.1+) into `~/.local/bin/`.  This unblocks the §5
  hash-comparison and §6.5b parallel-fetch parity checks in a fresh
  devcontainer with no manual operator setup.  The installer is
  idempotent — subsequent rebuilds skip the ~217 MB download.

### Fixed

- **Restart-resilience cycle-2 crash: `RollbackPointNotFound` after
  recovery (Round 88 operational parity).**  On node restart,
  `ChainState` was always constructed via `ChainState::new(k)` —
  empty.  The next ChainSync session immediately received
  `RollBackward(recovered_tip)` (the peer's confirmation of the
  resume point) and our `roll_backward` searched the empty `entries`
  vec, returning `RollbackPointNotFound` and crashing the node:

  ```text
  Notice  Node.Recovery       point=BlockPoint(SlotNo(88840), …)
  Notice  ConnectionManager   verified sync session established fromPoint=BlockPoint(SlotNo(88840), …)
  Error   Node.Sync           rollback point not found: slot 88840 …
  ```

  Surfaced by §6 restart-resilience operator rehearsal as a cycle-2
  failure on a real preprod sync.  Fix: new
  `ChainState::seed_from_entries` API + new node-side helper
  `crate::sync::seed_chain_state_from_volatile` that reads the
  volatile DB at restart and seeds the `ChainState` window with the
  most-recent k entries.  Wired into all 5 sync entry points
  (chaindb, shared-chaindb, with-tracer, run_verified_sync_service,
  run_verified_sync_service_chaindb) via a small
  `ChainDbVolatileAccess` trait so both `&mut ChainDb<I, V, L>` and
  `&Arc<RwLock<ChainDb<I, V, L>>>` access modes get the same seed.
  3 unit tests in `crates/consensus/src/chain_state.rs` lock the
  invariant; 3 integration tests in `node/tests/runtime.rs` were
  updated to provide chain-contiguous block-number / prev-hash
  fixtures (they previously relied on the empty-`ChainState` bug to
  bypass the chain validation).

  Reference: upstream `Ouroboros.Consensus.Storage.ChainDB.Init` /
  `getCurrentChain` rebuilds the in-memory chain fragment from the
  volatile DB on start-up.

  End-to-end verification: `node/scripts/restart_resilience.sh`
  with `CYCLES=2` against a real preprod peer now reports
  `[ok] all 2 cycles + final recovery completed monotonic tip
  progression`.

- **Vendored `peer-snapshot.json` placeholders for mainnet + preview
  (operator preflight).**  Both `node/configuration/mainnet/topology.json`
  and `node/configuration/preview/topology.json` referenced
  `peerSnapshotFile: "peer-snapshot.json"` but the actual files were
  missing, so `validate-config --network mainnet|preview` reported
  `peer_snapshot.status = "unavailable"` with a "could not be loaded"
  warning out of the box.  Vendored placeholder files matching the
  preprod skeleton (slot=0, single bootstrap-pool entry per network);
  preflight now reports `peer_snapshot.status = "loaded"` for all
  three networks.

### Security

- **Byzantine-path closures (Round 87 parity audit).**  Two upstream
  `Word8` / size-bound parity gaps fixed:
  - **PeerSharing amount cap.**  `MsgShareRequest` carries the
    requested amount as `u16` on our wire (HandshakeVersion-bound),
    but upstream `Ouroboros.Network.PeerSelection.PeerSharing`
    transports it as `Word8` (max 255).  Our
    `SharedPeerSharingProvider::shareable_peers` previously honoured
    the full `u16` range, so a malicious peer requesting `u16::MAX`
    forced the provider to walk the entire registry per request.
    Fixed: cap at `PEER_SHARING_MAX_AMOUNT = 255` BEFORE the registry
    walk in `node/src/server.rs`, plus a regression test
    `shared_peer_sharing_provider_clamps_to_upstream_word8_max` that
    populates 300 peers and asserts `u16::MAX` requests return ≤ 255.
  - **LocalTxSubmission decode-byte ceiling.**  The NtC
    `LocalTxSubmission` server in `node/src/local_server.rs` accepted
    arbitrary CBOR `tx_bytes` and only rejected oversized payloads
    AFTER the full mempool admission decode + `validate_max_tx_size`
    check (mainnet `max_tx_size = 16 384 B` Conway PV 10).  A
    malicious local client could submit a multi-MB well-formed-but-
    oversized CBOR blob and force the allocation before rejection.
    Fixed: explicit `LOCAL_TX_SUBMIT_MAX_BYTES = 64 KiB` ceiling at
    the wire boundary (~4× the protocol max for headroom), reject
    with structured reason before any decode.
- **Code audit C-1/H-1/H-2 + M-1..M-8 + L-1..L-9 closed.**  See
  [`docs/code-audit.md`](docs/code-audit.md) for the source audit;
  remediation summary:
  - **C-1 / H-1** — every CBOR decoder that allocates from a
    peer-supplied `count` field now goes through
    `vec_with_safe_capacity` (soft cap) or `vec_with_strict_capacity`
    (hard cap) defined in [`crates/ledger/src/cbor.rs`](crates/ledger/src/cbor.rs);
    per-protocol bounds live in
    [`crates/network/src/protocol_size_limits.rs`](crates/network/src/protocol_size_limits.rs).
    Fixes a pre-auth remote DoS via `Vec::with_capacity(u64::MAX)`.
  - **H-2** — `PeerListener::accept_peer` split into `accept_tcp` +
    `handshake_on` with a 5 s `HANDSHAKE_DEADLINE`.  Inbound rate-
    limit decision now runs **before** the handshake, so a hard-limit
    rejection costs only a TCP accept.
  - **M-1** — mux ingress-queue limit checked **before** the per-frame
    payload allocation in [`crates/network/src/mux.rs`](crates/network/src/mux.rs).
  - **M-3** — NtC Unix socket bound at `0o660` (was `0o755` from
    default umask) in [`node/src/local_server.rs`](node/src/local_server.rs).
  - **M-6 / L-8 / L-9** — value-preservation arithmetic in
    [`crates/ledger/src/utxo.rs`](crates/ledger/src/utxo.rs) now uses
    `checked_add` (new `LedgerError::ValueOverflow`); plutus
    `ExBudget::spend` uses `checked_sub`; mempool capacity arithmetic
    uses `checked_add`.  Closes the silent saturating-on-overflow
    path that diverged from upstream Haskell `Integer` arithmetic.
  - **M-8** — genesis-hash gate hard-fails on unpaired
    `(genesis-file, declared-hash)` in
    [`node/src/config.rs`](node/src/config.rs); previously a missing
    `*GenesisHash` skipped verification silently.
  - **L-6** — KES/VRF/cold key files rejected unless
    `mode & 0o077 == 0` in [`node/src/block_producer.rs`](node/src/block_producer.rs).
  - **M-4 / M-5** — `serde_yaml` (advisory-db #2132) and `serde_yml`
    (RUSTSEC-2025-0068) replaced with `serde_norway = "0.9"`;
    trace-forwarder migrated from `serde_cbor 0.11` (RUSTSEC-2021-0127)
    to `ciborium 0.2`.  `serde_cbor` retained transitionally for
    storage on-disk format only, ignored in `deny.toml` with rationale.
  - **L-4** — `cargo deny check` runs in CI on every push and PR.
  - **L-1 / L-2 / L-7** — release verification + maintainer signing
    sections in [`SECURITY.md`](SECURITY.md); `restart_resilience.sh`
    now uses `mktemp -d` + ephemeral ports so concurrent runs don't
    collide.

### Changed

- **Toolchain bumped from Rust 1.85.0 → 1.95.0** ([rust-toolchain.toml](rust-toolchain.toml),
  workspace `rust-version`).  All new 1.95 clippy lints are clean
  (`manual_is_multiple_of`, `manual_div_ceil`, `manual_abs_diff`,
  `manual_contains`, `manual_ok`, `cloned_ref_to_slice_refs`,
  `unnecessary_sort_by`, `useless_vec`, `single_match_else`,
  `manual_while_let_some`, `derivable_impls`, `doc_overindented_list_items`,
  `doc_list_items_indentation`).  Stylistic-bulk lints
  (`collapsible_if`, `result_large_err`, `large_enum_variant`)
  explicitly carried forward as `allow` in
  [`Cargo.toml`](Cargo.toml) `[workspace.lints.clippy]` with
  documented rationale.

- **Docs site converted to dark-only mode** with the YggdrasilNode
  branding.  `docs/_sass/color_schemes/yggdrasil.scss` is a
  self-contained dark scheme (no fragile `@import "./dark"` that
  broke under `remote_theme:`); `docs/_sass/custom/custom.scss`
  design tokens and per-component backgrounds rebound to
  dark-friendly values; the YggdrasilNode banner appears as a
  landing-page hero via `docs/_includes/header_custom.html` (gated
  by `hero: true` front-matter).  Sidebar logo wired via
  `_config.yml` `logo:`; favicon and Open Graph image set in
  `docs/_includes/head_custom.html`.

### Tests

- **4 634 passing, 0 failing** (was 4 630).  `+4` from the new
  `extract_block_tx_byte_spans_*` regression tests in
  [`crates/ledger/src/cbor.rs`](crates/ledger/src/cbor.rs).

## [0.1.0] — 2026-04-27

### Yggdrasil 1.0 closure

First feature-complete release after the 2026-Q2 parity audit. Every
confirmed-active parity slice is closed; every runtime integration
originally tracked as a follow-up has landed.

### Operator deliverables

- Documentation site published at <https://yggdrasil-node.github.io/Cardano-node/>
  with the user manual (install, configure, run, monitor, troubleshoot,
  block production, releases) and reference docs.
- Release workflow that builds Linux x86_64 + aarch64 binaries on `v*` tag
  push, computes SHA256 checksums, and publishes a GitHub Release.
- `Dockerfile` + `docker-compose.yml` + `.dockerignore` for container
  deployments.
- Operator scripts: `install_from_release.sh` (with build-from-source
  fallback), `healthcheck.sh`, `backup_db.sh`, `restart_resilience.sh`,
  `compare_tip_to_haskell.sh`, `check_upstream_drift.sh`, plus a
  systemd unit template.
- Issue templates, PR template, CODEOWNERS, dependabot config (with
  RustCrypto digest-ecosystem grouping).
- `SECURITY.md` with vulnerability disclosure policy.
- Operator-facing Prometheus metric names normalized across the manual,
  runbook, healthcheck, restart-resilience and pool-producer scripts:
  `yggdrasil_current_block_number`, `yggdrasil_reconnects`,
  `yggdrasil_rollbacks`, `yggdrasil_stable_blocks_promoted`,
  `yggdrasil_batches_completed`, `yggdrasil_mempool_tx_added`,
  `yggdrasil_mempool_tx_rejected`, `yggdrasil_inbound_connections_accepted`,
  `yggdrasil_inbound_connections_rejected`, `yggdrasil_active_peers`,
  `yggdrasil_blocks_synced`, `yggdrasil_current_slot`.

### Closure cycle slices

- **Slice B** — CDDL parser range constraints (`N..M`, `.le`, `.ge`,
  `.lt`, `.gt`, `.size N..M`).
- **Slice D** — `HotPeerScheduling` per-mini-protocol weight table
  mirroring upstream `Ouroboros.Network.PeerSelection.Governor.HotPeers`.
- **Slice E (foundation)** — `effective_block_fetch_concurrency` +
  `partition_fetch_range_across_peers` + `BlockFetchAssignment`
  primitives.
- **Slice GD** — genesis density tracking primitive
  (`crates/consensus/src/genesis_density.rs::DensityWindow`,
  `DEFAULT_SLOT_WINDOW = 6480`, `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6`).
- **Slice GD-RT** — ChainSync header density observation hook
  (`DensityRegistry`).
- **Slice GD-Governor** — density-biased hot demotion in `PeerMetrics`.
- **Slice GD-Final** — runtime data flow unifying the density seam.
- **Slice D-Scheduler** — `HotPeerScheduling`-driven mux egress weights.
- **Slice E-Dispatch** — `execute_multi_peer_blockfetch_plan`
  parallel executor with `tokio::JoinSet` + `ReorderBuffer`.
- **Slice E-Tentative** — `dispatch_range_with_tentative` consensus-
  correctness contract.
- **Slice E-Phase6-Seam** — `OutboundPeerManager` hot-peer accessors.
- **Slice E-Inline** — non-spawning multi-peer dispatcher
  (`execute_multi_peer_blockfetch_plan_inline`).
- **Slice E-Workers** — per-peer fetch worker primitive
  (`FetchWorkerHandle`, `FetchWorkerPool`) mirroring upstream
  `Ouroboros.Network.BlockFetch.ClientRegistry`.
- **Slice E-Production-Spawn** —
  `FetchWorkerHandle::spawn_with_block_fetch_client` wiring real
  `BlockFetchClient` into a worker.
- **Slice E-Migration** — `PeerSession.block_fetch: Option<...>` plus
  `migrate_session_to_worker` / `unregister_worker`.
- **Slice E-Wire** — sync-loop multi-peer dispatch branch +
  `MultiPeerDispatchContext`.
- **Slice E-Promote** — governor migrates `BlockFetchClient` on
  `promote_to_warm` when the operator knob is `> 1`.
- **Phase 6 observability** — Prometheus counters
  `yggdrasil_blockfetch_workers_registered` (gauge) and
  `yggdrasil_blockfetch_workers_migrated_total` (counter).

### Operator surface

- `max_concurrent_block_fetch_peers` config knob (default `1`,
  flippable to `2` after §6.5 rehearsal).
- §6.5 parallel-fetch rehearsal added to the manual test runbook.

### Test count

- 4,630 tests passing across the workspace, 0 failing (post-v0.1.0
  the count rose to 4,634 with the fee-validation regression tests
  added in the next cycle).
- All four gates clean: `cargo check-all`, `cargo test-all`,
  `cargo lint`, `cargo doc --workspace --no-deps`.

[Unreleased]: https://github.com/yggdrasil-node/Cardano-node/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.1.0
