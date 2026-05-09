# Guidance for the workspace-level scripts under `scripts/`.

This directory hosts vendored-tree refresh tooling plus the four CI
parity validators. None of the scripts execute Rust — they exist to
police the boundary between the vendored upstream tree, the per-file
parity allowlists, and the on-disk corpora used by the workspace tests.

## Directory shape

```
scripts/
├── setup-reference.sh             # one-shot vendored-tree fetch (refresh helper)
├── audit-strict-mirror.py         # discovery script, populates docs/strict-mirror-audit.tsv
├── check-strict-mirror.py         # CI gate (R288): file-mirror drift detector
├── check-parity-matrix.py         # CI gate: parity-matrix.json schema + paths
├── check-fixture-manifest.py      # CI gate (R303): cardano-base SHA pin consistency
└── check-reference-artifacts.py   # local-only: validates .reference-haskell-cardano-node/install/
```

## Validators

### `check-strict-mirror.py` (R275 warn-only → R288 fail-build)

Walks every production `.rs` under `crates/<crate>/src/` + `node/src/`
(excluding `**/tests/**` + `target/`) and verifies each file either:

1. Mirrors a single canonical upstream `.hs` file by snake_case
   basename (with directory-prefix fallback for sibling collisions), OR
2. Carries a `## Naming parity` docstring stanza ending in
   `**Strict mirror:** none.` plus the upstream symbol(s)/file(s) the
   helper surfaces.

Allowlist source-of-truth: [`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv).
The `--fail-on-violation` flag flips exit code on violation (CI mode).
Imports the audit module via importlib so the verdict heuristics stay
in one place.

Runs on every push via `.github/workflows/ci.yml`. Failure means a new
production `.rs` was added without either an upstream filename mirror
or the explicit `## Naming parity` block — author the docstring
(see `.claude/skills/round-extraction/SKILL.md` for the pattern) or
rename the file to mirror an upstream `.hs`.

### `check-parity-matrix.py` (CI gate)

Validates [`docs/parity-matrix.json`](../docs/parity-matrix.json):

- JSON schema (top-level keys, per-entry shape).
- `reference.tag` matches the policy tag (currently `11.0.1`).
- Every `haskell_reference[*].path` exists under
  `.reference-haskell-cardano-node/...` at validation time.
- Every `rust_surface[*].path` exists in the workspace.

Failure typically means upstream moved a path (paths can shift across
release tags) or a Rust file was renamed without updating the matrix.

### `check-fixture-manifest.py` (R303, CI gate)

Cross-checks the `cardano-base` SHA pin matrix:

- `node/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT` (Rust constant).
- `specs/upstream-test-vectors/cardano-base/<SHA>/` (vendored corpus directory).
- `docs/SPECS.md` (provenance prose).
- `docs/UPSTREAM_PARITY.md` (pin matrix table).

All four sources MUST agree on the same 40-char SHA. The script also
verifies the two required sub-corpora (`vrf-praos-vectors`,
`kes-test-vectors`) are present under the vendored-corpus directory.

Failure means a pin update missed one of the four locations, or the
vendored corpus directory is missing the SHA-named subdirectory.

### `check-reference-artifacts.py` (R303, local-only)

NOT wired to CI (because CI doesn't carry the 1.3 GB vendored install).
Validates `.reference-haskell-cardano-node/install/`:

- `bin/cardano-node --version` matches the policy tag (currently `11.0.1`).
- 9 binaries present + executable: `cardano-node`, `cardano-cli`,
  `db-analyser`, `db-synthesizer`, `db-truncater`, `cardano-tracer`,
  `cardano-submit-api`, `cardano-testnet`, `bech32`.
- 3 networks × 8 config files present under
  `share/{mainnet,preprod,preview}/` (`config.json`, `topology.json`,
  `peer-snapshot.json`, `checkpoints.json`, `tracer-config.json`,
  `byron-genesis.json`, `shelley-genesis.json`, `alonzo-genesis.json`,
  `conway-genesis.json`, `submit-api-config.json`).

Run after `bash scripts/setup-reference.sh --force` to confirm the
vendored install lines up with the policy tag.

## Refresh helper

### `setup-reference.sh`

One-shot script that brings `.reference-haskell-cardano-node/` to the
policy tag. It clones (or pulls) the upstream `cardano-node` repo +
all dependency repos AND fetches the corresponding compiled install
bundle, populating `install/bin/`, `install/share/`, and the
`<network>/db/` ChainDBs. The `--force` flag rebuilds from scratch
even if the directory exists.

The `CARDANO_NODE_VERSION` constant inside the script MUST stay in
lockstep with `docs/parity-matrix.json::reference.tag`,
`scripts/check-parity-matrix.py`, and prose mentions in `AGENTS.md` +
`CLAUDE.md`. See `intersectmbo_version_policy.md` in agent memory for
the full bump checklist.

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

- The four CI validators (strict-mirror, parity-matrix, fixture-manifest,
  reference-artifacts-local) MUST stay green between rounds. A failing
  validator is a closure-criterion violation, not a "fix later" item.
- New scripts MUST follow the `kebab-case.py` naming convention.
- Scripts MUST use `python3` (no virtualenv); only stdlib + system
  cargo/git CLI invocations are allowed. No third-party Python deps.
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
  (`check-reference-artifacts.py` is local-only because the install
  bundle is 1.3 GB).
- Wire CI validators into `.github/workflows/ci.yml` after the existing
  cargo gates; warn-only first if the validator surfaces existing
  violations, then promote to fail-build once violations are resolved
  (the R275 → R288 pattern).
- Document a new validator in CLAUDE.md's Commands section + the
  enclosing crate AGENTS.md if its scope is crate-local.
- The `scripts/` tree is small by design — resist adding shell helpers
  that belong in `node/scripts/` (operator-side) or
  `.claude/scripts/` (Claude Code session-side).

## Official Upstream References

- Vendored upstream tree (gitignored, refreshed by `setup-reference.sh`):
  `.reference-haskell-cardano-node/`
- Policy tag source-of-truth: [`docs/parity-matrix.json`](../docs/parity-matrix.json) (`reference.tag`)
- Strict-mirror allowlist source-of-truth: [`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv)
- Fixture-manifest pin source-of-truth: [`node/src/upstream_pins.rs`](../node/src/upstream_pins.rs) (`UPSTREAM_CARDANO_BASE_COMMIT`)
