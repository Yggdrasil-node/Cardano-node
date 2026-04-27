# Guidance for maintaining project policy and architecture documents under docs/.

Keep these documents synchronized with the implemented workspace, not with speculative future goals.

## Scope
- `ARCHITECTURE.md`, `DEPENDENCIES.md`, `SPECS.md`, `CONTRIBUTING.md`, and the
  per-cycle audit/parity docs (`PARITY_PLAN.md`, `PARITY_SUMMARY.md`,
  `AUDIT_VERIFICATION_*.md`, `MANUAL_TEST_RUNBOOK.md`,
  `UPSTREAM_PARITY.md`, `UPSTREAM_RESEARCH.md`,
  `REAL_PREPROD_POOL_VERIFICATION.md`).
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
    Provides hero blocks (`.yg-hero`), the stats banner (`.yg-stats`),
    the navigation card grid (`.yg-cards` + `a.yg-card`), refined
    typography, polished tables/code/blockquotes, and `.callout-*`
    sidebar styling.
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

##  Rules *Non-Negotiable*
- Documentation in this directory MUST describe current behavior or explicitly labeled near-term policy, not aspirational features.
- Dependency decisions MUST be recorded in `DEPENDENCIES.md` before a new crate is treated as accepted.
- Architecture and workflow changes MUST stay consistent with the actual crate graph and verification commands used in the workspace.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Node integration and operational behavior: <https://github.com/IntersectMBO/cardano-node/>
- Ledger implementation, per-era rules and CDDL: <https://github.com/IntersectMBO/cardano-ledger/>
- Formal ledger specifications (Agda, Conway-complete): <https://github.com/IntersectMBO/formal-ledger-specifications>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site>
- Consensus implementation, tech report, and architecture docs: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Networking implementation and protocol specification: <https://github.com/IntersectMBO/ouroboros-network/>
- Cryptographic primitives (hashing, VRF, KES, BLS): <https://github.com/IntersectMBO/cardano-base/>
- Plutus core and CEK machine: <https://github.com/IntersectMBO/plutus/>
- Haddock docs: ledger (<https://cardano-ledger.cardano.intersectmbo.org/>), consensus (<https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/>), network (<https://ouroboros-network.cardano.intersectmbo.org/>), base (<https://base.cardano.intersectmbo.org/>)

## Maintenance Guidance
- Update these docs in the same change when a subsystem milestone materially changes.
- Keep references to upstream IntersectMBO and Cardano sources current and traceable.
- Prefer concise policy and architecture guidance over long narrative explanation.