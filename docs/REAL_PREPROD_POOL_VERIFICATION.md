---
title: Real Preprod Pool Verification
layout: default
parent: Reference
nav_order: 9
---

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

`yggdrasil-node` now exposes a pure-Rust `cardano-cli` command group that resolves reference config paths by network preset.

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

---

## 2026-04-27 — Live preprod sync rehearsal

### What we ran

A 30-minute headless sync against canonical preprod relay
`3.126.235.206:3001`, no operator credentials, just BlockFetch +
ChainSync + ledger validation:

```bash
WORK=$(mktemp -d -t ygg-preprod-XXXXXX)
NTN_PORT=$(awk 'BEGIN { srand(); print 30200 + int(rand()*100) }')
METRICS_PORT=$(awk 'BEGIN { srand(); print 31200 + int(rand()*100) }')
./target/release/yggdrasil-node run \
  --network preprod \
  --database-path "$WORK/db" \
  --port $NTN_PORT \
  --host-addr 127.0.0.1 \
  --metrics-port $METRICS_PORT \
  --socket-path "$WORK/ygg.sock"
```

### Initial finding (before fix)

The first run **crashed at preprod slot ≈ 518 460** (start of epoch 5,
the Byron→Shelley boundary):

```
Error: ledger decode error: fee too small: minimum 208269 lovelace,
       declared 207829
```

A 440-lovelace (~0.2%) gap between Yggdrasil's computed `min_fee` and
the actual fee declared on a real preprod transaction the upstream
Haskell node accepted.

### Root cause

`min_fee = a · txSize + b`.  `txSize` upstream
([`Cardano.Ledger.Shelley.Tx.minfee`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Tx.hs))
is the byte length of the **on-wire** transaction encoding.

Yggdrasil's `*_block_to_block` converters in
[`node/src/sync.rs`](https://github.com/yggdrasil-node/Cardano-node/blob/main/node/src/sync.rs)
were re-serialising the typed `ShelleyTxBody` / `ShelleyWitnessSet`
values to compute `tx_body_size`:

```rust
// BEFORE
let raw = tx_body.to_cbor_bytes();   // re-encoded — NOT on-wire bytes
Tx { id: compute_tx_id(&raw), body: raw, witnesses: ws.map(|w| w.to_cbor_bytes()), ... }
```

CBOR has multiple byte-canonical encodings for the same logical value
(definite vs indefinite-length collections, set wrappers vs bare
arrays, integer-width canonicalisation).  When the on-wire encoding
chosen by the block author differed from our re-encoding, our `txSize`
came out 10 bytes too long → `min_fee` came out 440 lovelace too high
(`44 × 10 = 440`, with `min_fee_a = 44`).

### Fix

Capture each transaction's exact on-wire byte span at decode time and
use that for `tx_id` hashing and `tx_size` fee computation:

1. New helper [`yggdrasil_ledger::extract_block_tx_byte_spans`](../crates/ledger/src/cbor.rs)
   walks the outer block CBOR (`[header, [* tx_body], [* witness_set],
   …]`) using `dec.position()` markers and returns
   `BlockTxRawSpans { bodies: Vec<Vec<u8>>, witness_sets: Vec<Vec<u8>> }`
   — bytes-for-byte identical to what the block author serialised.
2. The four era converters now take `raw_block_bytes: &[u8]` and
   populate each `Tx { body, witnesses }` from the spans rather than
   from `to_cbor_bytes()`:
   - `shelley_block_to_block`
   - `alonzo_block_to_block`
   - `babbage_block_to_block`
   - `conway_block_to_block`
3. `multi_era_block_to_block` and the `TypedSyncStep::RollForward`
   variant carry raw block bytes alongside the typed values, sourced
   from BlockFetch's existing `request_range_collect_points_raw_with`
   API.
4. Four regression tests in [`crates/ledger/src/cbor.rs`](../crates/ledger/src/cbor.rs)
   exercise the helper, including a deliberately-mismatched
   indefinite-length-array case proving the helper returns the on-wire
   bytes (not what `to_cbor_bytes()` would emit).

### Verification

After the fix, all 4 634 workspace tests pass (4 630 baseline + 4 new
regression tests).  A repeat preprod sync then runs past the previous
crash point:

| Run | Outcome |
|---|---|
| Pre-fix | Crashed at slot ≈ 518 460 (epoch 5 boundary, first Shelley tx) with `FeeTooSmall { minimum: 208269, declared: 207829 }` |
| Post-fix | Cleared the boundary, applied the previously-failing transaction, continued syncing into the Shelley era |

### Audit linkage

This bug was **not** flagged in [`docs/code-audit.md`](code-audit.md)
because the audit's static-review pass did not exercise live preprod
block validation across the Byron→Shelley boundary; it surfaced only
during the operational quality-check pass on 2026-04-27.  The audit
finding M-6 (`saturating → checked` arithmetic in
[`crates/ledger/src/utxo.rs`](../crates/ledger/src/utxo.rs)) addressed
a parallel-but-distinct concern (value preservation, not fee
calculation).

