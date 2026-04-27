## Round 153 — Network-aware Interpreter / SystemStart for preview/preprod/mainnet

Date: 2026-04-27
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close Round 152's open follow-up #3: parameterise the LSQ
`Interpreter` and `SystemStart` outputs by the runtime-selected
Cardano network so `cardano-cli query tip` reports correct
epoch/slot math against preview, preprod, and mainnet — not just
the preprod values that were hardcoded in Round 152.

### Changes

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `NetworkKind` enum: `Preprod | Preview | Mainnet`.
- `encode_interpreter_for_network(NetworkKind) -> Vec<u8>` selects:
  - **Preprod** — Byron+Shelley summaries, Byron→Shelley at slot
    86_400 epoch 4, Shelley `epochSize=432_000` (5-day epochs).
  - **Preview** — single open Shelley-shape summary anchored at
    slot 0, `epochSize=86_400` (1-day epochs), no Byron because
    preview's `config.json` sets every `Test*HardForkAtEpoch=0`.
  - **Mainnet** — Byron+Shelley summaries, Byron→Shelley at slot
    4_492_800 epoch 208.  Byron `relativeTime` capped at
    slot-based picosecond approximation to stay within u64.
- `encode_system_start_for_network(NetworkKind)` emits the
  per-network genesis date:
  - Preprod: `2022-06-01` (day-of-year 152).
  - Preview: `2022-10-25` (day-of-year 298).
  - Mainnet: `2017-09-23` (day-of-year 266).

`node/src/local_server.rs`:

- New `NetworkPreset` enum mirrored from `NetworkKind`.
- `BasicLocalQueryDispatcher::new(NetworkPreset)` constructor;
  `Default` impl pins preprod for tests.
- `NetworkPreset::from_network_magic(u32)` derives the preset
  from runtime config (1=preprod, 2=preview, 764824073=mainnet,
  default=preprod for custom magics).
- `dispatch_upstream_query(snapshot, query, network_preset)` now
  threads the preset to the encoder helpers.

`node/src/main.rs`:

- NtC dispatcher built via
  `BasicLocalQueryDispatcher::new(NetworkPreset::from_network_magic(ntc_network_magic))`
  so the runtime-configured magic auto-selects the right encoders.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean (clippy --workspace --all-targets --all-features -- -D warnings)
cargo test-all                   # passed: 4687  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4684 (Round 152) → 4687.

### Regression tests

- `preview_interpreter_emits_single_shelley_summary_with_1day_epochs`
  — pins the `0x1a 00015180 1903e8` Shelley params marker
  (`epochSize=86_400`, `slotLength=1000ms`) and asserts preprod's
  `0x69780` (=432_000) signature does NOT appear in preview output.
- `preview_system_start_is_2022_day_298` — pins `83 19 07e6 19 012a 00`.
- `preprod_system_start_is_2022_day_152` — regression baseline.

### Operational verification

**Preprod (post-fix, no regression)**

```
$ CARDANO_NODE_SOCKET_PATH=/tmp/ygg-verify-multi.sock \
  cardano-cli query tip --testnet-magic 1
{
    "block": 88040,
    "epoch": 4,
    "era": "Shelley",
    "hash": "96a02bdd3ad07a905d231297d57c670463bd3c3f028d8c3e18f9556d9354ba36",
    "slot": 88040,
    "slotInEpoch": 1640,
    "slotsToEpochEnd": 430360,
    "syncProgress": "1.40"
}
```

Same shape as Round 152 — confirms the network-aware refactor
preserves preprod's existing operator-visible output verbatim.

**Preview (codec verified, sync blocked)**

Preview's `Test*HardForkAtEpoch=0` configuration produces blocks
whose envelope era_tag (Alonzo=5) does not match the protocol
version embedded in the block (`(7, 2)` = Babbage's range).
yggdrasil's `validate_block_protocol_version_for_era` rejects this
strictly:

```
Error ChainDB.AddBlockEvent.InvalidBlock node=yggdrasil-preview
  peer sent an invalid block; disconnecting currentPoint=Origin
  error=protocol version mismatch: block in era Alonzo carries
        version (7, 2), expected major in 5..=6
  peer=3.134.226.73:3001
```

This is a **separate** sync-layer parity gap unrelated to the
NtC Interpreter codec — yggdrasil treats era_tag and protocol
version as a strict pair, but upstream Haskell uses the hard-fork
combinator's "lifted" era handling that allows looser pairs at
era-transition boundaries.  Preview NtC codec output was therefore
verified via the captured-bytes regression tests in
`crates/network/src/protocols/local_state_query_upstream.rs`
instead of an end-to-end socket query.

### Open follow-ups

1. **Strict era_tag/PV validation loosening** — port the
   hard-fork combinator's "lifted" era handling so preview (and
   any future testnet using `Test*HardForkAtEpoch`) syncs without
   tripping `expected major in N..=M` rejection.  Reference:
   `Ouroboros.Consensus.HardFork.Combinator.Embed.Nary`.
2. **Allegra+ era summaries** — current single-Shelley summary
   covers the first ~10M slots post-Byron.  Mainnet's tip is past
   slot 75M, so emitting Allegra/Mary/Alonzo/Babbage/Conway
   summaries when the snapshot's current era exceeds Shelley
   would extend slot↔epoch math past the synthetic far-future end.
3. **CLI tip parity** — yggdrasil's own `cardano-cli query-tip`
   subcommand still uses internal flat-table queries; threading
   `NetworkPreset` through to the CLI mode would unify the JSON
   output with the upstream-shaped LSQ path.

### Diagnostic captures

- `/tmp/ygg-verify-cli-tip-r153.txt` — cardano-cli output against
  yggdrasil's preprod NtC socket.
- `/tmp/ygg-verify-metrics-r153.txt` — Prometheus snapshot.
- `/tmp/ygg-preview.log` — preview run log (shows the
  PV-mismatch rejection reproducer).

### References

- `Ouroboros.Consensus.HardFork.History.Summary`
- `Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`
- `Ouroboros.Consensus.HardFork.Combinator.Embed.Nary`
- Previous round: `docs/operational-runs/2026-04-27-round-152-cardano-cli-tip-parity.md`
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`, `node/src/main.rs`
