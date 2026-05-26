# Guidance for the workspace-level scripts under `scripts/`.

This directory hosts vendored-tree refresh tooling, CI parity validators, and
operator/runbook shell helpers. The split matches the upstream Haskell
repository's root `scripts/` placement while keeping validator scripts and
operator scripts documented separately.

## Directory shape

```
scripts/
├── setup-reference.sh             # one-shot vendored-tree fetch (refresh helper)
├── audit-strict-mirror.py         # discovery script, populates docs/strict-mirror-audit.tsv
├── check-strict-mirror.py         # CI gate (R288): file-mirror drift detector
├── check-stale-placement.py       # CI gate: post-reorganization path/status guard
├── check-doc-status-headers.py    # CI gate (R824): parity-doc status/header guard
├── check-parity-matrix.py         # CI gate: parity-matrix.json schema + paths
├── check-fixture-manifest.py      # CI gate (R303): cardano-base SHA pin consistency
├── check-reference-artifacts.py   # Linux/WSL local-only: validates .reference-haskell-cardano-node/install/
├── check_upstream_drift.sh        # operator pin-drift report
├── run-tools.sh                   # sister-tool launcher
└── *producer* / *soak* helpers    # operator runbook harnesses
```

## Validators

### `check-strict-mirror.py` (R275 warn-only → R288 fail-build, R311 drift-aware)

Walks every production `.rs` under `crates/<crate>/src/` (including
the `crates/node/cardano-node/src/` binary-crate tree after Wave 4
PR 6 relocation), excluding `**/tests/**` + `target/`, and verifies
each file either:

1. Mirrors a single canonical upstream `.hs` file by snake_case
   basename (with directory-prefix fallback for sibling collisions), OR
2. Carries a `## Naming parity` docstring stanza ending in
   `**Strict mirror:** none.` plus the upstream symbol(s)/file(s) the
   helper surfaces.

Also cross-checks the working tree against the git index (R311+):
any production `.rs` that exists locally but is NOT tracked in
`git ls-files` is flagged as an index-vs-tree drift violation.
Catches the R310 failure mode where an over-broad `.gitignore`
pattern silently swallowed an entire strict-mirror subtree (the
local tree built clean but a fresh CI clone failed module
resolution).

Allowlist source-of-truth: [`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv).
The `--fail-on-violation` flag flips exit code on violation (CI mode).
Imports the audit module via importlib so the verdict heuristics stay
in one place. The drift check degrades gracefully if `git` is
unavailable (returns `None` from `get_tracked_rust_files()`).

Runs on every push via `.github/workflows/ci.yml`. Failure modes:

- **Mirror/docstring violation**: a new production `.rs` was added
  without either an upstream filename mirror or the explicit
  `## Naming parity` block. Author the docstring (see
  `.claude/skills/round-extraction/SKILL.md` for the pattern) or
  rename the file to mirror an upstream `.hs`.
- **Index-vs-tree drift**: a production `.rs` exists locally but is
  not tracked. Check `.gitignore` for over-broad bare patterns
  (e.g., `debug` instead of `/target/debug/`) and `git add` the file
  once the ignore rule is fixed.

### `check-stale-placement.py` (CI gate)

