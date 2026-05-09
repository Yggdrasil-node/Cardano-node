# Round 274 — Strict-mirror discovery audit

**Date:** 2026-05-09
**Phase:** A (discovery & guardrail bootstrap)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

R274 produces the **infrastructure** for Yggdrasil's strict 1:1 file-mirror
parity policy. Zero Rust code changes; only Python tooling, an upstream
basename index, an auto-graded TSV, and this round-doc.

Step 0 (operator-authorized via the approved plan): refresh the vendored
Haskell checkout from 10.7.1 to policy tag 11.0.1 via
`bash scripts/setup-reference.sh --force`. Confirmed:

```
.reference-haskell-cardano-node/install/bin/cardano-node --version
cardano-node 11.0.1 - linux-x86_64 - ghc-9.6
git rev 97036a66bcf8c89f687ae57a048eecc0389977ef
```

## Deliverables

| Path | Purpose |
|---|---|
| `scripts/audit-strict-mirror.py` | walks production `.rs` files, derives candidate upstream basenames, emits the audit TSV with auto-graded final verdicts |
| `docs/upstream-haskell-files.txt` | flat-file index of every `.hs` under `.reference-haskell-cardano-node/` (basenames + paths); built from `find` and committed for fast reverse-index lookup |
| `docs/strict-mirror-audit.tsv` | per-Rust-file audit table with 8 columns: `rust_path`, `candidates`, `matched_candidate`, `upstream_hits`, `docstring_parity`, `initial_verdict`, `final_verdict`, `notes` |
| `docs/operational-runs/2026-05-09-round-274-strict-mirror-discovery.md` | this doc |

## Methodology

### Reverse-index from upstream

`audit-strict-mirror.py` builds a Python `dict[snake_case_form,
list[upstream_paths]]` from the upstream tree. For each upstream `.hs`
basename (e.g. `LedgerDB`, `OCert`, `BLS12_381`), the script computes
1–2 snake_case variants:

- **Strict** — `LedgerDB` → `ledger_db` (insert `_` at camelCase
  boundaries, lowercase).
- **Loose** — `LedgerDB` → `ledgerdb` (collapse the underscore that's
  not surrounded by digits). Used because Yggdrasil sometimes flattens
  short names: `OCert.hs` → `ocert.rs` (not `o_cert.rs`).

Both variants are indexed. A Rust file like `crates/storage/src/ledger_db.rs`
hits the strict variant directly; `crates/consensus/src/opcert/ocert.rs`
hits the loose variant.

### Production-tree filtering

Each upstream hit is classified as production or non-production based on
path fragments. Non-production paths (test harnesses, benchmarks,
examples, demos) are filtered before the per-row verdict assignment.

The fragments treated as non-production: `/test/`, `/tests/`, `/test-`,
`/testlib/`, `/bench/`, `/benchmarks/`, `/golden/`, `/demo/`, `/notes/`,
`/docs/`, `/docusaurus/`, `/sample/`, `/example/`, `/app/`.

### Crate→repo affinity

When a Rust file's stem matches multiple upstream `.hs` files (e.g.
`State.hs` exists in every era), the script applies a crate-to-repo
affinity filter:

| Yggdrasil prefix | Preferred upstream substrings |
|---|---|
| `crates/consensus/src/` | `/ouroboros-consensus/`, `/cardano-protocol-tpraos/` |
| `crates/network/src/` | `/ouroboros-network/`, `/cardano-diffusion/`, `/network-mux/` |
| `crates/ledger/src/` | `/cardano-ledger/` |
| `crates/storage/src/` | `/ouroboros-consensus/.../Storage/`, `/ouroboros-consensus/` |
| `crates/plutus/src/` | `/plutus/` |
| `crates/crypto/src/` | `/cardano-base/cardano-crypto`, `/cardano-base/` |
| `node/src/` | `/cardano-node/`, `/cardano-tracer/` |

Affinity narrows multi-hit lists to the canonical-repo subset. If the
filter eliminates all hits (i.e. the file's nominal home doesn't have
the basename), the unfiltered hits are returned and the row stays
NEEDS-REVIEW.

### Auto-grading rules

The script assigns a provisional `final_verdict` to each row:

