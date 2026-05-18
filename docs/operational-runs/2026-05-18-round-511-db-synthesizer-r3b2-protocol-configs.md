# Round 511 — db-synthesizer A3 R3b-2: per-era protocol-config types

**Date:** 2026-05-18
**Area:** sister-tools / `crates/tools/db-synthesizer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`unstable-cardano-tools/Cardano/Node/Types.hs` (the
`Node*ProtocolConfiguration` records), `DBSynthesizer/Orphans.hs` (the
`FromJSON` instances).

## Summary

R3b-2 — the second slice of the R3b consensus-config arc — ports the
per-era protocol-configuration records the synthesizer's `initProtocol`
needs. Six structs: the four era configs
(`Node{Shelley,Alonzo,Conway,Dijkstra}ProtocolConfiguration`, 2 fields
each), `NodeByronProtocolConfiguration` (9 fields), and
`NodeHardForkProtocolConfiguration` (8 fields). The Byron + HardFork
structs carry `#[derive(Deserialize)]` mirroring `Orphans.hs`'s
`FromJSON` instances; the four era configs need no deserializer —
upstream `initProtocol` inline-constructs them from `NodeConfigStub`
paths.

## Grounding correction

The roadmap's original framing ("six serde parsers") and the first
grounding pass were imprecise. The shape was verified against the
*correct* upstream file — db-synthesizer's own vendored
`unstable-cardano-tools/Cardano/Node/Types.hs`, NOT `cardano-node`'s
separate (drifted) `Cardano.Node.Types`:

- `NodeByronProtocolConfiguration` = **9 fields** — includes
  `application_name` / `application_version`; `cardano-node`'s copy
  dropped those to 7.
- `NodeHardForkProtocolConfiguration` = **8 fields** —
  `TestEnableDevelopmentHardForkEras` + 7 `*HardForkAtEpoch`; no
  `*AtVersion`.
- It is **6 structs + 2 deserializers**, not "six parsers" — the four
  era configs have no `FromJSON` instance upstream.

(The corrected spec was recorded in commit `8ce1825` before this
implementation round.)

## Changes

- `types.rs` — 6 new structs. Byron + HardFork derive `Deserialize`
  with `#[serde(rename / default)]` attributes mirroring `Orphans.hs`'s
  key-by-key decoding: `ByronGenesisFile` and
  `LastKnownBlockVersion-{Major,Minor}` required, the rest defaulted;
  the Byron `application_name` is hard-coded `"cardano-sl"` via
  `#[serde(skip, default = …)]` (upstream `pure (ApplicationName
  "cardano-sl")`). 5 new tests.
- `RequiresNetworkMagic` is reused from `yggdrasil-node-config` — not a
  parallel type — its `Deserialize` already reads the
  `"RequiresNoMagic"` / `"RequiresMagic"` strings.
- `orphans.rs` — carve-out doc updated: the Byron + HardFork `FromJSON`
  instances are now ported (as derives in `types.rs` — they have no
  cross-field assertion, unlike `NodeConfigStub`, whose hand-written
  `Deserialize` stays for its `Protocol == "Cardano"` check).
- `Cargo.toml` — `yggdrasil-node-config` workspace path-dependency
  added (for `RequiresNetworkMagic`).
- `AGENTS.md` — functional surface refreshed for R3b-2.

## Verification

- Focused (`yggdrasil-db-synthesizer`): `cargo test` — 85 lib tests
  (+5 new) + 7 integration tests pass.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,536 passing, 0 failing** (+5 over the R3b-1
  baseline of 6,531 — exactly the new tests).
- A plain `cargo test-all` aborts early on the documented timing-flaky
  cardano-tracer test (cargo's default fail-fast); `--no-fail-fast` ran
  every crate clean.

## Remaining (A3 R3b)

- **R3b-3** — the `CardanoProtocolParams` aggregator +
  `mk_consensus_protocol_cardano` orchestration: fold R3b-1's
  `GenesisBundle` + R3b-2's protocol configs + R3a's credentials;
  `initialize` returns `(DBSynthesizerConfig, CardanoProtocolParams)`;
  absorbs genesis-hash verification.

Then R3c — the Praos forge loop.
