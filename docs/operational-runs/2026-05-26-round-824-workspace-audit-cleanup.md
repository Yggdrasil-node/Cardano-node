---
title: "Round 824 workspace audit cleanup"
parent: Reference
---

# Round 824 workspace audit cleanup

Date: 2026-05-26

## Goal

Run a full-workspace quality and completeness audit, repair the issues
that prevent trustworthy baseline signals, and leave explicit evidence
for the remaining completeness risks.

## Scope

- Cleaned an EOL-only dirty checkout signal before auditing. The semantic
  diff was empty under `git diff --ignore-space-at-eol`; refreshing the
  Git index returned the tree to a meaningful baseline.
- Repaired the parity-matrix validator so the accepted Phase 2.D and
  Phase 2.E sister-tool milestone ranges cover the current R591/R823-era
  matrix entries.
- Repaired `.claude/scripts/filetree.py` description generation so Rust
  doc summaries stop before `## Naming parity` stanzas, markdown summaries
  ignore YAML front matter, operational-run descriptions are stable, and
  unchanged accepted entries keep their existing accepted metadata.
- Added a root `.gitignore` entry for the transient tx-generator
  `plutus-budget-summary.json` selftest output when the upstream-shaped
  command is run from the tx-generator crate directory.
- Added a test-only thread-local budget-summary path for tx-generator
  unit tests. Production still writes the upstream-named
  `plutus-budget-summary.json`; tests no longer share one crate-root
  side-effect file under the parallel Rust test harness.
- Centralized the `YGG_LSQ_ERA_FLOOR` process-environment mutation used
  by NtC LocalStateQuery tests behind helper functions that require the
  held mutex guard. This keeps Rust 2024's unsafe environment mutation
  localized to two documented helper bodies instead of repeating unsafe
  blocks through the test cases.
- Regenerated and accepted `.claude/filetree/manifest.json` and
  `.claude/filetree/FILETREE.md`.
- Tightened `deny.toml` license policy by removing unused `MPL-2.0`
  and `Unicode-DFS-2016` allowances after `cargo-deny` reported that
  neither license appears in the current dependency graph.
- Tightened `scripts/check-doc-status-headers.py` so central status
  headers must agree on `As of date` and must not declare a round
  ceiling behind the highest `docs/operational-runs/*round-*.md` record.
- Extended `scripts/check-doc-status-headers.py` to load
  `docs/parity-matrix.json` and fail if the central docs' `Parity tag`
  drifts from `reference.tag`, keeping prose status metadata tied to
  the machine-readable parity source of truth.
- Extended `scripts/check-doc-status-headers.py` to validate
  `docs/PARITY_DASHBOARD.md` against the canonical header date and the
  live `docs/parity-matrix.json` entry/status counts, then refreshed
  the dashboard's last-updated date to the R824 audit date.
- Hardened the dashboard parser in `scripts/check-doc-status-headers.py`
  so duplicate dashboard date rows, total rows, or status-count rows
  fail explicitly instead of being masked during summary comparison.
- Scoped dashboard total/status parsing to the compact metric summary
  table so unrelated later Markdown tables can document examples
  without being misread as parity status-count rows.
- Tightened the dashboard parser to reject duplicate compact metric
  summary tables instead of silently accepting the first one and
  ignoring later copies.
- Anchored dashboard last-updated parsing to full status-date lines so
  inline examples in later prose are ignored while duplicate status-date
  lines are still rejected.
- Anchored document-classification parsing to banner lines, while
  preserving the living docs' blockquote-plus-period banner form. Inline
  prose mentions no longer satisfy the required classification banner.
- Consolidated the doc-status self-test's repeated status-header and
  dashboard Markdown fixtures behind small local helpers, so future
  parser edge cases can reuse the canonical fixture shape instead of
  copy-pasting whole documents.
- Hardened the doc-status self-test fixtures so temporary `ROOT` and
  `DASHBOARD_PATH` overrides are restored immediately after each
  fixture, even when an assertion fails.