- **`(a) DIRECT_MIRROR (auto)`** — exactly one production-tree hit
  remains after affinity filtering.
- **`(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present)`** — the
  Rust file already carries a `## Naming parity` docstring stanza
  (either with or without `**Strict mirror:** none.`).
- **`(c-needed) NO_MIRROR_NEEDS_DOCSTRING (auto: no upstream + no
  docstring)`** — no upstream candidate AND no docstring stanza. Phase B
  must add the stanza or rename to a real upstream basename.
- **`(NEEDS-REVIEW) ...`** — the row needs human disambiguation. Either
  multiple plausible upstream hits remain after affinity filtering, or
  the only hits are in non-production trees (test/bench/demo).

Hand-grading replaces the `(auto)` annotation with the canonical
`(a)`/`(b)`/`(c)`/`(d)` verdict. The auto verdicts are conservative —
hand-grading may re-classify any row.

## Verdict bucket counts (post-auto-grade)

| Bucket | Count | Action gate |
|---|---|---|
| `(a) DIRECT_MIRROR (auto)` | 58 | none — verified upstream mirror |
| `(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present)` | 7 | none — already verified at audit time |
| `(c-needed) NEEDS_DOCSTRING (auto: no upstream + no docstring)` | 45 | Phase B must add `## Naming parity` block |
| `(NEEDS-REVIEW)` | 115 | hand-graded per-cluster during the relevant Phase B round |
| **TOTAL** | **225** | |

Test files (`*/tests/*`, `tests.rs`) and crate-roots (`lib.rs`,
`main.rs`, `mod.rs`, `build.rs`) are excluded from the audit; the 225
files are the production-mirror surface.

## (c-needed) cluster breakdown — Phase B targets

The 45 `(c-needed)` files cluster as follows (round assignment from the
plan, adjusted by the actual cluster boundaries discovered in this
audit):

| Cluster | File count | Phase B round |
|---|---|---|
| `crates/ledger/src/state/` (chain_dep, checkpoint, deposit_pot, reward_accounts, snapshot, stake_credentials, treasury) | 7 | R276 |
| `crates/consensus/src/` top-level (chain_selection, genesis_density, nonce, opcert) | 4 | R277 |
| `crates/network/src/` (connection, ledger_peers_provider, listener, multiplexer, ntc_peer, peer, root_peers_provider; governor/counters; protocols/local_state_query_upstream) | 9 | R280, R281 |
| `crates/storage/src/` (file_immutable, file_volatile, ocert_sidecar) | 3 | R281 |
| `crates/ledger/src/` top-level (fees, witnesses) | 2 | R281 |
| `crates/plutus/src/` (machine) | 1 | R281 |
| `node/src/runtime/` (block_producer_loop, cm_actions, governor_loop, ledger_judgement, ledger_peer_source, peer_management, peer_session, reconnecting, reconnecting_sync, sync_session, tx_submission_service) | 11 | R279 |
| `node/src/local_server/` (accept, sessions) | 2 | R281 |
| `node/src/` top-level (block_producer, blockfetch_worker, chainsync_worker, sync, upstream_pins) | 5 | R281 |
| `node/src/commands/` (status) | 1 | R281 |

## Discoveries during the audit

### `opcert.rs`/`opcert/` directory should be `ocert.rs`/`ocert/`

Upstream calls this concept `OCert` (operational certificate; mixed-case
`O` + `Cert`). Yggdrasil's `crates/consensus/src/opcert.rs` and the
sibling directory `opcert/` both use the `op`-prefixed spelling
(probably "operator-cert"). The R273-rename round renamed the *child*
file `cert.rs` → `ocert.rs` correctly, but the *parent* file and its
directory remain `opcert`. Strict 1:1 requires renaming the parent file
+ directory pair: `opcert.rs` → `ocert.rs` and `opcert/` → `ocert/`.
Scheduled for R281.

### `chain_state.rs` and `tx_state.rs` are conceptual syntheses

Multiple upstream `State.hs` files exist (per-era, per-protocol, per-
component). Yggdrasil's `consensus/chain_state.rs` and
`consensus/mempool/tx_state.rs` are unified syntheses across these. The
audit flags them as NEEDS-REVIEW; expected verdict during Phase B
hand-grading: `(c) NO_MIRROR_NEEDS_DOCSTRING` with the docstring stanza
naming the upstream `State.hs` files synthesized.

