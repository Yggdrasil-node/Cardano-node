# Changelog

All notable changes to Yggdrasil are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Strict 1:1 file-mirror arc (R273-rename + R274–R311) plus
docstring-classification cleanup sub-arc (R313–R320) plus closure-
status triad refresh (R321). Refreshes the vendored upstream Haskell
tree to policy tag `11.0.1`, lands a strict-mirror CI drift-guard
(warn-only at R275 → fail-build at R288 → index-vs-tree drift check
added R311), sweeps every production `.rs` to declare exactly one of
two canonical docstring forms (`**Strict mirror:** <upstream/path.hs>`
or `**Strict mirror:** none.`), purges all production
`#[allow(dead_code)]` sites + the lone production TODO, expands a
new `crates/cardano-cli/` workspace member mirroring the full
upstream cardano-cli surface (~237 files), trims `docs/` from 23 to
11 top-level markdown files (5 archived to `docs/archive/`), adds two
new CI parity validators (`check-fixture-manifest.py` +
`check-reference-artifacts.py`), and eliminates the entire
`(c) strict-partial` bucket via 24 docstring promotions, 11
reclassifications to canonical synthesis, 1 file merge (`multiplexer
.rs` → `mux.rs`), 2 file splits (`handshake.rs` → 3 leaves;
`inbound_governor.rs` → 2 leaves), and 2 final docstring tightens.
Workspace tests: **4,856 passing, 0 failing**. Final audit table
(post-R324): **246 `(a) DIRECT_MIRROR`** + **202 `(c) strict-none`**
= **448 graded files**; **zero `(c) strict-partial`** (R320);
**zero `(a) auto`** + **zero `(a) auto (affinity-filtered)`**
(R323/R324 — every (a) row has explicit
`**Strict mirror:** <upstream/path.hs>` declaration). The audit
table has exactly two canonical verdicts; zero ambiguity, zero
basename-heuristic reliance.

### Added

- **R274 — strict-mirror audit infrastructure.** Refreshes vendored
  install to `cardano-node 11.0.1`. Adds `scripts/audit-strict-mirror.py`
  (walks production `.rs` files, derives candidate upstream basenames
  via snake_case<->PascalCase, applies crate-to-repo affinity filter
  for ambiguous matches), `docs/upstream-haskell-files.txt` (4,676-path
  upstream `.hs` index), and `docs/strict-mirror-audit.tsv` (per-file
  verdict table; every Rust file graded `(a) DIRECT_MIRROR` or
  `(c) NO_MIRROR_NEEDS_DOCSTRING`).
- **R275 — strict-mirror drift-guard (warn-only).** Adds
  `scripts/check-strict-mirror.py` as a CI gate counterpart to the
  authoring-time skill at `.claude/skills/round-extraction/SKILL.md`.
  Flags any new file lacking either an upstream `.hs` mirror or a
  `## Naming parity` docstring stanza.
- **R288 — drift-guard promoted to fail-build.** After R281 closed
  the last violation, the drift-guard flips from `continue-on-error:
  true` to `--fail-on-violation`. Promoted to the named baseline gate
  set ("the five verification expectations").
- **R285 — Phase 6 wiring closure.** Phase 6 multi-peer BlockFetch
  was already shipped via alternative paths in R258; R285 cleans up
  5 stale `#[allow(dead_code)]` annotations on the original
  scaffolding helpers (3 marked `#[cfg(test)]` for test-only seam
  use; 1 routed via `unregister_worker`; 1 already wired but
  carrying a stale allow).
- **R289–R295 — `crates/cardano-cli/` workspace crate.** New
  `yggdrasil-cardano-cli` crate strict-mirrors all 180 upstream
  `Cardano.CLI.*` modules (~237 Rust files including parent shells).
  Sub-trees: Byron (10), Compatible (21), EraBased (57),
  EraIndependent (34), Type (33), Legacy (5), plus IO/Json/OS/Read/
  Run/Option (~10) and the 8 top-level Cardano.CLI namespace files.
  Each leaf carries a strict-mirror `## Naming parity` block;
  parent shells carry synthesis declarations.