- Added `scripts/check-doc-status-headers.py --self-test` so the
  markdown-only latest-round scan and duplicate dashboard-row checks
  are permanent regression coverage, then wired the self-test into CI
  before the live doc-status scan.
- Tightened `scripts/check-doc-status-headers.py` to reject duplicate
  canonical status-header fields instead of letting later field lines
  silently overwrite earlier ones.
- Replaced the doc-status self-test's last Python `assert` statement
  with an explicit `AssertionError` branch so the regression check is
  not disabled under optimized Python execution.
- Tightened `scripts/check-doc-status-headers.py` to reject duplicate
  `## Canonical Status Header` sections instead of parsing the first
  section and silently ignoring a later duplicate.
- Removed the now-unused `parity_matrix_tag()` helper after the
  doc-status validator was generalized to read the full
  parity-matrix summary.
- Tightened the doc-status validator's latest-round scan to consider
  only `docs/operational-runs/*round-*.md` records, so sibling logs or
  helper artifacts cannot accidentally raise the canonical round
  ceiling.
- Added the doc-status validator to the root and docs `AGENTS.md`
  validator inventories so future agents see it as part of the
  workspace verification surface.
- Wired the doc-status validator into `.github/workflows/ci.yml` and
  documented it in `scripts/AGENTS.md`, closing the gap where the
  guard existed locally but was not enforced by CI.
- Corrected the `scripts/AGENTS.md` `check-reference-artifacts.py`
  inventory so its per-network file list matches the validator's
  actual eight-file install-tree contract.
- Removed stale `shutil` commentary and the unused import from
  `scripts/check-reference-artifacts.py`; its docstring now states that
  the policy tag is read at runtime from `docs/parity-matrix.json`.
- Removed stale current-facing BlockFetch default-flip wording from
  the manual runbook and upstream parity notes. The code default is
  already pinned at `2`; §6.5 sign-off now closes the operator gate
  rather than flipping a default that has already graduated.
- Removed the same stale BlockFetch default-flip wording from the
  completion roadmap and widened `scripts/check-stale-placement.py`
  so the unbackticked `from 1 to 2` variant is rejected as well.
- Renamed the sync crate's explicit single-peer concurrency test so it
  no longer describes knob `1` as the shipped default.
- Clarified the runtime governor config constructor comment: its
  isolated-test default remains single-peer, while the binary runtime
  overrides it from `NodeConfigFile`, whose shipped default is pinned
  at `2`.
- Extended `scripts/check-stale-placement.py` and the script/root/docs
  AGENTS guidance so old pre-R258 BlockFetch default-flip wording is
  rejected in living docs.
- Refreshed the canonical status headers in `docs/PARITY_SUMMARY.md`,
  `docs/UPSTREAM_PARITY.md`, and `docs/COMPLETION_ROADMAP.md` to the
  R824 audit ceiling, while explicitly preserving the open
  operator/wire-evidence gates.
- Refreshed the current verification-baseline wording in
  `docs/UPSTREAM_PARITY.md` so it names the R824 parity-flow validators
  instead of the older post-R529 focused cleanup pair.
- Refreshed the current header in `docs/PARITY_PROOF.md` so the
  living-status arc reaches R824 instead of the older R529 cleanup cap.
- Refreshed the living broad verification baseline in `AGENTS.md`,
  `docs/PARITY_SUMMARY.md`, `docs/UPSTREAM_PARITY.md`,
  `docs/COMPLETION_ROADMAP.md`, and `docs/PARITY_PROOF.md` after a
  fresh four-gate Cargo run on 2026-05-26.
- Refreshed the README, docs home page, architecture reference, and
  installation manual so user-facing docs no longer advertised the
  old 2026-05-17 / 6,519-test or 4.7K+ baselines.
- Extended `scripts/check-stale-placement.py` and its self-test to
  catch stale README/docs-site verification-baseline wording such as
  the old rounded/shorthand test-count strings and pre-R824 status
  headings.
- Corrected the README test-count badge from the stale encoded
  `7,295 passing` value to the verified `7,211 passing` baseline, and
  extended the stale-placement self-test so URL-encoded badge counts are
  guarded alongside prose counts.
