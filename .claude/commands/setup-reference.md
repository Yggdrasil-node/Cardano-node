---
description: Refresh `.reference-haskell-cardano-node/` to the policy IntersectMBO/cardano-node tag.
allowed-tools: Bash(bash scripts/setup-reference.sh*)
---

Run `bash scripts/setup-reference.sh` to materialize / refresh the
pinned IntersectMBO/cardano-node reference tree at
`.reference-haskell-cardano-node/`.

The script defaults `CARDANO_NODE_VERSION` to **the latest upstream
release** (currently 11.0.1). Yggdrasil tracks the latest tag at all
times — bump in lockstep across:

- `scripts/setup-reference.sh` (`CARDANO_NODE_VERSION`)
- `scripts/check-parity-matrix.py` (`REFERENCE_TAG` + `ALLOWED_STATUS`)
- `docs/parity-matrix.json` (`reference.tag` + every `haskell_reference.path`)
- `CLAUDE.md` and root `AGENTS.md` prose

Pass `--sources-only` to refresh the metadata-free source snapshot without
downloading or executing the Linux binary bundle. This mode is portable across
Windows/macOS/Linux and is enough for path-based parity checks:

```bash
bash scripts/setup-reference.sh --sources-only
```

Pass `--force` to wipe and rebuild the full source snapshot plus compiled
install (~1.3 GB, ~5 minutes). The full install path requires Linux/WSL because
the upstream release artifact is a Linux executable bundle:

```bash
bash scripts/setup-reference.sh --force
```

After the full script completes on Linux/WSL, verify:

- `.reference-haskell-cardano-node/install/bin/cardano-node --version`
  reports the policy tag.
- `python3 scripts/check-parity-matrix.py` returns clean (paths under
  `haskell_reference` may have moved between releases — re-validate).

If the `parity-matrix.json` paths fail validation after a tag bump,
that is the first signal that semantic locations have moved upstream
and the matrix needs updating before claiming parity for those
features.