Validates current, non-historical surfaces after the node-crate
reorganization. The guard fails if live code, CI, generated navigation,
commands, resolved Cargo metadata, or living docs point at stale placements:
the legacy node-local binary crate, old root-node or yggdrasil-node shorthand
metadata/tests paths, node-local configuration/scripts directories,
nested configuration/scripts under any `crates/node/*` package, or the old
Claude skill path. It also fails on exact stale filesystem directories even if
they are empty and untracked, including nested
`crates/node/*/{configuration,scripts}/` and old top-level sister-tool crate
directories.
The same guard rejects stale current-status claims that proved easy to
reintroduce during the cleanup: node-local parameterized LocalStateQuery
wording, old cardano-cli subcommand counts, three-command subset wording,
and active-migration wording for the closed C-arc; it also rejects
obsolete parity-summary/proof/upstream verification baselines,
old README/docs-site test baselines, stale BlockFetch default-flip wording
from before the R258 default graduation, tx-generator/cardano-testnet still
being described as blocked by the closed cardano-cli C-arc, stale
cardano-submit-api structured-decoder/R345-R346 evidence wording, stale
kes-agent/kes-agent-control early-mini-arc current-status wording, stale
root-manifest sister-tool labels, stale dmq-node pre-R816 current-status
wording, stale cardano-testnet pre-R823, Command-payload, and
process-handle type gap wording, and the closed workspace-member gap.
It also fails if the vendored Haskell reference snapshot contains nested
`.git` metadata, is not ignored by Git, or would otherwise stop being a
metadata-free corpus. A `.gitmodules` entry or Git-index submodule entry for
`.reference-haskell-cardano-node/` is rejected for the same reason. Any regular
Git-index entry under that reference tree is also rejected; the reference corpus
must remain local-only.
The guard also asserts the accepted replacement placements still exist:
`crates/node/cardano-node/{Cargo.toml,src/main.rs}`,
the canonical `configuration/{mainnet,preprod,preview}/` operator bundles
(`config.json`, `config-legacy.json`, `topology.json`,
`{byron,shelley,alonzo,conway}-genesis.json`, `peer-snapshot.json`,
`submit-api-config.json`, `tracer-config.json`, plus `checkpoints.json` where
the preset carries it), `configuration/poolMetaData.json`, `scripts/run-tools.sh`,
root reference/operator helpers under `scripts/` (`setup-reference.sh`,
`install_haskell_cardano_node.sh`, `compare_tip_to_haskell.sh`,
`parallel_blockfetch_soak.sh`, `preview_producer_harness.sh`, and
`yggdrasil-node.service`), and
`.claude/skills/cardano-haskell-node/SKILL.md`.
Every tracked root `scripts/*.sh` file must also keep Git executable mode
`100755`; the systemd unit template remains non-executable.
Release/repro packaging surfaces are pinned too: `.github/workflows/release.yml`
and `.github/workflows/repro-check.yml` must stage root `configuration/` and
root `scripts/`, and the Dockerfile must copy root `configuration/` plus its
root helper scripts from `scripts/`. Docker must also pin
`YGGDRASIL_CONFIG_ROOT=/usr/share/yggdrasil/configuration` to the copied
bundle. The release installer must require those bundles in the extracted
archive and install them under `<prefix>/share/yggdrasil/`. The packaged
`scripts/yggdrasil-node.service` unit must set
`YGGDRASIL_CONFIG_ROOT=/usr/local/share/yggdrasil/configuration`. The
`yggdrasil-node --network <preset>` resolver must also keep probing the
installed configuration root `<prefix>/share/yggdrasil/configuration` after
honoring `YGGDRASIL_CONFIG_ROOT`, so release installs do not depend on a source
checkout.
Cargo metadata is also bucket-checked: the shipped `yggdrasil-node` package
must resolve from `crates/node/cardano-node/`, `yggdrasil-node-*` support
packages must remain under `crates/node/`, and sister-tool packages must remain
under `crates/tools/`.

Historical evidence is intentionally excluded: tagged `CHANGELOG.md`
sections, `docs/archive/**`, pre-R505 `docs/operational-runs/*.md` records,
and operational-run logs/artifacts. The `[Unreleased]` changelog section and
R505+ operational-run markdown from the post-reorganization cleanup window are
scanned so new notes cannot reintroduce stale placement guidance. Current
instructions must use `crates/node/cardano-node/`, `configuration/`, and
`scripts/`.

Run `python3 scripts/check-stale-placement.py --self-test` before the scan
when editing the guard itself. CI runs both the self-test and the live tree
scan. The live scan invokes `cargo metadata --no-deps` and falls back to
`~/.cargo/bin/cargo` when the current shell's PATH has not inherited the Rust
toolchain yet.

### `check-doc-status-headers.py` (R824, CI gate)

Validates living parity/status docs:

- `docs/PARITY_SUMMARY.md`, `docs/UPSTREAM_PARITY.md`, and
  `docs/COMPLETION_ROADMAP.md` must agree on `As of date`, `Round ceiling`,
  `Parity tag`, and `Test baseline date`.
- The declared round ceiling must be at or ahead of the newest
  `docs/operational-runs/*round-*.md` note; sibling logs/artifacts are
  intentionally ignored.