- Removed the obsolete `cardano-submit-api` structured-rejection decoder
  entry from `docs/TECH-DEBT.md`; the A5 typed decoder arc is now code
  complete in the living roadmap, so that item no longer belongs in the
  current technical-debt ledger.
- Refreshed `crates/tools/cardano-submit-api/AGENTS.md`, the
  cardano-submit-api Rustdoc in `types.rs`, and the
  `sister-tool.cardano-submit-api` parity-matrix entry so they describe
  the R569-R688 typed rejection decoder state instead of the older
  R344 raw-bytes-only surface.
- Extended `scripts/check-stale-placement.py` so the closed
  cardano-submit-api structured-decoder debt wording cannot return in
  current scanned docs or Rustdoc.
- Extended `scripts/check-parity-matrix.py` with the R824-R843
  follow-on band and moved the cardano-submit-api next milestone to
  R825, preserving the validator contract that milestones are concrete
  R-round identifiers.
- Re-anchored the cardano-submit-api operator-evidence wording from the
  superseded R345/R346 mini-arc to the current R825+ follow-on in the
  local AGENTS guidance, roadmap, comparison script, and parity matrix.
- Closed the cardano-submit-api accepted-response `TxId` body gap after
  confirming the upstream Haskell API returns `Handler TxId` /
  `PostAccepted '[JSON] TxId`. `tx_submit_post` now derives the TxId
  from the first raw transaction-body CBOR item and returns it in the
  202 Accepted JSON response.
- Added the shared `yggdrasil_ledger::compute_tx_id_from_tx_cbor`
  helper so both `cardano-cli transaction txid` and cardano-submit-api
  derive `getTxId . getTxBody` from the same raw transaction-body CBOR
  span instead of carrying duplicate local extraction logic.
- Reused that shared helper in the cardano-cli transaction runner,
  preserving the existing CLI txid behavior while moving the raw-CBOR
  body-span extraction to the ledger crate.
- Extended `scripts/check-stale-placement.py` and its self-test so old
  cardano-submit-api R345/R346 current-status wording is rejected in
  scanned current surfaces.
- Extended `scripts/check-stale-placement.py` and its self-test so the
  closed cardano-submit-api accepted-response `"OK"` wording cannot
  return in current-facing surfaces.
- Refreshed the root `Cargo.toml` workspace-member label for
  `cardano-submit-api` so it no longer describes the tool as the old
  R338-R345 implementation arc; the current gate is the R825+ operator
  soak plus the accepted-response `TxId` body gap.
- Refreshed the stale-placement validator descriptions in the root,
  docs, scripts, and script docstring guidance so they name the
  cardano-submit-api structured-decoder/R345-R346 evidence guard that
  now protects current surfaces.
- Refreshed `kes-agent` current-status evidence in the root workspace
  manifest, crate manifest, local AGENTS guidance, Rust deferral status
  descriptor, and parity matrix so the next milestone is the R444+
  daemon/socket follow-on instead of the superseded early mini-arc.
- Refreshed `kes-agent-control` current-status evidence in the root
  workspace manifest, local AGENTS guidance, Rust deferral status
  descriptor, parity matrix, and unreleased changelog so its
  ControlClient socket I/O gate points at the R444+ kes-agent
  daemon/socket follow-on instead of the superseded early mini-arc.
- Extended `scripts/check-stale-placement.py` and its self-test so old
  kes-agent / kes-agent-control early-mini-arc current-status wording
  cannot return in scanned current surfaces.
- Refreshed the remaining root `Cargo.toml` sister-tool member comments
  so they match the current parity-matrix status/milestone view instead
  of older historical arc labels.
- Extended `scripts/check-stale-placement.py` and its self-test so those
  old root-manifest sister-tool status labels cannot return in scanned
  current surfaces.
- Refreshed the root/docs/scripts validator guidance so the documented
  stale-placement coverage names the root-manifest sister-tool label
  guard too.
