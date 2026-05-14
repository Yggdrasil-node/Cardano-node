## Round 160 — Babbage PParams + PV-aware era classification

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Continue cardano-cli operational parity by:

1. Adding the **Babbage PP shape** (22-element list, drops
   `d`/`extraEntropy`, renames `coinsPerUtxoWord` →
   `coinsPerUtxoByte`) so yggdrasil can serve `query
   protocol-parameters` for any snapshot reporting era_index=5.
2. Adding **PV-aware era classification** so yggdrasil's LSQ
   reports the chain's active era driven by the latest block
   header's protocol version, not just the wire-format era_tag.
   Upstream's hard-fork combinator uses PV major to determine the
   canonical era; the wire era_tag tracks the codec used to
   encode the block but can lag the active era during a
   transition.

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `encode_babbage_pparams_for_lsq(params)` emits the upstream
  22-element list — same as Alonzo minus `d` and `extraEntropy`,
  using `coinsPerUtxoByte` directly (Babbage's name; the value
  is `coinsPerUtxoWord / 8`).

`crates/ledger/src/tx.rs`:

- New `BlockHeader::protocol_version: Option<(u64, u64)>` field.
  Populated by all era-specific `*_block_to_block_with_spans`
  conversions in `node/src/sync.rs` from each era's
  `header.body.protocol_version`.

`crates/ledger/src/state.rs`:

- `LedgerState` gains `latest_block_protocol_version:
  Option<(u64, u64)>`, set in `apply_block_validated` after every
  block apply.
- `LedgerStateSnapshot::latest_block_protocol_version()` accessor
  exposes it to the LSQ dispatcher.

`node/src/local_server.rs`:

- New helper `effective_era_index_for_lsq` maps PV major → era_index
  per upstream's `*Transition` ProtVer table:
  - PV 1 → Byron (0)
  - PV 2 → Shelley (1)
  - PV 3 → Allegra (2)
  - PV 4 → Mary (3)
  - PV 5–6 → Alonzo (4)
  - PV 7–8 → Babbage (5)
  - PV 9+ → Conway (6)
- Promotes the snapshot's wire-era_tag-derived era to the higher
  of (wire vs PV-derived).  Wired into `GetCurrentEra` response
  and `QueryIfCurrent` era-mismatch comparisons.
- Dispatcher PP encoder branches: 1..=3 Shelley, 4 Alonzo,
  5 Babbage, 6+ null (Conway pending).

### Test scaffolding updates

30+ test files (`crates/ledger/tests/integration/*.rs`,
`node/tests/runtime.rs`, `node/tests/sync.rs`,
`crates/storage/tests/integration.rs`,
`node/src/sync.rs`, `node/src/server.rs`, `node/src/block_producer.rs`,
`node/src/runtime.rs`) updated via bulk `perl -i -0pe`
substitution to add `protocol_version: None,` in
`tx::BlockHeader` constructors.  Production sites
(`shelley_block_to_block`, `alonzo_family_block_to_block_with_spans!`
macro, block producer, server's BlockFetch handler) populate
from the real header PV.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4701  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4701 (Round 159) → 4701 (no new tests
this round; the era-promotion changes are exercised via the
existing PP/UTxO operational tests).

### Operational verification

**Preprod era progression**

```
$ cardano-cli query tip --testnet-magic 1
{
    "block": 86640,
    "epoch": 4,
    "era": "Allegra",
    "hash": "7e3acb330bd35cdcd0ea0df5187ff2fdc0f52ce5d9d06a1e2557c318b8a34636",
    "slot": 86640,
    "slotInEpoch": 240,
    "slotsToEpochEnd": 431760,
    "syncProgress": "1.40"
}
```

Pre-Round-160 yggdrasil reported `era: Shelley` (using the wire
era_tag).  Post-Round-160 it reports `era: Allegra` because the
first non-Byron block on preprod has PV major=3 (the Allegra
transition signal).  This matches upstream cardano-node's
behaviour and is the canonical Cardano semantics.

**Preview at Alonzo intra-era**

```
$ cardano-cli query tip --testnet-magic 2
{
    "block": 4160,
    "era": "Alonzo",
    ...
}
```

Preview's chain at slot ~4160 has PV=(6, 0) (intra-era Alonzo,
post-`Test*HardForkAtEpoch=0` activation).  Reaching Babbage
requires syncing further until the chain's PV bumps to 7 at the
first epoch boundary.  Once that happens, the existing
infrastructure will:
- `effective_era_index_for_lsq` maps PV 7 → era_index=5 (Babbage)
- `GetCurrentPParams` dispatcher emits Babbage 22-element shape
- cardano-cli's per-era query gating unblocks `query stake-pools`
  / `query stake-distribution` / etc.

**No regression**

All 10 cardano-cli operations work on preprod (tip,
protocol-parameters, utxo --whole-utxo / --address / --tx-in,
era-history, tx-mempool info / next-tx / tx-exists, submit-tx).

### Open follow-ups

1. Conway PP shape (adds DRep/governance/committee fields and
   tiered ref-script fees per
   `Cardano.Ledger.Conway.PParams.encCBOR`).
2. Regression test pinning the PV→era_index table.
3. Promote `LedgerStateSnapshot::current_era()` itself (currently
   only LSQ-dispatch promotes; internal era classification stays
   at wire era_tag value).

### References

- `Cardano.Ledger.Babbage.PParams.encCBOR` — 22-element shape.
- `Ouroboros.Consensus.Cardano.CanHardFork` — `*Transition`
  ProtVer table.
- Previous round: `docs/operational-runs/2026-04-28-round-159-alonzo-pparams.md`.
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `crates/ledger/src/tx.rs`, `crates/ledger/src/state.rs`,
  `node/src/local_server.rs`, `node/src/sync.rs`.
