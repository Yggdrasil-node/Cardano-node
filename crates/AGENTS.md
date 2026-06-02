# Guidance for maintaining crate boundaries and shared conventions across the Rust workspace crates.
Keep this directory as a crate index, not as a place for cross-cutting implementation logic.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 dev/test/check-strict-mirror.py`
(warn-only since R275; fail-build at R288). Allowlist source-of-truth:
[`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv).

## Scope
- Adding, removing, or renaming workspace crates.
- Maintaining crate boundaries, ownership, and dependency direction.
- Keeping crate-local AGENTS files aligned with the actual responsibility of each crate.

##  Rules *Non-Negotiable*
- Each child crate MUST own a clear protocol or subsystem boundary before new code is added.
- Shared behavior MUST live in the appropriate crate, not in this directory.
- Cross-crate dependency direction MUST stay aligned with `docs/ARCHITECTURE.md`.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Workspace architecture anchor](../.reference-haskell-cardano-node/cardano-node/)
- [Ledger era package layout](../.reference-haskell-cardano-node/deps/cardano-ledger/eras/)
- [Ledger support libraries](../.reference-haskell-cardano-node/deps/cardano-ledger/libs/)
- [Consensus package layout](../.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/)
- [Consensus protocol modules](../.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Protocol/)
- [Networking package layout](../.reference-haskell-cardano-node/deps/ouroboros-network/)
- [Network mini-protocol packages](../.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/protocols/lib/Ouroboros/Network/Protocol/)
- [Crypto package layout](../.reference-haskell-cardano-node/deps/cardano-base/)
- [Plutus core and CEK machine](../.reference-haskell-cardano-node/deps/plutus/)

## Current Layout

The Cargo workspace currently has 34 members: 6 core protocol crates, 4
support/observability crates, 10 node integration crates, 13 sister-tool
crates under `crates/tools/`, and `xtask`. Most production crates either
mirror an upstream Haskell package/file tree or carry explicit synthesis
docstrings so the strict-mirror gate can keep file-level parity intact.
**Do not merge crates** merely because individual ones look small; crate
boundaries preserve upstream package ownership, operator-tool boundaries, or
node integration seams.

### Directory grouping (R447 restructure)

- `crates/{crypto, ledger, storage, consensus, network, plutus}/` —
  **core runtime crates** (6 entries). Foundational subsystems
  imported by every other crate; dependency direction is strictly
  downward through this list.
- `crates/{error, telemetry, test-fixtures}/` plus
  `crates/observability/yggdrasil-metrics/` — **support crates**
  shared by the runtime, tests, and observability surfaces.
- `crates/node/<subcrate>/` — **node integration crates** (10 entries:
  `cardano-node` (package/binary `yggdrasil-node`), `block-producer`, `config`, `genesis`, `ntc-server`,
  `ntn-server`, `plutus-eval`, `runtime`, `sync`, `tracer`). These own
  orchestration, operator configuration, local sockets, runtime loops, and
  binary assembly; reusable protocol logic belongs in the core crates.
- `crates/tools/<tool>/` — **sister tools** (13 entries: bech32,
  cardano-cli, cardano-submit-api, cardano-testnet, cardano-tracer,
  db-analyser, db-synthesizer, db-truncater, dmq-node, kes-agent,
  kes-agent-control, snapshot-converter, tx-generator). Operator-facing
  binaries / SPO tooling; each remains its own workspace crate with
  its own `Cargo.toml`, binary target, and `AGENTS.md`. The
  `crates/tools/` grouping captures their shared role (operator
  binaries) without altering the per-tool parity contract.

The grouping is purely organizational — the strict-mirror gate, parity-
matrix, CI workflows, and run-tools harness all reference paths under
`crates/tools/<tool>/` (R447 path rewrite). Cargo workspace members
+ workspace dependency `path = "..."` entries point at the new
locations. `xtask/` owns repository automation that is not part of the node
runtime.

### Core crates — foundational subsystems