- Refreshed the `bech32` parity-matrix role after finding that the
  verified entry still described concrete implementation as future
  A.1 work instead of the shipped R331-R334 closeout.
- Extended `scripts/check-stale-placement.py` and its self-test so the
  old bech32 pre-verified current-status wording cannot return in
  scanned current surfaces.
- Refreshed the `dmq-node` current-status surface after finding that
  the code/AGENTS evidence had advanced through the R717-R816 pure-logic
  and mux-bundle arcs while `status.rs`, `lib.rs`, the parity matrix,
  and tail AGENTS guidance still described the old pre-R816 boundary.
- Extended `scripts/check-stale-placement.py` and its self-test so the
  old dmq-node pre-R816 current-status strings cannot return in scanned
  current surfaces.
- Refreshed the `cardano-testnet` current-status surface after finding
  that code and operational evidence had advanced through the R772-R823
  type/parser-composition arcs while `status.rs`, `lib.rs`,
  `parser.rs`, root `Cargo.toml`, AGENTS guidance, and the parity
  matrix still described older R359/R367/R445/R534 boundaries.
- Extended `scripts/check-stale-placement.py` and its self-test so the
  old cardano-testnet pre-R823 current-status strings and root manifest
  label cannot return in scanned current surfaces.
- Repaired the Windows `cargo test-all` failure in
  `yggdrasil-cardano-testnet` by keeping the `filepath.rs`
  string-returning `FilePath` helpers on explicit `/` joins. The
  helpers are upstream-shaped strings, not platform-native `PathBuf`
  return values.
- Added a `Sprocket::system_name` regression for a `/` base path so the
  slash-join helper keeps Unix-root socket paths absolute instead of
  trimming them to relative paths.
- Corrected the living 2026-05-26 test-count baseline after the exact
  `cargo test-all -- --list --format terse` alias reported 7,214
  tests in the current checkout, not the earlier higher count.

## Audit findings

- The four required Cargo gates are green after cleanup.
- Parity-flow guardrails are green: parity matrix, strict mirror,
  stale placement, fixture manifest, filetree, and doc-status headers.
- No production tx-generator behavior changed in this round; the Rust
  change is limited to `#[cfg(test)]` path isolation and to routing the
  budget-summary writer through that helper during unit tests.
- No production NtC server behavior changed in this round; the LSQ
  env-var cleanup is limited to the `src/tests.rs` harness.
- `cargo tree -d` and `cargo deny` both still report duplicate
  transitive dependency stacks. They remain warnings under
  `deny.toml` (`multiple-versions = "warn"`), with current sources in
  crypto/hash, telemetry/tonic, HTTP/TLS, Windows-target support, and
  dev/test dependency chains. No dependency graph changes were made in
  this round.
- The license allowlist no longer carries unused `MPL-2.0` or
  `Unicode-DFS-2016` entries. Future dependencies using either license
  must reintroduce the allowance with a current justification.
- The status-header guard now catches the stale-header class where all
  central docs agree with each other but lag behind the operational-run
  history.
- The status-header guard now catches parity-tag drift between the
  central docs and `docs/parity-matrix.json`.
- The status-header guard now catches stale dashboard dates and
  dashboard status-count drift against `docs/parity-matrix.json`.
- The status-header guard now catches duplicate dashboard summary rows
  before comparing the dashboard against `docs/parity-matrix.json`.
- The status-header guard now ignores unrelated dashboard tables when
  deriving the compact status-count summary, while preserving duplicate
  row checks inside the metric summary table.
- The status-header guard now rejects duplicate compact dashboard
  metric summary tables before comparing the parsed counts to
  `docs/parity-matrix.json`.
- The status-header guard now derives the dashboard date only from
  full `_Last updated: YYYY-MM-DD._` lines, not inline prose examples.
- The status-header guard now requires a real document-classification
  banner line and rejects duplicate classification banners, while
  accepting the current blockquoted banner spelling.
- The status-header guard's self-test fixtures now share canonical
  status-header and dashboard table builders, reducing duplication in
  the CI gate without changing parser behavior.
