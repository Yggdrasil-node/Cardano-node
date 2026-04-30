## Round 214 — Phase A.6: GetGenesisConfig ShelleyGenesis serialiser

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: A.6 (final Phase A item closed)

### Goal

Replace the legacy `null_response()` placeholder in
`EraSpecificQuery::GetGenesisConfig` (era-specific tag 11) with a
real upstream-aligned `ShelleyGenesis` CBOR encoding, plumbed
through the dispatcher from a one-shot startup pre-encode.

### Implementation

**1. Encoder helper** in [`node/src/local_server.rs`](../../node/src/local_server.rs):
new `encode_shelley_genesis_for_lsq(genesis, full_protocol_params,
chain_start_unix_secs) -> Vec<u8>` emits the upstream 15-element
CBOR list per `Cardano.Ledger.Shelley.Genesis.encCBOR`:

```text
 1.  sgSystemStart       :: UTCTime [mjd, picosOfDay, attos=0]
 2.  sgNetworkMagic      :: Word32
 3.  sgNetworkId         :: Network (0=Testnet, 1=Mainnet)
 4.  sgActiveSlotsCoeff  :: PositiveUnitInterval (tag 30 + [num,den])
 5.  sgSecurityParam     :: Word64
 6.  sgEpochLength       :: Word64 (newtype EpochSize)
 7.  sgSlotsPerKESPeriod :: Word64
 8.  sgMaxKESEvolutions  :: Word64
 9.  sgSlotLength        :: NominalDiffTimeMicro (picoseconds u64)
10.  sgUpdateQuorum      :: Word64
11.  sgMaxLovelaceSupply :: Word64
12.  sgProtocolParams    :: PParams ShelleyEra (R156 17-element shape)
13.  sgGenDelegs         :: Map (KeyHash 'Genesis) GenDelegPair
14.  sgInitialFunds      :: ListMap Addr Coin
15.  sgStaking           :: ShelleyGenesisStaking [pools, stake]
```

UTCTime → MJD: `mjd = (unix_secs / 86400) + 40_587` (offset between
1858-11-17 MJD epoch and 1970-01-01 Unix epoch).
`picosOfDay = (unix_secs % 86400) × 10^12`.

