# Guidance for maintaining project policy and architecture documents under docs/.

Keep these documents synchronized with the implemented workspace, not with speculative future goals.

## Validators that protect this tree

`docs/` carries policy + parity + operator-facing markdown, not Rust
code, so the workspace strict-mirror file-policy (R274+) does not
apply directly here. Five validators guard this tree's invariants:

- `python3 scripts/check-parity-matrix.py` (CI gate since R303) —
  validates `parity-matrix.json` schema + every
  `haskell_reference.path` and `rust_surface.path` exists on disk.
  This directory hosts the source-of-truth `parity-matrix.json`.
- `python3 scripts/check-stale-placement.py` — rejects current-facing
  stale post-reorganization paths and status baselines in living docs,
  including obsolete parity-summary/proof/upstream verification claims,
  stale README/docs-site test baselines, old BlockFetch default-flip
  wording from before the R258 default graduation, and stale
  cardano-submit-api structured-decoder/R345-R346 evidence wording plus
  kes-agent/kes-agent-control early-mini-arc current-status wording and
  root-manifest sister-tool labels, plus stale dmq-node pre-R816
  current-status wording and stale cardano-testnet pre-R823,
  Command-payload, and process-handle type gap wording.
- `python3 scripts/check-doc-status-headers.py` (CI gate since R824) — keeps central
  parity-doc status headers aligned with each other, the latest
  operational-run markdown round, `docs/parity-matrix.json::reference.tag`, and
  the compact `PARITY_DASHBOARD.md` status-count summary. When editing the
  guard itself, run `python3 scripts/check-doc-status-headers.py --self-test`
  before the live scan.