- The status-header guard's self-test fixtures now restore temporary
  global path overrides after each case, preventing one failed fixture
  from cascading into unrelated dashboard or round-scan failures.
- The status-header guard now carries a stdlib-only self-test mode for
  the markdown-only operational-round scan and duplicate dashboard-row
  cases.
- The status-header guard now catches duplicate fields inside the
  canonical status-header block before comparing docs against each
  other.
- The status-header guard now catches duplicate canonical status-header
  sections before parsing individual fields.
- The status-header guard now anchors canonical status-header section
  detection to actual heading lines, so prose mentions of the marker
  text are allowed while duplicate heading sections are still rejected.
- The stale-placement guard now catches pre-R258 BlockFetch
  default-flip wording in living docs.
- The stale-placement guard now also catches the unbackticked
  "from 1 to 2" spelling of stale BlockFetch default-flip wording.
- The central verification baseline now reflects the current R824 audit
  run: 7,211 passing tests, 0 failing, 3 ignored, with 7,214 tests
  listed by Cargo.
- `cargo test-all` exposed a Windows-only separator bug in
  `yggdrasil-cardano-testnet::filepath`; the local `filepath` module
  and full crate test reruns are green after preserving explicit `/`
  joins for upstream-shaped `FilePath` strings.
- The `Sprocket::system_name` slash join now preserves a Unix root base
  (`/`) instead of producing a relative socket path.
- The stale-placement guard now catches the superseded higher
  test-count baseline so living docs stay aligned with the exact
  `cargo test-all` alias inventory.
- Living user-facing docs now carry the same broad verification
  baseline as the central parity docs, and the stale-placement guard
  protects the old README/docs-site baseline strings from returning.
- The README badge now carries the same 7,211 passing-test baseline as
  the README body and central parity docs; the stale-placement guard
  catches the prior URL-encoded 7,295 badge form.
- The current tech-debt ledger no longer advertises the closed
  cardano-submit-api A5 structured-decoder work as deferred. Local
  cardano-submit-api guidance and the parity matrix now point to the
  remaining operator soak / verified-promotion evidence instead.
- The cardano-submit-api parity matrix no longer carries the accepted
  response-body gap: accepted submissions now return the submitted
  transaction TxId. Verified promotion remains blocked on the R825+
  comparison soak against a live upstream submit-api binary.
- The ledger crate now owns complete-transaction-CBOR TxId extraction,
  keeping cardano-cli and cardano-submit-api aligned on the same
  raw-body-byte hashing rule.
- The root workspace manifest no longer advertises cardano-submit-api
  as the old R338-R345 implementation arc; its member comment now
  agrees with the parity matrix's R825+ operator/TxId evidence gate.
- The stale-placement guard's operational descriptions now match its
  live cardano-submit-api coverage, so future audit passes see the
  structured-decoder/R345-R346 stale-evidence protection in the local
  instructions, not only in the Python regex table.
- The kes-agent deferral surface now agrees with the parity matrix:
  R443 exposed a structured deferral and the next concrete gate is the
  R444+ daemon/socket follow-on with byte-equivalent socket-protocol
  fixture capture.
- The kes-agent-control deferral surface now agrees with that gate:
  R440 exposed typed ControlClient socket-I/O deferral, but concrete
  control-client behavior remains blocked until the R444+ daemon/socket
  follow-on provides the byte-equivalent server-side protocol.
- The root workspace manifest now uses the parity matrix as its
  source-of-truth summary for sister-tool status labels: verified tools,
  implemented-needing-11.0.1-evidence tools, and partial tools are no
  longer mixed with obsolete historical-arc labels.
- The bech32 parity-matrix entry now describes the completed R331-R334
  implementation and verified closeout instead of future A.1 work.
- The stale-placement guard now protects that root-manifest summary from
  drifting back to old bech32, kes-agent, tracer, db-tool, testnet,
  tx-generator, or DMQ labels.
- The dmq-node parity surface now agrees with the shipped code: the
  protocol, inbound-V2, NodeKernel helper, peer-sharing, KeepAlive,
  DeltaQ, and NtN/NtC mux-bundle components are complete through R816;
  the remaining open gate is the final `run()` event-loop assembly plus
  operator comparison evidence.
