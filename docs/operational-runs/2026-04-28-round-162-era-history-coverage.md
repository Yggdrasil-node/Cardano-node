## Round 162 — Era-history coverage to slot 2^48 + bignum relativeTime

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close Round 152's open follow-up #2: extend the synthetic
far-future end of every network's `Interpreter` so
`cardano-cli query slot-number` works for any realistic
timestamp.  Pre-fix: a request for `2030-06-15T00:00:00Z`
returned `Past horizon` because the synthetic Shelley summary
ended at slot 10_000_000 (~116 days post-Byron).

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- Changed `encode_relative_time` signature from `(enc, u64)` to
  `(enc, u128)` and added bignum-fallback dispatch:
  - Value fits in u64 → emit as CBOR uint (matches captured
    upstream wire for real era boundaries like Byron's eraEnd
    `1b 17fb16d83be00000`).
  - Value exceeds u64 → emit as CBOR positive-bignum (tag 2)
    via `encode_bignum_u128`.
- Bumped synthetic `SHELLEY_END_SLOT` from `10_000_000` to
  `1u64 << 48` in all three network encoders:
  - `encode_interpreter_preprod`
  - `encode_interpreter_preview`
  - `encode_interpreter_mainnet`
- Recomputed `SHELLEY_END_PICOS` as `u128` so the
  out-of-u64-range relativeTime is encoded via the bignum path.
- Mainnet's Byron eraEnd relativeTime (4.4928e18 ps was a
  workaround-capped value; real value is 8.9856e19) now emits
  the real value via bignum.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4706  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged (4706) — the bignum path is exercised
through the existing interpreter encoders which already have
byte-pinned regression tests for the Byron prefix.

### Operational verification

**`cardano-cli query slot-number` for far-future timestamps**

```
$ cardano-cli query slot-number --testnet-magic 1 2030-06-15T00:00:00Z
252028800

$ cardano-cli query slot-number --testnet-magic 1 2100-01-01T00:00:00Z
2446761600
```

Pre-Round-162 both queries returned `Past horizon` errors with
the synthesised Shelley summary ending at slot 10M.  Post-fix
both work — the synthetic far-future end at slot 2^48 covers any
date the user could realistically query.

**Regression check on preprod**

All other operations confirmed working:
- `query tip` → `epoch:4, era:Allegra, slot:86840, syncProgress:1.40`
- `query protocol-parameters` → Shelley shape (17 fields)
- `query utxo --whole-utxo` → 3 Byron-genesis bootstrap entries
- `query era-history` → era summary CBOR (now bignum-encoded for
  the synthetic Shelley far-future end)

### Cumulative cardano-cli parity (Round 162)

| Command | Status |
|---|---|
| `query tip` | ✓ |
| `query protocol-parameters` | ✓ Shelley/Alonzo/Babbage/Conway shapes |
| `query utxo --whole-utxo` | ✓ |
| `query utxo --address X` | ✓ |
| `query utxo --tx-in T#i` | ✓ |
| `query era-history` | ✓ |
| `query tx-mempool info` | ✓ |
| `query tx-mempool next-tx` | ✓ |
| `query tx-mempool tx-exists` | ✓ |
| **`query slot-number`** | **✓ (R162)** |
| `submit-tx` | ✓ |
| `query stake-pools` | client-blocked (need Babbage+ snapshot) |
| `query stake-distribution` | client-blocked |
| `query protocol-state` | client-blocked |
| `query ledger-state` | client-blocked |
| `query ledger-peer-snapshot` | client-blocked |
| `query stake-address-info` | client-blocked |

11 cardano-cli operations now work end-to-end.

### Open follow-ups

1. Babbage TxOut datum_inline/script_ref encoding — already
   correct in `BabbageTxOut::encode_cbor` but not yet exercised
   operationally because preview hasn't synced past Alonzo.
2. `query stake-address-info` — Bech32 stake-address parsing
   + tag 10 dispatcher.  Currently era-blocked client-side.
3. `LedgerStateSnapshot::current_era()` PV-aware promotion —
   currently only LSQ-dispatch promotes; internal era
   classification stays at wire era_tag value.

### References

- `Ouroboros.Consensus.HardFork.History.Summary` — `RelativeTime`
  encoding (uint when fits in u64, bignum tag 2 otherwise).
- Previous round: `docs/operational-runs/2026-04-28-round-161-conway-pparams.md`.
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`.
