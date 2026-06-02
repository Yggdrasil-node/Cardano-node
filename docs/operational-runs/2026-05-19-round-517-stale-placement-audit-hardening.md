# Round 517 - stale-placement audit hardening

**Date:** 2026-05-19
**Area:** workspace layout / stale post-reorganization placement guard
**Upstream reference:** upstream keeps operator configuration and scripts at
repository scope, while node runtime integration stays under the node package
tree. Yggdrasil mirrors that split with root `configuration/`, dev-owned `dev/{scripts,evidence,reference,test}/`,
and the executable shell at `crates/node/cardano-node`.

## Summary

This round records the current post-reorganization placement audit and hardens
the guard so stale locations cannot silently reappear as empty or untracked
directories. The cleanup preserves the shipped package and binary name
`yggdrasil-node`; that name is an operator compatibility surface, not the
workspace directory owner.

## Current accepted placements

- `crates/node/cardano-node` owns the thin binary crate and integration tests.
- `crates/node/{config,genesis,sync,runtime,tracer,ntc-server,ntn-server,plutus-eval,block-producer}`
  own reusable node subsystems.
- `crates/tools/<tool>` owns sister-tool ports.
- `configuration/` owns mainnet, preprod, and preview operator presets.
- `dev/{scripts,evidence,reference,test}/` owns operator helpers, parity harnesses, reference setup, and local validators.

## Guard coverage

`dev/test/check-stale-placement.py` now validates these current surfaces:

- live current-facing text paths from tracked and unignored files;
- resolved Cargo metadata for workspace members, package manifests, and target
  source paths;
- Cargo package-bucket invariants: shipped `yggdrasil-node` from
  `crates/node/cardano-node/`, node support packages under `crates/node/`, and
  sister-tool packages under `crates/tools/`;
- R505+ operational-run notes and only the `[Unreleased]` changelog section;
- exact stale filesystem directories, including empty or untracked directories;
- operator-artifact directories under any `crates/node/*` package, so
  configuration presets and helper scripts stay in root `configuration/` and
  `scripts/`;
- required current placement files for the replacement layout: the
  `crates/node/cardano-node` binary-crate shell, canonical root operator
  preset bundles, dev sister-tool launcher plus dev reference/operator helper scripts;
- Git executable mode for every tracked `dev/{scripts,evidence,reference}/*.sh`, preserving direct
  operator invocation after the scripts moved out of the node crate;
- release, reproducibility, and Docker packaging snippets that stage or copy
  root `configuration/` and dev tooling from their accepted locations;
- release-installer checks that require and install the bundled root
  `configuration/` and `dev/` trees under `<prefix>/share/yggdrasil/`;
- nested Git metadata inside the vendored Haskell reference snapshot;
- the committed Git ignore rule and `git check-ignore` result for the
  `.reference-haskell-cardano-node/` local-only corpus;
- `.gitmodules` and Git-index submodule entries that would turn the reference
  snapshot back into a nested checkout.
- regular Git-index entries under the Haskell reference snapshot, preserving it
  as a local-only research corpus rather than tracked source input.

The `--self-test` mode covers rejected legacy classes by label and also proves
accepted current paths stay allowed. R505+ operational-run markdown is scanned
by the guard, while older run records and logs remain historical evidence, so
this note intentionally describes rejected path classes without restating the
rejected literal path strings.

## Continuation audit

A follow-up filesystem pass confirmed:

- none of the rejected legacy directory classes exist in the current worktree;
- the vendored Haskell reference snapshot contains no nested Git metadata
  directories or submodule-style metadata files;
- the repository has no `.gitmodules`, the reference snapshot is ignored by
  Git, and the reference snapshot has no Git index entries, either as a
  submodule or as regular tracked files;
- the only broad-search hits for rejected legacy classes are historical
  operational-run evidence or the guard's own test vectors.
- a live upstream tag check reported `11.0.1` as the newest semver release tag,
  matching the local parity policy pin.
- branch cleanup is currently a no-op: the repository has one local branch
  (`main`) and one remote branch target (`origin/main`).
- local Git identity is configured as `Fraction.estate <noreply@fraction.estate>`,
  using Fraction Estate attribution for local commits.
- ownership and ignore surfaces are aligned with the cleaned layout:
  `.github/CODEOWNERS` owns `/crates/node/`, `/crates/tools/`, `/dev/`,
  and `/configuration/`, while `.gitignore` keeps the metadata-free Haskell
  reference snapshot and build outputs local-only without hiding the rejected
  stale source directories.
- generated navigation in `dev/filetree/` records the active
  `crates/node/cardano-node` binary-crate shell and the operator artifact
  directories.
- required current placement checks pass for `crates/node/cardano-node`, every
  root `configuration/{mainnet,preprod,preview}/` canonical preset file
  (`config.json`, `config-legacy.json`, `topology.json`,
  `{byron,shelley,alonzo,conway}-genesis.json`, `peer-snapshot.json`,
  `submit-api-config.json`, `tracer-config.json`, and `checkpoints.json` where
  present), `configuration/poolMetaData.json`, `dev/scripts/run-tools.sh`,
  `dev/reference/setup-reference.sh`, `dev/reference/install_haskell_cardano_node.sh`,
  `dev/evidence/compare_tip_to_haskell.sh`, `dev/evidence/parallel_blockfetch_soak.sh`,
  `dev/scripts/preview_producer_harness.sh`, `dev/scripts/yggdrasil-node.service`, and
  the dev reference/evidence helper set.
