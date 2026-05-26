# Guidance for the pure-Rust port of upstream `dmq-node`.

**Status:** `partial`. The DMQ pure-logic surface and every `run()`
integration **component** are **complete** — the protocol
definitions, all six peer drivers, the NtN/NtC version surfaces, the
`MempoolSeq` store, the full inbound-V2 governor, and the
`NodeKernel` + its registries + the NtN/NtC mux bundles (R717-R816 —
see *Current functional surface*). Only the `run()` event loop
itself — the socket accept-loop assembly — remains. Scope band:
**MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/dmq-node/` (51 `.hs` files).

## Mini-arc scope

Delegated Mempool Queue diffusion-layer node (sister project for Mithril). Phase D.1 mini-arc R450-R459 (10 rounds, MEDIUM). R453-R454 port the DMQ wire protocol + mempool queue logic; R455 reuses the local-socket pattern from `crates/network/src/local_state_query_server.rs`; R456 reuses `crates/network` mux for the cardano-node connection.

## Current functional surface (post-R816)

### CLI surface

- ✅ `<binary> --help` / `--version` byte-equivalent to upstream
  (golden tests in `tests/`).
- ✅ Typed `parser::Args` + `configuration::Configuration` dispatch —
  host/port/local-socket/config-file/topology-file/cardano-socket/
  network-magic parsed, validated, and merged with config-file
  contents (R369 layered).

### DMQ protocol surface — complete (R717-R754)

The R731 architectural fork — upstream `SigSubmission = TxSubmission2
SigId Sig` cannot reuse `crates/network`'s **concrete** `TxSubmission2`
(it hardcodes the ledger `TxId` / `Vec<u8>`, not generic over the
id/tx types) — was resolved at **R732** (advisor-confirmed) in favour
of **dmq-node-local protocol modules**: the wire format is identical
either way, so a generic refactor of the core network crate would buy
zero wire-parity bytes while risking the node's own tx-submission. The
protocol-definition surface is now comprehensively ported:

- ✅ `protocol/sig_submission.rs` — V1 `SigSubmission`
  (`Protocol/SigSubmission/{Type,Codec,Validate}.hs`): all `Type.hs`
  data types; the full CBOR codec (`SigId`, `SigOpCertificate`,
  `SigRaw`, `Sig` with `sigRawSignedBytes` capture); the complete
  `validateSig` validator — all five checks (`validate_kes_period`,
  `validate_ocert_counter`, `validate_pool_eligibility`,
  `validate_ocert_signature`, `validate_kes_signature`) plus the
  `validate_sig` / `validate_sig_batch` composition with context
  rollback; the state machine + transition + message codec +
  `timeLimits` / `byteLimits`.
- ✅ `protocol/sig_submission_v2.rs` — V2 `SigSubmissionV2`
  (`Protocol/SigSubmissionV2/{Type,Codec}.hs`): the count newtypes,
  state machine, message enum, transition, full CBOR codec, and the
  limit tables.
- ✅ `protocol/local_msg_submission.rs` — `LocalMsgSubmission`
  (`= LocalTxSubmission Sig SigValidationError`): types, transition,
  codec (incl. the `encodeReject` / `decodeReject` error codec).
- ✅ `protocol/local_msg_notification.rs` — `LocalMsgNotification`:
  `HasMore`, `BlockingReplyList`, the state machine, transition, and
  the indefinite-array message codec.
- ✅ `node_to_node/version.rs` + `node_to_client/version.rs` — the
  NtN / NtC protocol-version enums + CBOR-term codecs.
- ✅ `policy.rs` — `SigDecisionPolicy` + the ingress limit
  (`DMQ/Policy.hs`).
- ✅ `topology.rs` — the topology-file reader
  (`DMQ/Configuration/Topology.hs`, reusing `crates/network`'s
  `TopologyConfig`).
- ✅ `diffusion.rs` — `PoolValidationCtx` / `PoolId` /
  `StakeSnapshot` (the validation context from
  `Diffusion/NodeKernel/Types.hs`).
- ✅ `sig_submission_v2.rs` — `SigSubmissionProtocolError`
  (`SigSubmissionV2/Types.hs`).

### Runtime / diffusion sub-arc — peer drivers + version surfaces complete (R758-R768)

The decomposable runtime surface is ported as 10 bounded slices,
following the `crates/network` mini-protocol-driver pattern
(`keepalive_client.rs` — a driver struct wrapping a
`yggdrasil_network::MessageChannel`):

- ✅ Peer drivers — `LocalMsgNotificationClient` / `Server`,
  `LocalMsgSubmissionClient` / `Server`, `SigSubmissionV2Inbound` /
  `Outbound` (V1 `SigSubmission` reuses upstream `TxSubmission2`'s
  peers). Upstream's pipelined peers are ported as the non-pipelined
  linear driver — consistent with yggdrasil's other drivers.
- ✅ NtN / NtC version surfaces — `NodeToNodeVersionData` /
  `NodeToClientVersionData`: the handshake version data, the
  `Acceptable` negotiation, and the CBOR-term codecs.
- ✅ The NtN / NtC mux mini-protocol numbers (`node_to_node.rs` /
  `node_to_client.rs`) and `NTC_MAX_SIGS_TO_ACK`.

### Signature mempool + inbound-V2 governor — complete (R787-R801)

- ✅ `mempool.rs` — `MempoolSeq` / `WithIndex`: the in-memory
  signature mempool the DMQ `NodeKernel` holds (port of the pure core
  of `Ouroboros.Network.TxSubmission.Mempool.Simple`).
- ✅ `inbound_v2.rs` — the complete inbound-V2 tx-submission governor
  (port of `Ouroboros.Network.TxSubmission.Inbound.V2.{Types,State,
  Decision}`): the state surface (`TxDecision`, `PeerTxState`,
  `SharedTxState`), the seven decision-path functions
  (`split_acknowledged_tx_ids`, `acknowledge_tx_ids`,
  `pick_peer_step`, `pick_txs_to_download`, `make_decisions`,
  `filter_active_peers`, `update_ref_counts` / `tick_timed_txs`), and
  both inbound handlers (`received_tx_ids_impl`, `collect_txs_impl`).
  dmq-node-local — `crates/consensus`'s governor is concrete over
  ledger txs.

### run() integration components — complete (R805-R816, Option A)

The Option A `run()` integration arc (per the
`docs/COMPLETION_ROADMAP.md` A4 dmq-node entry) ported every
component the event loop assembles:

- ✅ `registry.rs` — `TxChannels` / `TxChannelsVar` / `TxMempoolSem`
  (the inbound-V2 channel registry).
- ✅ `diffusion.rs` — `StakePools`, and `NodeKernel` +
  `new_node_kernel` (the shared runtime state composing all eight
  registry / state components).
- ✅ `peer_sharing.rs` — the peer-sharing policy constants,
  `PublicPeerSelectionState`, `PeerSharingAPI`, and the
  `PeerSharingController` / `PeerSharingRegistry`.
- ✅ `delta_q.rs` — the DeltaQ `Distribution` / `Gsv` / `PeerGsv`
  latency model.
- ✅ `keep_alive.rs` — `KeepAliveRegistry` (the `NodeKernel`'s
  `fetchClientRegistry` field, per the R813 resolution).
- ✅ `node_to_node.rs` / `node_to_client.rs` — `dmq_ntn_bundle` /
  `dmq_ntc_bundle`, the NtN / NtC mux `OuroborosBundle`s.

### Deferred — the run() event loop

- ❌ The `run()` event loop itself: the socket accept-loop driving
  `crates/network`'s `ConnectionManagerState`, per-connection
  handshake + mux, and the per-protocol driver runners that convert
  the `OuroborosBundle` descriptors into running protocol tasks. The
  components above are all in place; this is the final concurrent-
  runtime assembly. `run` returns `RunError::DiffusionWiringDeferred`
  until it lands; its verification is the end-to-end byte-equivalence
  soak against the upstream binary (operator-gated).
- ❌ End-to-end behavioral tests against the upstream binary —
  pending that integration.

## Carve-out inventory (post-R816 run-loop boundary)

`crates/tools/dmq-node/src/status.rs` ships
`diffusion_wiring_status()` returning a `DiffusionWiringStatus`
descriptor.

| Carve-out                            | Status helper                          | Deferral rationale (one-liner)                                            |
|--------------------------------------|----------------------------------------|---------------------------------------------------------------------------|
| Mux / diffusion event-loop integration | `status::diffusion_wiring_status()` | The DMQ protocol surface (R717-R757) and the runtime sub-arc's decomposable surface — all six peer drivers, the NtN/NtC version surfaces, the mux protocol numbers (R758-R768) — are complete. What remains is the `ntnApps` / `ntcApps` mux-application wiring plus `NodeKernel` / `Diffusion/*` / `Tracer.hs` and the `run()` loop: STM-var-record + event-loop integration that wires the drivers into a live process, reusing `crates/network`'s mux + peer-selection machinery. Verified by the operator-gated end-to-end soak, not standalone slices. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-dmq-node

# Run via the universal launcher (recommended).
scripts/run-tools.sh dmq-node --help
scripts/run-tools.sh dmq-node --version

# Or invoke the binary directly:
target/release/dmq-node --help
```

The binary is named `dmq-node` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once the `run()` event loop and upstream comparison evidence land.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `dmq-node` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

The historical R326-R459 skeleton/config/parser plan is closed. The
current A4 continuation has shipped the pure-logic and mux-bundle
surface through R816; only the final runtime event loop remains.

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ Parser/config/protocol/inbound-governor/NodeKernel/mux-bundle
  components shipped through R816.
- 🟡 Next: `run()` event-loop assembly plus upstream comparison soak.
- 🟡 Closeout — when all subcommands are functional, parity-matrix
  entry advances `partial → verified_11_0_1`. Operators can then
  swap upstream binary for the yggdrasil binary without script
  changes.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-dmq-node

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/dmq-node --help) \
     <(target/debug/dmq-node --help)
diff <(.reference-haskell-cardano-node/install/bin/dmq-node --version) \
     <(target/debug/dmq-node --version)
# (empty diffs expected — byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration round numbers in sync with the
  authoritative plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