These mirror the upstream `cardano-base` / `cardano-ledger` /
`ouroboros-consensus` / `ouroboros-network` / `plutus` packages.
Dependency direction is strictly downward through this list (see
[`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md)).

| Crate          | LOC     | Upstream package                                              | Status     |
|----------------|---------|---------------------------------------------------------------|------------|
| `crypto`       | ~5.5k   | `cardano-base/cardano-crypto-class` + `-praos` + bls + secp   | complete   |
| `ledger`       | ~65.2k  | `cardano-ledger/eras/{byron..conway}` + libs                  | complete   |
| `storage`      | ~2.7k   | `ouroboros-consensus/Storage/{Immutable,Volatile,LedgerDB}`   | complete   |
| `consensus`    | ~8.9k   | `ouroboros-consensus/{Protocol,Praos,Mempool,Forge}`          | complete   |
| `network`      | ~34.6k  | `ouroboros-network` + `-framework` + `-protocols` + Diffusion | complete   |
| `plutus`       | ~11.2k  | `plutus/plutus-core/{CEK,builtins,cost-model}`                | complete   |

Per-era `CborEncode`/`CborDecode` impls under
`crates/ledger/src/eras/*/cbor.rs` are hand-coded against upstream CDDL
(`../.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/`);
CDDL is treated as authoritative documentation. Codegen scaffolding was
removed in favor of hand-coding because real upstream parity needs Byron
/ array-vs-map / optional-field semantics that CDDL underspecifies.

`consensus/mempool` consolidates upstream's `Ouroboros.Consensus.Mempool.*`
sub-tree into this crate (R256), mirroring upstream's package layout.

### Operator CLI

| Crate          | LOC     | Upstream package | Status                                       |
|----------------|---------|------------------|----------------------------------------------|
| `cardano-cli`  | ~3.9k   | `cardano-cli`    | `implemented_needs_11_0_1_evidence` — C-arc complete; 40 `Command` variants wired in the standalone `yggdrasil-cardano-cli`; byte-equivalence evidence against upstream remains the operator-side gate. |

The C-arc parallel port is complete from the Rust implementation side. See
[`tools/cardano-cli/AGENTS.md`](tools/cardano-cli/AGENTS.md) for the current
surface and remaining upstream-binary evidence requirements.

### Sister-tools — Tier 1 (deployment-essential SPO operations)

| Crate                | LOC    | Upstream package        | Status                                                                  |
|----------------------|--------|-------------------------|-------------------------------------------------------------------------|
| `bech32`             | ~0.7k  | `IntersectMBO/bech32`   | `verified_11_0_1`                                                       |
| `cardano-submit-api` | ~4.0k  | `cardano-submit-api`    | `implemented_needs_11_0_1_evidence` — HTTP server + LocalTxSubmission + Prometheus shipped; operator soak/evidence pending. |
| `kes-agent`          | ~0.2k  | `IntersectMBO/kes-agent`| `partial` — daemon work remains gated on socket-protocol fixture capture and byte-equivalence. |
| `kes-agent-control`  | ~1.2k  | `IntersectMBO/kes-agent`| `partial` — typed config/parser/env-var surface shipped; server-backed runtime work remains. |
| `cardano-tracer`     | ~5.6k  | `cardano-tracer`        | `partial` — forwarder/log/metrics surface substantially ported; full Network.Mux semantics and operator soak remain. |

### Sister-tools — Tier 2 (operator forensics + maintenance)

R386-R407 arc.

| Crate                | LOC    | Upstream package                                          | Status                                                                  |
|----------------------|--------|-----------------------------------------------------------|-------------------------------------------------------------------------|
| `db-truncater`       | ~0.8k  | `ouroboros-consensus-cardano/Tools/DBTruncater`           | `implemented_needs_11_0_1_evidence` — storage trim + Run.hs equivalent shipped; upstream binary evidence pending. |
| `db-analyser`        | ~2.9k  | `ouroboros-consensus-cardano/Tools/DBAnalyser`            | `partial` — typed config/parser plus block-only analysis dispatch shipped; ledger-state apply-loop work remains. |
| `snapshot-converter` | ~0.9k  | `ouroboros-consensus-cardano/app/snapshot-converter.hs`   | partial — R353 typed-config + R363 typed-parser; convertSnapshot LSM/Mem logic deferred pending yggdrasil-format LedgerStore |

### Sister-tools — Tier 3 (testing + benchmarking)

| Crate                | LOC    | Upstream package        | Status                                                                  |
|----------------------|--------|-------------------------|-------------------------------------------------------------------------|
| `db-synthesizer`     | ~1.3k  | `ouroboros-consensus-cardano/Tools/DBSynthesizer` | `partial` — typed config/parser/orphans shipped; Praos-valid forge-loop work remains. |
| `cardano-testnet`    | ~0.7k  | `cardano-testnet`       | `partial` — typed config/parser shipped; process/property/testnet orchestration remains. |
| `tx-generator`       | ~0.2k  | `bench/tx-generator`    | `partial` — calibration subtrees remain a synthesis carve-out; concrete generator work continues under the sister-tools arc. |

### Sister-tools — Tier 4 (sister project)

R450-R459 arc.

| Crate                | LOC    | Upstream package | Status                                                                  |
|----------------------|--------|------------------|-------------------------------------------------------------------------|
| `dmq-node`           | ~1.1k  | `IntersectMBO/dmq-node` | `partial` — typed config/parser/config-file load shipped; remaining runtime parity work tracked in the matrix. |

### LOC + status reading instructions

LOC counts above are approximate (rounded to the nearest 100 lines). For
exact current values:

```bash
# Core crates
for d in crates/{crypto,ledger,storage,consensus,network,plutus}/; do
  loc=$(find "$d/src" -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')
  printf "%-25s %s\n" "$(basename "$d")" "$loc"
done
# Sister tools (R447 grouping)
for d in crates/tools/*/; do
  loc=$(find "$d/src" -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')
  printf "%-25s %s\n" "$(basename "$d")" "$loc"
done
```

For per-crate next-milestone status, consult
[`docs/parity-matrix.json`](../docs/parity-matrix.json) and the
`next_milestone` field on each entry.

## Maintenance Guidance
- When a crate boundary changes, update the child crate AGENTS file, `docs/ARCHITECTURE.md`, the workspace root `AGENTS.md`, and the layout tables in this file together.
- Do not add umbrella instructions here that conflict with more specific crate-local guidance.
- The 34-member workspace count is intentional and reflects the current split
  across core protocol crates, support crates, node integration crates, sister
  tools, and `xtask`. Do not propose merging crates as a "cleanup" step —
  merging breaks the strict 1:1 file-mirror and upstream-package ownership
  contract that the R273+ and R447+ arcs built to enforce. Cleanups should
  target documentation (this file), workspace member ordering (root
  `Cargo.toml`), or per-crate `Cargo.toml` description fields, not the crate
  count itself.
