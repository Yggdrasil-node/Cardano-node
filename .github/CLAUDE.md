# GitHub Assistant Guidance

This file is a short entry point for Claude Code and other repository assistants. The authoritative workspace rules live in [../AGENTS.md](../AGENTS.md) and the broader assistant guide lives in [../CLAUDE.md](../CLAUDE.md).

Before making changes:
- Read the root [AGENTS.md](../AGENTS.md) plus the nearest folder-specific `AGENTS.md`.
- Keep `AGENTS.md` files operational and current when behavior, verification, or ownership changes.
- Anchor parity-sensitive behavior to official IntersectMBO sources: `cardano-node`, `cardano-ledger`, `ouroboros-consensus`, `ouroboros-network`, `cardano-base`, and `plutus`.
- For storage and rollback work, cross-check the Ouroboros Consensus LedgerDB/openDB docs, caught-up node storage model, and UTxO-HD snapshot/rollback notes before introducing local terminology.
- Treat `docs/operational-runs/*.md` as historical run records. Add a new run record for new evidence and update living docs (`README.md`, `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/PARITY_PROOF.md`, `docs/UPSTREAM_PARITY.md`, manual/runbook) for current status.

Default verification remains:

```bash
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
```
