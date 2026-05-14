# Round 276 — `crates/ledger/src/state/` naming-parity sweep

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings)
**Predecessor:** R275 (`docs/operational-runs/2026-05-09-round-275-strict-mirror-drift-guard.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Resolve every `(c-needed)` and `(NEEDS-REVIEW)` row in `docs/strict-mirror-audit.tsv`
that lives under `crates/ledger/src/state/`. Each affected file gets a
`## Naming parity` docstring stanza naming the upstream symbol(s) the
file synthesizes — bringing the entire state/ tree to drift-guard
compliance.

Also fixes one infrastructure bug surfaced by R274's audit output: 16
`tests.rs` / `*_tests.rs` files were incorrectly included in the audit
sample. The audit script now excludes them by name.

## Files affected

| File | Pre-R276 verdict | Post-R276 verdict |
|---|---|---|
| `state/cbor.rs` | `(a) DIRECT_MIRROR (auto)` (false-positive against Byron `Common/CBOR.hs`) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/chain_dep.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/checkpoint.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/committee_state.rs` | `(NEEDS-REVIEW)` (8 affinity-filtered State.hs hits) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/deposit_pot.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/drep_state.rs` | `(NEEDS-REVIEW)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/allegra.rs` | `(a) DIRECT_MIRROR (auto)` (over-confident — Allegra.hs is era-config, not block-apply) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/alonzo.rs` | `(a) DIRECT_MIRROR (auto)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/babbage.rs` | `(a) DIRECT_MIRROR (auto)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/byron.rs` | `(NEEDS-REVIEW)` (only `cardano-node/src/Cardano/Node/Protocol/Byron.hs` hits — different concept) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/conway.rs` | `(NEEDS-REVIEW)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/mary.rs` | `(a) DIRECT_MIRROR (auto)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/eras/shelley.rs` | `(a) DIRECT_MIRROR (auto)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/governance_action_state.rs` | `(NEEDS-REVIEW)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/phase1_validation.rs` | `(NEEDS-REVIEW)` (multi-Validation.hs hits) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/pool_state.rs` | `(NEEDS-REVIEW)` (8 affinity-filtered State.hs hits) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/ppup.rs` | `(NEEDS-REVIEW)` (5 per-era Ppup.hs hits) | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/reward_accounts.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/snapshot.rs` | `(c-needed)` (also a `(d)` name-clash with `Storage/LedgerDB/Snapshots.hs`) | `(c) NO_MIRROR_NEEDS_DOCSTRING` (clash documented inline) |
| `state/stake_credentials.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |
| `state/treasury.rs` | `(c-needed)` | `(c) NO_MIRROR_NEEDS_DOCSTRING` |

3 state/ files retain `(a) DIRECT_MIRROR`: `enact.rs`, `mir.rs`,
`ratify.rs` — all canonical mirrors of upstream
`Cardano/Ledger/{Conway,Shelley}/Rules/<Foo>.hs`.

## Pattern: era-apply files are not strict mirrors of `<Era>.hs`

R274's auto-grade marked `state/eras/{shelley,allegra,mary,alonzo,babbage}.rs`
as `(a) DIRECT_MIRROR` against upstream
`cardano-ledger/eras/<era>/impl/src/Cardano/Ledger/<Era>.hs`. This is
incorrect: those upstream files are high-level era-config / type-alias
modules (`type ShelleyEra = …`, era-specific instances). The Yggdrasil
`eras/<era>.rs` files are *block-application* code that orchestrates
the per-rule helpers (Bbody / Ledger / Utxow / Utxo / Deleg / Pool /
Cert / Certs / NewEpoch / Epoch / Mir / PPUP / etc.) — concepts upstream
splits across `Rules/*.hs` files. So the right verdict is `(c)
synthesis`, with the docstring naming the per-rule files synthesized.

This pattern likely applies to other auto-graded `(a)` rows where the
upstream basename collision is conceptually wrong; R277–R281 will spot
similar cases and re-grade as needed.

## Snapshot.rs name clash documented

`state/snapshot.rs` is a Yggdrasil-side LSQ-friendly read-only capture
that aggregates ledger fields for `Ouroboros.Consensus.Shelley.Ledger.Query`
answer paths. Upstream's
`Ouroboros.Consensus.Storage.LedgerDB.Snapshots.hs` is a different
concept (on-disk snapshot codec — file format, not in-memory restore).
The docstring's `## Naming parity` block notes this clash explicitly:
the on-disk codec lives in Yggdrasil at `crates/storage/src/file_ledger.rs`
and `crates/storage/src/ocert_sidecar.rs`, not in `state/snapshot.rs`.
Per the strict-naming policy, the file COULD be renamed (e.g.
`query_snapshot.rs`) to disambiguate; R281 will verdict whether to
rename or keep the docstring caveat.

## Audit infrastructure improvement

`scripts/audit-strict-mirror.py` now excludes unit-test modules at any
level:
- `tests.rs` (the conventional sibling-of-mod-file unit-test module).
- `*_tests.rs` (e.g. `node/src/main_tests.rs`).

Pre-R276 audit included 16 such files; post-R276 audit excludes them.
Total file count drops from 225 → 209. This is a more accurate
strict-mirror sample because tests are inline `#[cfg(test)]` modules
that don't strictly mirror upstream files (upstream typically has its
own test trees under `test/` directories that are already filtered).

