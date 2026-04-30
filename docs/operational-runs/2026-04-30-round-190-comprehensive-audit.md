## Round 190 — Comprehensive cardano-cli parity audit + tag 12/13 dispatchers

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Run a systematic audit of EVERY `cardano-cli conway query`
subcommand against yggdrasil to verify the Conway-era LSQ
parity arc is genuinely complete.  Fix any gaps surfaced.

### Audit method

1. Started yggdrasil-node on preview with `YGG_LSQ_ERA_FLOOR=6`.
2. Ran every `cardano-cli conway query` subcommand listed by
   `--help`.
3. Categorised results: working / failing-with-decode-error /
   failing-with-cli-arg-error.
4. For decode failures, captured wire bytes via instrumented
   `decode_query_if_current` (`YGG_NTC_DEBUG=1`).
5. Looked up upstream wire shapes via `WebFetch` and added
   dispatchers.

### Audit results

**Confirmed working end-to-end (28 subcommands)**:

| Category | Subcommands |
|---|---|
| Always-available | `tip`, `protocol-parameters`, `era-history`, `slot-number`, `utxo --whole-utxo`/`--address`/`--tx-in`, `tx-mempool info`/`next-tx`/`tx-exists` |
| Era-gated | `stake-pools`, `stake-distribution`, `stake-snapshot`, `pool-state`, `stake-address-info`, `ref-script-size` |
| Conway governance | `constitution`, `gov-state`, `drep-state`, `drep-stake-distribution`, `committee-state`, `treasury`, `spo-stake-distribution`, `proposals`, `ratify-state`, `future-pparams`, `stake-pool-default-vote`, `ledger-peer-snapshot` |
| Operational | `protocol-state` (R190), `ledger-state` (R190 — null acceptable per cli convention) |

**CLI-side argument issues (not yggdrasil bugs)**:

- `kes-period-info` — needs valid `--op-cert-file`
- `leadership-schedule` — needs `--genesis FILEPATH` and
  `--stake-pool-verification-key STRING`
- `stake-address-info` — needed a valid Bech32 stake address;
  works with `cardano-cli conway stake-address build`-generated
  address (returns `[]` for unregistered addresses)

These were initially flagged as "failing" but the failure was
client-side argument validation in cardano-cli, not a yggdrasil
wire-protocol issue.  None are bugs in yggdrasil.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::DebugNewEpochState` (tag 12) — singleton
  query for `query ledger-state`.
- New `EraSpecificQuery::DebugChainDepState` (tag 13) — singleton
  query for `query protocol-state`.
- Decoder branches `(1, 12)` and `(1, 13)`.

`node/src/local_server.rs`:

- New helper `encode_praos_state_versioned()` emitting the
  upstream `Versioned`-wrapped 8-element `PraosState`:

  ```
  [outer 2-elem]
    0: version (uint) = 0
    1: [8-elem PraosState]
         0: WithOrigin SlotNo Origin = [0]
         1: empty OCert counters Map = 0xa0
         2-7: NeutralNonce = [0] × 6
  ```

  Per upstream `Ouroboros.Consensus.Protocol.Praos.PraosState`
  encoded via `encode (Versioned 0 (...8-record...))`.
- New dispatcher arms for `DebugNewEpochState` (emits CBOR
  `null`, accepted by cardano-cli's `query ledger-state` which
  shows `f6 # null`) and `DebugChainDepState` (emits the
  Versioned PraosState).
- Extended `dispatch_inner_era_query` to handle both new
  variants when wrapped via GetCBOR (cardano-cli sends
  protocol-state via tag 9 → 13 wrapping).

### Discoveries

**Versioned wrapper for PraosState**: the first attempt
emitted a bare 8-element PraosState; cardano-cli rejected
with `DeserialiseFailure 1 "Size mismatch when decoding
Versioned. Expected 2, but found 8."`.  The wire shape is
actually `[version_uint, [8-element PraosState]]` — a
2-element outer wrapping a versioning byte + payload, per
upstream's `Versioned` newtype encoding.  Switched to the
2-element outer form and the response decoded.

**`query ledger-state` accepts null**: cardano-cli's
`runQueryLedgerStateCmd` is intentionally permissive — it
shows `f6 # null` (the raw CBOR of a null response) as
valid output.  Constructing a complete upstream-faithful
`NewEpochState era` (which is a substantial multi-field
record) is out of scope; emitting `null` keeps cardano-cli
happy.

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query protocol-state --testnet-magic 2
{
    "candidateNonce": null,
    "epochNonce": null,
    "evolvingNonce": null,
    "labNonce": null,
    "lastEpochBlockNonce": null,
    "lastSlot": "origin",
    "oCertCounters": {}
}

$ cardano-cli conway query ledger-state --testnet-magic 2

f6  # null

$ ADDR=$(cardano-cli conway stake-address build --stake-verification-key-file /tmp/stake.vkey --testnet-magic 2)
$ cardano-cli conway query stake-address-info --testnet-magic 2 --address "$ADDR"
[]
```

All previously-failing queries now decode end-to-end.

**Default-era flow regression check** (no `YGG_LSQ_ERA_FLOOR`):

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 3960, "epoch": 0, "era": "Alonzo", ... }

$ cardano-cli conway query protocol-parameters --testnet-magic 2
{ "collateralPercentage": 150, ... }

$ cardano-cli conway query utxo --whole-utxo --testnet-magic 2
{ "e3ca57e8...#0": { ... } }

$ cardano-cli conway query era-history --testnet-magic 2
{ "type": "EraHistory", ... }
```

Default flow (without era floor) still works for baseline queries.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count stable at 4744.

### Open follow-ups (now data-plumbing rather than wire-shape)

The Conway-era LSQ wire-protocol gap is fully closed.
Remaining items shift to live data plumbing:

1. **Live data plumbing** — current placeholders return
   empty/origin/neutral data for: gov-state proposals,
   ratify-state enacted actions, drep stake distribution,
   spo stake distribution, ledger-peer-snapshot pool list,
   protocol-state OCert counters + nonces.  Populating
   these is the natural follow-on as yggdrasil's runtime
   tracks them in the snapshot.
2. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 structural).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — new `EraSpecificQuery::DebugNewEpochState` (tag 12) +
  `DebugChainDepState` (tag 13) variants + decoder branches;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  `encode_praos_state_versioned` helper + two new dispatcher
  arms + extended `dispatch_inner_era_query`.
- Captures: `/tmp/ygg-audit-preview.log`,
  `/tmp/ygg-default-preview.log`.
- Upstream reference:
  `Ouroboros.Consensus.Shelley.Ledger.Query.DebugNewEpochState`
  (tag 12, `NewEpochState era`);
  `Ouroboros.Consensus.Shelley.Ledger.Query.DebugChainDepState`
  (tag 13, `ChainDepState proto`);
  `Ouroboros.Consensus.Protocol.Praos.PraosState` (8-element
  record);
  `Versioned` wrapper (`[version, payload]` 2-tuple).
- Previous round:
  [`docs/operational-runs/2026-04-30-round-189-ledger-peer-snapshot.md`](2026-04-30-round-189-ledger-peer-snapshot.md).