- Git-index mode checks pass for `dev/{scripts,evidence,reference}/*.sh`: shell helpers are
  tracked as `100755`, while `dev/scripts/yggdrasil-node.service` remains a
  non-executable unit template.
- release/repro workflows stage root `configuration/` and `dev/`, and
  the Dockerfile copies root `configuration/` plus helper scripts from
  `dev/scripts/`.
- `dev/scripts/install_from_release.sh` now requires the archive's
  `configuration/` and `dev/` trees and installs them to
  `<prefix>/share/yggdrasil/`, so the quick-install path preserves the moved
  operator artifacts instead of installing only the binary.
- `yggdrasil-node --network <preset>` now resolves config-relative genesis,
  topology, and peer-snapshot files from an explicit `YGGDRASIL_CONFIG_ROOT`,
  then the installed binary prefix's `<prefix>/share/yggdrasil/configuration`,
  then local checkout/archive fallbacks. This keeps release installs independent
  of the old node-local artifact placement and of the build machine's source
  path.
- The packaged systemd template now pins
  `YGGDRASIL_CONFIG_ROOT=/usr/local/share/yggdrasil/configuration`, and the
  Docker runtime image pins
  `YGGDRASIL_CONFIG_ROOT=/usr/share/yggdrasil/configuration`, matching the
  configuration bundle location each packaging path installs.

The live placement remains the root operator-artifact split plus the
`crates/node/cardano-node` binary-crate shell.

The release and reproducibility workflows intentionally look for the
CycloneDX SBOM under the `crates/node/cardano-node` crate shell while keeping
the generated package/binary artifact named `yggdrasil-node`. The repro
workflow now fails with a clear error if the SBOM lookup misses both the
expected current path and cargo-cyclonedx fallback search.

An operator-surface pass checked the release installer, systemd unit template,
Docker Compose quick start, Docker manual, running manual, and release manual.
Those surfaces already use the root operator-artifact layout and shipped
binary name. The installation-manual link target used by the installer exists.

A live node-crate source pass checked `crates/node/cardano-node`,
`crates/node/config`, `crates/node/runtime`, and `crates/node/sync` for old
operator-artifact assumptions. The active binary shell resolves preset configs
through `../../../configuration`, smoke tests resolve dev scripts through
`../../../dev/scripts`, and the config/plutus parity fixtures resolve the root
`configuration/` tree from their current crate locations. The only remaining
`node/configuration`, `node/scripts`, or old root `scripts/` hits are historical run records, logs,
or the stale-placement guard's own rejection vectors.

A metadata and packaging pass checked `Cargo.toml`, `Cargo.lock`, CI workflow
YAML, release packaging, reproducibility packaging, Docker, `justfile`,
current living docs, scripts, configuration, crates, specs, and supply-chain
metadata. Current release surfaces stage root `configuration/` and `dev/`, while CycloneDX SBOM lookup resolves from
`crates/node/cardano-node/yggdrasil-node.cdx.json`. No current-facing metadata
surface points back at the retired node-local operator artifact paths or the
old top-level sister-tool crate layout.

A Codex workspace pass checked that retired AI-harness files are no longer
required by current guidance. Live reference, evidence, validator, and operator
helpers now live under `dev/{reference,evidence,test,scripts}/` so they are not
confused with runtime source or packaging artifacts.

A helper-configuration pass checked `.cargo/config.toml`,
`.config/nextest.toml`, `.devcontainer/post-create.sh`, `.github/`, dev
helper scripts, `justfile`, Docker surfaces, root README guidance, and root
agent instructions. Cargo aliases and nextest configuration do not reference
retired placement paths. The devcontainer bootstrap invokes the `dev/reference/install_haskell_cardano_node.sh` helper, matching the accepted dev reference-helper placement. The only broad-search hits in those surfaces are
upstream reference links, current release artifact paths, or the stale-placement
guard's own rejection vectors.

## Verification

- `cargo metadata --no-deps --format-version 1` confirmed the workspace member
  is `crates/node/cardano-node#yggdrasil-node@0.2.0`, with no stale member
  under the former node-crate directory class.
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python3 dev/test/check-stale-placement.py --self-test`
- `python3 dev/test/check-stale-placement.py`
- `python3 -m py_compile dev/test/check-stale-placement.py`
- `python3 dev/test/filetree.py check`
- `python3 dev/test/check-parity-matrix.py`
- `python3 dev/test/check-strict-mirror.py --fail-on-violation`
- `python3 dev/test/check-fixture-manifest.py`
- `git diff --check` exited 0 with only Windows LF-to-CRLF warnings.

## Remaining scope

This closes another stale-placement audit class. It does not claim full
functional parity with upstream Cardano node behavior; that remains governed by
the parity matrix, strict-mirror audit, fixture manifest, manual runbook, and
operator endurance gates.