- The cardano-testnet parity surface now agrees with the shipped code:
  era-free option/runtime/path/default/component records and
  Parsers/Cardano option composition are complete through R823; the
  remaining open gate is threading typed parser records through
  `Command` plus the runtime / era-genesis / Process harness.
- The upstream-parity current-baseline section now names the R824
  parity-flow validators, and the stale-placement guard rejects the old
  post-R529 focused-cleanup label in living docs.
- The parity-proof header now names R824 as the living-status arc, and
  the stale-placement guard rejects the older R529 capped proof wording.
- Known completeness gaps remain and are intentionally not closed by this
  cleanup round: the operator-side mainnet endurance rehearsal, the
  parallel-fetch sign-off, the BO/BP/R178-followup wire-comparison gaps,
  and the sister-tool parity debt already tracked in living docs.

## Validation

- `cargo fmt --all -- --check` - green on 2026-05-26 after the
  final doc-status and stale-placement hardening.
- `cargo check-all` - green on 2026-05-26 after the final doc-status
  and stale-placement hardening.
- `cargo lint` - green on 2026-05-26 after the final doc-status and
  stale-placement hardening.
- `cargo lint-no-default` - green on 2026-05-26 after the final
  doc-status and stale-placement hardening.
- `cargo test-all` - green on 2026-05-26 after the final doc-status
  and stale-placement hardening; command exited 0, with
  7,211 passing tests, 0 failing, and the expected three ignored
  node-tracer doctests.
- `cargo test --workspace --all-features -- --list --format terse` -
  counted 7,214 tests and 0 benches after the final doc-status and
  stale-placement hardening.
- `cargo test -p yggdrasil-tx-generator` - green; 249 lib tests,
  5 CLI golden tests, and doctests passed under normal parallel execution.
- `cargo clippy -p yggdrasil-tx-generator --all-targets -- -D warnings` -
  green.
- `cargo test -p yggdrasil-node-ntc-server` - green; 69 unit tests and
  doctests passed.
- `cargo clippy -p yggdrasil-node-ntc-server --all-targets -- -D warnings` -
  green.
- `.codex-tools/cargo-deny/bin/cargo-deny check advisories bans licenses sources` -
  green; duplicate-version findings remain warning-level under
  `deny.toml`.
- `.codex-tools/cargo-deny/bin/cargo-deny check licenses` - green
  with no unused-license allowance warnings after tightening
  `deny.toml`.
- `git diff --check` - green.
- `python3 -m py_compile .claude/scripts/filetree.py scripts/check-parity-matrix.py scripts/check-doc-status-headers.py` - green.
- `python3 scripts/check-doc-status-headers.py` - green after the
  R824 header refresh, stale-ceiling guard tightening, and parity-matrix
  tag cross-check.
- `python3 scripts/check-doc-status-headers.py --self-test` - green
  after adding permanent regression coverage for doc-status edge cases,
  including prose mentions of the canonical status-header heading text
  and unrelated dashboard tables after the metric summary, while
  rejecting duplicate compact dashboard metric summary tables and
  duplicate dashboard status-date lines, inline classification mentions,
  duplicate classification banners, duplicate status-count rows, and
  self-test fixture path leaks.
- `python3 -O scripts/check-doc-status-headers.py --self-test` - green;
  the self-test remains active under optimized Python execution.