- `python3 scripts/check-fixture-manifest.py` (CI gate since R303) —
  cross-checks the `cardano-base` SHA pin across
  `crates/node/config/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT`,
  `specs/upstream-test-vectors/cardano-base/<SHA>/`, `SPECS.md`,
  and `UPSTREAM_PARITY.md` (this directory's own pin matrix).
- `python3 scripts/check-strict-mirror.py --fail-on-violation`
  (R288) — uses [`strict-mirror-audit.tsv`](strict-mirror-audit.tsv)
  as its allowlist; this directory is the source of truth for the
  audit table.

When adding a new top-level `.md` to `docs/`, decide its role first
(see [`reference.md`](reference.md) for the "Architecture & parity"
+ "Specs & dependencies" + "Validation & release" + "Archived
planning docs" sections). Closed/historical docs go in
[`archive/`](archive/) with explicit Jekyll permalinks; per-round
records go in [`operational-runs/`](operational-runs/) and are
immutable once committed.

## Scope
- `ARCHITECTURE.md`, `DEPENDENCIES.md`, `SPECS.md`, `CONTRIBUTING.md`, and the
  per-cycle audit/parity docs (`archive/PARITY_PLAN.md`, `PARITY_SUMMARY.md`,
  `PARITY_PROOF.md`, `UPSTREAM_PARITY.md`, `COMPLETION_ROADMAP.md`,
  `TECH-DEBT.md`, `AUDIT_VERIFICATION_*.md`, `MANUAL_TEST_RUNBOOK.md`,
  `archive/UPSTREAM_RESEARCH.md`, `REAL_PREPROD_POOL_VERIFICATION.md`).
- `parity-matrix.json` — machine-readable Rust ↔ Haskell parity inventory
  (validated by `scripts/check-parity-matrix.py`). The `reference.tag`
  tracks the latest IntersectMBO/cardano-node release; bump it whenever
  upstream ships a new tag and re-validate every `haskell_reference.path`
  (paths can move across releases). See `intersectmbo_version_policy.md`
  in agent memory for the full bump checklist.
- The user-facing manual under `docs/manual/` rendered as the operator
  documentation site (Jekyll + just-the-docs remote theme; built by
  `.github/workflows/pages.yml`).
- Site infrastructure: `docs/_config.yml`, `docs/Gemfile`,
  `docs/index.md`, `docs/reference.md`, `docs/manual/index.md`.
- Site theming under `docs/_sass/`:
  - `_sass/color_schemes/yggdrasil.scss` — custom Sass-variable colour
    scheme (forest-teal primary, copper accent). Selected via
    `color_scheme: yggdrasil` in `_config.yml`.
  - `_sass/custom/custom.scss` — auto-imported by just-the-docs.
    Provides hero blocks (`.yg-hero`), the hero-banner figure
    (`.yg-hero-banner` — embedded inline in `docs/index.md`, **not**
    via `header_custom.html`, because the theme injects custom-header
    HTML into the fixed-height `.main-header` strip and clipped the
    banner image), the stats banner (`.yg-stats`), the navigation card
    grid (`.yg-cards` + `a.yg-card`), refined typography, polished
    tables/code/blockquotes, and `.callout-*` sidebar styling.
  - When updating site styling, change Sass variables in the colour
    scheme for global tone and CSS variables (`--yg-*` defined in
    `:root`) inside `custom.scss` for component-level rules.
- Project-wide workflow, dependency policy, specification provenance, and architecture updates.
- Documentation changes that reflect implemented behavior or accepted policy.

## User Manual contract
- `docs/manual/` chapters are the operator-facing reference. They MUST
  stay accurate against the live binary CLI surface, config schema,
  and metric names — when those change in code, update the
  corresponding chapter in the same PR.
- Chapter Jekyll front matter (`title`, `parent: User Manual`,
  `nav_order`) controls site navigation; do not reorder casually.
- Reference docs (Architecture, Parity Plan, etc.) carry
  `parent: Reference` front matter so they appear under a separate
  navigation parent from the user manual.

## Manual chapter inventory
- `overview.md` — conceptual frame.
- `installation.md` — build from source.
- `releases.md` — install from pre-built release artifacts.
- `quick-start.md` — five-command mainnet sync.
- `networks.md` — mainnet / preprod / preview presets.
- `configuration.md` — full config-key + CLI-flag reference.
- `running.md` — systemd unit, graceful shutdown, log rotation.
- `docker.md` — `docker compose` deployment.
- `monitoring.md` — Prometheus metrics, tracing, health.
- `block-production.md` — SPO setup, KES rotation procedure.
- `cli-reference.md` — every subcommand and flag.
- `maintenance.md` — backups, GC, upgrades.
- `troubleshooting.md` — symptom-keyed error catalogue.
- `glossary.md` — Cardano terminology.

## Release-aligned docs
- `docs/CHANGELOG.md` mirrors the root `CHANGELOG.md` for the docs
  site reference section; both must be updated together when shipping
  a release.
- The `release.yml` workflow's release-notes generator pulls commit
  log between consecutive tags. Commits that should land in release
  notes MUST use clear conventional-commit prefixes
  (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Operational run records
- `docs/operational-runs/*.md` files are dated evidence snapshots.
  Do not rewrite historical "open follow-up" wording in old run
  records just because later rounds closed the follow-up. Add a new
  operational run record for new evidence, then update living status in
  `README.md`, `archive/PARITY_PLAN.md`, `PARITY_SUMMARY.md`,
  `PARITY_PROOF.md`, `UPSTREAM_PARITY.md`, and
  `MANUAL_TEST_RUNBOOK.md`.
- Filename convention is `YYYY-MM-DD-round-NNN-<slug>.md`; the
  `/round-doc` slash command (defined in
  `.claude/commands/round-doc.md`) authors the skeleton.
- If a run record itself has a typo or incorrect fact about that same
  run, correct it narrowly and leave the rest of the record intact.

## Parity matrix maintenance
- `parity-matrix.json` is operational evidence, not a rolling work log.
  Update an entry when its `status`, `implemented_evidence`,
  `remaining_work`, or `acceptance` set genuinely changes — not on
  every code edit.
- Allowed `status` values: `verified_<TAG>`,
  `implemented_needs_<TAG>_evidence`, `partial`, `absent`, where
  `<TAG>` is the underscore-encoded latest IntersectMBO release
  (currently `11_0_1`).
- Every `haskell_reference[*].path` MUST exist under
  `.reference-haskell-cardano-node/...` at validation time; every
  `rust_surface[*].path` MUST exist in the workspace. The
  `scripts/check-parity-matrix.py` gate enforces both.
- Status transitions tied to operator-time gates (e.g. R267 mainnet
  endurance) only flip to `verified_<TAG>` when the gate has been
  signed off, not when the implementation is "ready".

##  Rules *Non-Negotiable*
- Documentation in this directory MUST describe current behavior or explicitly labeled near-term policy, not aspirational features.
- Dependency decisions MUST be recorded in `DEPENDENCIES.md` before a new crate is treated as accepted.
- Architecture and workflow changes MUST stay consistent with the actual crate graph and verification commands used in the workspace.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Node integration and operational behavior: <../.reference-haskell-cardano-node/cardano-node/>
- Node system/E2E parity harness: <https://github.com/IntersectMBO/cardano-node-tests/> and <https://tests.cardano.intersectmbo.org/>
- Ledger implementation, per-era rules and CDDL: <../.reference-haskell-cardano-node/deps/cardano-ledger/>
- Formal ledger specifications (Agda, Conway-complete): <https://github.com/IntersectMBO/formal-ledger-specifications>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site>
- Consensus implementation, tech report, and architecture docs: <../.reference-haskell-cardano-node/deps/ouroboros-consensus/>
- LedgerDB/openDB restore-replay semantics: <https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/ouroboros-consensus/Ouroboros-Consensus-Storage-LedgerDB.html>
- Caught-up node storage model: <https://ouroboros-consensus.cardano.intersectmbo.org/docs/explanations/node_tasks/>
- UTxO-HD rollback/snapshot design: <https://ouroboros-consensus.cardano.intersectmbo.org/docs/references/miscellaneous/utxo-hd/utxo-hd_in_depth/>
- Networking implementation and protocol specification: <../.reference-haskell-cardano-node/deps/ouroboros-network/>
- Cryptographic primitives (hashing, VRF, KES, BLS): <../.reference-haskell-cardano-node/deps/cardano-base/>
- Plutus core and CEK machine: <../.reference-haskell-cardano-node/deps/plutus/>
- Haddock docs: ledger (<https://cardano-ledger.cardano.intersectmbo.org/>), consensus (<https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/>), network (<https://ouroboros-network.cardano.intersectmbo.org/>), base (<https://base.cardano.intersectmbo.org/>)

## Maintenance Guidance
- Update these docs in the same change when a subsystem milestone materially changes.
- Keep references to upstream IntersectMBO and Cardano sources current and traceable.
- Prefer concise policy and architecture guidance over long narrative explanation.