- **R296–R297 — concrete cardano-cli migration kickoff.** Wires the
  new crate into the `yggdrasil-node cardano-cli` subcommand:
  R296 routes the `Version` banner through
  `yggdrasil_cardano_cli::helper::version_info()`; R297 migrates
  `ShowUpstreamConfig`'s path resolution + JSON output into
  `yggdrasil_cardano_cli::environment::*`. `QueryTip` remains in
  the node binary pending an LSQ-client trait abstraction.

### Changed

- **R273-rename — strict naming parity for R273b–i sub-modules.**
  9 `git mv` renames in `consensus/opcert/`, `plutus/types/`,
  `plutus/cost_model/`, `plutus/flat/` aligning sub-file basenames
  with upstream (`OCert.hs`, `Builtins.hs`, `Internal.hs` etc.). The
  authoring-time skill at `.claude/skills/round-extraction/SKILL.md`
  is hardened with explicit filename-mirror rules.
- **R276–R281 — Phase B sub-module docstring sweep.** Adds
  `## Naming parity` blocks to 161 sub-module files across
  `crates/ledger/src/state/` (24), `consensus/{nonce,opcert,
  diffusion_pipelining}/` (9), `consensus/mempool/` (7),
  `node/src/runtime/` (18), `network/governor/` (6), and the
  R281 sweeper (97 residuals). Includes 3 `git mv` renames
  (`opcert.rs`/`opcert/` -> `ocert.rs`/`ocert/` for upstream
  `Cardano.Protocol.TPraos.OCert`).