- `Parity tag` must match `docs/parity-matrix.json::reference.tag`.
- `docs/PARITY_DASHBOARD.md` must use the canonical status date and status
  counts derived from `docs/parity-matrix.json`.

Run whenever central parity docs, the dashboard, operational-run round
records, or `docs/parity-matrix.json` statuses change.
Run `python3 scripts/check-doc-status-headers.py --self-test` before the live
scan when editing the guard itself. CI runs both the self-test and live scan.

### `check-parity-matrix.py` (CI gate)

Validates [`docs/parity-matrix.json`](../docs/parity-matrix.json):

- JSON schema (top-level keys, per-entry shape).
- `reference.tag` matches the policy tag (currently `11.0.1`).
- `.reference-haskell-cardano-node/REFERENCE_TAG` matches the policy tag.
- `.reference-haskell-cardano-node/` is metadata-free and contains no nested
  `.git` directory or file.
- Every `haskell_reference[*].path` exists under
  `.reference-haskell-cardano-node/...` at validation time.
- Every `rust_surface[*].path` exists in the workspace.

Failure typically means upstream moved a path (paths can shift across
release tags) or a Rust file was renamed without updating the matrix.

### `check-fixture-manifest.py` (R303, CI gate)

Cross-checks the `cardano-base` SHA pin matrix:

- `crates/node/config/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT` (Rust constant).
- `specs/upstream-test-vectors/cardano-base/<SHA>/` (vendored corpus directory).
- `docs/SPECS.md` (provenance prose).
- `docs/UPSTREAM_PARITY.md` (pin matrix table).

All four sources MUST agree on the same 40-char SHA. The script also
verifies the two required sub-corpora (`vrf-praos-vectors`,
`kes-test-vectors`) are present under the vendored-corpus directory.

Failure means a pin update missed one of the four locations, or the
vendored corpus directory is missing the SHA-named subdirectory.

### `check-reference-artifacts.py` (R303, local-only)

