---
title: 'R311: strict-mirror index-vs-tree drift check'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-311-strict-mirror-index-drift-check/
---

# Round 311 — strict-mirror index-vs-tree drift check

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R310`](2026-05-09-round-310-gitignore-debug-pattern-fix.md)

## Motivation

R310 surfaced a class of CI failure that the existing strict-mirror
gate could not detect: an over-broad `.gitignore` pattern silently
swallowed 12 production `.rs` files (the entire R294
`crates/cardano-cli/src/era_independent/debug/` subtree). The local
working tree built clean across 35 R-arc rounds (R294–R309) because
the files existed on disk; CI's first attempt at `cargo fmt --all
-- --check` on a fresh clone failed with an opaque
`failed to resolve mod` error.

Root cause was a `.gitignore` pattern bug, fixed in R310. But the
**failure-detection gap** remains: `dev/test/check-strict-mirror.py`
walks the local filesystem (`audit.iter_rust_files()`), not the git
index. A future contributor introducing a sibling subtree under
another inadvertently-gitignored basename would hit the same opaque
CI failure.

R311's "Lessons" entry from the previous round-doc:

> The strict-mirror gate is local-tree-aware, not index-aware. A
> future enhancement could cross-check against `git ls-files` to
> catch divergences between the local tree and the index.

R311 closes that gap.

## Implementation

`dev/test/check-strict-mirror.py` now has a second pass per file:

```python
def get_tracked_rust_files() -> set[str] | None:
    """Return tracked production `.rs` paths from `git ls-files`.

    Returns None if `git` is unavailable or the cwd is not a git
    repository (so the caller can skip the index-vs-tree cross-check
    rather than emitting noise).
    """
    try:
        result = subprocess.run(
            ["git", "ls-files", "--", "crates/*.rs", "node/*.rs"],
            cwd=str(ROOT),
            capture_output=True,
            text=True,
            check=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return {line for line in result.stdout.splitlines() if line}
```

In the main loop, alongside the existing strict-mirror verdict
walk:

```python
if tracked is not None and rel not in tracked:
    drift_violations.append(rel)
```

Drift violations are reported as a separate class with a distinct
`::warning::` payload so the operator can tell at a glance whether
they need to author a `## Naming parity` block (mirror violation)
or fix `.gitignore` and re-stage (drift violation). Both classes
trigger fail-build under `--fail-on-violation`.

The check degrades gracefully:

- `git` unavailable → `subprocess.run` raises `FileNotFoundError` →
  `get_tracked_rust_files()` returns `None` → drift loop is
  skipped. The gate still runs the existing mirror/docstring check.
- Not a git repo → `git ls-files` exits nonzero →
  `subprocess.CalledProcessError` → same graceful skip.

## Smoke tests

**Test 1: synthetic violation without docstring + untracked.**

```text
$ touch crates/cardano-cli/src/era_independent/synthetic_drift_test.rs
$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 1 new file(s) violate the policy …
::warning file=…/synthetic_drift_test.rs::(NEEDS-REVIEW) … must either
  rename to an upstream `.hs` basename or add a `## Naming parity`
  docstring block
strict-mirror: 1 file(s) exist locally but are NOT tracked in
  `git ls-files` (R311 index-vs-tree drift …):
::warning file=…/synthetic_drift_test.rs::index-vs-tree drift -
  file exists locally but is not tracked in git; check `.gitignore`
  patterns and `git add` if needed
exit code: 1
```

**Test 2: R310-class scenario — file with valid docstring,
gitignored.**

```text
$ cat > crates/cardano-cli/src/era_independent/r310_simulate.rs <<EOF
//! Simulated R310-class file.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Synthesis test fixture.

fn x() {}
EOF
$ echo "/crates/cardano-cli/src/era_independent/r310_simulate.rs" >> .gitignore
$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 1 file(s) exist locally but are NOT tracked in
  `git ls-files` (R311 index-vs-tree drift …):
::warning file=…/r310_simulate.rs::index-vs-tree drift - …
exit code: 1
```

The R310 scenario is now caught by the gate **before** CI runs
`cargo fmt`, surfacing a clear actionable warning instead of an
opaque module-resolution error.

## Diff inventory

| Path | Change |
|---|---|
| `dev/test/check-strict-mirror.py` | New `subprocess` import; new `get_tracked_rust_files()` helper; main loop now walks both the existing mirror/docstring check AND the new index-vs-tree check; combined exit-code logic. ~50 lines added. |
| `scripts/AGENTS.md` | `check-strict-mirror.py` section: subhead bumped to "(R275 warn-only → R288 fail-build, R311 drift-aware)"; new "Also cross-checks the working tree against the git index" paragraph; failure-mode list expanded from a single sentence to two named classes (Mirror/docstring violation + Index-vs-tree drift). |
| `docs/operational-runs/2026-05-09-round-311-strict-mirror-index-drift-check.md` | This round-doc. |

R311 ships zero Rust changes; the workspace test baseline (4,855
passing) is preserved by construction.

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

## Closure criterion

- Strict-mirror gate exits zero on a clean tree.
- Synthetic untracked file (without docstring or with valid
  docstring) triggers the new R311 drift warning and exit 1 under
  `--fail-on-violation`.
- The check degrades gracefully when `git` is unavailable.
- Documentation in `scripts/AGENTS.md` reflects the new behavior.

All four are met.

## Future enhancements (deferred)

- **Per-file `pub mod`-graph walk.** Today's check uses the simpler
  filesystem-vs-index diff. A more targeted check would parse `pub
  mod X;` declarations from each tracked .rs and verify the
  declared child path is also tracked. This would catch the R310
  failure mode even at the *commit-time* of the parent shell
  (before the children are even authored). Deferred — the
  filesystem-vs-index diff is sufficient for the actual R310 bug
  pattern (children authored locally, never committed) and is
  simpler to maintain.
- **Promote drift-only violations to a separate exit class.** Today
  both mirror-violation and index-drift trigger the same exit 1.
  An operator could want to gate CI strictly on drift while
  tolerating mirror violations during a Phase-B refactor (or vice
  versa). Deferred — no current operational need; both classes
  are fail-build by policy.