- `python3 scripts/check-parity-matrix.py` - green; 22 entries validated
  against reference tag 11.0.1.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` - green;
  0 violations.
- `python3 scripts/check-stale-placement.py --self-test` - green,
  including the stale parity-proof R529 cap guard.
- `python3 scripts/check-stale-placement.py` - green.
- `python scripts/check-stale-placement.py --self-test` - green after
  the README/docs-site stale-baseline guard extension and the
  unbackticked BlockFetch default-flip wording plus post-R529
  current-baseline wording guards in this Windows shell.
- `python scripts/check-stale-placement.py` - green after the
  README/docs-site baseline refresh in this Windows shell.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the URL-encoded stale README badge fixture, then green after
  widening the stale-baseline pattern.
- `python scripts/check-stale-placement.py` - green after correcting
  the README badge to `7,211 passing`.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the closed cardano-submit-api structured-decoder debt fixture,
  then green after adding the stale pattern.
- `python scripts/check-stale-placement.py` - red while the obsolete
  cardano-submit-api tech-debt entry and Rustdoc comments remained, then
  green after removing/updating those current surfaces.
- `python scripts/check-parity-matrix.py` - green after refreshing the
  cardano-submit-api entry and allowing the R824-R843 follow-on band.
- `cargo fmt --all -- --check` - green after the cardano-submit-api
  documentation and parity-matrix cleanup.
- `cargo test -p yggdrasil-cardano-submit-api` - green after the
  cardano-submit-api documentation cleanup; 355 lib tests, 4 CLI
  golden tests, and 1 doctest passed.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding stale R345/R346 cardano-submit-api evidence fixtures, then
  green after adding the stale pattern.
- `python scripts/check-stale-placement.py` - red while current
  cardano-submit-api AGENTS/script/roadmap surfaces still pointed at
  R345/R346, then green after the R825+ refresh.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the root `Cargo.toml` stale R338-R345 cardano-submit-api member
  label fixture, then green after widening the R345/R346 stale-evidence
  pattern.
- `python scripts/check-stale-placement.py` - green after the root
  `Cargo.toml` workspace-member comment was refreshed away from the old
  R338-R345 implementation-arc label.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the stale kes-agent early-mini-arc fixture, then green after
  adding the stale pattern.
- `cargo test -p yggdrasil-kes-agent daemon_status_describes_deferral --lib`
  - red after requiring the R444+ follow-on in `daemon_status()`, then
  green after refreshing the deferral descriptor.
- `python scripts/check-stale-placement.py` - red while current
  kes-agent and kes-agent-control AGENTS/status/changelog surfaces still
  pointed at the superseded early mini-arc, then green after the R444+
  daemon/socket refresh.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding stale kes-agent-control pre-R444 fixture strings, then green
  after adding the stale pattern.
- `cargo test -p yggdrasil-kes-agent-control control_client_status_describes_deferral --lib`
  - red after requiring the R444+ daemon follow-on in
  `control_client_status()`, then green after refreshing the descriptor.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding root-manifest sister-tool status-label fixtures for the old
  parity-matrix drift, then green after adding the stale pattern.
- `python scripts/check-stale-placement.py` - green after the root
  `Cargo.toml` sister-tool member comments were refreshed to the current
  parity-matrix status/milestone view.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the stale bech32 pre-verified role fixture, then green after
  adding the stale pattern.
- `python scripts/check-stale-placement.py` - green after the bech32
  parity-matrix role was refreshed to the verified closeout state.
- `cargo test -p yggdrasil-dmq-node diffusion_wiring_status_describes_deferral --lib`
  - red after requiring the status descriptor to name R717-R816 and the
  R817+ event-loop gate, then green after refreshing the descriptor.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding stale dmq-node pre-R816 current-status fixtures, then green
  after adding the stale pattern.
- `python scripts/check-stale-placement.py` - green after the dmq-node
  status descriptors, AGENTS guidance, and parity-matrix entry were
  refreshed to the post-R816 state.
- `python scripts/check-parity-matrix.py` - green after advancing the
  dmq-node entry to the R817+ run-loop event-loop gate.
- `cargo test -p yggdrasil-cardano-testnet era_dispatch_status_describes_deferral --lib`
  - red after requiring the status descriptor to name R772-R823 and the
  R824+ `Command` payload/runtime gate, then green after refreshing the
  descriptor.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding stale cardano-testnet pre-R823 current-status fixtures and the
  root manifest label fixture, then green after adding the stale
  pattern.
- `python scripts/check-stale-placement.py` - green after the
  cardano-testnet status descriptors, parser/lib docs, AGENTS guidance,
  root manifest comment, parity-matrix entry, and filetree descriptions
  were refreshed to the post-R823 state.
- `python scripts/check-parity-matrix.py` - green after advancing the
  cardano-testnet entry to the post-R823 parser/runtime gate.
- `python scripts/check-parity-matrix.py` - green after closing the
  accepted-response TxId gap in the cardano-submit-api entry.
- `cargo test -p yggdrasil-ledger compute_tx_id_from_tx_cbor --lib` -
  red before adding the shared helper, then green with 2 focused tests.
- `cargo test -p yggdrasil-cardano-submit-api tx_submit_post_success_returns_tx_id_json_and_traces_medium_id --lib` -
  red before adding the injectable success-path handler, then green
  after `tx_submit_post` returned the computed TxId JSON body.
- `cargo test -p yggdrasil-cardano-cli txid --lib` - green after the
  transaction runner delegated to the shared ledger helper; 4 focused
  tests passed.
- `cargo test -p yggdrasil-cardano-submit-api` - green after the
  accepted-response TxId repair; 356 lib tests, 4 CLI golden tests, and
  1 doctest passed.
- `cargo clippy -p yggdrasil-ledger -p yggdrasil-cardano-cli -p yggdrasil-cardano-submit-api --all-targets -- -D warnings` -
  green after the shared helper and submit-api response changes.
- `python scripts/check-stale-placement.py --self-test` - red after
  adding the stale accepted-response `"OK"` fixture, then green after
  adding the stale pattern.
- `python scripts/check-stale-placement.py` - green after current
  cardano-submit-api docs, roadmap, parity matrix, and operational
  evidence stopped advertising the closed `"OK"` response gap.
- `python scripts/check-doc-status-headers.py` - green after the B2
  roadmap refresh.
- `python3 scripts/check-fixture-manifest.py` - green.
- `python3 scripts/check-reference-artifacts.py` - green under WSL;
  cardano-node 11.0.1 install, nine binaries, and three network share
  dirs validated. Native Windows correctly refuses this gate because the
  reference install contains Linux executables.
- `python3 .claude/scripts/filetree.py check` - green.
- `cargo test -p yggdrasil-cardano-testnet filepath::tests:: --lib` -
  green after the explicit slash-join repair; 7 focused tests passed.
- `cargo test -p yggdrasil-cardano-testnet` - green after the explicit
  slash-join repair; 94 lib tests, 2 CLI golden tests, and doctests
  passed.
- Final closeout pass on 2026-05-26 after the bech32, dmq-node,
  cardano-testnet, and filetree metadata refreshes:
  `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and
  `cargo test-all` all exited 0. The broad test gate retained
  0 failures and the expected three ignored node-tracer doctests.
