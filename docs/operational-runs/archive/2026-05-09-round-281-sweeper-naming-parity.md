# Round 281 — Phase B sweeper naming-parity round

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings) — closing round
**Predecessor:** R280 (`docs/operational-runs/2026-05-09-round-280-network-governor-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Closes Phase B by:

1. Adding `## Naming parity` docstring stanzas to all remaining 97
   files (`(c-needed)` × 24 + `(NEEDS-REVIEW)` × 73). Coverage spans
   network mini-protocol drivers, network framework / registry,
   ledger top-level types and rules, plutus runtime, crypto helpers,
   storage backends, and node top-level + commands + local_server.
2. Performing the `opcert.rs` → `ocert.rs` parent rename surfaced by
   R274 discovery, plus the `opcert/` → `ocert/` directory rename.
   The public API path moves from `yggdrasil_consensus::opcert::*` to
   `yggdrasil_consensus::ocert::*`; downstream consumers updated at
   the same time.
3. Updating `docs/parity-matrix.json` to reflect the post-rename
   paths under `crates/consensus/src/ocert/`.
4. Tightening the audit grader to fix two clippy `doc_list_item_without_indent`
   regressions surfaced by the bulk-add (`+` / `-` line starts in
   continuation paragraphs).

After R281, every production `.rs` under `crates/<crate>/src/` and
`node/src/` either:
- mirrors a single canonical upstream `.hs` file by snake_case-of-
  PascalCase (52 files, verdict `(a)`), OR
- carries a `## Naming parity` docstring stanza explicitly declaring
  its synthesis story with the upstream symbols/files surfaced (157
  files, verdict `(c)`).

## Files affected

### 97 files received `## Naming parity` docstring stanzas

Grouped by cluster:

- **consensus top-level (8 files):** `chain_selection`, `chain_state`,
  `epoch`, `error`, `genesis_density`, `in_future`, `praos`,
  `praos/common`.
- **crypto (3 files):** `error`, `sha3_hash`, `test_vectors`.
- **ledger top-level (14 files):** `error`, `fees`, `min_utxo`,
  `native_script`, `plutus_validation`, `rewards`, `stake`, `state`,
  `tx`, `types`, `utxo`, `witnesses`, `eras/byron`, `eras/conway`.
- **plutus (5 files):** `error`, `builtins`, `flat`, `types`,
  `machine`.
- **storage (5 files):** `error`, `file_immutable`, `file_volatile`,
  `file_ledger`, `ocert_sidecar`.
- **node top-level (~22 files):** `block_producer`, `blockfetch_worker`,
  `chainsync_worker`, `sync`, `upstream_pins`, `genesis`, `handlers`,
  `handlers/shutdown`, `path_resolve`, `plutus_eval`, `run_node`,
  `runtime`, `startup`, `trace_forwarder`, `tracer`, `ledger_peers`,
  `bin/dump_block`, `commands`, `commands/{query,run,status,submit_tx,tx_mempool}`,
  `local_server/{accept,sessions}`.
- **network mini-protocol drivers (16 files):** `blockfetch_{client,server}`,
  `chainsync_{client,server}`, `keepalive_{client,server}`,
  `local_state_query_{client,server}`, `local_tx_monitor_{client,server}`,
  `local_tx_submission_{client,server}`, `peersharing_{client,server}`,
  `txsubmission_{client,server}`.
- **network protocols/ tree (5 files):** `protocols/chain_sync`,
  `protocols/local_tx_monitor`, `protocols/local_tx_submission`,
  `protocols/peer_sharing`, `protocols/local_state_query_upstream`.
- **network framework / registry (15 files):** `blockfetch_pool`,
  `connection`, `diffusion`, `handshake`, `inbound_governor`,
  `ledger_peers_provider`, `listener`, `multiplexer`, `mux`,
  `ntc_peer`, `peer`, `peer_selection`, `peer_state_actions`,
  `protocol_limits`, `protocol_size_limits`, `root_peers_provider`.

Each file's `## Naming parity` block declares:
- which upstream symbol(s) / file(s) the Rust file synthesizes;
- whether the relationship is strict-partial (subset of one upstream
  file) or synthesis (combine of multiple upstream files);
- the design rationale for the synthesis pattern (cohesion / dependency-
  direction / runtime-async).

### `opcert.rs` → `ocert.rs` rename

| Old path | New path |
|---|---|
| `crates/consensus/src/opcert.rs` | `crates/consensus/src/ocert.rs` |
| `crates/consensus/src/opcert/ocert.rs` | `crates/consensus/src/ocert/ocert.rs` |
| `crates/consensus/src/opcert/rules_ocert.rs` | `crates/consensus/src/ocert/rules_ocert.rs` |

Performed via `git mv` to preserve history. Affected source updates:
- `crates/consensus/src/lib.rs:30` — `pub mod opcert;` → `pub mod ocert;`
- `crates/consensus/src/lib.rs:62` — `pub use opcert::{...}` → `pub use ocert::{...}`
- `crates/consensus/src/header.rs:16` — `use crate::opcert::` → `use crate::ocert::`
- `docs/parity-matrix.json` — 3 path entries updated (parent + 2 sub-modules)

### Clippy `module_inception` allow

After the rename, `crates/consensus/src/ocert.rs` carries a `pub mod
ocert;` declaration referring to the strict-mirror sub-file
`crates/consensus/src/ocert/ocert.rs` (which mirrors upstream
`Cardano.Protocol.TPraos.OCert.hs`). Clippy's `module_inception` lint
flags this. Fixed via `#[allow(clippy::module_inception)]` on the
`pub mod ocert;` line with an inline comment explaining the strict-
mirror rationale. The public API path remains
`yggdrasil_consensus::ocert::OpCert` (no inception in import paths)
because of the `pub use` re-export.

### Doc-list-item lint fixes

Two files initially failed clippy's `doc_list_item_without_indent`
lint after the bulk-add because their parity-block continuation
paragraphs began with `+ ` (interpreted as a list bullet by clippy):
- `node/src/blockfetch_worker.rs` — replaced `+ \`...\`` with
  `plus \`...\``.
- `node/src/local_server/sessions.rs` — same replacement.

Both files re-render correctly after the fix.

## Verdict bucket counts (post-R281)

| Bucket | Pre-R281 | Post-R281 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 52 | 52 |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 60 | 157 (+97) |
| `(c-needed)` | 24 | 0 (-24) |
| `(NEEDS-REVIEW)` | 73 | 0 (-73) |
| **TOTAL** | 209 | 209 |

**Phase B is complete.** Every production `.rs` file is graded `(a)` or
`(c)`. The drift-guard's allowlist is fully populated.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.85s)
cargo lint                          clean (Finished `dev` profile in 19.00s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat (summary)

```text
97 files received `## Naming parity` blocks (~+970 lines total)
3 files renamed via git mv (opcert.rs / opcert/ -> ocert.rs / ocert/)
2 files updated for opcert -> ocert imports (lib.rs, header.rs)
1 file received `#[allow(clippy::module_inception)]` (ocert.rs)
2 files fixed for doc-list-item lint (blockfetch_worker.rs, sessions.rs)
docs/parity-matrix.json — 3 path entries updated for the rename
docs/strict-mirror-audit.tsv — rebuilt
docs/operational-runs/2026-05-09-round-281-... (new)
```

## Phase B summary

| Round | Cluster | Files |
|---|---|---|
| R276 | `crates/ledger/src/state/` | 24 |
| R277 | `consensus/{nonce,opcert,diffusion_pipelining}/` | 9 |
| R278 | `consensus/mempool/` | 7 |
| R279 | `node/src/runtime/` | 18 |
| R280 | `crates/network/src/governor/` | 6 |
| **R281** | **sweeper (residuals + opcert→ocert rename)** | **97 + 3 renames** |
| **TOTAL Phase B** | | **161 files annotated; 3 renamed** |

## Stop point — Phase B complete

| Phase | Rounds | Status |
|---|---|---|
| A — Discovery & guardrail bootstrap | R274, R275 | ✅ closed |
| **B — Targeted renames + docstrings** | **R276 … R281** | ✅ **closed** |
| C — Tech-debt purge | R282 … R287 | next |
| D — Living-doc parity language sweep | (parallel to C) | pending |
| E — Drift-guard fail-build flip | R288 | pending |
| F — cardano-cli surface expansion | R289 … R295 | pending |

R282 starts Phase C — tech-debt purge of 5 production `#[allow(dead_code)]`
sites + 1 production TODO + stale-doc re-grades. First file:
`node/src/block_producer.rs` (the serde-required `description: String`
field).

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R280 (`docs/operational-runs/2026-05-09-round-280-network-governor-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
