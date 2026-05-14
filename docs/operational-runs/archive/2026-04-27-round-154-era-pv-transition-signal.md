## Round 154 — Era-PV pairing admits hard-fork transition signal

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close Round 153's open follow-up #1: loosen yggdrasil's strict
era_tag/protocol-version pairing so preview's at-genesis hard-fork
state syncs end-to-end instead of being rejected with
`expected major in 5..=6`.

### Background

Upstream Cardano's hard-fork combinator (`Ouroboros.Consensus.
Cardano.CanHardFork`) bumps the protocol-version major *within*
era N via an in-band protocol-parameters update to signal that
era N+1 will activate at the next epoch boundary.  Concretely:

- `shelleyTransition = 2`
- `allegraTransition = 3`
- `maryTransition = 4`
- `alonzoTransition = 5`
- `babbageTransition = 7`  (skips 6)
- `conwayTransition = 9`

The LAST block of era N can carry the next era's transition
major because the chain is in mid-transition state.  Preview's
`Test*HardForkAtEpoch=0` testnet configuration produces this
state immediately at chain genesis (the first Alonzo-codec block
carries PV major=7 = Babbage signal).

### Pre-fix symptom

Round 153's preview operational verification log showed:

```
Error ChainDB.AddBlockEvent.InvalidBlock node=yggdrasil-preview
  peer sent an invalid block; disconnecting currentPoint=Origin
  error=protocol version mismatch: block in era Alonzo carries
        version (7, 2), expected major in 5..=6
  peer=3.134.226.73:3001
```

Yggdrasil's `validate_protocol_version_for_era` enforced exact
intra-era ranges:

```rust
Era::Alonzo => (major == 5 || major == 6, "5..=6"),
Era::Babbage => (major == 7 || major == 8, "7..=8"),
```

This was a yggdrasil-specific defensive check; upstream's
`Cardano.Protocol.Praos.Rules.Prtcl.headerView` only enforces the
`MaxMajorProtVer` ceiling, with the era-pairing being an HFC
type-level invariant rather than a runtime check.

### Fix

`node/src/sync.rs::validate_protocol_version_for_era` now admits
each era's intra-era range PLUS the next era's transition major:

```rust
Era::Shelley => (major == 2 || major == 3, "2..=3"),
Era::Allegra => (major == 3 || major == 4, "3..=4"),
Era::Mary    => (major == 4 || major == 5, "4..=5"),
Era::Alonzo  => ((5..=7).contains(&major), "5..=7"),
Era::Babbage => ((7..=9).contains(&major), "7..=9"),
Era::Conway  => (major >= 9, "9+"),
```

The `MaxMajorProtVer` ceiling delegation to
`yggdrasil_consensus::check_header_protocol_version` is unchanged
— that's the canonical PRTCL rule and remains the primary
defensive gate.

### Regression tests

`node/src/sync.rs`:

- `protocol_version_constraints_enforce_alonzo_era_gate` —
  updated to assert `Alonzo + PV(7,0)` succeeds (Babbage
  transition signal).  Retains pre-Alonzo (`PV 4`) and
  post-transition (`PV 8`) rejection assertions.
- `protocol_version_constraints_enforce_babbage_era_gate` —
  new test pinning `Babbage + PV(9,0)` Conway transition signal
  acceptance and `PV(6,0)` / `PV(10,0)` rejections.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4688  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4687 (Round 153) → 4688.

### Operational verification

**Preview — past the PV-mismatch blocker**

Pre-Round-154: rejected at `currentPoint=Origin` with `expected
major in 5..=6`.

Post-Round-154: yggdrasil now accepts the first Alonzo-codec
block with PV major=7 and proceeds to apply transactions.  The
next blocker surfaces:

```
Error Node.Sync node=yggdrasil-preview node run failed
  error=ledger decode error: fee too small: minimum 237837 lovelace,
        declared 237793
  primaryPeer=52.211.202.88:3001
```

Difference: exactly 44 lovelace = `minFeeA × 1 byte`.  This
indicates yggdrasil's tx CBOR re-encoding produces a 1-byte-
different size from upstream — a separate, deeper CBOR
canonicalization parity gap unrelated to the era-PV check.
Documented as Round 154 follow-up #1.

**Preprod — no regression**

Post-Round-154 preprod knob=2 ~30s soak:

```json
{
    "block": 87340,
    "epoch": 4,
    "era": "Shelley",
    "hash": "7350c51d6787f9eca1bb18f73f80ca164daaa0009068247c8d4fc95bdaddafb4",
    "slot": 87340,
    "slotInEpoch": 940,
    "slotsToEpochEnd": 431060,
    "syncProgress": "1.40"
}
```

Same shape as Round 153's preprod baseline — the relaxed era-PV
pairing didn't break preprod's strict-pairing chain.

### Open follow-ups

1. **Byte-perfect CBOR tx-size parity** — preview's first
   transactions trip `validateFeeTooSmallUTxO` because yggdrasil's
   `AlonzoTxBody`/`AlonzoBlock` re-encoding produces a 1-byte-
   different size from upstream.  Closing this requires aligning
   yggdrasil's CBOR encoder canonicalization (key order, integer
   length form, set/map encoding) with `Cardano.Ledger.Alonzo.Tx`.
2. **Era-summary auto-derivation** — Round 153 follow-up #2 still
   open: emit Allegra+ summaries based on
   `LedgerStateSnapshot.current_era` instead of hardcoded
   preprod/preview shapes.
3. **NetworkPreset auto-detection from genesis hash** —
   currently selects from `network_magic`; deriving from the
   loaded genesis-hash set would let custom-magic operators get
   correct era-history without explicit configuration.

### Diagnostic captures

- `/tmp/ygg-preview-r154.log` — preview run log showing
  past-PV-mismatch progress and the new fee-calculation blocker.
- `/tmp/ygg-verify-cli-tip-r154.txt` — preprod cardano-cli
  output post-Round-154 (regression baseline).

### References

- `Ouroboros.Consensus.Cardano.CanHardFork`
- `Cardano.Protocol.Praos.Rules.Prtcl.headerView`
- `Cardano.Ledger.Shelley.Rules.Utxo.validateFeeTooSmallUTxO`
- Previous round: `docs/operational-runs/2026-04-27-round-153-network-aware-interpreter.md`
- Code: `node/src/sync.rs::validate_protocol_version_for_era`
