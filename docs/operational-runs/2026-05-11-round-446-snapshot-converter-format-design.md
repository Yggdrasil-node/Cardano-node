---
title: 'R446 — snapshot-converter format-version design scaffolding'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-446-snapshot-converter-format-design/
---

# R446 — snapshot-converter format-version design scaffolding

**Date:** 2026-05-11
**Predecessor:** R439 (`crates/snapshot-converter/src/status.rs`'s
`convert_snapshot_status` deferral surface).
**Plan:** [`playful-tickling-plum.md`](https://github.com/.../playful-tickling-plum.md)
("R446 — Snapshot-Converter Format Design Round").

## Context

The upstream `snapshot-converter` binary handles a 3×3 conversion matrix
(mem ↔ mem, mem ↔ lmdb, mem ↔ lsm) because upstream has 3 ledger-DB
backends. **Yggdrasil has exactly 1 backend** (`FileLedgerStore` —
file-backed atomic snapshot persistence). The upstream 3×3 matrix
collapses for Yggdrasil:

| Upstream                           | Yggdrasil                                  |
|------------------------------------|--------------------------------------------|
| `mem ↔ mem` conversion             | no-op — single format                      |
| `mem ↔ lmdb` / `mem ↔ lsm`         | not applicable — Yggdrasil has 1 backend  |
| **Format-version migration over time** | **R446's actual scope**               |
| Upstream-to-Yggdrasil cross-port   | separate compat layer; out of scope        |

The realistic snapshot-converter work for Yggdrasil is **format-version
migration over time** — when the snapshot encoding evolves (e.g. R500+
era extensions, governance-state additions), older snapshots need to be
upgradable without operator intervention. R446 ships the version-tag
scaffolding that gates the migration logic in future rounds.

## Deliverables

### 1. Format-version tag

Lands `LedgerSnapshotVersion(u32)` newtype + named constants in
`crates/storage/src/ledger_db.rs`:

```rust
pub const MAGIC: [u8; 4] = *b"YgLS";  // Yggdrasil Ledger Snapshot
pub const V1: Self = Self(1);          // current format (no header)
pub const LATEST: Self = Self::V1;     // alias; bumps when V2 ships
```

Plus a `has_header()` predicate distinguishing V1 (no magic prefix) from
V2+ (`[MAGIC ; 4][version ; 4 big-endian][cbor-payload ; N]`).

### 2. Wire format

V1 snapshots (current) are plain CBOR blobs with no header — for
backwards compatibility with every pre-R446 snapshot on disk.

V2+ snapshots prepend:
- 4-byte magic: `b"YgLS"`
- 4-byte big-endian `u32` version number

This scheme is forward-compatible: future versions can carry richer
headers behind the magic without breaking V1 readers. A reader
encountering the magic + unknown version number preserves the tag
verbatim (`LedgerSnapshotVersion::new(99)`) for diagnostic surface.

### 3. Detection helper

`detect_version(data: &[u8]) -> LedgerSnapshotVersion` — pure, no
allocations, no I/O. Inspects the first 8 bytes for the magic prefix;
falls back to V1 for absent/malformed input. Safe to call in hot
loops (e.g. snapshot-converter scanning a directory).

### 4. Trait extension

Adds one default method to `LedgerStore`:

```rust
fn latest_snapshot_version(&self) -> Option<LedgerSnapshotVersion> {
    self.latest_snapshot().map(|(_, data)| detect_version(data))
}
```

Existing implementations (`InMemoryLedgerStore`, `FileLedgerStore`)
inherit it without per-impl wiring. Implementations that already carry
the version in a sidecar can override.

## Out-of-scope for R446 (deferred to follow-on rounds)

- **Actual conversion logic** (`V1 → V2` migration body): defers until
  V2 format is defined. R446 just establishes the version-tag
  scaffolding.
- **snapshot-converter binary wiring**: `convert_snapshot_status` stays
  as the operator-facing deferral surface; once V2 exists, the
  converter's `run()` will dispatch through the new trait methods.
- **`FileLedgerStore` on-disk header writing**: V1 snapshots
  intentionally have no header (backwards compatibility with existing
  data directories). V2+ writes will land with V2's introduction round.

## Verification gates

All 5 baseline gates clean at HEAD:

```text
cargo fmt --all -- --check                                  # clean
cargo check-all                                              # clean
cargo test-all                                               # 5,962 passing
cargo lint                                                   # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
```

R446 test surface: yggdrasil-storage gains **12 R446 version tests**
covering:
- canonical constants (V1=1, LATEST==V1, MAGIC==`"YgLS"`)
- `has_header()` predicate (false for V1, true for V2+)
- `detect_version()` for: legacy no-header payload, empty payload,
  short legacy payload (<8 bytes), V2-shaped header, unknown future
  version (preserves tag), almost-magic prefix (must not misclassify)
- `latest_snapshot_version()` via `InMemoryLedgerStore`: empty store,
  V1 legacy payload, V2-shaped payload

Workspace: 5,950 → 5,962.

## Follow-on roadmap

- **R447+**: When the snapshot format actually needs to evolve (e.g.
  R500+ era extensions), define `LedgerSnapshotVersion::V2` + ship
  the V1→V2 migration body. R446's scaffolding makes that round
  bounded — header writing + reading paths are already specified.
- **snapshot-converter binary wiring**: once V2 exists, replace
  `RunError::ConvertSnapshotDeferred` with a real migration dispatcher
  that calls `detect_version` per-snapshot + applies the appropriate
  migration step.
- **AGENTS.md refresh** for sister-tools that got status helpers in
  R439-R445 (the parallel direction-option from the previous decision;
  not blocked by R446).

## Parity-matrix delta

`docs/parity-matrix.json` `sister-tool.snapshot-converter`:

```diff
-  "next_milestone": "R440",
+  "next_milestone": "R447",
```

Status remains `partial` — full `verified_11_0_1` promotion still
requires the V1↔V2 migration body + an operator-driven rehearsal once
V2 lands.

## Critical files modified

- `crates/storage/src/ledger_db.rs` — adds `LedgerSnapshotVersion` +
  `detect_version` + trait method + 12 tests.
- `docs/parity-matrix.json` — `sister-tool.snapshot-converter`
  next_milestone advance.
- `docs/operational-runs/2026-05-11-round-446-snapshot-converter-format-design.md` — this doc.
- `CHANGELOG.md` — R446 entry under `[Unreleased]`.

## Cross-crate impact

None. The trait extension uses default impls, so:
- `node/` crate compiles unchanged.
- `crates/{consensus, network, ledger, plutus, crypto}` compile unchanged.
- All sister-tool crates compile unchanged.