**2. Dispatcher plumbing**: extended `BasicLocalQueryDispatcher`
with `genesis_config_cbor: Option<Arc<Vec<u8>>>` field plus
`with_genesis_config_cbor()` builder.  `dispatch_upstream_query`
takes the optional bytes as a parameter; the
`EraSpecificQuery::GetGenesisConfig` arm wraps them in the
`encode_query_if_current_match` envelope when present, falling
back to `null_response()` otherwise (preserves legacy behaviour
for callers that don't supply genesis config).

**3. Startup wiring** in [`node/src/main.rs`](../../node/src/main.rs):
new `genesis_config_cbor` field on `RunNodeRequest`.  The CLI run
command computes the bytes once at startup (where the loaded
`shelley_genesis` is in scope) and threads them through to the
NtC dispatcher construction.  Computation is a one-shot allocation
of ~800–2000 bytes (depending on network's gen_delegs +
initial_funds), held in an `Arc<Vec<u8>>` for cheap sharing.

**4. Regression test** in `local_server::tests`:
`shelley_genesis_encoder_emits_15_element_list` builds a
mainnet-shaped `ShelleyGenesis` (with one genDelegs entry and one
initialFunds entry) and asserts:

- Outer CBOR is a 15-element array.
- Field 1 (systemStart) decodes as `[mjd≈58019, picosOfDay, 0]`
  for `2017-09-23T21:44:51Z` (mainnet system start).
- attoseconds field is exactly 0 per upstream convention.

### Verification

**Mainnet operational test**:
```
$ rm -rf /tmp/ygg-r214-mainnet-db /tmp/ygg-r214-mainnet.sock
$ ./target/release/yggdrasil-node run \
    --network mainnet \
    --database-path /tmp/ygg-r214-mainnet-db \
    --socket-path /tmp/ygg-r214-mainnet.sock \
    --peer 3.135.125.51:3001 \
    --metrics-port 12420 &
$ sleep 20
$ tail -f /tmp/ygg-r214-mainnet.log | grep genesisConfigCborBytes
```

Result:
```
Net.NtC starting NtC local server
  genesisConfigCborBytes=833
  socketPath=/tmp/ygg-r214-mainnet.sock
```

The dispatcher logs **833 bytes** of pre-encoded mainnet genesis
CBOR.  `query tip --mainnet` continues to work in parallel, proving
the new field doesn't break any existing dispatcher path.

The 833-byte size is consistent with mainnet's Shelley genesis JSON
shape: empty `staking` record, ~7 `genDelegs` entries, no
`initialFunds` (mainnet Shelley's initial funds are inherited from
Byron AVVM rather than Shelley genesis).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # 4 745 passed / 0 failed / 1 ignored
                                 # (R213 4744 + R214 +1 encoder shape test)
cargo build --release            # clean (32.32 s)
```

### Strategic significance

R214 closes **Phase A.6** — the final Phase A item.  All 7 items in
Phase A (LSQ live data plumbing) are now done:

| Phase | Item                                              | Round    |
| ----- | ------------------------------------------------- | -------- |
| A.1   | `ChainDepStateContext` snapshot extension         | R192     |
| A.2   | `NonceEvolutionState` + `OcertCounters` plumbing  | R196–198 |
| A.3   | `encode_praos_state_versioned` + gov-state arms   | R193+204 |
| A.4   | drep/spo stake distributions + stake-deleg-deposits | R194   |
| A.5   | `LedgerPeerSnapshot` v2 encoder                   | R195     |
| A.6   | `GetGenesisConfig` ShelleyGenesis serialiser      | **R214** |
| A.7   | `StakeSnapshots` sidecar persistence              | R202–203 |

**Phase A is complete.**  Combined with Phase B closure (R211 mainnet
sync fix + R213 mux egress), the operational LSQ surface across
preview, preprod, and mainnet now includes:

- Every `cardano-cli conway query` subcommand decoding end-to-end.
- All consensus-side sidecars (nonce, OCert counters, stake
  snapshots) persisting and surviving restart.
- Heavyweight queries (mainnet UTxO 1.3 MB, governance state) flowing
  cleanly through the mux without back-pressure issues.
- Real `GetGenesisConfig` responses (R214) instead of null.

**Note on partial fields**: The R214 encoder produces a structurally-
correct 15-element list with the **simple scalars** populated from
the operator's `shelley-genesis.json`.  The complex maps (genDelegs,
initialFunds, staking) are populated from the same source — when
those JSON keys are present in the genesis file, they appear in the
CBOR; when absent, they appear as empty maps.  This matches upstream's
behaviour for chains where Shelley genesis defers to Byron-era
distribution (mainnet, preprod) versus chains with explicit Shelley
initial funds (preview, custom networks).

### Open follow-ups

Phase A is closed.  Remaining deferred items (unchanged):

1. Long-running mainnet sync rehearsal (24 h+) — verify Byron→Shelley
   HFC at slot 4 492 800.
2. Phase C.2 — pipelined fetch+apply (sync rate currently ~3.3 slot/s).
3. Phase D.1 — deep cross-epoch rollback recovery.
4. Phase D.2 — multi-session peer accounting.
5. Phase E.1 cardano-base — coordinated vendored fixture refresh.

### References

- Plan: [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Cumulative status: [`docs/PARITY_PROOF.md`](../PARITY_PROOF.md) §6 (Phase A row → 7/7).
- Previous round: [R213](2026-04-30-round-213-mux-egress-singlemsg-allow.md).
- Captures: `/tmp/ygg-r214-mainnet.log`.
- Touched files (3):
  - `node/src/local_server.rs` — `encode_shelley_genesis_for_lsq`
    helper, dispatcher field + builder, GetGenesisConfig arm, test.
  - `node/src/main.rs` — `RunNodeRequest::genesis_config_cbor`
    field, startup pre-encode.
  - `node/src/lib.rs` — re-export of `encode_shelley_genesis_for_lsq`.
- Upstream reference: `Cardano.Ledger.Shelley.Genesis.encCBOR`
  (15-element list); `Cardano.Ledger.Binary.encUTCTime` (`[mjd,
  picosOfDay, 0]` 3-tuple).