- **R283 — era_tag wiring.** Drops the stale `#[allow(dead_code)]`
  on `node/src/sync.rs::mod era_tag`. Introduces a new named-
  constant module `lsq_era_index` in `node/src/local_server.rs`
  (collapses Byron into a single LSQ ordinal vs the wire-tag's two)
  and replaces magic-number era arms in `GetCurrentPParams`
  dispatch with named constants.
- **R287 — closure annotations on planning docs.** `docs/code-audit.md`
  C-1/H-1/H-2/M-1..M-8/L-1..L-9 findings each receive an inline
  `[CLOSED in 2026-Q3]` annotation. `docs/REFACTOR_BLUEPRINT.md`
  Phase C–G headers each receive a `[DONE in RNNN]` annotation.
- **Phase D + R288 — living-doc parity-language sweep + drift-guard
  fail-build.** Adds strict-1:1-policy citations to every per-crate
  AGENTS.md (13 files); registers `check-strict-mirror.py` as the
  fifth named verification gate in CLAUDE.md and root AGENTS.md.

### Removed

- **R282 — unused `TextEnvelope::description` field.** Drops the
  `#[allow(dead_code)]` field in `node/src/block_producer.rs`. Serde's
  default behavior silently ignores unknown JSON keys, so wire-format
  compatibility with upstream-produced text-envelope files is
  preserved.
- **R286 — `_runstate_impl_marker` + unused `mk_txout`.** Deletes
  `node/src/runtime/reconnecting.rs::_runstate_impl_marker` (replaced
  with a comment line carrying the same visual seam) and the unused
  `crates/ledger/src/eras/shelley.rs::mk_txout` test helper.

### Documentation

- **R298–R300 — `docs/` cleanup arc.** Top-level markdown reduced
  23 -> 11 (-52%). Archived `docs/code-audit.md`,
  `docs/REFACTOR_BLUEPRINT.md`, `docs/AUDIT_VERIFICATION_2026Q2.md`,
  `docs/PARITY_PLAN.md`, `docs/UPSTREAM_RESEARCH.md` to
  `docs/archive/` with explicit Jekyll permalinks preserving
  published URLs. Trimmed `docs/PARITY_SUMMARY.md` from 485 -> 392
  lines by retiring six pre-execution planning sections superseded
  by the R269–R299 execution arc. Relocated `docs/poolMetaData.json`
  (operator artifact, not a doc) to `node/configuration/`.
- **R301 — SPECS.md improvements.** Adds CIPs repository link,
  a new "Local parity surfaces (machine-readable)" section pointing
  at `parity-matrix.json` + `strict-mirror-audit.tsv` +
  `upstream-haskell-files.txt`, and a new Usage Rule capturing the
  R274+ strict 1:1 file-mirror policy.
- **R302 — CLAUDE.md gate-count consistency.** Lines 12 + 127
  updated from "four verification gates" to "five verification
  gates" to match the post-R288 fail-build flip and the existing
  "All five are the required verification expectations" wording in
  the Commands section (line 96).
- **R303 — two new CI parity validators.** Adds
  `scripts/check-fixture-manifest.py` (cross-checks the
  `cardano-base` SHA pin across `node/src/upstream_pins.rs::
  UPSTREAM_CARDANO_BASE_COMMIT`, `specs/upstream-test-vectors/
  cardano-base/<SHA>/`, `docs/SPECS.md`, `docs/UPSTREAM_PARITY.md`;
  verifies 2 required corpora present) and
  `scripts/check-reference-artifacts.py` (validates
  `.reference-haskell-cardano-node/install/`: `cardano-node
  --version` matches policy tag, 9 binaries present, 3 networks ×
  8 config files). The fixture-manifest validator wired to CI;
  reference-artifacts kept local-only (CI doesn't carry the 1.3 GB
  vendored install).
- **R304 — refine strict-mirror citation in non-Rust dirs.**
  Adjusts `specs/AGENTS.md` and `node/configuration/AGENTS.md`
  to clarify the strict-mirror policy applies to production `.rs`
  files only (vendored fixtures + operator config files don't
  fall under R274's file-naming policy).
- **R305 — Reference landing + index + docs/AGENTS.md cleanup.**
  Reorganizes `docs/reference.md` to group docs into "Architecture
  & parity", "Specs & dependencies", "Validation & release", and
  "Archived planning docs" sections. Updates `docs/AGENTS.md`
  policy + manual chapter inventory.
- **R306 — `crates/cardano-cli/AGENTS.md`.** Adds operational guide
  for the new workspace crate: directory shape, strict-mirror policy
  recap, current status (R289–R295 bootstrap, R296+R297 migration
  kickoff), R298+ migration roadmap, integration with the `node`
  binary. Registers the new AGENTS.md in `crates/AGENTS.md` and
  `CLAUDE.md`.
- **R307 — PARITY_SUMMARY.md round-count + test-badge refresh.**
  Bumps round count from 251 to 306+; updates README test badge
  4.7K+ → 4.8K+ to reflect post-R273 baseline.
- **R308 — PARITY_PROOF.md header refresh + `scripts/AGENTS.md`.**
  Front-matter title `(Round 248)` → ``; document-round line
  refreshed; test-count line refreshed (4.7K+ → 4,855); five-gate
  snapshot added; R273-rename + R274–R307 arc summary blockquote
  added at the top of the body. New `scripts/AGENTS.md` registered
  in `CLAUDE.md`. Tail repair: `cargo fmt` auto-fix on 3 files
  with pre-R308 rustfmt drift.
- **R309 — AGENTS.md Current Phase R273-R308 arc closure.**
  Appends single arc-closure sentence to `AGENTS.md`'s Current
  Phase paragraph; long round-by-round notes (lines 540+) remain
  unchanged per existing line 159 guidance.
- **R310 — `.gitignore` over-broad `debug` pattern fix (CI
  failure).** Root `.gitignore` line 2 was a bare `debug` pattern
  intended for Cargo's `target/debug/` build output. Without a
  leading slash or `**/` prefix, gitignore rules match files or
  dirs with that basename anywhere in the tree, silently swallowing
  `crates/cardano-cli/src/era_independent/debug/` (12 R294 files
  never tracked despite existing locally and being graded in
  `docs/strict-mirror-audit.tsv`). Fixed: replace bare `debug` and
  redundant bare `target` with anchored `/target/`. Stage the 12
  previously-invisible files. Surveyed all other bare-basename
  ignore patterns; only `debug` had a current source-tree
  collision.
- **R311 — strict-mirror index-vs-tree drift check.** Closes the
  failure-detection gap that R310 exposed.
  `scripts/check-strict-mirror.py` previously walked only the local
  filesystem; an over-broad `.gitignore` pattern that silently
  swallowed production `.rs` files (R310's failure mode) was
  invisible to the gate, manifesting only as an opaque `cargo fmt`
  module-resolution error on a fresh CI clone. Adds a second pass
  cross-checking every production `.rs` against `git ls-files`;
  files present locally but not tracked surface as a distinct
  drift-violation class with actionable wording.
- **R312 — `docs/UPSTREAM_PARITY.md` arc-closure cross-walk.**
  Closes the canonical-status doc triad refresh by adding a top-
  of-document arc-closure blockquote covering R273-R311 and
  splitting Verification Baseline into "Five-gate snapshot
  (post-R311)" + "Historical R244–R249 closure evidence" subheads.
- **R313 — synthesis-file census.** Read-only census of the 215
  production `.rs` files that carry no 1:1 upstream mirror under
  `.reference-haskell-cardano-node/`. Re-runs
  `scripts/audit-strict-mirror.py` (TSV byte-identical to committed
  allowlist), groups the synthesis bucket by subsystem, sample-
  verifies docstring stanzas. Headline: 230 `(a) DIRECT_MIRROR` +
  215 `(c) NO_MIRROR_NEEDS_DOCSTRING` = 445 graded files; zero
  `(b)` rename-needed; zero `(d)` clash-regrade.
- **R314 — promote partial-mirror docstrings to canonical strict-
  mirror.** Closes the docstring-classification gap from R313's
  census: audit regex bug fix
  (`STRICT_PARTIAL_PATTERN` didn't match `**Strict mirror
  (partial):**` with the trailing colon — only without — so 41
  files fell through to "unspecified" instead of being recognized
  as "strict-partial"); 24 files promoted to canonical
  `**Strict mirror:** <upstream/path.hs>.` form.
- **R315 — reclassify Yggdrasil-side aggregator/glue files to
  synthesis.** R314 left 17 files in `(c) strict-partial` form. Of
  those, 8 were misclassified — they have no upstream 1:1 parallel
  and should declare canonical synthesis form
  (`**Strict mirror:** none.`) rather than the misleading
  `**Strict mirror (partial):**` form. Reclassified:
  `consensus/genesis_density.rs`, `consensus/in_future.rs`,
  `crypto/sha3_hash.rs`, `ledger/rewards.rs`, `ledger/utxo.rs`,
  `network/blockfetch_pool.rs`, `network/listener.rs`, `node/runtime/keep_alive.rs`.
- **R316 — reclassify 3 more partial-mirrors after content audit.**
  Operator asked whether the 9 remaining files could be promoted
  to direct mirrors. Content-vs-name audit revealed 3 more files
  where the basename matches an upstream `.hs` but the content
  diverges (`governor/churn.rs`, `governor/peer_metric.rs`,
  `praos/common.rs`). Reclassified to synthesis with explicit
  upstream symbol cross-references preserved.
- **R317 — merge `multiplexer.rs` into `mux.rs` (1:1 with
  `Mux.hs`).** Promotes `mux.rs` from `(c) strict-partial` to
  `(a) DIRECT_MIRROR` of upstream `Ouroboros.Network.Mux.hs` by
  merging the previously-separate `multiplexer.rs` types module
  back in. Upstream's `Mux.hs` carries SDU framing types +
  per-channel state machine + multiplexer/demultiplexer runtime
  in a single file; Yggdrasil's earlier split was a code-
  organization choice with no upstream basis. multiplexer.rs
  deleted; 9 importing files bulk-updated to use `crate::mux`.
- **R318 — split `handshake.rs` into 3 leaves matching upstream
  `Type/Version/Codec`.** Promotes 3 new leaves to
  `(a) DIRECT_MIRROR`. Split-impl pattern: `codec.rs` adds
  `impl HandshakeMessage` methods (to_cbor / from_cbor) for the
  type defined in `type.rs` — both modules in the same crate so
  Rust coherence accepts. Parent shell `handshake.rs` preserves
  the flat `crate::handshake::Foo` API via `pub use` re-exports;
  no caller in the workspace needs an import path update.
- **R319 — split `inbound_governor.rs` to mirror upstream
  `State.hs` separation.** Promotes 2 files to `(a) DIRECT_MIRROR`
  of upstream `InboundGovernor.hs` + `InboundGovernor/State.hs` by
  splitting Yggdrasil's previous monolithic file along the same
  axis upstream uses (data definitions vs runtime decision engine).
  Same split-impl pattern as R318.
- **R320 — promote 2 plutus partial-mirrors; strict-partial bucket
  now 0.** Closes the `(c) strict-partial` bucket entirely. The 2
  remaining files (`plutus/builtins.rs` + `plutus/machine.rs`) are
  promoted via docstring tighten with sibling-file rationale
  documented as implementation detail. Both files carrying
  `**Strict mirror (partial):**` because Yggdrasil's idiomatic
  split places supporting concerns (data types in `types/*.rs`,
  cost-model parameters in `cost_model/*.rs`) in sibling modules —
  but the primary runtime denotation logic each file carries IS a
  1:1 mirror of its upstream `.hs`. The `(partial)` qualifier was
  obscuring this.
- **R326-R334 — sister-tools port arc Phase Prep + Phase A.1 (bech32) shipped.**
  Prep block (R326-R330) bootstrapped the 12-tool sister-tools port
  arc: vendored bech32 + kes-agent + dmq-node sources (R326b);
  created 12 sister-tool skeleton crates (R327); extended parity-
  matrix +12 entries + upstream_pins +3 SHAs + drift detector
  cross-org URL support (R328); landed `node/scripts/run-tools.sh`
  12-binary dispatcher (R329); added `bech32 v0.11` workspace dep
  (R330). Phase A.1 (R331-R334) shipped the **first sister tool with
  full deployment-ready 100% parity**: `bech32` is now drop-in
  byte-equivalent to upstream `IntersectMBO/bech32 1.1.10` for every
  documented CLI surface (`--help`, `--version`, base16/bech32/base58
  encoding detection, encode/decode dispatch via stdin). Workspace
  test count: 4,856 → 4,887 (+31 new bech32 tests). Parity-matrix
  entry `sister-tool.bech32` advanced from `absent` to
  `verified_11_0_1`.
- **R321 — closure-status doc triad refresh for R313–R320.**
  Refreshes the four canonical closure-status documents
  (`PARITY_SUMMARY.md`, `PARITY_PROOF.md`, `AGENTS.md` Current
  Phase, `UPSTREAM_PARITY.md`) to reflect the R313–R320 docstring-
  classification cleanup arc. Status banner round count `306+ →
  320+`, test count `4,855 → 4,856`, audit table `230 (a) +
  215 (c) = 445 → 262 (a) + 186 (c) = 448`.
- **R322 — CHANGELOG.md backfill for R303–R321.** The CHANGELOG's
  `[Unreleased]` section's last comprehensive update was R302
  (covering R273–R301). After R302–R321 (20 rounds), R322 adds
  19 new bullets (one per round, ordered chronologically after
  the existing R302 bullet) and refreshes the `[Unreleased]`
  header summary. Without these entries the next tagged release
  would have shipped without mention of operationally-important
  rounds like R310 (gitignore CI failure fix) and R311
  (drift-detection hardening).
- **R323 — eliminate `(a) DIRECT_MIRROR (auto)` bucket via
  explicit declarations.** Closes the gap where the audit grader
  was relying on basename-match heuristic + crate-affinity filter
  alone for 25 files. Each file hand-audited against actual
  content: 17 promoted to canonical strict-mirror declarations
  (`blake2b.rs`, `bls12_381.rs`, `collateral.rs`, era files,
  state-rule files, `bearer.rs`, `root_peers.rs`, storage layer,
  protocol Type files); 8 reclassified to synthesis where the
  basename match was misleading (`secp256k1.rs` aggregating ECDSA
  + Schnorr, `epoch_boundary.rs` cross-era processor not Byron
  block boundary, `cost_model.rs` runtime parameter table not
  Agda metatheory, etc.). After R323, the `(a) auto` sub-bucket
  is empty.
- **R324 — eliminate `(a) DIRECT_MIRROR (auto (affinity-filtered))`
  bucket; audit table now binary.** Closes the last auto-graded
  sub-bucket of 18 files. 10 promoted to canonical strict-mirror
  declarations (Praos `Header.hs` + `VRF.hs`, `Ed25519.hs`,
  `KES/Sum.hs`, `VRF.hs` umbrella, `Alonzo.hs` + `Shelley.hs`
  era files, `LocalStateQuery/Type.hs`, `ChainDB.hs` + `LedgerDB.hs`
  storage). 8 reclassified to synthesis (`kes.rs` aggregator over
  Single + CompactSingle + Simple variants; `cbor.rs` workspace-
  wide helper not Byron-only; node-binary integration files).
  After R324 the audit table has **exactly two canonical
  verdicts**: `(a) declares strict mirror` (246 files) and
  `(c) declares synthesis` (202 files); 448 graded files total;
  zero auto-graded-by-basename. Cumulative R313–R324 closure:
  `(c) unspecified` 41 → 0; `(c) strict-partial` peaked at 17 → 0;
  `(a) auto` 25 → 0; `(a) auto (affinity-filtered)` 18 → 0;
  `(a) declares strict mirror` 187 → 246; `(c) strict-none`
  174 → 202.

## [0.2.0] - 2026-05-01

Public code-level parity closure release for the 2026-Q2 audit cycle.
This release includes the operational-parity arc from Rounds 144 → 245.
Highlights: full cardano-cli 10.16 query parity at preprod (Shelley
era) and preview (Alonzo era), **every Conway-era LSQ subcommand**
(constitution, gov-state, drep-state, drep-stake-distribution,
committee-state, treasury, spo-stake-distribution, proposals,
ratify-state, future-pparams, stake-pool-default-vote,
ledger-peer-snapshot) decoding end-to-end, multi-round
sync-speed and apply-correctness fixes, exact ChainDepState rollback
sidecar recovery, all four preset genesis hashes verified at startup,
and the Conway BBODY `HeaderProtVerTooHigh` testnet grace matched
through Dijkstra.  Workspace tests: **4.7K+ passing, 0 failing**.

### Added

- **R236 — Live `PoolDistr` for `GetStakeDistribution` and
  `GetSPOStakeDistr`**.  `encode_stake_distribution_map` and
  `encode_spo_stake_distribution_for_lsq` now source per-pool
  active stake from `LedgerStateSnapshot::stake_snapshots()`'s
  `set` snapshot (matching upstream `nesPd`).
  `cardano-cli conway query stake-distribution` and `query
  spo-stake-distribution` now render real per-pool data
  post-epoch-rotation; the empty-snapshot fallback (`0x82 0xa0
  0x01`) is preserved for pre-rotation chains.  The
  `IndividualPoolStake` 3-tuple wire shape `[Rational stake_share,
  CompactCoin pool_stake, VRFKeyHash]` matches upstream
  `Cardano.Protocol.TPraos.API.IndividualPoolStake.encCBOR`.
  Closes a Phase A.3 LSQ data-plumbing gap.
- **R238 — ChainDepState rollback sidecar hardening.** Startup,
  rollback, and LocalStateQuery recovery now restore the exact
  slot-indexed nonce/OpCert bundle from `chain_dep_state/<slot>.cbor`
  and replay stored blocks from that point. Persistent non-origin
  rollback fails closed when the required bundle history is missing.
- **R239/R243/R245 — upstream pin refreshes.** All six documentary
  IntersectMBO pins are in sync with live upstream heads; R245 updates
  `cardano-ledger` through the BBODY/GOV drift.
- **R240 — reproducible parallel BlockFetch soak harness.**
  `node/scripts/parallel_blockfetch_soak.sh` captures the §6.5
  multi-peer BlockFetch default-flip evidence path instead of relying
  on hand-assembled operator notes.
- **R242 — optional upstream `cardano-node-tests` wrapper workflow.**
  The workflow is manual-only and documented as an external parity
  harness, not a required CI gate.
- **R244 — Byron genesis hash parity.** `validate-config` now verifies
  Byron genesis files with upstream Canonical JSON hashing while
  Shelley-family genesis files continue to use raw-byte SHA256.
- **R245 — Conway BBODY testnet grace.** `HeaderProtVerTooHigh` is
  enforced on mainnet, suppressed on testnets before Dijkstra
  protocol major 12, and re-enabled on testnets from protocol major
  12 onward.

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


## 0.2.0 candidate checkpoint - 2026-04-27

This was an internal local checkpoint before the public `v0.2.0`
release tag. Later R211→R245 rounds closed the known issue noted in
this checkpoint and are included in the public `v0.2.0` section above.

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
  `docs/archive/code-audit.md` now list all four CI gates
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
  [`docs/archive/code-audit.md`](docs/archive/code-audit.md) for the source audit;
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

[Unreleased]: https://github.com/yggdrasil-node/Cardano-node/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.2.0
[0.1.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.1.0
