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
- **R367 — cardano-testnet: typed CLI parser (subcommand dispatch from Parsers/Run.hs::commands).**
  Lands the top-level subcommand dispatch for cardano-testnet,
  replacing the R335 passthrough Args. parser.rs ports upstream's
  `commands :: EnvCli -> Parser CardanoTestnetCommands` — a 4-way
  subcommand recognition layer covering `cardano`, `create-env`,
  `version`, `help`. Each subcommand variant currently carries an
  opaque PassthroughArgs (raw post-subcommand argv tail); the deep
  era-aware option records (CardanoTestnetCliOptions,
  CardanoTestnetCreateEnvOptions, VersionOptions) are carved out
  pending yggdrasil-ledger's era surface being exposed at crate
  boundaries — subsequent rounds will replace PassthroughArgs with
  the typed records. Top-level --help / --version short-circuit
  anywhere in argv (matches upstream's helper combinator behavior).
  ParseError variants: HelpRequested / VersionRequested /
  MissingSubcommand / UnknownSubcommand. Carve-outs documented:
  CardanoTestnetCliOptions/CreateEnvOptions payloads (era-aware
  records depending on Cardano.Api machinery); EnvCli env-var
  threading. lib.rs::run_main() wires parser → Command → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting which
  subcommand was selected + roadmap pointer.
  Tests: cardano-testnet 22 → 32 (+10: 3 help/version detection +
  1 missing-subcommand + 1 unknown-subcommand + 4 subcommand-
  dispatch verifications [cardano, create-env, version, help] +
  3 passthrough-args round-trips [cardano with flags, create-env
  with flags, version with no args] + 1 help-inside-subcommand-
  window short-circuits + 1 PassthroughArgs default check).
  Workspace: 5,348 → 5,358. Parity-matrix entry sister-tool.cardano-
  testnet advanced: next_milestone R360 → R368.
- **R366 — cardano-tracer: typed CLI parser (port of CLI.hs::parseTracerParams).**
  Lands the typed parser dispatcher for cardano-tracer. parser.rs ports
  upstream's `parseTracerParams :: Parser TracerParams` — a thin
  3-flag CLI shell since the bulk of the operator surface lives in
  the YAML config file (parsed at startup via
  configuration::parse_tracer_config_json).
  Flags:
  - `-c` / `--config FILEPATH` — mandatory; tracer's YAML/JSON config.
  - `--state-dir FILEPATH` — optional; RTView state directory (RTView
    itself is carved out per the plan; the flag is parsed verbatim).
  - `--min-log-severity SEVERITY` — optional; per-message severity
    floor.
  New SeverityS enum (Debug | Info | Notice | Warning | Error |
  Critical | Alert | Emergency) mirroring upstream
  Cardano.Logging.SeverityS — distinct from the existing
  configuration::Verbosity (Minimum | ErrorsOnly | Maximum) which
  controls the tracer's own verbosity rather than per-message
  severity floor. SeverityS::from_str_strict parses the
  upstream-canonical Haskell constructor names case-sensitively
  (matching `option auto`'s Read instance).
  ParseError variants: MissingConfig / InvalidSeverity /
  UnknownFlag / MissingValue / HelpRequested / VersionRequested.
  lib.rs::run_main() wires parser → Args → run() chain end-to-end.
  lib.rs::run() returns a sentinel reporting the resolved config /
  state-dir / min-log-severity + roadmap pointer (config-file load +
  Acceptors/Handlers/Logs/Metrics wiring land in subsequent rounds).
  Tests: cardano-tracer 21 → 40 (+19: 3 help/version + 1 minimal-
  config-only + 1 config-short-form + 1 state-dir + 1 all-8-severity-
  levels [single test that loops] + 2 unknown-severity / case-
  sensitive-rejection + 1 full-canonical + 3 error rejections
  [missing-config / unknown-flag / missing-value] + 2 SeverityS
  default+ordering checks). Workspace: 5,337 → 5,348. Parity-matrix
  entry sister-tool.cardano-tracer advanced: next_milestone R359 →
  R367.
- **R365 — db-analyser: typed CLI parser (port of DBAnalyser/Parsers.hs::parseDBAnalyserConfig).**
  Lands the typed parser dispatcher for db-analyser, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parseDBAnalyserConfig :: Parser DBAnalyserConfig` and the per-section
  helpers (parseSelectDB / parseValidationPolicy / parseAnalysis /
  parseLimit + per-analysis sub-parsers).

  Mandatory flag: `--db PATH`.

  Optional flags: `--verbose`, `--analyse-from SLOT_NUMBER`
  (default Origin), `--db-validation {validate-all-blocks,
  minimum-block-validation}`, `--num-blocks-to-process INT`
  (default Limit::Unlimited).

  LedgerDB backend (mutually exclusive; one required):
  `--in-mem` (V2InMem) | `--lsm` (V2LSM).

  Analysis-name dispatch (mutually exclusive; default OnlyValidation
  when none supplied — matches upstream's `pure OnlyValidation`
  last-resort branch):
  --show-slot-block-no / --count-tx-outputs / --show-block-header-size
  / --show-block-txs-size / --show-ebbs / --count-blocks /
  --trace-ledger / --store-ledger SLOT [--full-ledger-validation] /
  --checkThunks N / --repro-mempool-and-forge INT /
  --benchmark-ledger-ops [--out-file PATH] [--full-ledger-validation] /
  --get-block-application-metrics N [--out-file PATH].

  Carve-out documented: parseCardanoArgs / CardanoBlockArgs (era-aware
  Byron/Shelley/Cardano block-construction args) deferred until
  yggdrasil-ledger's era surface is exposed at crate boundaries; the
  current parser ignores any era-specific flags and the deeper round
  wires them in alongside per-era HasAnalysis dispatch.

  ParseError variants: MissingDb / MissingLedgerDbBackend /
  ConflictingLedgerDbBackend / ConflictingAnalysisName /
  InvalidDbValidation / UnknownFlag / MissingValue / InvalidValue.

  lib.rs::run_main() wires parser → DBAnalyserConfig → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting the
  resolved db/analysis/backend/limit + roadmap pointer (per-era
  HasAnalysis + Analysis.hs dispatch land in subsequent rounds).

  Tests: db-analyser 22 → 50 (+28: 3 help/version + 13 analysis-name
  variants [show-slot-block-no, count-tx-outputs, show-block-header-
  size, show-block-txs-size, show-ebbs, count-blocks, trace-ledger,
  store-ledger default-reapply, store-ledger with-full-validation,
  check-thunks, repro-mempool-and-forge, benchmark-ledger-ops with
  and without --out-file/--full-ledger-validation,
  get-block-application-metrics with and without --out-file] + 5
  optional-flag round-trips [verbose, lsm-backend, analyse-from,
  db-validation modes, num-blocks-to-process] + 7 rejection paths
  [missing-db, missing-backend, conflicting-backends, conflicting-
  analysis-flags, invalid-db-validation, unknown-flag, missing-value,
  invalid-slot-value]). Workspace: 5,307 → 5,337. Parity-matrix
  entry sister-tool.db-analyser advanced: next_milestone R352 → R366.
- **R364 — db-synthesizer: typed CLI parser (port of DBSynthesizer/Parsers.hs::parserCommandLine).**
  Lands the typed parser dispatcher for db-synthesizer, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parserCommandLine :: Parser (NodeFilePaths, NodeCredentials,
  DBSynthesizerOptions)`. Args struct collapses the upstream tuple
  into a single record. Mandatory flags: --config FILE + --db PATH.
  Mutually-exclusive forge-limit flags: -s/--slots / -b/--blocks /
  -e/--epochs. Mutually-exclusive open-mode flags: -f (force) / -a
  (append); default OpenCreate. Optional credential flags:
  --shelley-operational-certificate, --shelley-vrf-key,
  --shelley-kes-key, --bulk-credentials-file. ParseError variants:
  MissingConfig, MissingDb, MissingForgeLimit,
  ConflictingForgeLimits, ConflictingOpenModes, plus the standard
  HelpRequested/VersionRequested/UnknownFlag/MissingValue/
  InvalidValue. lib.rs::run_main() wires parser → Args → run() chain
  end-to-end. lib.rs::run() returns a sentinel reporting the
  resolved config/db/limit/open-mode + roadmap pointer (Forging.hs +
  Run.hs land in subsequent rounds gated on Phase C entry per the
  plan's Phase C authorization checkpoint).
  Tests: db-synthesizer 16 → 29 (+13: 3 help/version + 1 minimal
  canonical + 2 alternate-forge-limit forms [blocks / epochs short-
  form] + 2 open-mode-overrides [force / append] + 1 all-credentials
  + 4 missing-flag rejections [config / db / forge-limit / conflict
  ing-forge-limits + conflicting-open-modes] + 3 unknown-flag /
  missing-value / invalid-slot-number rejections). Workspace:
  5,294 → 5,307. Parity-matrix entry sister-tool.db-synthesizer
  advanced: next_milestone R355 → R365.
- **R363 — snapshot-converter: typed CLI parser (port of snapshot-converter.hs::parseConfig).**
  Lands the typed parser dispatcher for snapshot-converter, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  `parseConfig :: Parser Config` — the mutually-exclusive daemon-vs-
  oneshot mode dispatch.

  Daemon mode (3 required flags):
  - `--monitor-lsm-snapshots-in PATH` — directory to watch
  - `--lsm-database PATH` — backing LSM database file
  - `--output-mem-snapshots-in PATH` — output directory

  Oneshot mode (input + output, each from mutually-exclusive forms):
  - input: `--input-mem PATH` OR `--input-lsm-snapshot PATH +
    --input-lsm-database PATH`
  - output: `--output-mem PATH` OR `--output-lsm-snapshot PATH +
    --output-lsm-database PATH`

  ParseError variants for the dispatch failure modes:
  MissingMode / ConflictingModes / MissingDaemonFlag /
  MissingOneshotInput / MissingOneshotOutput /
  ConflictingOneshotInput / ConflictingOneshotOutput /
  LsmInputMissingDatabase / LsmOutputMissingDatabase / UnknownFlag /
  MissingValue. lib.rs::run_main() wires parser → Config → run()
  chain end-to-end. lib.rs::run() returns a sentinel reporting which
  mode was selected + roadmap pointer (convertSnapshot LSM/Mem
  conversion logic + filesystem-watcher daemon land in subsequent
  rounds gated on yggdrasil-format LedgerStore reader/writer being
  available).

  Tests: snapshot-converter 23 → 30 (+7 net after the 7 unused-pre-R363
  passthrough tests dropped + 14 new parser unit tests):
  - 3 help/version detection
  - 1 missing-mode (empty argv) + 1 unknown-flag + 1 missing-value
  - 1 daemon canonical 3-flag invocation + 2 daemon-missing-flag rejections
  - 4 oneshot canonical: mem-to-lsm / lsm-to-mem / mem-to-mem /
    lsm-to-lsm
  - 6 oneshot rejection paths: conflicting-modes /
    conflicting-input / conflicting-output /
    lsm-input-missing-database /
    lsm-output-missing-database / missing-output

  Workspace: 5,279 → 5,294. Parity-matrix entry sister-tool.snapshot-
  converter advanced: next_milestone R354 → R364.
- **R362 — kes-agent-control: typed CLI parser (port of ControlMain.hs::pProgramOptions).**
  Lands the typed parser dispatcher for kes-agent-control, replacing
  the R335 passthrough Args. parser.rs ports upstream's
  pProgramOptions + per-subcommand parsers (pCommonOptions /
  pGenKeyOptions / pQueryKeyOptions / pDropStagedKeyOptions /
  pDropKeyOptions / pInstallKeyOptions). Two-pass walk:
  (1) Locate the subcommand keyword (gen-staged-key /
      export-staged-vkey / drop-staged-key / install-key / drop-key /
      info); split argv into before/subcommand/after windows.
  (2) Parse common options from both before- and after- windows
      (filtering per-subcommand flags out of the after-window before
      passing to the common-options parser).
  (3) Apply common-options overrides to the chosen subcommand via
      ProgramOptions::with_common_options.
  Common options dispatched from any position around the subcommand:
  -c / --control-address, -v / --verbose, --retry-interval (alias
  --retry-delay), --retry-exponential (boolean switch),
  --retry-attempts. Per-subcommand options: --kes-vkey for
  gen-staged-key + export-staged-vkey; --op-cert for install-key.
  --help and --version short-circuit at parse time matching upstream's
  helper combinator behavior. lib.rs::run_main() wires parser →
  ProgramOptions → run() chain end-to-end. lib.rs::run() returns a
  sentinel reporting the chosen subcommand + roadmap pointer
  (per-subcommand ControlClient socket I/O lands when the kes-agent
  server mini-arc completes). Tests: kes-agent-control 17 → 36 (+19:
  3 help/version detection + 1 missing-subcommand + 1 unknown-
  subcommand + 6 per-subcommand-minimal parses + 4 common-options-
  before/after/short-form/retry-exponential + 3 retry-interval /
  retry-delay alias / retry-attempts + 3 missing-value /
  invalid-verbosity / unknown-flag rejections + 1 full canonical
  install-key invocation). Workspace: 5,260 → 5,279. Parity-matrix
  entry sister-tool.kes-agent-control advanced: next_milestone R356
  → R363; runtime ControlClient socket I/O still pending kes-agent
  server mini-arc per the per-tool roadmap.
- **R361 — dmq-node: typed CLI parser (port of CLIOptions.hs::parseCLIOptions).**
  Lands the typed parser dispatcher for dmq-node, replacing the R335
  passthrough Args. parser.rs ports upstream's
  `parseCLIOptions :: Parser PartialConfig` — the 10-flag grammar
  (--host-addr, --host-ipv6-addr, -p/--port, --local-socket,
  -c/--configuration-file, -t/--topology-file, --cardano-node-socket,
  --cardano-network-magic, --dmq-network-magic, -v/--version,
  -h/--help) maps each flag to the matching field in
  types::PartialConfig. `--version` is an in-grammar switch (sets
  show_version: Some(true) for downstream dispatch; not a parser
  short-circuit) matching upstream's optparse-applicative behavior;
  `--help` short-circuits via ParseError::HelpRequested. ParseError
  enum gains UnknownFlag/MissingValue/InvalidValue variants for
  the typed-flag dispatch failure modes. lib.rs::run_main() now
  wires the parser → resolve → run() chain: HelpRequested emits
  HELP_TEXT and exits 0; show_version emits VERSION_TEXT and exits
  0; otherwise resolves PartialConfig → Configuration via
  PartialConfig::resolve and hands off to run(&Configuration).
  lib.rs::run() returns a sentinel reporting resolved-config field
  values + roadmap pointer (R362+ for Diffusion/NodeKernel/
  PeerSelection wiring). Tests: dmq-node 14 → 33 (+19: 1 detect-help-
  long + 1 detect-help-short + 2 version-flag-sets-show-version +
  9 individual-flag round-trips + 1 full canonical invocation + 4
  rejection paths [unknown-flag / missing-value / invalid-port /
  invalid-network-magic] + 2 resolve-end-to-end checks). Workspace:
  5,241 → 5,260. Parity-matrix entry sister-tool.dmq-node advanced:
  next_milestone R357 → R362; remaining_work narrowed
  (Configuration.hs port → mux wiring → Diffusion/NodeKernel +
  Tracer + integration + closeout).
- **R359 — cardano-testnet: simple-types port (operator-facing knobs from Start/Types.hs).**
  Lands the simple operator-facing types from upstream's
  Testnet/Start/Types.hs that don't pull in the deeper Cardano.Api /
  Cardano.Ledger.* machinery. New types.rs module ports:
  - DEFAULT_TESTNET_MAGIC = 42 const (matches upstream
    defaultTestnetMagic).
  - NodeId(i32), NumPools(i32), NumRelays(i32), NumDReps(i32) — numeric
    newtypes with Ord/PartialOrd for natural sorting.
  - InputNodeConfigFile(PathBuf) — user-provided node-config file path.
  - UpdateTimestamps (UpdateTimestamps [default] | DontUpdateTimestamps).
  - RpcSupport (RpcDisabled [default] | RpcEnabled).
  - NodeLoggingFormat (AsJson [default] | AsText) with from_string
    mirroring upstream readNodeLoggingFormat (case-insensitive
    "json"/"text" parse, error otherwise).
  - GenesisHashesPolicy (WithHashes [default] | WithoutHashes).
  - PraosCredentialsSource (UseKesKeyFile [default] | UseKesSocket).
  - UserProvidedData<A> generic wrapper (UserProvidedData a |
    NoUserProvidedData [default]) with as_ref + into_option helpers.
  Carve-outs documented in module docstring:
  - Cardano.Api era machinery (cardanoEra / AnyShelleyBasedEra /
    AnyCardanoEra) — port lands when yggdrasil-ledger era surface is
    exposed at crate boundaries.
  - Cardano.Ledger.Alonzo.Genesis.AlonzoGenesis +
    Cardano.Ledger.Conway.Genesis.ConwayGenesis — kept as
    serde_json::Value at this surface; typed parsing happens at
    use-site in yggdrasil-ledger.
  - Hedgehog.MonadTest — carved out per the plan's pre-approved
    Process/Property module carve-out (Rust uses tokio::process +
    proptest equivalents at R363+).
  Tests: cardano-testnet 8 → 22 (+14: 1 default-magic + 4 numeric-
  newtype round-trips + 1 NodeId-Ord + 1 InputNodeConfigFile + 5
  default-impl checks + 3 NodeLoggingFormat::from_string + 2
  UserProvidedData round-trips). Workspace: 5,225 → 5,241.
  Parity-matrix entry sister-tool.cardano-testnet advanced:
  next_milestone R417 → R360; rust_surface description updated to
  reflect R359 simple-types + per-module-roadmap; remaining_work
  refreshed (Filepath/Conf/Process → era-aware records →
  Testnet/Types.hs runtime types → Process/Property carve-outs →
  per-subcommand wiring → integration + closeout).
- **R358 — cardano-tracer: typed configuration surface (port of Configuration.hs).**
  Lands the typed configuration surface for cardano-tracer. New
  configuration.rs module ports the full upstream
  Cardano.Tracer.Configuration surface:
  - TracerConfig (17-field record with serde renames matching
    upstream's Aeson-derived FromJSON field names exactly:
    networkMagic, loRequestNum, ekgRequestFreq, hasEKG,
    hasPrometheus, hasRTView, hasTimeseries, tlsCertificate,
    hasForwarding, logging, rotation, verbosity, metricsNoSuffix,
    metricsHelp, resourceFreq, ekgRequestFull, prometheusLabels).
  - HowToConnect (LocalPipe FilePath | RemoteSocket host port)
    untagged sum mirroring Cardano.Logging.Types.HowToConnect; type
    alias Address = HowToConnect.
  - Endpoint (host + port + optional force_ssl); is_null predicate
    mirroring upstream's nullEndpoint.
  - Certificate (file + key_file + optional chain).
  - RotationParams (frequency_secs / log_limit_bytes /
    max_age_minutes / keep_files_num) with defaults matching
    upstream's hand-written FromJSON: frequency_secs = 60,
    max_age_minutes = 24 * 60.
  - LogMode (FileMode | JournalMode), LogFormat (ForHuman | ForMachine).
  - LoggingParams (root + mode + format) with is_invalid_file_mode
    predicate.
  - Network (AcceptAt accept_at | ConnectTo connect_to) untagged sum.
  - Verbosity (Minimum | ErrorsOnly | Maximum).
  - FileOrMap untagged sum (File PathBuf | Map BTreeMap<String,
    String>).
  - HasForwarding (network + optional path-prefixes + options).
  - TraceOptionForwarder JsonValue placeholder; typed parsing lands
    when the trace-forwarder mini-protocol port is wired.
  - well_formed validator mirroring upstream's wellFormed:
    catMaybes-then-intercalate ', '-shaped problem-list reporting
    AcceptAt-not-empty + ConnectTo-not-all-empty + logRoot-non-empty-
    when-FileMode + duplicate-port-detection + non-empty-hosts on
    hasEKG/hasPrometheus/hasRTView.
  - parse_tracer_config_json runs serde_json + nubLogging dedup +
    well_formed validation.
  Carve-outs documented in module docstring:
  - Network.Wai.Handler.Warp.HostPreference/Port/Settings replaced
    by String/u16 + std::net::SocketAddr at use-sites.
  - Cardano.Logging.Types.TraceOptionForwarder kept as
    serde_json::Value at this layer.
  - readTracerConfig IO-with-die replaced by parse_tracer_config_json
    returning a Result.
  - YAML parsing not yet wired (only JSON); serde_yaml can be added
    when operator-side wiring lands.
  Cargo deps: serde + serde_json added (eyre/thiserror were already
  present).
  Tests: cardano-tracer 8 → 21 (+13: 2 HowToConnect is_null + 1
  Endpoint is_null + 2 RotationParams serde-with-defaults + 2
  LoggingParams invalid-mode + 7 well_formed cases + 2
  parse_tracer_config_json round-trips/error-paths + 4 serde
  round-trips for FileOrMap/Verbosity/LogMode/LogFormat).
  Workspace: 5,202 → 5,225.
  Parity-matrix entry sister-tool.cardano-tracer advanced:
  next_milestone R361 → R359; rust_surface description updated to
  reflect R358 + per-module roadmap; remaining_work refreshed
  (Types.hs → CLI.hs → Acceptors → Handlers/Logs → Handlers/Metrics
  → Handlers/Notifications → Run.hs → RTView carve-out → integration
  + closeout).
- **R356 — dmq-node: typed config surface (port of CLIOptions.hs CLI shape).**
  Lands the operator-facing CLI shape for dmq-node. New types.rs
  module ports the partial-vs-resolved Configuration distinction:
  - LocalAddress(PathBuf) newtype (mirrors upstream's
    `LocalAddress` from Configuration.hs).
  - NetworkMagic(u32) newtype.
  - PartialConfig (10-field record, all Option<_>) — represents the
    CLI-derived partial configuration before merging with file-derived
    defaults. Mirrors upstream's `Configuration' Last`.
  - Configuration (10-field record, all concrete) — the fully-resolved
    runtime config. Mirrors upstream's `Configuration' Identity`.
  - PartialConfig::merge: left-priority field-level merge mirroring
    the Haskell Generic-derived Semigroup instance.
  - PartialConfig::resolve: fills in defaults for missing fields,
    yielding a fully-applied Configuration.
  - Configuration::defaults: matches upstream's defaultConfiguration
    (host=0.0.0.0/IPv6=::, port=3001, local_address=dmq-node.socket,
    config_file=dmq-node.json, topology_file=dmq-node-topology.json,
    cardano_node_socket=node.socket, cardano_network_magic=mainnet,
    network_magic=0).
  Carve-outs documented:
  - Generic-derived Semigroup/Monoid via gmappend/gmempty replaced by
    explicit merge() helper (one line per field; same shape as
    kes-agent-control's CommonOptions).
  - Data.Act action-on-types machinery (gpact derivation) replaced by
    straight field-level merging — the action abstraction has no
    semantic role at this surface.
  - Higher-level Configuration.hs functions (mkDiffusionConfiguration,
    readConfigurationFile, etc.) deferred — they touch
    Ouroboros.Network.Diffusion which is a substantial separate port.
  Tests: dmq-node 8 → 14 (+6: 1 LocalAddress round-trip + 1 NetworkMagic
  + 1 PartialConfig default-all-None + 1 merge-left-priority + 1
  defaults-match-upstream + 1 resolve-uses-defaults + 1 round-trip-all-
  supplied + 1 resolve-empty-yields-defaults). Workspace: 5,194 → 5,202.
  Parity-matrix entry sister-tool.dmq-node advanced: next_milestone
  R451 → R357; remaining_work refreshed with the per-module roadmap
  (Parsers → Configuration.hs port → mux wiring → Diffusion/NodeKernel
  → Tracer + integration + closeout).
- **R355 — kes-agent-control: typed config surface (port of ControlMain.hs option types).**
  Lands the typed configuration surface for kes-agent-control. New
  types.rs module ports the full upstream ControlMain.hs option types:
  - CommonOptions (5-field record: control_path, verbosity,
    retry_delay, retry_exponential, retry_attempts; all Option<_>
    so env-var-derived defaults can merge with CLI-flag-derived
    overrides via `merge()` mirror of the Haskell Semigroup instance).
  - GenKeyOptions, QueryKeyOptions, DropStagedKeyOptions,
    DropKeyOptions, InstallKeyOptions per-subcommand records (each
    carries CommonOptions + subcommand-specific paths).
  - ProgramOptions 6-variant sum (RunGenKey / RunQueryKey /
    RunDropStagedKey / RunInstallKey / RunDropKey / RunGetInfo).
  - with_common_options method mirroring upstream's WithCommonOptions
    typeclass + programOptionsWithCommonOptions dispatcher.
  Defaults match upstream's defXyzOptions byte-for-byte:
  control_path = "/tmp/kes-agent-control.socket", verbosity = 0,
  kes_verification_key_file = "kes.vkey", op_cert_file = "kes.vkey"
  (the latter is preserved verbatim from upstream — likely an
  upstream bug since logically it should default to a node.cert
  path; documented as such in the variant docstring).
  Carve-outs documented:
  - Haskell Semigroup instance for CommonOptions / GenKey / etc.
    replaced by explicit `merge()` inherent method (left wins per
    field; first non-None wins).
  - WithCommonOptions typeclass replaced by per-options-struct
    `with_common_options` helpers (each implementation is one-line;
    a trait would be over-engineering).
  Tests: kes-agent-control 8 → 17 (+9: 1 CommonOptions defaults
  match upstream + 1 merge-left-priority + 4 per-subcommand defaults
  + 1 install-key-options upstream-quirk preservation + 2
  ProgramOptions round-trips + 2 with_common_options applications).
  Workspace: 5,183 → 5,194.
  Parity-matrix entry sister-tool.kes-agent-control: rust_surface
  description + implemented_evidence refreshed; next_milestone stays
  at R356 (per-subcommand runtime ControlClient socket logic still
  pending — gated on kes-agent server mini-arc).
- **R354 — db-synthesizer: typed config surface (port of Types.hs).**
  Lands the typed configuration surface for db-synthesizer. New types.rs
  module ports the full upstream Cardano.Tools.DBSynthesizer.Types
  surface:
  - NodeConfigStub (6-field record: node_config, alonzo/shelley/byron/
    conway/dijkstra-genesis-file paths; dijkstra optional).
  - NodeFilePaths (config + chain_db PathBufs).
  - NodeCredentials (cert/VRF/KES/bulk Option<PathBuf> 4-tuple, default
    all None).
  - ForgeLimit (Block u64 | Slot SlotNo | Epoch u64).
  - ForgeResult newtype (forged: i64).
  - DBSynthesizerOpenMode (OpenCreate [default] | OpenCreateForce |
    OpenAppend).
  - DBSynthesizerOptions (limit + open_mode).
  - DBSynthesizerConfig (5-field record: config_stub, options,
    protocol_credentials, shelley_genesis, db_dir).
  Carve-outs documented in module docstring:
  - Aeson.Value → serde_json::Value (untyped JSON storage).
  - ProtocolFilepaths (from Cardano.Node.Types) collapsed to
    NodeCredentials struct of optional paths — db-synthesizer only
    consumes path values, not the typed credential machinery.
  - ShelleyGenesis (from Ouroboros.Consensus.Shelley.Node) kept as
    serde_json::Value at the surface layer; typed parsing happens in
    yggdrasil-ledger's genesis module at use-site.
  Cargo deps: yggdrasil-ledger added (for SlotNo); serde_json added
  (for the untyped JSON fields).
  Tests: db-synthesizer 8 → 16 (+8: 1 NodeConfigStub + 1 NodeFilePaths
  + 1 NodeCredentials default + 3 ForgeLimit variants + 1 ForgeResult
  + 1 default-impl + 1 DBSynthesizerOptions + 1 DBSynthesizerConfig).
  Workspace: 5,173 → 5,183.
  Parity-matrix entry sister-tool.db-synthesizer advanced:
  next_milestone R409 → R355; remaining_work refreshed with the
  per-module roadmap (Parsers → Forging → Run + integration + closeout).
- **R353 — snapshot-converter: typed config surface from CLI shape.**
  Lands the typed configuration surface for snapshot-converter. New
  types.rs module ports the operator-facing data declarations from
  upstream's app/snapshot-converter.hs:
  - SnapshotsDirectory(PathBuf), LsmDatabaseFilePath(PathBuf) PathBuf
    newtypes (re-exported upstream from
    Ouroboros.Consensus.Storage.LedgerDB.Snapshots).
  - StandaloneFormat (Mem [default] | Lsm) — only Mem is currently
    CLI-reachable; Lsm reserved for future parity.
  - SnapshotsDirectoryWithFormat (LsmSnapshot { directory, database })
    used by daemon mode.
  - SnapshotSpec (Standalone { path, format } | Lsm { path, database })
    — renamed from upstream's `Snapshot'` (with prime; Rust does not
    allow apostrophes in identifiers). The upstream `Snapshot`
    (no prime) is a different type defined in the LedgerDB module
    and is carved out — yggdrasil's parser-side surface stops at
    SnapshotSpec; the slot-name parsing + Snapshot pairing lands
    when the conversion logic is ported.
  - Config (Daemon { watch, output } | Oneshot { input, output }).
  Carve-outs documented:
  - convertSnapshot mem↔lsm logic carved-out — operates on upstream's
    ledger-DB on-disk format which yggdrasil does not implement
    (yggdrasil's LedgerStore uses a different on-disk layout under
    data_dir/ledger/). Future round paths: (a) yggdrasil-format ↔
    upstream-mem-format converter (semantic parity); or (b) implement
    upstream LSM/mem readers/writers as separate compat-snapshot crate.
  - Daemon-mode filesystem watcher (withManager / watchTree from
    System.FSNotify) — port-able but needs Rust `notify` crate
    equivalent. Tracked separately.
  Tests: snapshot-converter 8 → 23 (+15: 1 SnapshotsDirectory + 1
  LsmDatabaseFilePath + 1 StandaloneFormat default + 1
  SnapshotsDirectoryWithFormat + 2 SnapshotSpec constructors + 1
  SnapshotSpec path accessor + 2 Config round-trips + 7 implicit
  round-trip checks). Workspace: 5,164 → 5,173.

  Parity-matrix entry sister-tool.snapshot-converter advanced:
  next_milestone R402 → R354; remaining_work refreshed with the
  per-module roadmap (Parsers → Conversion → Daemon → Run +
  integration + closeout).
- **R352 — closure-status refresh for R346-R351 multi-arc work.**
  Updates docs/PARITY_SUMMARY.md Status banner: 345+ → 351+ rounds,
  prepared/updated date 2026-05-10, R347-R350 db-truncater Phase B.1
  + R351 db-analyser Phase B.2 added to closed-arcs list. Workspace
  test count refreshed 5,115 → 5,164. Audit table unchanged.
- **R351 — db-analyser: typed config surface (port of Types.hs).**
  Lands the typed configuration surface for db-analyser. New types.rs
  module ports the full upstream Cardano.Tools.DBAnalyser.Types
  surface: DBAnalyserConfig (7-field record: db_dir, verbose, select_db,
  validation, analysis, conf_limit, ldb_backend), AnalysisName (13-variant
  sum covering every analysis mode the upstream binary exposes —
  ShowSlotBlockNo / CountTxOutputs / ShowBlockHeaderSize /
  ShowBlockTxsSize / ShowEBBs / OnlyValidation / StoreLedgerStateAt /
  CountBlocks / CheckNoThunksEvery / TraceLedgerProcessing /
  BenchmarkLedgerOps / ReproMempoolAndForge / GetBlockApplicationMetrics),
  AnalysisResult (ResultCountBlock | ResultMaxHeaderSize), NumberOfBlocks
  newtype, Limit (Limit u64 | Unlimited), LedgerDBBackend (V2InMem |
  V2LSM), ValidateBlocks (ValidateAllBlocks | MinimumBlockValidation),
  LedgerApplicationMode (LedgerReapply | LedgerApply), SelectDB
  (SelectImmutableDB), WithOrigin<A> (Origin | At a). Reuses
  yggdrasil_ledger::SlotNo. Default impls match upstream's documented
  semantics (LedgerDBBackend=V2InMem, ValidateBlocks=
  MinimumBlockValidation, LedgerApplicationMode=LedgerReapply). Cargo
  deps: yggdrasil-ledger added. db-analyser tests: 8 → 22 (+14: 1
  WithOrigin + 1 SelectDB + 1 Limit + 3 default-impl + 4 AnalysisName
  variants + 2 AnalysisResult + 1 DBAnalyserConfig + 1 NumberOfBlocks
  ord). Workspace: 5,150 → 5,164. Parity-matrix entry
  sister-tool.db-analyser advanced: next_milestone R392 → R352;
  remaining_work refreshed with the per-module roadmap (Parsers →
  HasAnalysis → Analysis → CSV → Run + integration + closeout).
- **R350 — db-truncater: comparison harness for operator soak vs upstream.**
  Ships node/scripts/compare_db_truncater_to_upstream.sh — 200-line
  bash script for verification of yggdrasil-db-truncater against the
  upstream Haskell binary across the canonical surface. Three stages:
  (1) byte-equivalent --help / --version (already pinned by R335
  golden tests; re-checked here as smoke); (2) error-input rejection
  shape parity (missing --db, missing truncate target, conflicting
  truncate targets); (3) post-truncate semantic parity — operator
  supplies an upstream-format ChainDB + a yggdrasil-format ChainDB,
  the script copies both, runs the corresponding binary's truncate,
  and verifies both report completion. Storage-format divergence
  acknowledged: yggdrasil's ChainDB on-disk format diverges from
  upstream's chunked-binary-index layout, so the two binaries
  cannot operate on the same DB; semantic parity (both truncate
  successfully, both report a count) is verified instead. The
  script is operator-runnable, not CI-runnable: it requires the
  vendored upstream binary plus per-format synthesized ChainDBs.
  Promotion of the parity-matrix entry to verified_11_0_1 (R351
  closeout) is gated on operator running this script and reporting
  all stages passed. Stage 3 is skip-able (UPSTREAM_DB / YGGDRASIL_DB
  unset → CLI-only smoke). Parity-matrix entry sister-tool.db-
  truncater advanced: next_milestone R350 → R351.
- **R349 — db-truncater: Run.hs equivalent (functional binary).**
  Lands the operator-facing run procedure for db-truncater. New run.rs
  module mirrors upstream Cardano.Tools.DBTruncater.Run: resolve_target
  maps TruncateAfter::TruncateAfterSlot through verbatim, scans the
  immutable DB for the matching block on TruncateAfter::TruncateAfterBlock
  (errors on BlockNumberNotFound). run_with_store is generic over any
  ImmutableStore impl for unit-testability against InMemoryImmutable;
  run() opens FileImmutable at config.db_dir and delegates. lib.rs::run()
  now calls run::run() and reports `truncated immutable DB at slot N:
  K block(s) removed` to stderr (verbose-mode adds open + resolve
  trace lines). The binary is now end-to-end functional: an operator
  can invoke `db-truncater --db /path --truncate-after-slot N` (or
  --truncate-after-block N) and the on-disk immutable DB is rewound
  to that point. Carve-outs documented: upstream's async ChainDB
  bracket collapsed (FileImmutable is sync); upstream's
  Ouroboros.Consensus.Block.Abstract.Cardano type-level dispatch
  collapsed (Yggdrasil operates on era-tagged CBOR pass-through).
  Cargo deps: tempfile dev-dep added for the tempdir-backed
  FileImmutable smoke test. Tests: db-truncater 22 → 30 (+8: 3
  resolve_target + 4 run_with_store + 1 tempdir-backed FileImmutable
  smoke). Workspace: 5,142 → 5,150.
- **R348 — db-truncater: typed config surface + into_config validation.**
  Lands the typed CLI surface for db-truncater. New types.rs module
  mirrors upstream Cardano.Tools.DBTruncater.Types: DBTruncaterConfig
  (db_dir, truncate_after, verbose) + TruncateAfter (TruncateAfterSlot
  | TruncateAfterBlock) reusing yggdrasil_ledger::SlotNo + BlockNo as
  the canonical workspace types. parser.rs replaces the R335 raw-
  passthrough Args with typed fields (--db PATH, --truncate-after-slot
  SLOT_NUMBER, --truncate-after-block BLOCK_NUMBER, --verbose) +
  into_config(args) validation: errors on MissingDb, MissingTruncate-
  Target (neither truncate flag), or ConflictingTruncateTargets (both).
  lib.rs::run_main now invokes into_config after parsing to surface
  missing-flag errors clearly; lib.rs::run() returns a sentinel
  noting that R349 (Run.hs equivalent) is pending. Cargo deps:
  yggdrasil-ledger + yggdrasil-storage added (the storage dep is for
  R349; included now to keep dep wiring in one round). Tests: db-
  truncater 8 → 22 (+14: 3 types.rs round-trip + 11 parser unit + new
  into_config validation cases). Workspace: 5,126 → 5,142.
- **R347 — storage: ImmutableStore::trim_after_slot extension (db-truncater unblock).**
  Extends the ImmutableStore trait with trim_after_slot — the inverse
  of the existing trim_before_slot GC primitive. Removes all immutable
  blocks with slots strictly after the given slot; blocks at the slot
  or earlier are retained. Implementations on both InMemoryImmutable
  (simple retain) and FileImmutable (full crash-safe variant with
  mark-dirty / delete CBOR + legacy-JSON / mark-clean ceremony,
  mirroring the existing trim_before_slot pattern). ChainDb wrapper:
  truncate_immutable_after_slot delegates to the storage primitive
  with a docstring warning that callers must coordinate volatile +
  ledger cleanup separately. This unblocks Phase B.1 (db-truncater)
  by providing the storage-level truncation primitive that
  db-truncater's Run.hs-equivalent implementation needs at R388+.
  Tests: +11 (7 InMemoryImmutable cases including
  inverse_of_trim_before_slot invariant; 2 FileImmutable cases with
  crash-safe re-open verification; 2 ChainDb cases including
  volatile/ledger isolation contract). Workspace: 5,115 → 5,126.
- **R346 — closure-status refresh for R338-R345 cardano-submit-api Phase A.2 arc.**
  Updates docs/PARITY_SUMMARY.md Status banner: 336+ → 345+ rounds,
  prepared/updated date 2026-05-10, Phase A.2 cardano-submit-api
  implementation arc added to closed-arcs list. Workspace test count
  refreshed 4,982 → 5,115 (+133 in this session). Audit table
  unchanged at 257 (a) + 215 (c) = 472 graded files (R338-R345
  populated already-tracked stub files rather than adding new ones).
  Notes: cardano-submit-api closeout to verified_11_0_1 gated on
  operator running node/scripts/compare_submit_api_to_upstream.sh
  and reporting an empty diff.
- **R345 — cardano-submit-api comparison harness: operator-runnable soak vs upstream.**
  Ships node/scripts/compare_submit_api_to_upstream.sh — 175-line bash
  script that POSTs canonical inputs (empty body, malformed CBOR) to
  both upstream and yggdrasil binaries and diffs HTTP status + response
  body, then scrapes /metrics from both and diffs the # HELP / # TYPE
  shape. Counter line ordering and inner-reason-bytes for malformed-
  CBOR failures are documented as legitimate divergences (counter
  emit-order vs registry-insertion-order; mempool-reason hex bytes vs
  rendered cardano-api Show string). The script is operator-runnable,
  not CI-runnable: it requires a live cardano-node socket + the
  upstream binary, neither available in CI. Promotion of the parity-
  matrix entry to verified_11_0_1 (R346 closeout) is gated on an
  operator running this script and reporting an empty diff. Round-doc
  documents the procedure with a sample bringup (preview testnet) and
  expected output. Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R345 → R346.
- **R344 — cardano-submit-api Prometheus metrics: registry, port-retry server, tracer composition.**
  Lands the Prometheus metrics surface for cardano-submit-api,
  mirroring upstream Cardano.TxSubmit.Metrics. New metrics.rs
  module: MetricsRegistry with atomic AtomicU64 counters for
  tx_submit / tx_submit_fail; apply(MetricUpdate[]) +
  observe(TraceSubmitApi) wiring; render_prometheus emits
  `# HELP / # TYPE counter / <name> <value>` shape byte-equivalent
  to upstream serveMetrics; register_metrics_server with port-
  occupied retry up to MAX_PORT_OFFSET=1000 adjacent ports + tracing
  of MetricsServerError/Started/PortOccupied/PortNotBound events
  (matches upstream's "metrics endpoint disabled" semantic if every
  retry fails); spawned per-request tokio task serving
  GET /metrics → 200 OK with text/plain; version=0.0.4 / other → 404.
  web.rs::run_tx_submit_server_from_params now spawns both the HTTP
  server and the metrics server concurrently;
  make_metrics_aware_tracer wraps the operator tracer with registry
  observation so counter updates ride the same trace stream the
  operator's logger sees — no separate counter-bumping path. The
  ApplicationInitializeMetrics event applies a counter zero-set at
  startup matching upstream's forMachine semantic. Carve-outs
  documented: System.Metrics.Prometheus.Http.Scrape.serveMetrics +
  RegistrySample replaced by raw-tokio HTTP + AtomicU64 — no
  prometheus-client ecosystem dependency. Workspace tests:
  5,100 → 5,115 (+15: 13 metrics.rs tests + 2 web.rs
  metrics_aware_tracer tests). Crate total: 133 → 148.
  Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R344 → R345.
- **R343 — cardano-submit-api LocalTxSubmission wiring: async Handler + ntc_connect integration.**
  Completes the Phase A.2 web round: the placeholder 503 response from
  R342 is replaced with real NtC LocalTxSubmission integration, and
  lib.rs::run() now spins a tokio runtime + binds + serves until the
  listener exits. The cardano-submit-api binary is now end-to-end
  functional against a real cardano-node socket. Diff inventory:
  Cargo.toml adds `hex` + `yggdrasil-network` deps; rest/web.rs
  refactors `Handler` type alias to `Arc<dyn Fn(HttpRequest) ->
  Pin<Box<dyn Future<Output=HttpResponse> + Send>> + Send + Sync>`
  with sync_handler test helper; web.rs `tx_submit_post` is now async
  and calls new `submit_via_ntc` which opens ntc_connect per request,
  extracts NTC_LOCAL_TX_SUBMISSION ProtocolHandle, drives
  LocalTxSubmissionClient::submit, maps MsgAcceptTx → 202 / MsgRejectTx
  → 400 (TxCmdTxSubmitValidationError) / connect|protocol failure →
  503 (TxCmdTxSubmitConnectionError); MAINNET_NETWORK_MAGIC =
  764824073 constant exposed; lib.rs::run() builds tokio multi-thread
  runtime + Arc tracer forwarding to stderr via render_human + calls
  runtime.block_on(web::run_tx_submit_server_from_params). Carve-outs
  documented: Cardano.Api.deserialiseFromCBOR + multi-era FromSomeType
  table NOT ported (raw bytes pass through to NtC; cardano-node returns
  MsgRejectTx for malformed bytes — equivalent observable behavior);
  Cardano.Api.getTxId NOT ported (Yggdrasil returns empty 'OK' success
  body — operators can compute Blake2b-256 client-side; future
  enhancement). Workspace tests: 5,099 → 5,100 (+1 net). Crate total:
  132 → 133. Parity-matrix entry `sister-tool.cardano-submit-api`
  `next_milestone` advanced R343 → R344.
- **R342 — cardano-submit-api web server: raw-tokio HTTP listener + tx_submit_app dispatch.**
  Lands the HTTP server core for cardano-submit-api. Two production
  modules graduate from R335 stub-only to working web server:
  `rest/web.rs` (HttpRequest, HttpResponse types with response
  constructors for 202/400/404/405/413/503; parse_request scanning
  Content-Length / Content-Type / Transfer-Encoding with chunked-
  rejection + 32 KiB MAX_REQUEST_BYTES cap; encode emitting RFC 7230
  wire format with Connection: close; run_settings TCP listener
  tracing EndpointListeningOnPort + spawning per-connection handlers);
  `web.rs` (run_tx_submit_server outer supervisor mirroring
  runTxSubmitServer; tx_submit_app dispatch closure routing
  POST /api/submit/tx and emitting 404/405 for off-path / wrong-method;
  tx_submit_post placeholder returning 503 with byte-equivalent
  TxSubmitFail JSON for non-empty bodies and 400 TxSubmitEmpty for
  empty bodies). The raw-tokio TCP approach matches the existing
  node/src/metrics_server.rs pattern; NO axum / hyper / tower / warp
  dependency added — just the workspace tokio dep already present.
  Carve-outs documented in strict-mirror docstrings:
  Network.Wai.Handler.Warp.runSettingsSocket / bindPortTCP →
  tokio::net::TcpListener::bind; Servant.Application → Handler type
  alias + path-prefix dispatch; chunked transfer-encoding rejected
  (clients always send Content-Length). Real LocalTxSubmission
  integration deferred to R343 (currently a placeholder body wired
  with byte-equivalent JSON shape so client integrations can be
  tested against this binary now). Workspace tests: 5,076 → 5,099
  (+23: 4 encode + 11 parse + 2 run_settings #[tokio::test] + 6
  web.rs handlers/routing). Crate total: 109 → 132. Parity-matrix
  entry `sister-tool.cardano-submit-api` `next_milestone` advanced
  R342 → R343.
- **R341 — cardano-submit-api trace surface: for_machine, as_metrics, Namespace tables.**
  Completes the trace surface for cardano-submit-api by porting upstream
  `LogFormatting` and `MetaTrace` instance methods. R339 had landed
  the data-only TraceSubmitApi enum + render_human (mirror of
  `forHuman`); R341 adds: `for_machine` (forMachine — JSON shape per
  event byte-equivalent to upstream Aeson), `as_metrics` (asMetrics —
  counter increment instructions), `namespace_for` (namespaceFor — 11-
  variant Namespace enum), Namespace::segments / severity / metrics_doc
  inherent methods (mirror of MetaTrace.segments/severityFor/metricsDocFor),
  ALL_NAMESPACES const (allNamespaces). Three supporting types:
  `Severity` (Debug | Info | Warning | Error mirroring
  Cardano.Logging.SeverityS), `Namespace` (11-variant closed enum),
  `MetricUpdate` (CounterInc | CounterSet mirroring
  Cardano.Logging.MetricM's CounterM name (Maybe v) shape). The Rust
  port intentionally does not implement `LogFormatting` /
  `MetaTrace` typeclasses (no Rust analog under our backend-agnostic
  tracing layer); data is exposed via inherent methods + constants
  so callers can map to whatever tracing backend (`tracing`, `slog`,
  cardano-tracer NtN protocol) is wired at runtime. Workspace tests:
  5,052 → 5,076 (+24: 7 for_machine + 4 as_metrics + 3 namespace +
  8 severity/metrics_doc + 2 MetricUpdate). Parity-matrix entry
  `sister-tool.cardano-submit-api` `next_milestone` advanced R341 →
  R342.
- **R340 — cardano-submit-api type bridges: cli/types, cli/parsers, rest/types, rest/parsers.**
  Bridges Yggdrasil's flat `parser::Args` argv representation to
  upstream's typed parser surface (`TxSubmitCommand`/`TxSubmitNodeParams`).
  Four production modules graduate from R335 stub-only to full upstream
  port: `cli/types.rs` (ConfigFile/GenesisFile/SocketPath PathBuf
  newtypes; ConsensusModeParams Cardano-only enum with `#[default]`;
  NetworkId Mainnet|Testnet sum with From<NetworkMagic> glue;
  TxSubmitNodeParams 6-field record; TxSubmitCommand Run|Version sum);
  `cli/parsers.rs` (`into_command(&Args) → Result<TxSubmitCommand,
  CommandError>` mirroring upstream `pTxSubmit envCli`; per-field
  bridge fns `config_file_from_args` / `socket_path_from_args` /
  `network_id_from_args` / `metrics_port_from_args`; default
  constants `DEFAULT_WEBSERVER_PORT=8090`, `DEFAULT_METRICS_PORT=8081`);
  `rest/types.rs` (`WebserverConfig { host, port }` + `to_socket_addr`
  with wildcard support `*`/`0.0.0.0`/`::` → unspecified IPv4); `rest/
  parsers.rs` (`from_args(&Args, default_port) → WebserverConfig`
  mirroring `pWebserverConfig`). `lib.rs::run()` now validates argv
  → TxSubmitCommand before its sentinel error so missing-flag errors
  surface clearly to operators even before R341 lands the actual
  HTTP listener. Carve-outs documented in strict-mirror docstrings:
  `Cardano.CLI.Environment.EnvCli` (Yggdrasil parser is environment-
  blind), `Options.Applicative.Parser` combinators (centralized in
  `parse_args`), `Warp.HostPreference`/`Warp.Settings` (axum uses
  SocketAddr), `Cardano.Api.SocketPath`'s polymorphic `File 'Out`
  envelope (collapsed to direct PathBuf newtype). Workspace tests:
  5,023 → 5,052 (+29: 7 cli/types.rs + 11 cli/parsers.rs + 7
  rest/types.rs + 4 rest/parsers.rs). Parity-matrix entry
  `sister-tool.cardano-submit-api` `next_milestone` advanced
  R340 → R341.
- **R339 — cardano-submit-api foundations: Types, Util, TraceSubmitApi data enum.**
  Lands the dependency-closed foundation of the cardano-submit-api
  crate ahead of the R340 web round. Three production modules graduate
  from R335 stub-only to full upstream port: `types.rs` (TxSubmitPort,
  RawCborDecodeError, DecoderError, EnvSocketError, TxCmdError,
  TxSubmitWebApiError, render_tx_cmd_error — JSON shapes byte-
  equivalent to upstream Aeson via serde tag/content + transparent +
  untagged); `tracing/trace_submit_api.rs` (data-only TraceSubmitApi
  enum + MediumTxId helper + render_human strings byte-matching
  upstream forHuman); `util.rs` (log_exception generic over
  FnOnce(TraceSubmitApi)). Servant API types (`TxSubmitApi`,
  `TxSubmitApiRecord`, `CBORStream`) carved out under a `**Strict
  mirror:** none.` synthesis docstring with rationale: axum's
  router-based design has no Servant analog; CBOR content-type
  negotiation is handled inline at handler in R340. The
  `LogFormatting`/`MetaTrace`/`forMachine`/`asMetrics` tables for
  TraceSubmitApi are intentionally deferred to R340 alongside web
  round (when trace-receiver wiring is decided). Workspace tests:
  4,982 → 5,023 (+41: 24 in types.rs + 13 in trace_submit_api.rs +
  4 in util.rs). Parity-matrix entry `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed.
- **R335-R336 — Phase A skeleton milestone: 12/12 sister tools deployable.**
  R335 lands cardano-submit-api skeleton + parser (14 file-mirror
  tree, byte-equivalent --help/--version, 7-flag parser). The bulk
  skeleton commit brings 10 more sister-tool crates from `absent`
  → `partial`: cardano-testnet, cardano-tracer, db-analyser,
  db-synthesizer, db-truncater, dmq-node, kes-agent,
  kes-agent-control, snapshot-converter, tx-generator. Each ships
  lib.rs + main.rs + parser.rs (with `HELP_TEXT`/`VERSION_TEXT`
  fixtures captured from upstream binary) + golden test pinning
  byte-equivalence. R336 round-doc records the milestone:
  **all 12 sister tools have deployable Rust binaries** with
  byte-equivalent --help/--version output, 126 sister-tool tests
  total (31 bech32 + 15 cardano-submit-api + 80 bulk skeleton),
  workspace test count 4,856 → 4,982 (+126).
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
