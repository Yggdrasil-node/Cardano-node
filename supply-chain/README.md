# `supply-chain/` — cargo-vet audit trail

Wave 7 PR 21. Seeds the [cargo-vet](https://mozilla.github.io/cargo-vet/)
manual-audit framework on top of the existing `cargo-deny` advisory /
license / bans gate.

## Files

| File | Role |
| --- | --- |
| `audits.toml` | Source-of-truth for human audit records. Defines the `crypto-reviewed` and `observability-reviewed` criteria (both `implies = "safe-to-deploy"`). Empty `[audits]` at seed; populated via `cargo vet certify`. |
| `config.toml` | Workspace policy: which third-party crates require which criteria, which trusted audit chains we import from (Mozilla, Google, Bytecode Alliance). |
| `imports.lock` | Cached hash of imported audit chains. Generated on first `cargo vet`; do not hand-edit. |
| `publishers.snapshot.txt` | (Optional, generated) Snapshot of `cargo supply-chain publishers --diffable` output committed alongside the workflow. Tracking baseline so the weekly drift detector can open an issue only when the publisher set changes. |

## How to add an audit

```bash
cargo vet certify <crate> <version>          # interactive
# or
cargo vet certify ed25519-dalek 2.1.1 \
  --criteria crypto-reviewed \
  --notes "Verified constant-time scalar ops; Zeroize on SigningKey; no unsafe."
```

This appends a `[[audits.<crate>]]` block to `audits.toml`. Commit
the change and reference the round / audit ticket in the PR
description.

## How to update imported chains

```bash
cargo vet update                  # refreshes imports.lock
```

The weekly `.github/workflows/supply-chain-audit.yml` job runs this
implicitly and opens an issue if the chains drifted.

## Relationship to `deny.toml`

`deny.toml` enforces *automated* policy: no openssl / native-tls,
permissive licenses only, no copyleft, no yanked deps, no unknown
git registries. It runs on every push.

`cargo-vet` enforces *human-attested* policy: a specific person
audited a specific version under specific criteria. It runs
weekly + on-demand via `cargo vet`.

Both gates layer; neither subsumes the other. `deny.toml` catches
a copyleft slip in seconds; `cargo-vet` ensures the cryptography
hot path has had eyes on it before it ships.

## Future bumps

When new audit criteria are needed (e.g. `consensus-reviewed` for
the Wave 5 `yggdrasil-node-sync` extraction's external deps), add
them under `[criteria.*]` in `audits.toml` with an `implies` chain
plus a written description. Adding policy entries in `config.toml`
is a one-line addition once the criteria exists.
