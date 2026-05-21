# Guidance for the pure-Rust port of upstream `dmq-node`.

**Status:** `partial`. The DMQ protocol-definition surface is
**complete** (R717-R754 — see *Current functional surface*); the
remaining runtime / diffusion sub-arc is a deliberate carve-out.
Scope band: **MEDIUM**.

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

## Current functional surface (post-R754)

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

### Deferred — the runtime / diffusion sub-arc

- ❌ The typed-protocols **peer drivers** (`Protocol/*/{Client,Server,
  Inbound,Outbound}.hs`) and the **runtime** (`NodeKernel`,
  `Diffusion/*`, the NtN / NtC mux bundles, `Tracer.hs`,
  `Handlers/TopLevel.hs`). A peer driver is only meaningful plugged
  into the mux + diffusion layer, so these ship **together** as one
  deliberate `crates/network`-integration sub-arc — not as
  standalone slices. `run` returns
  `RunError::DiffusionWiringDeferred` until that sub-arc lands. See
  the **Carve-out inventory** below.
- ❌ End-to-end behavioral tests against the upstream binary —
  pending that sub-arc.

## Carve-out inventory (R444 structured deferral surface)

`crates/tools/dmq-node/src/status.rs` ships
`diffusion_wiring_status()` returning a `DiffusionWiringStatus`
descriptor.

| Carve-out                            | Status helper                          | Deferral rationale (one-liner)                                            |
|--------------------------------------|----------------------------------------|---------------------------------------------------------------------------|
| Peer drivers + Diffusion / NodeKernel / mux wiring | `status::diffusion_wiring_status()` | The DMQ protocol-definition surface is complete (R717-R754); what remains is the runtime sub-arc — the typed-protocols peer drivers (`Protocol/*/{Client,Server,Inbound,Outbound}.hs`) plus `NodeKernel` / `Diffusion/*` / the NtN-NtC mux bundles / `Tracer.hs`. These are entangled (a peer driver is only exercisable inside the mux) and reuse `crates/network`'s mux + peer-selection machinery — one deliberate integration sub-arc, not standalone slices. |

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
once concrete dispatch lands at `R451+`.

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

Per the R326-R459 plan, this crate's full implementation lands across
the named mini-arc rounds:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- 🟡 Next: **R451** — first concrete-impl round of the mini-arc.
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