NOT wired to CI (because CI doesn't carry the 1.3 GB vendored install) and
requires Linux/WSL because the IntersectMBO release bundle contains Linux
executables. Validates `.reference-haskell-cardano-node/install/`:

- `bin/cardano-node --version` matches the policy tag (currently `11.0.1`).
- 9 binaries present + executable: `cardano-node`, `cardano-cli`,
  `db-analyser`, `db-synthesizer`, `db-truncater`, `cardano-tracer`,
  `cardano-submit-api`, `cardano-testnet`, `bech32`.
- 3 networks × 8 config files present under
  `share/{mainnet,preprod,preview}/` (`config.json`, `topology.json`,
  `peer-snapshot.json`, `tracer-config.json`,
  `byron-genesis.json`, `shelley-genesis.json`, `alonzo-genesis.json`,
  `conway-genesis.json`).

Run after `bash scripts/setup-reference.sh --force` to confirm the
vendored install lines up with the policy tag.

## Refresh helper

### `setup-reference.sh`

One-shot script that brings `.reference-haskell-cardano-node/` to the
policy tag. It uses temporary shallow clones, copies metadata-free source
snapshots for the upstream `cardano-node` repo + all dependency repos, and
fetches the corresponding compiled install bundle, populating `install/bin/`,
`install/share/`, and the `<network>/db/` ChainDBs. The `--force` flag
rebuilds from scratch even if the directory exists.

`--sources-only` is portable and stops after refreshing the metadata-free
source snapshot. The full install path requires Linux/WSL because it downloads
and runs IntersectMBO's `linux-amd64` binary bundle.

The refresh writes `.reference-haskell-cardano-node/REFERENCE_TAG` after the
top-level source snapshot is materialised. That file replaces the old
`git describe` check; the reference tree intentionally contains no nested
`.git` metadata. `setup-reference.sh` fails if a future source refresh leaks
git metadata back into the reference tree.

The `CARDANO_NODE_VERSION` constant inside the script MUST stay in
lockstep with `docs/parity-matrix.json::reference.tag`,
`scripts/check-parity-matrix.py`, and prose mentions in `AGENTS.md` +
`CLAUDE.md`. See `intersectmbo_version_policy.md` in agent memory for
the full bump checklist.

The generated `install/run-node.sh` launcher defaults to
`.reference-haskell-cardano-node/install/run/<network>/`, but accepts
`RUN_ROOT=/tmp/cardano-reference` so reference sockets and ChainDBs can live on
a native Unix-socket-capable filesystem during local Haskell relay comparisons.

## Discovery script

### `audit-strict-mirror.py` (R274)

Populates the strict-mirror allowlist. Walks every production `.rs`,
derives candidate upstream basenames via snake_case ↔ PascalCase
(handles consecutive-uppercase runs: `kes`→`KES`, `vrf`→`VRF`,
`ocert`→`OCert`, `bls`→`BLS`, `dsign`→`DSIGN`, `cbor`→`CBOR`, plus
mixed-case forms `OCert` / `TPraos`), applies a crate-to-repo
affinity filter against the flat upstream index at
`docs/upstream-haskell-files.txt` (built one-time per
`setup-reference.sh` run), and emits a TSV with auto-graded verdicts:

- **(a) DIRECT_MIRROR** — exactly one upstream `.hs` matches in name
  AND concept.
- **(b) RENAME_NEEDED** — a real upstream parent file exists with a
  different basename.
- **(c) NO_MIRROR_NEEDS_DOCSTRING** — genuinely synthesis (combine of
  multiple upstream files; orchestration loop with no Haskell file
  parallel; Rust-idiomatic split).
- **(d) NAME_CLASH_REGRADE** — same basename in upstream maps to a
  different concept; either rename or add a parity-caveat docstring.

The TSV is hand-graded after the first pass and committed as the
audit allowlist. `check-strict-mirror.py` imports this module and
re-uses the same heuristics so authoring-time + CI-time agree on
which filename is "in scope".

##  Rules *Non-Negotiable*

- The CI validators (strict-mirror, stale-placement, doc-status,
  parity-matrix, fixture-manifest) and the local reference-artifacts
  validator MUST stay green between rounds. A failing
  validator is a closure-criterion violation, not a "fix later" item.
- New Python validators MUST follow the `kebab-case.py` naming convention and
  use `python3` (no virtualenv); only stdlib + system cargo/git CLI
  invocations are allowed. No third-party Python deps.
- New operator shell helpers SHOULD use descriptive `snake_case.sh` names,
  matching the existing runbook harnesses.
- `setup-reference.sh` and `check-reference-artifacts.py` MUST cite
  the policy tag from `docs/parity-matrix.json::reference.tag` rather
  than hardcoding it; a tag bump should require updating exactly one
  source-of-truth.
- `check-strict-mirror.py` MUST import (not duplicate) the basename-
  derivation heuristics from `audit-strict-mirror.py` so authoring
  time + CI time agree byte-for-byte on the allowlist algorithm.
- The `__pycache__/` directory created by Python imports MUST stay
  ignored via `.gitignore` (already wired R274).

## Maintenance Guidance

- When adding a new validator, decide CI vs local-only first
  (`check-reference-artifacts.py` is Linux/WSL local-only because the install
  bundle is 1.3 GB and contains Linux executables).
- Wire CI validators into `.github/workflows/ci.yml` after the existing
  cargo gates; warn-only first if the validator surfaces existing
  violations, then promote to fail-build once violations are resolved
  (the R275 → R288 pattern).
- Document a new validator in CLAUDE.md's Commands section + the
  enclosing crate AGENTS.md if its scope is crate-local.
- Keep CI validators and operator harnesses named by purpose; do not add
  session-local helpers here. Claude Code helpers belong in `.claude/scripts/`.

## Official Upstream References

- Vendored upstream tree (gitignored, metadata-free, refreshed by `setup-reference.sh`):
  `.reference-haskell-cardano-node/`
- Policy tag source-of-truth: [`docs/parity-matrix.json`](../docs/parity-matrix.json) (`reference.tag`)
- Strict-mirror allowlist source-of-truth: [`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv)
- Fixture-manifest pin source-of-truth: [`crates/node/config/src/upstream_pins.rs`](../crates/node/config/src/upstream_pins.rs) (`UPSTREAM_CARDANO_BASE_COMMIT`)