- Post-TxId closeout pass on 2026-05-26 after the shared ledger helper,
  cardano-cli reuse, and cardano-submit-api accepted-response repair:
  `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and
  `cargo test-all` all exited 0. The broad test gate retained
  0 failures and the expected three ignored node-tracer doctests.
- Final non-Cargo closeout pass on 2026-05-26 exited 0 for
  `python scripts/check-parity-matrix.py`,
  `python scripts/check-strict-mirror.py --fail-on-violation`,
  `python scripts/check-stale-placement.py --self-test`,
  `python scripts/check-stale-placement.py`,
  `python scripts/check-fixture-manifest.py`,
  `python scripts/check-doc-status-headers.py --self-test`,
  `python scripts/check-doc-status-headers.py`,
  `python -m py_compile scripts/check-stale-placement.py scripts/check-parity-matrix.py scripts/check-doc-status-headers.py .claude/scripts/filetree.py`,
  `python .claude/scripts/filetree.py check`,
  `wsl.exe -e bash -lc "cd /mnt/v/workspace/Cardano-node && python3 scripts/check-reference-artifacts.py"`,
  and `git diff --check`.

## Remaining risk

This round improves the audit machinery and restores a clean verification
baseline; it does not claim project completion. Full quality and
completeness closure still depends on the tracked parity/operator gates
outside the Cargo gates required by the root workspace instructions.
