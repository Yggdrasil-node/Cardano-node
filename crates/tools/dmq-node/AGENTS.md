# Guidance for the pure-Rust port of upstream `dmq-node`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R451+** per the R326-R459
sister-tools port arc plan. Scope band: **MEDIUM**.

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

## Current functional surface (post-R444)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Args` + `configuration::Configuration` dispatch —
  host/port/local-socket/config-file/topology-file/cardano-socket/
  network-magic parsed + validated + merged with config-file
  contents (R369 layered).
- ✅ `protocol/sig_submission.rs` — DMQ `SigSubmission` mini-protocol,
  **self-contained surface complete** (R717-R727, collapses upstream
  `DMQ/Protocol/SigSubmission/{Type,Codec,Validate}.hs`):
  - `Type.hs` data types — `SigHash`/`SigId`/`SigBody`/`CborBytes`
    (R717), `SigValidationError`/`Trace`/`Exception` (R718),
    `SigKesSignature`/`SigColdKey`/`SigOpCertificate` (R719-R720),
    `PosixTime`/`SigRaw`/`SigRawWithSignedBytes`/`Sig` (R721).
  - `Codec.hs` — `encode/decode_sig_id` (R722),
    `encode/decode_sig_op_certificate` (R723), `encode_sig` +
    `encode/decode_sig_raw` (R724), `decode_sig` with
    `sigRawSignedBytes` capture (R725).
  - `SigValidationError::to_json` (R726); `validate_kes_period` +
    `MAX_CLOCK_SKEW_SEC` (R727).
- ✅ `node_to_node/version.rs` (R728) + `node_to_client/version.rs`
  (R729) — the NtN / NtC protocol-version enums + CBOR-term codecs.
- 🟡 **dmq-node ↔ `crates/network` integration sub-arc (pending).**
  The remaining `SigSubmission` work — the `codecSigSubmission`
  `TxSubmission2` wrapper, the `timeLimits`/`byteLimits` tables, the
  rest of `validateSig` (the stateful pool-eligibility / opcert-counter
  / KES-signature checks, gated on `Diffusion/NodeKernel`'s
  `PoolValidationCtx`) — plus `Policy.hs`, `Configuration/Topology.hs`,
  the `LocalMsgSubmission` / `LocalMsgNotification` mini-protocols, and
  the NtN/NtC version-data + negotiation all reuse `crates/network`
  mini-protocol machinery (`TxSubmission2`, `NetworkTopology`,
  `TxDecisionPolicy`, `BlockingReplyList`).
  **R731 investigation — a hard prerequisite.** `crates/network`'s
  `protocols/tx_submission.rs::TxSubmissionMessage` is **concrete**:
  it hardcodes the ledger `TxId` for identifiers and `Vec<u8>` for
  transaction bodies (`use yggdrasil_ledger::TxId`; the file comment
  states "Transaction identifiers use the canonical ledger `TxId`
  wrapper"). Upstream `TxSubmission2 txid tx` is **generic**, and
  `SigSubmission crypto = TxSubmission2 SigId (Sig crypto)` depends on
  that genericity. So `codecSigSubmission` cannot reuse
  `crates/network`'s codec as-is. This is an **architectural fork**
  that warrants a deliberate decision (and a `parity-plan` for
  whichever path):
  1. Refactor `crates/network`'s `TxSubmission2` message / codec /
     drivers / inbound-governor to be generic over `<Id, Tx>` (the
     node instantiates `Id = TxId, Tx = Vec<u8>`). This matches
     upstream's generic shape and lets DMQ reuse it — but it is a
     protocol-critical, multi-round change to a core crate the node's
     own tx-submission depends on.
  2. Give dmq-node its own `SigSubmission` mini-protocol state
     machine + codec, independent of `crates/network` (no core-crate
     change; duplicates the `TxSubmission2` protocol shape).
- ❌ Diffusion / NodeKernel / PeerSelection wiring — returns
  `RunError::DiffusionWiringDeferred { host, local_socket,
  config_file, topology_file, cardano_socket, cardano_magic,
  dmq_magic }` (R444 structured deferral). See **Carve-out
  inventory** below.
- ❌ End-to-end behavioral tests against upstream binary — pending
  the dmq-node mini-arc (R450-R459).

## Carve-out inventory (R444 structured deferral surface)

`crates/tools/dmq-node/src/status.rs` ships
`diffusion_wiring_status()` returning a `DiffusionWiringStatus`
descriptor.

| Carve-out                            | Status helper                          | Deferral rationale (one-liner)                                            |
|--------------------------------------|----------------------------------------|---------------------------------------------------------------------------|
| Diffusion / NodeKernel / PeerSelection wiring | `status::diffusion_wiring_status()` | Gated on dmq-node mini-arc (R450-R459 — Tier 4 sister project). Leverages `crates/network/`'s existing surfaces (shipped) but needs the dmq-specific wire protocol + local-socket server. |

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
