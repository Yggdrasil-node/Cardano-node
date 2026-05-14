# Round 244 - Byron genesis hash verification

Date: 2026-05-01
Phase: operator-trust parity / config integrity
Scope: node config preflight, genesis hash verification, documentation

### Summary

Yggdrasil previously verified Shelley, Alonzo, and Conway genesis hashes
over raw file bytes, while Byron was limited to hex syntax checks. This
left `validate-config` reporting `3/4 verified` and required a special
case in the startup trace.

The upstream source path is different for Byron:

- `cardano-node` `Cardano.Node.Protocol.Byron.readGenesis` calls
  `Cardano.Chain.Genesis.readGenesisData` and compares the returned
  `GenesisHash` against `ByronGenesisHash`.
- `cardano-ledger` `Cardano.Chain.Genesis.Data.readGenesisData` parses
  the file with `Text.JSON.Canonical.parseCanonicalJSON` and computes
  `GenesisHash $ hashRaw (renderCanonicalJSON genesisDataJSON)`.

This round ports that behavior locally. Byron now hashes Canonical JSON
rendering before Blake2b-256; Shelley, Alonzo, and Conway keep the
upstream raw-file Blake2b-256 path.

### Changes

- `node/src/genesis.rs`
  - Added `compute_byron_genesis_file_hash()` and
    `verify_byron_genesis_file_hash()`.
  - Added a small Canonical JSON parser/renderer for the subset used by
    upstream Byron genesis files, preserving duplicate object keys and
    sorting object fields only during rendering.
  - Added regression tests for canonical rendering, non-canonical
    escapes, raw control bytes in strings, and all three vendored Byron
    genesis hashes.
- `node/src/config.rs`
  - `verify_known_genesis_hashes()` now verifies Byron first, then
    Shelley/Alonzo/Conway.
  - Unpaired file/hash hard-fail behavior is unchanged.
- `node/src/main.rs`
  - `Node.GenesisHash.Verified` now reports `byronVerified` and counts
    Byron in `verifiedCount`.
  - `validate-config` no longer carries the "Byron pending" nuance.
- Documentation now reports `Genesis hashes: 4/4 verified`.

### Vendored Byron hashes

| Network | `ByronGenesisHash` |
|---|---|
| mainnet | `5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb` |
| preprod | `d4b8de7a11d929a323373cbab6c1a9bdc931beffff11db111cf9d57356ee1937` |
| preview | `83de1d7302569ad56cf9139a41e2e11346d4cb4a31c00142557b6ab3fa550761` |

### Verification

Focused checks:

```sh
cargo test -p yggdrasil-node genesis --lib
cargo test -p yggdrasil-node validate_config_report --bin yggdrasil-node
```

Full gates:

```sh
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
node/scripts/check_upstream_drift.sh
git diff --check
```

### Upstream references

- `cardano-node` Byron loader:
  <https://github.com/IntersectMBO/cardano-node/blob/799325937a4598899c8cab61f4c957662a0aeb53/cardano-node/src/Cardano/Node/Protocol/Byron.hs>
- `cardano-ledger` Byron genesis hash:
  <https://github.com/IntersectMBO/cardano-ledger/blob/110b30e7abd8f507ea21625f8ac06fb6c8b66768/eras/byron/ledger/impl/src/Cardano/Chain/Genesis/Data.hs>

### Status impact

- Startup/preflight now verifies all four configured preset genesis
  hashes.
- No new dependency was introduced.
- Operator-time gates remain unchanged: runbook §6.5 BlockFetch
  sign-off and the §2-9 mainnet endurance rehearsal.