## Drift-guard verification

```text
$ python3 scripts/check-strict-mirror.py
strict-mirror: 0 violations (clean)
```

All state/ files now satisfy the policy: either a strict upstream
`.hs` mirror (3 files) or an explicit `## Naming parity` docstring
stanza (21 files).

## Verdict bucket counts

| Bucket | Pre-R276 | Post-R276 |
|---|---|---|
| `(a) DIRECT_MIRROR (auto)` | 58 | 52 (5 era-apply files re-graded to (c)) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING (docstring present)` | 7 | 28 |
| `(c-needed) NEEDS_DOCSTRING (no upstream + no docstring)` | 45 | 38 |
| `(NEEDS-REVIEW)` hand-grading required | 115 | 91 |
| **TOTAL** | 225 | 209 (16 tests.rs files excluded) |

24 state/ files closed out (cbor + 13 c-needed + 8 NEEDS-REVIEW + 2 era
files moved to (c)). Outside state/, 7 (c-needed) and 91 (NEEDS-REVIEW)
remain for R277–R281.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 8.88s)
cargo lint                          clean (Finished `dev` profile in 16.57s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/ledger/src/state/cbor.rs                          +13 lines (parity block)
crates/ledger/src/state/chain_dep.rs                     +9 lines
crates/ledger/src/state/checkpoint.rs                    +8 lines
crates/ledger/src/state/committee_state.rs               +9 lines
crates/ledger/src/state/deposit_pot.rs                   +9 lines
crates/ledger/src/state/drep_state.rs                    +8 lines
crates/ledger/src/state/eras/allegra.rs                  +8 lines
crates/ledger/src/state/eras/alonzo.rs                   +9 lines
crates/ledger/src/state/eras/babbage.rs                  +9 lines
crates/ledger/src/state/eras/byron.rs                    +13 lines
crates/ledger/src/state/eras/conway.rs                   +9 lines
crates/ledger/src/state/eras/mary.rs                     +8 lines
crates/ledger/src/state/eras/shelley.rs                  +10 lines
crates/ledger/src/state/governance_action_state.rs       +9 lines
crates/ledger/src/state/phase1_validation.rs             +10 lines
crates/ledger/src/state/pool_state.rs                    +11 lines
crates/ledger/src/state/ppup.rs                          +12 lines
crates/ledger/src/state/reward_accounts.rs               +9 lines
crates/ledger/src/state/snapshot.rs                      +12 lines
crates/ledger/src/state/stake_credentials.rs             +8 lines
crates/ledger/src/state/treasury.rs                      +8 lines
docs/strict-mirror-audit.tsv                             rebuilt
docs/operational-runs/2026-05-09-round-276-state-naming-parity.md  (new)
scripts/audit-strict-mirror.py                           +6 lines (tests.rs exclusion)
```

## Stop point — Phase B progress

| Round | Cluster | Status |
|---|---|---|
| R276 | `crates/ledger/src/state/` (24 files) | ✅ closed |
| R277 | `consensus/{nonce,diffusion_pipelining}/` reverify | next |
| R278 | `consensus/mempool/{queue,tx_state}/` reverify | pending |
| R279 | `node/src/runtime/` synthesis pass (~17 files) | pending |
| R280 | `crates/network/src/governor/` regrade | pending |
| R281 | sweeper (residuals: storage, crypto, network non-governor, node top-level) | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R275 (`docs/operational-runs/2026-05-09-round-275-strict-mirror-drift-guard.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
