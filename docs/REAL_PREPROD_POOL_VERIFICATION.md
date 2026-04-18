# Real Preprod Pool Verification

This workflow verifies yggdrasil block-producer runtime behavior against the real preprod network using operator-provided pool credentials.

## Prerequisites

- Real, already-registered preprod pool credentials:
  - KES signing key (text envelope)
  - VRF signing key (text envelope)
  - Node operational certificate (text envelope)
  - Issuer cold verification key (text envelope)
- Pool has active stake/delegation on preprod if you want forged blocks to appear/adopt.
- yggdrasil binary is built:
  - `cargo build -p yggdrasil-node`
- Cardano CLI binaries are staged at `/tmp/cardano-bin` (or override `CARDANO_BIN_DIR`).

## Run Verification

```bash
KES_SKEY_PATH=/abs/path/kes.skey \
VRF_SKEY_PATH=/abs/path/vrf.skey \
OPCERT_PATH=/abs/path/node.cert \
ISSUER_VKEY_PATH=/abs/path/cold.vkey \
node/scripts/run_preprod_real_pool_producer.sh
```

Strict mode for active pools (longer observation window):

```bash
KES_SKEY_PATH=/abs/path/kes.skey \
VRF_SKEY_PATH=/abs/path/vrf.skey \
OPCERT_PATH=/abs/path/node.cert \
ISSUER_VKEY_PATH=/abs/path/cold.vkey \
RUN_SECONDS=900 \
EXPECT_FORGE_EVENTS=1 \
EXPECT_ADOPTED_EVENTS=1 \
node/scripts/run_preprod_real_pool_producer.sh
```

## What The Script Verifies

- `Startup.BlockProducer` observed
- block producer loop started
- no `invalid VRF proof` errors
- at least one preprod bootstrap connection observed
- when `EXPECT_FORGE_EVENTS=1`: leader/forge evidence (`elected as slot leader` or forged/adopted events)
- when `EXPECT_ADOPTED_EVENTS=1`: at least one `adopted forged block` event
- the node remains alive for the full `RUN_SECONDS` window (early exit is treated as failure)
- evidence summary counters are printed at the end (`leaders`, `forged`, `adopted`, `notAdopted`)

## Notes

- This verifies runtime producer wiring and network integration.
- Actual forged/adopted blocks require real registered stake-pool credentials with active stake on preprod.
- If `peer-snapshot.json` is absent in the preprod config directory, warning logs are expected and non-fatal.

## Rust cardano-cli Integration

`yggdrasil-node` now exposes a Rust-wrapped upstream `cardano-cli` command group that resolves upstream reference config paths by network preset.

Examples:

```bash
# Print upstream cardano-cli version
cargo run -p yggdrasil-node -- \
  cardano-cli --network preprod version

# Show resolved upstream reference config + topology + network magic
cargo run -p yggdrasil-node -- \
  cardano-cli --network preprod show-upstream-config

# Query tip through cardano-cli using the node socket and upstream magic
cargo run -p yggdrasil-node -- \
  cardano-cli --network preprod query-tip \
  --socket-path /tmp/yggdrasil-preprod-real-pool.socket
```

Path resolution order for upstream references:

- `--upstream-config-root <root>` when provided
- `/tmp/cardano-tooling/share/<network>` (official release layout)
- fallback: vendored `node/configuration/<network>`