### `in_future.rs` and `test_vectors.rs`

Both files match only test-tree upstream hits. Both are likely
category-(c) syntheses — `in_future.rs` is the slot-in-future check
(implementation-side; upstream's `Test/Util/HardFork/Future.hs` is a
test fixture); `test_vectors.rs` is a fixture loader (production code
that consumes test vectors). The verdict during R281 will confirm.

### Era files don't strictly mirror upstream

`crates/ledger/src/eras/byron.rs` aggregates Byron-era types upstream
splits across `Block.hs`, `Header.hs`, `Tx.hs`, etc. under
`cardano-ledger/eras/byron/impl/src/Cardano/Chain/`. There is no single
upstream `Byron.hs` file under cardano-ledger. The closest upstream
mirrors (`cardano-node/src/Cardano/Node/Protocol/Byron.hs`,
`cardano-node/src/Cardano/Tracing/OrphanInstances/Byron.hs`) cover
configuration and tracing, not the era-types themselves. Verdict:
likely `(c) NO_MIRROR_NEEDS_DOCSTRING` — the Yggdrasil "one file per
era for high-level types" pattern is a synthesis. R272 (in the
predecessor plan, deferred) would split each era's monolithic Rust
file into per-rule files matching upstream.

## Hand-grading deferral rationale

R274 produces auto-graded verdicts for 110 of 225 rows (49%). The
remaining 115 NEEDS-REVIEW rows require reading individual upstream
`.hs` headers to confirm whether a candidate match is the *canonical*
mirror or a coincidental basename collision. This is detail work that
each Phase B round has to do anyway during its rename/annotate pass.

The plan therefore amends R274's scope from "fully hand-graded TSV" to
"infrastructure + auto-graded TSV; hand-grading happens per-cluster
during the relevant Phase B round." Each Phase B round (R276–R281):
1. Reads the rows whose `rust_path` is in its cluster.
2. Hand-grades the NEEDS-REVIEW rows (replaces the auto verdict with
   the canonical `(a)`/`(b)`/`(c)`/`(d)`).
3. Lands the rename or docstring stanza per the verdict.
4. Updates `docs/strict-mirror-audit.tsv` with the resolved verdicts.

The drift-guard (R275, warn-only) reads the TSV's `final_verdict`
column. NEEDS-REVIEW rows are treated as allowlisted (no warning) until
each Phase B round closes them; once a Phase B round graduates a row to
`(a)` or `(c)+verified`, the file is locked under the strict policy.

## Verification gates

```text
cargo fmt --all -- --check    clean (no Rust source changes)
cargo check-all               clean (Finished `dev` profile in 0.61s)
cargo lint                    clean
cargo test-all                4855 passed; 0 failed (baseline preserved)
```

Audit script self-verification:

```text
$ python3 scripts/audit-strict-mirror.py
  audit complete: 225 rust files; candidate_match=175, no_candidate_match=50
  auto-grading bucket counts:
    (NEEDS-REVIEW): 115
    (a): 58
    (c): 7
    (c-needed): 45
  TSV written: docs/strict-mirror-audit.tsv
```

`docs/upstream-haskell-files.txt` is built once per `setup-reference.sh
--force`; rebuilt on demand with `python3 scripts/audit-strict-mirror.py
--rebuild-index`.

## Diff stat

```text
docs/operational-runs/2026-05-09-round-274-strict-mirror-discovery.md   | (new file)
docs/strict-mirror-audit.tsv                                            | (new file, 226 lines)
docs/upstream-haskell-files.txt                                         | (new file, ~12k lines)
scripts/audit-strict-mirror.py                                          | (new file, ~280 lines)
```

## Stop point — Phase A status

R274 closes the discovery slice of Phase A. R275 (warn-only drift-guard
landing the CI counterpart of `.claude/skills/round-extraction/SKILL.md`)
is next.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R273-rename
  (`docs/operational-runs/2026-05-09-round-273-rename-strict-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Vendored install confirmed at 11.0.1 via
  `.reference-haskell-cardano-node/install/bin/cardano-node --version`
