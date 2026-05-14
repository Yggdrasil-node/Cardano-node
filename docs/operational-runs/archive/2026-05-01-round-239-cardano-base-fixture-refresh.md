## Round 239 - Phase E.1 cardano-base fixture refresh

Date: 2026-05-01
Phase: E.1 upstream pin maintenance
Scope: documentation/pin/fixture refresh, no runtime behavior change

### Summary

R239 closes the remaining Phase E.1 upstream-maintenance gap. The
SHA-anchored `cardano-base` fixture tree now mirrors live upstream HEAD:

- Old pin: `db52f43b38ba5d8927feb2199d4913fe6c0f974d`
- New pin: `7a8a991945d401d89e27f53b3d3bb464a354ad4c`
- Upstream source: `IntersectMBO/cardano-base`
- Refreshed fixture paths:
  - `cardano-crypto-praos/test_vectors`
  - `cardano-crypto-class/bls12-381-test-vectors/test_vectors`

The refresh was coordinated across the vendored fixture directory,
`crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`,
`node/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT`, and the
project status/provenance docs. The vector files are content-identical
at this upstream advance; the directory name and pins still move in
lockstep so future fixture drift remains visible.

### Drift detector

Command:

```sh
node/scripts/check_upstream_drift.sh
```

Result:

```text
cardano-base            in-sync  7a8a991945d4
cardano-ledger          in-sync  42d088ed84b7
ouroboros-consensus     in-sync  b047aca4a731
ouroboros-network       in-sync  0e84bced45c7
plutus                  in-sync  4cd40a14e364
cardano-node            in-sync  799325937a45

[summary] drifted=0 unreachable=0 total=6
```

### Verification

Commands run at the R239 slice boundary:

```sh
cargo test -p yggdrasil-crypto upstream
cargo test -p yggdrasil-node upstream_pins
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
git diff --check
git diff --cached --check
```

Results:

- `cargo test -p yggdrasil-crypto upstream`: 5 passed, 0 failed.
- `cargo test -p yggdrasil-node upstream_pins`: 3 passed, 0 failed.
- `cargo fmt --all -- --check`: clean.
- `cargo check-all`: clean.
- `cargo test-all`: clean; all workspace tests and doctests passed.
- `cargo lint`: clean.
- Diff whitespace checks: clean.

### Status impact

- Phase E.1 is closed for all 6 canonical IntersectMBO repositories.
- `node/scripts/check_upstream_drift.sh` now reports no drift.
- Remaining production-readiness gates are operator-time only:
  long-duration mainnet rehearsal and runbook section 6.5 parallel
  BlockFetch default-flip sign-off.
