# Round 245 - cardano-ledger BBODY/GOV drift refresh

Date: 2026-05-01

## Summary

`node/scripts/check_upstream_drift.sh` found a fresh `cardano-ledger`
drift after R244:

- pinned: `110b30e7abd8f507ea21625f8ac06fb6c8b66768`
- live HEAD: `b90b97488da3cbdc01c5c4a610c674a22d467882`

The upstream range contains two behavior-relevant Conway changes:

1. GOV consistency cleanup: `preceedingHardFork` now reads accumulated
   proposals instead of the original state.
2. BBODY header protocol-version policy: `HeaderProtVerTooHigh` is
   temporarily disabled for testnets until Dijkstra, then re-enabled once
   `curProtVerMajor >= 12`; mainnet remains strict.

## Local Impact

The GOV drift did not require a code change. Yggdrasil's
`validate_conway_proposals()` already validates hard-fork sequencing
against the accumulated pending-proposal view, and the hard-fork
sequencing tests remain green.

The BBODY drift required a sync verifier update:

- `VerificationConfig` now carries `network_magic: Option<u32>`.
- `verify_multi_era_block_with_raw()` enforces `HeaderProtVerTooHigh`
  only when `network_magic` is mainnet or the current ledger protocol
  parameter major is at least `12`.
- `MaxMajorProtVer` remains enforced independently on every network.
- `node/src/upstream_pins.rs::UPSTREAM_CARDANO_LEDGER_COMMIT` now points
  at `b90b97488da3cbdc01c5c4a610c674a22d467882`.

## Focused Verification

```text
cargo test -p yggdrasil-node header_protocol_version_window --lib
4 passed; 0 failed

cargo test -p yggdrasil-ledger hard_fork --lib
14 passed; 0 failed
```

Full workspace gates:

```text
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
node/scripts/check_upstream_drift.sh
git diff --check
```

All passed. The final drift detector snapshot was:

```text
[summary] drifted=0 unreachable=0 total=6
```

## Upstream References

- Compare from R243 pin to live HEAD:
  <https://github.com/IntersectMBO/cardano-ledger/compare/110b30e7abd8f507ea21625f8ac06fb6c8b66768...master>
- GOV cleanup commit:
  <https://github.com/IntersectMBO/cardano-ledger/commit/d7462d86ed08e83a66107579d0bbe47b88914372>
- BBODY testnet grace commit:
  <https://github.com/IntersectMBO/cardano-ledger/commit/146fe56d0c22e1b22d8bf2fc0296b1894e5fd5bf>
- Merge commit pinned after R245:
  <https://github.com/IntersectMBO/cardano-ledger/commit/b90b97488da3cbdc01c5c4a610c674a22d467882>
