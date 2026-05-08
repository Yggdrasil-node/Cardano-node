---
description: Validate docs/parity-matrix.json against the local Rust + Haskell trees.
allowed-tools: Bash(python3 scripts/check-parity-matrix.py)
---

Run `python3 scripts/check-parity-matrix.py` and report:

1. Whether the matrix is clean.
2. The reference tag the script enforces (currently 11.0.1).
3. Any per-entry path or schema failures that need a fix.

If the matrix is clean: stop, no further action.

If the matrix fails:

- Identify whether the failure is a path that no longer exists (Haskell
  upstream may have moved the module across releases) or a schema
  violation (`status`, `next_milestone`, `area`, etc.).
- Cite the failing entry id + key + path.
- Suggest the corrective edit, but do not make it without explicit
  authorization (the parity matrix is operational evidence, not a
  rolling work log).
