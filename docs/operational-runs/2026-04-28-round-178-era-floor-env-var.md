## Round 178 — `YGG_LSQ_ERA_FLOOR` bypasses cardano-cli's era gate

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Address operator complaints about the "era blockage": across
R163/R171/R172/R173 yggdrasil shipped wire-correct dispatchers
for `query stake-pools`, `query stake-distribution`,
`query stake-address-info`, `query pool-state`, and
`query stake-snapshot` — but cardano-cli 10.16 *client-side*
gates each of those queries at Babbage+ and refuses to send
them to a node reporting Alonzo era.  Preview / preprod fresh
syncs spend thousands of slots in early-PV Alonzo
(PV=(6,0) = Alonzo per upstream `*Transition` table) before
the chain naturally crosses the Babbage hard-fork — so
operators can't exercise the dispatchers without committing to
multi-hour syncs.

### Code change

`node/src/local_server.rs::effective_era_index_for_lsq`: read
`YGG_LSQ_ERA_FLOOR=N` env var (parsed as `u32`, valid range
`0..=6`) and clamp the reported LSQ era ordinal to at least
`N`.  When unset / unparseable / out-of-range, the helper
preserves the existing `wire_era.max(pv_derived_era)`
behaviour from R160.  Lower-than-derived floors are no-ops
(never demote — would confuse cardano-cli's era-progression
expectations).

```rust
let derived = wire_era_ordinal.max(pv_era_index);
match std::env::var("YGG_LSQ_ERA_FLOOR")
    .ok()
    .and_then(|v| v.parse::<u32>().ok())
{
    Some(floor) if floor <= 6 => derived.max(floor),
    _ => derived,
}
```

The helper feeds both the `GetCurrentEra` HardForkBlockQuery
response (which `cardano-cli query tip` reads to populate the
`era` field) AND the per-era PP-encoder selection inside
`dispatch_upstream_query`, so a floored era automatically
routes PP responses through the matching era-shape encoder
(e.g. `YGG_LSQ_ERA_FLOOR=6` → tip says Conway → cardano-cli
sends queries marked era=6 → yggdrasil's PP encoder picks
Conway 31-element shape).

### Operational verification

#### Before (era gate active, default behaviour)

```
$ cardano-cli query tip --testnet-magic 2 | grep '"era"'
    "era": "Alonzo",

$ cardano-cli query stake-pools --testnet-magic 2
Command failed: query stake-pools
Error: This query is not supported in the era: Alonzo.
Please use a different query or switch to a compatible era.
```

#### After (era gate bypassed)

```
$ YGG_LSQ_ERA_FLOOR=6 target/release/yggdrasil-node run --network preview ...
$ cardano-cli query tip --testnet-magic 2 | grep '"era"'
    "era": "Conway",

$ cardano-cli query stake-pools --testnet-magic 2
cardano-cli: DecoderFailure ... DeserialiseFailure 2 "expected list len"
```

Era gate **bypassed**: cardano-cli no longer refuses the query
client-side.  It now actually sends the wire query to
yggdrasil and starts decoding the response.

### Known follow-up: response-shape mismatch

Bypassing cardano-cli's era gate exposes a separate downstream
issue: cardano-cli 10.16's HFC envelope decoder for
Conway-era responses (from a node-to-client wire version that
includes `DijkstraEra` in the era list) expects a different
result-body shape than yggdrasil's R156 `[1, body]` envelope.
Specifically, cardano-cli rejects the response with
`DeserialiseFailure 2 "expected list len"` — the decoder
consumed our `0x81` envelope header successfully but failed at
the next byte expecting either a different envelope
discriminator or a different inner-value shape.

The hypothesis space for the actual upstream wire format
includes:

- A 2-element `[era_index, value]` envelope superseding the
  pre-Conway `[1, value]` for newer node-to-client versions.
- A bare-list inner value (no CIP-21 tag 258 wrap) — but
  empirical testing showed the failure persists even with the
  tag dropped, ruling this out as the sole cause.
- An additional level of wrapping (`Either Mismatch (Era,
  Value)` rather than `Either Mismatch Value`).

Without a running upstream Babbage+ Cardano node to capture
real wire bytes and compare, we can't confidently pin the
exact shape.  Tracking this as a dedicated follow-up:
**R178-followup: capture upstream Conway-era HFC response wire
fixtures and align yggdrasil's `encode_query_if_current_match`
+ era-specific encoders accordingly**.

The R178 env-var bypass is honest about this trade-off:
documented as opt-in for partial-sync chains exercising the
era-gated query paths, with the response-shape compatibility
still pending.

### Regression test (+1)

`era_floor_env_var_promotes_reported_era` — covers the matrix:

- No env var → derived era (Alonzo at PV=(6,0))
- `YGG_LSQ_ERA_FLOOR=5` → Babbage
- `YGG_LSQ_ERA_FLOOR=6` → Conway
- `YGG_LSQ_ERA_FLOOR=2` (lower than derived) → no-op
- `YGG_LSQ_ERA_FLOOR=99` (out of range) → no-op
- `YGG_LSQ_ERA_FLOOR=not-a-number` → no-op

Env-var manipulation is serialised via a static `Mutex` so
concurrent test execution doesn't race on the process-wide
env table.

Test count progression: 4734 → **4735**.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4735  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Why this matters

The era gate is the **first-line operational blocker** for
exercising the R163/R171/R172/R173 dispatchers — without
bypassing it, operators on partial syncs of preview/preprod
can't even reach yggdrasil's response code paths.  R178 closes
that operational gap with a documented, opt-in env var so
operators can:

- Smoke-test that era-gated query paths route correctly inside
  yggdrasil (sync session establishes, dispatcher decodes the
  wire query, response bytes get emitted).
- Compare yggdrasil's response bytes against future upstream
  fixtures once captured.
- Run end-to-end CI against the response-shape work in flight
  (R178-followup) without needing a multi-hour preprod sync to
  natural Babbage transition.

### Open follow-ups

1. **R178-followup: upstream Conway-era HFC response wire
   fixtures**.  Capture real cardano-node 10.7.x → cardano-cli
   10.16 query response bytes (e.g. against mainnet via a
   running upstream node), then align yggdrasil's
   `encode_query_if_current_match` + era-specific encoders so
   that with `YGG_LSQ_ERA_FLOOR=6` set, all five era-gated
   queries (`stake-pools`, `stake-distribution`,
   `stake-address-info`, `pool-state`, `stake-snapshot`)
   decode end-to-end.
2. Live stake-snapshot plumbing into `LedgerStateSnapshot`
   (R163/R173 follow-up).
3. `GetGenesisConfig` ShelleyGenesis serialisation (R163).
4. Apply-batch duration histogram (R169).
5. Multi-session peer accounting (R168 structural).
6. Pipelined fetch + apply (R166).
7. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — `effective_era_index_for_lsq` env-var floor + 1 new
  regression test.
- Captures: `/tmp/ygg-r178-preview.log` (post-bypass preview
  sync; tip reports Conway with `YGG_LSQ_ERA_FLOOR=6`).
- Upstream reference:
  `Ouroboros.Consensus.HardFork.Combinator.Ledger.Query` —
  `decodeQueryIfCurrent` envelope structure;
  `Cardano.Ledger.Core.Era` `*Transition` `ProtVer` table;
  `cardano-cli`'s era-gating in
  `Cardano.CLI.EraBased.Query.Run`.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-177-filtered-delegations-fixes.md`](2026-04-28-round-177-filtered-delegations-fixes.md).
