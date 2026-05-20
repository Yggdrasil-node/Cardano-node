---
title: Completion Roadmap
layout: default
parent: Reference
nav_order: 6
---

# Completion Roadmap

**Created:** 2026-05-17 · **Status reference:** R503 + post-R503 commit-level work

## Purpose

This is the forward-looking backlog of work remaining to take Yggdrasil from
its current state — a verified, code-level parity-closed core node — to *full*
completion of the project's stated goal: 100% protocol, naming, functionality,
and filename parity with upstream `cardano-node`, including the full
sister-tool suite.

It complements, and does not replace:

- [`PARITY_SUMMARY.md`](PARITY_SUMMARY.md) / [`PARITY_PROOF.md`](PARITY_PROOF.md) / [`UPSTREAM_PARITY.md`](UPSTREAM_PARITY.md) — what is *already* done and proven.
- [`TECH-DEBT.md`](TECH-DEBT.md) — narrow, named consolidation-debt items.
- [`parity-matrix.json`](parity-matrix.json) — the machine-readable per-feature inventory.

**Honest framing.** Categories B and C below *cannot be closed in a normal
development environment.* They require a long-running operator soak, or a
running upstream Haskell `cardano-node` for byte-level wire comparison. They
are listed so the full path is visible — not because they are executable in
one session.

## Status snapshot

- **Build health baseline (verified 2026-05-17):** all four cargo gates green on Rust 1.95.0 — `cargo fmt`,
  `cargo check-all`, `cargo lint`, `cargo test-all` (**6,519 tests passing,
  0 failing, 3 ignored**). `check-strict-mirror.py` + `check-fixture-manifest.py`
  clean.
- **Core node:** crypto, ledger, storage, consensus, mempool, network, plutus,
  and the `crates/node/*` runtime crates are feature-complete for syncing and
  validating the official networks (code-level parity closure, v0.2.0).
- **Parity matrix (updated through R532):** 22 tracked entries — 2 `verified_11_0_1`,
  12 `implemented_needs_11_0_1_evidence`, 8 `partial`.
- **Sister tools (13):** 1 verified (`bech32`); 4 functional-pending-soak
  (`cardano-cli` with 40 operational subcommands, `cardano-submit-api`,
  `db-truncater`, `db-synthesizer`); 2 functional-partial (`cardano-tracer`,
  `db-analyser`); 6 skeleton (`kes-agent`, `kes-agent-control`,
  `snapshot-converter`, `tx-generator`, `dmq-node`, `cardano-testnet`).
- **Reference parity tag:** `11.0.1`.

---

## Category A — executable now (no external dependency)

Needs only the workspace + the vendored reference tree. Each item is a
standard R-arc round series: one bounded slice per round, four cargo gates
green, one `docs/operational-runs/` doc, one commit. Use the
`continuous-agent-loop` + `round-extraction` skills; author a `parity-plan`
first for any slice touching protocol/CBOR/crypto behavior.

### A1 — Feature-flag gating  (`TECH-DEBT.md` §"Wave 3 / Wave 5 feature flags")
Status verified 2026-05-20 (R591+R592 update): `forge` and
`yggdrasil-plutus/{secp256k1, bls12-381}` are wired (real `#[cfg]`
sites; `--no-default-features` builds and `cargo lint-no-default`
clean). R591 removed the inert `yggdrasil-network/ntn` flag. R592
removed the inert `yggdrasil-ledger/plutus` flag — the Phase-5.4
audit comment in `crates/ledger/Cargo.toml` had explicitly identified
it as mis-scoped: gating off `plutus_validation` would only remove
validation logic without slimming the dependency graph (this crate
never depended on `crates/plutus`; the heavy CEK-machine code lives
behind the inverted `PlutusEvaluator` trait wired by
`crates/node/plutus-eval`).
The remaining inert flag is `yggdrasil-network/ntc` (0 `#[cfg]`
sites). `ntc` is cleanly wireable — a relay/producer with the
node-to-client local socket excluded is still a valid node — but it
is a multi-crate round (`yggdrasil-network` NtC modules + the
`yggdrasil-node-ntc-server` crate + the binary's `query`/`submit-tx`
subcommands). **Exit:** the remaining `ntc` flag conditionally
compiles the code it names; `cargo lint-no-default` stays green.

### A2 — cardano-cli subcommand migration — ✅ COMPLETE (verified 2026-05-20)
The cardano-cli C-arc closed at R515 and the R527-R529 stale-placement
cleanup moved the remaining query plans into the shared CLI crate.
`crates/tools/cardano-cli/src/command.rs` carries all **40 `Command`
variants**, `run.rs` dispatches them, and the crate has passing focused
coverage for the post-R529 LSQ additions. The standalone
`yggdrasil-cardano-cli` binary covers the
offline operator toolkit (keys / addresses / txid / sign / build / build-raw /
view), the full 27-query LocalStateQuery surface, and `transaction submit`.
Older central docs undercounted the migrated subcommands; the current surface
is the 40-command / 27-query split above, matching
`crates/tools/cardano-cli/AGENTS.md`. The only outstanding item is
byte-equivalence evidence against a real upstream `cardano-cli 11.0` binary —
Category-B operator-soak work, tracked by the `parity-matrix.json`
`sister-tool.cardano-cli` entry (`implemented_needs_11_0_1_evidence`).

### A3 — db-synthesizer Phase 4 R3 (R1 ✅, R2 ✅ done 2026-05-17)
`crates/tools/db-synthesizer` shipped the forge-loop slice (Phase 4 R1) and the
R2 genesis-loading slice (round 504, commit `a46bae1`). **Remaining — R3 is a
multi-slice arc.** The earlier "1 round" estimate was wrong: grounding (2026-05-18)
against upstream `runForge` showed the synthesizer's flat `FileImmutable` loop
carries none of the ledger state / `ChainDepState` / consensus config that real
Praos forging needs. Verified decomposition:

- **R3a — credential loading.** Wire the operator `cert`/`vrf`/`kes`/`bulk`
  paths into validated forger credentials. Credential model verified via the
  `haskell-reference-auditor` (2026-05-18, `Cardano.Node.Protocol.Shelley`
  `readLeaderCredentialsSingleton`/`Bulk`): the cold issuer VKey is the trailing
  32-byte field of the `NodeOperationalCertificate` text-envelope — there is no
  separate cold-vkey artifact or CLI flag upstream. Prep findings in
  `crates/node/block-producer`: `decode_opcert_cbor` already parses that cold
  vkey but discards it; `load_block_producer_credentials` takes a divergent
  separate `issuer_vkey_path`; the singleton path is missing upstream's
  `MismatchedKesKey` check (KES key ↔ opcert hot-vkey).
  - ✅ **Slice 1** (round 505, commit `55ee243`) — opcert loader carries the
    cold vkey: `decode_opcert_cbor` returns `(OpCert, Option<VerificationKey>)`
    and the new `load_operational_certificate_with_issuer` surfaces it. Carried
    *alongside* `OpCert` in a tuple — the 45-site `yggdrasil_consensus::OpCert`
    type is untouched (zero blast radius).
  - ✅ **Slice 2** (round 506, commit `91e1ee3`) — `MismatchedKesKey` check:
    `load_block_producer_credentials` now rejects a (KES key, opcert) pair
    where `derive_sum_kes_vk(kes_key) != operational_cert.hot_vkey`, mirroring
    upstream `opCertKesKeyCheck` (`Cardano.Node.Protocol.Shelley`). Additive —
    function signature unchanged, zero caller blast radius.
  - ✅ **Slice 3** (round 507, commit `0089c6a`) — `issuer_vkey_path`
    removal: `load_block_producer_credentials` drops the param and sources
    the issuer cold vkey from `load_operational_certificate_with_issuer`
    (a bare-`OCert` envelope → `OpCertMissingIssuerKey`). The
    upstream-divergent `--shelley-operational-certificate-issuer-vkey` CLI
    flag and `NodeConfigFile` field are removed end-to-end (cli / main / run /
    configuration / validate_config + the 3 config presets); the
    credential-policy set is now 3 fields, not 4. **A3 R3a is complete.**
    Follow-up (round 508): swept the removed flag from the producer scripts
    and the operator manual / runbooks.
- **R3b — consensus config.** Port `Run.initProtocol` /
  `mkConsensusProtocolCardano` — parse every era genesis file + the hard-fork
  config into the protocol params the leader check + forge need. Grounding
  (2026-05-18) verified R3b is a **3-slice arc and overwhelmingly wiring** of
  existing yggdrasil machinery: `crates/node/genesis` already parses every era
  genesis, verifies hashes, and exposes `shelley_genesis_hash_to_praos_nonce`;
  R3a supplies credential loading. New code is aggregator structs + JSON
  parsers + one orchestration fn — no new crypto / ledger / consensus algorithm.
  - ✅ **R3b-1** (round 510, commit `73ffcb4`) — multi-era genesis bundle:
    `load_genesis_bundle` reads every era's genesis (Byron / Shelley /
    Alonzo / Conway) into a typed `GenesisBundle` plus the initial Praos
    nonce, wiring the existing `genesis`-crate loaders;
    `synthesize_from_config` builds it. Hash verification deferred to
    R3b-3 (needs the config's `*GenesisHash` fields); Dijkstra omitted
    (era not yet activated — no `load_dijkstra_genesis`).
  - ✅ **R3b-2** (round 511, commit `f560692`) — per-era protocol-config
    types: `types.rs` declares `NodeByronProtocolConfiguration` (9 fields),
    `NodeHardForkProtocolConfiguration` (8 fields), and the four
    `Node{Shelley,Alonzo,Conway,Dijkstra}ProtocolConfiguration` records,
    mirroring db-synthesizer's vendored
    `unstable-cardano-tools/Cardano/Node/Types.hs`. Byron + HardFork carry
    `#[derive(Deserialize)]` (`#[serde(rename/default)]` mirroring
    `Orphans.hs`'s `FromJSON`); the 4 era structs need no deserializer.
    `RequiresNetworkMagic` reused from `yggdrasil-node-config`.
  - ✅ **R3b-3** (round 512, commit `a0e7b1b`) — `CardanoProtocolParams`
    aggregator: `run::load_consensus_protocol` / `mk_consensus_protocol_cardano`
    fold R3b-1's `GenesisBundle` + R3b-2's per-era configs into a
    synthesizer-scoped 6-field `CardanoProtocolParams` (upstream field
    *names*, simplified types — faithful mirrors of the hard-fork-combinator
    `NP` / `TransitionConfig` machinery are not on the db-synthesizer arc and
    a single-era forge consumes none of it). Hard-fork triggers case-mapped
    from `Test*HardForkAtEpoch`; `ProtVer` `(11,0)`/`(10,7)` on the dev flag.
    Genesis-hash verification: upstream `initProtocol` passes `Nothing` for
    the Shelley-family hashes — nothing to fold in. **A3 R3b is complete.**
  Scope boundary: R3b stops at the config bundle. Building the initial
  `ExtLedgerState` the synthesizer forges on (`pInfoInitLedger`) is R3c.
- **R3c — Praos forge loop.** Re-architect `run_forge` to forge Praos-valid
  blocks. Grounding (2026-05-18) verified R3c is a **6-slice arc** — the
  roadmap's "hard part". The forge *cryptography* is ~100% reuse
  (`crates/node/block-producer`, R3a-complete — `check_should_forge` /
  `forge_block` callable as-is; zero new VRF/KES/OpCert/CBOR code). What is
  genuinely new is the **offline state-evolution orchestration** — the
  synthesizer is the first yggdrasil forge path with no network and no wall
  clock, so it must own the ledger / nonce / stake evolution the runtime gets
  from the sync pipeline and upstream `runForge` gets from the ChainDB.
  Verified decomposition:
  - ✅ **R3c-1 — initial ledger + nonce state.** Construct the
    `pInfoInitLedger` analog. Grounding (2026-05-19) found the faithful
    genesis→`LedgerState` build is the ~115-line `strict_base_ledger_state`
    (`crates/node/cardano-node/src/startup.rs`) — UTxO + stake + delegs + protocol
    params + epoch config — which lives in the `yggdrasil-node` *binary*
    crate, tied to `NodeConfigFile`. The synthesizer needs the identical
    state; duplicating 115 drift-prone lines is wrong. Two sub-slices:
    - ✅ **R3c-1a** (round 514, commit `c19b8f8`) — extracted the shared
      `build_base_ledger_state` + `BaseLedgerStateInputs` into
      `yggdrasil-node-genesis`; `startup.rs::strict_base_ledger_state`
      refactored to load-pieces + call the shared builder. Behavior-
      preserving — node unchanged, four gates green (6,539 tests, the
      baseline, 0 fail).
    - ✅ **R3c-1b** (round 515, commit `c902285`) — db-synthesizer's
      `load_initial_forge_state` builds the genesis-seeded initial
      `LedgerState` (via the shared `build_base_ledger_state`) +
      `NonceEvolutionState`, returned as `InitialForgeState`. **A3 R3c-1
      is complete.**
  - ✅ **R3c-2 — bulk credentials + multi-forger** (round 516, commit
    `305e0b0`). `load_bulk_block_producer_credentials` — a
    `yggdrasil-node-block-producer` port of `readLeaderCredentialsBulk` —
    parses the inline `[cert,vrf,kes]` text-envelope triples;
    `run::read_leader_credentials` returns the singleton ∪ bulk
    `Vec<BlockProducerCredentials>` forger set. The per-slot loop picking
    the first leader is R3c-4.
  - ✅ **R3c-3 — thread evolving state** (round 530). The forge loop now threads
    `LedgerState` + `NonceEvolutionState` through `ForgeState`; append-mode
    runs replay the existing ChainDB prefix into the genesis-seeded state
    before forging more blocks; each new structural block applies to cloned
    ledger/nonce state before append. Blocks stay structural here — a
    four-gates-green intermediate.
  - ✅ **R3c-4 — real Praos forge** (round 531). The production path now
    consumes `BlockProducerCredentials`, runs the shared
    `check_should_forge` leader check (skipping `NotLeader` slots), calls
    `forge_block`, persists raw Conway block CBOR via
    `forged_block_to_storage_block`, replays raw-CBOR VRF nonce inputs in
    append mode, and returns before ChainDB open when no forgers are
    supplied, matching upstream `Run.hs`.
  - ✅ **R3c-5 — epoch-boundary stake rebuild** (round 532). The
    production path now computes each forger's leader-check `sigma`
    from `StakeSnapshots.set`, seeds the initial forecast snapshot from
    Shelley genesis `staking.pools` / `staking.stake` / `initialFunds`,
    activates genesis pools on the first Shelley-family block, and uses
    `apply_epoch_boundary` to rotate snapshots as the synthetic chain
    advances across epochs.
  - 🟡 **R3c-6 — `FileImmutable` → `ChainDb` migration.** Persist a
    `LedgerStore` snapshot so `db-analyser` can validate the synthesized chain.

**Each slice is its own protocol-critical round** — author a `parity-plan`
first; R3a/R3c touch the consensus `OpCert` / forge surface. **Exit (R3c):**
synthesizer produces a Praos-valid on-disk ChainDB that `db-analyser` validates.

### A4 — Sister-tool build-out
Six tools remain implementation arcs:
`kes-agent`, `kes-agent-control`, `snapshot-converter`, `tx-generator`,
`dmq-node`, `cardano-testnet`. One is still pre-gated: `kes-agent` on a
socket-protocol byte-equivalence fixture capture (highest-stakes — key
custody). `tx-generator` is no longer blocked on the cardano-cli C-arc;
that prerequisite closed at R515/R529, so its remaining blocker is its
own parser / generator / submission implementation plus upstream
  comparison evidence. R533 shipped its upstream `Command.hs` parser
  mirror, R534 shipped `Setup/TestnetDiscovery.hs`, R535 shipped
  `Setup/NixService.hs` high-level option parsing/projections, R536
  shipped `Compiler.hs` script generation plus the `Script/Types.hs` IR,
  R537 shipped `Script/Aeson.hs` low-level script JSON parsing, and R538
  shipped `Script/Env.hs` plus `Script/Action.hs` state-only action
  execution. R539 moved the Core-owned state helpers into a strict
  `Script/Core.hs` mirror. R540 wired the Core node-to-client
  current-era and protocol-parameter query path with upstream
  LocalStateQuery envelopes. R541 added the strict
  `GeneratorTx/SizedMetadata.hs` mirror (`mkMetadata` chunking, metadata
  cost assumptions, and `Script/Core.toMetadata` preflight for `NtoM`).
  R542 added the upstream FIFO-backed fund/wallet queue surface
  (`Internal/Fifo.hs`, `Fund.hs`, `FundQueue.hs`, and
  `Benchmarking/Wallet.hs`) and moved `Script/Env` wallet state off the
  ad hoc Vec carrier. R543 added the `Cardano.TxGenerator.Utils` value
  splitting surface (`inputsToOutputsWithFee`, `includeChange`,
  `mkTxIn`) and wires `Script/Core.submitInEra` to preflight `Split`,
  `SplitN`, and `NtoM` wallet value splitting before transaction
  assembly. R544 added the `Cardano.TxGenerator.UTxO` key-output
  builder surface (`ToUTxO`, `ToUTxOList`, `makeToUTxOList`, and
  key-witnessed `mkUTxOVariant`) using ledger-native Shelley-family
  outputs and pure-Rust signing-key derivation. R545 wired the
  `Script/Core.hs` pay-mode and collateral preflight boundary by
  porting `selectCollateralFunds`, the key-output half of
  `interpretPayMode`, and the `Split` / `SplitN` / `NtoM`
  output-address trace points before value splitting. R546 added the
  `Cardano.TxGenerator.UTxO.mkUTxOScript` output/fund builder surface:
  Plutus script enterprise addresses, datum-hash outputs for
  Alonzo/Babbage-family eras, script-language support checks, and
  script-witnessed generated funds without signing keys. R547 added the
  static-budget `makePlutusContext` path by porting
  `Cardano.TxGenerator.Setup.Plutus.readPlutusScript`,
  `Cardano.TxGenerator.PlutusContext.readScriptData`, bundled
  `scripts-fallback` resolution, detailed-schema Plutus data parsing,
  real datum/redeemer/execution-unit script witnesses, and
  `PayToScript` -> `mkUTxOScript` wiring. R548 added the
  `Cardano.TxGenerator.Tx` key-spend path:
  `sourceToStoreTransaction`, `sourceToStoreTransactionNew`,
  `sourceTransactionPreview`, signed Shelley-family `genTx`, tx-size
  measurement, tx-id-based generated-fund storage, and the missing
  `Benchmarking.Wallet` `createAndStore` / `mangle` helpers. R549
  wired finite `Script/Core.submitInEra` execution for key-spend
  `Split`, `SplitN`, `NtoM`, `Sequence`, and `Take (Cycle ...)`
  generators, including source/destination wallet mutation,
  `DiscardTX`, `NtoM` preview traces, and NtC LocalTxSubmission for
  `LocalSocket`. R550 wired `Benchmarking.Command.runCommand`
  high-level execution: `json_highlevel` now performs config
  discovery/mangling, initial/final option reporting, Plutus
  datum/redeemer preflight, `compileOptions`, and `run_script`; the
  `version` subcommand is concrete. R551 wired
  `Benchmarking.Script.Action.startProtocol` so it now loads node
  config, sets protocol/genesis/network/tracer env state, and lets
  high-level runs advance beyond the old `mkConsensusProtocol`
  sentinel. R552 added `Cardano.TxGenerator.Genesis` and
  `SecureGenesis`: `startProtocol` now hash-verifies and loads Shelley
  initial funds, and `Submit ... SecureGenesis` spends the matching
  genesis pseudo-input into a wallet-managed payment fund. R553 added
  `Benchmarking.Script.Selftest` for the no-output-file path, so the
  `selftest` command runs the upstream static DiscardTX action list
  against bundled protocol parameters. R554 closed `RoundRobin` /
  `OneOf` upstream-TODO error-shape parity by preserving the exact
  intentional `Core.hs` crash messages. R555 added script-spend
  transaction assembly: `genTx` now carries Plutus scripts, datums,
  redeemers, collateral key witnesses, and script-integrity hashes for
  static-budget script funds. R556 added Plutus pre-execution checking
  for static `withCheck` budgets via the shared pure-Rust CEK evaluator.
  R557 added upstream-shaped Plutus auto-budget fitting, binary-search
  loop calibration, budget summaries, and `AutoScript` wiring. R558
  wired successful `NtoM` previews to update the Plutus budget summary's
  projected transaction size and fee before dumping. R559 added the
  Allegra selftest `DumpToFile` renderer for newline-prefixed Haskell
  `Show (Tx)` output. R560 closed the generated selftest byte drift by
  matching upstream `StrictSeq` variable-length CBOR for Shelley-family
  transaction body output/certificate sequences; the selftest setup
  stages and final 4,000-record stream now compare byte-for-byte
  against the vendored upstream binary. R561 added the
  `Cardano.Benchmarking.Types` and `TpsThrottle` foundations for
  `GeneratorTx.Submission` and `walletBenchmark`; R562 added
  `Cardano.Benchmarking.LogTypes` plus the
  `GeneratorTx.SubmissionClient` request-state core; R563 added the
  `GeneratorTx.Submission` report refs, submission summaries, stream
  state, and `txStreamSource` bridge; R564 wired
  `GeneratorTx.SubmissionClient` to the typed TxSubmission2 network
  driver with a muxed loopback test; R565 added the
  `walletBenchmark` NtN control/connect layer, including IPv4 target
  resolution, upstream V14 initiator-only proposals, worker/feeder
  spawning, shutdown/summary control, and a peer-connect loopback test;
  R566 wired `Script/Core.hs` `SubmitMode::Benchmark` into that
  control, stores real `AsyncBenchmarkControl` runtime state in
  `Script/Env.hs`, and covers the path with a script-core loopback
  submission/summary test. R567 extended `SubmitMode::DumpToFile`
  beyond the Allegra fixture to Shelley and Mary key-witnessed streams
  with upstream-shaped body/witness hashes and explicit unsupported-
  field boundaries. R568 added the matching Alonzo key-witnessed
  renderer with `AlonzoTxBodyRaw`, `AlonzoTxWitsRaw`, empty `TxDats`
  / `Redeemers`, and `IsValid` fields. R569 extended the renderer into
  Babbage key-witnessed streams: the 16-field `BabbageTxBodyRaw`
  record (including `btbrCollateralInputs`, `btbrReferenceInputs`,
  `btbrCollateralReturn`, `btbrTotalCollateral`), `Sized {sizedValue =
  (addr, val, datum, refScript), sizedSize = N}` output wrappers, the
  `NoDatum` / `DatumHash (SafeHash ...)` Babbage datum shape, and the
  `ShelleyTx ShelleyBasedEraBabbage (AlonzoTx ...)` envelope, reusing
  `AlonzoTxWitsRaw` for the witness set. R570 extended again into
  Conway key-witnessed streams with the 19-field `ConwayTxBodyRaw`
  governance-aware record (`ctbrSpendInputs` rename, combined
  `ctbrVldt`, `ctbrCerts` as `OSet`, dropped `btbrUpdate`, plus
  `ctbrVotingProcedures` / `ctbrProposalProcedures` /
  `ctbrCurrentTreasuryValue` / `ctbrTreasuryDonation`) and the
  `ShelleyTx ShelleyBasedEraConway (AlonzoTx ...)` envelope, reusing
  the Babbage outputs renderer (Conway shares `BabbageTxOut`) and the
  Alonzo witness renderer (Conway `TxWits = AlonzoTxWits`). The
  `show_tx_for_dump` dispatch is now exhaustive across all
  `MultiEraSubmittedTx` variants. R571 lifted the multi-asset value
  boundary across the Mary, Alonzo, Babbage, and Conway `tx_out`
  renderers in one round by extending `show_mary_value` to produce
  upstream `MaryValue (Coin N) (MultiAsset (fromList [(PolicyID
  {policyID = ScriptHash "..."},fromList [("<asset>",qty)])]))` Show
  output for non-empty multi-asset bundles, including `BTreeMap`
  byte-lex iteration order that mirrors upstream `Data.Map toAscList`.
  R572 lifted the next boundary — `show_alonzo_witness_set` now
  renders non-empty `plutus_data` and `redeemers` via upstream-
  structured Show helpers (`show_plutus_data` for `Constr/Map/List/I/B`,
  `show_haskell_bytestring` for Latin1 byte-string Show with `\NNN`
  escapes and `\&` escape-boundary separator, `show_alonzo_tx_dats`
  with sorted-DataHash + `tag 258` set-tag CBOR for the outer hash,
  `show_alonzo_redeemers` with `(tag, index)` sorted + array-of-
  `[tag,index,data,ex_units]` CBOR for the outer hash, plus
  `show_alonzo_plutus_purpose` for the `AlonzoSpending` / `Minting` /
  `Certifying` / `Rewarding` constructors and `show_alonzo_ex_units`
  for the `ExUnits {exUnitsMem, exUnitsSteps}` record). Native scripts,
  bootstrap witnesses, and Plutus V1/V2/V3 script-witness bytes are
  the remaining `TxGenError` boundaries inside the witness set, plus
  the inline datum / reference script paths on `BabbageTxOut`. R573
  closed the inline-datum path: `show_babbage_datum` now renders
  `DatumOption::Inline(PlutusData)` as upstream `Datum (BinaryData
  "<latin1-escaped-cbor>")` using R572's `show_haskell_bytestring`
  over the PlutusData's canonical CBOR. R574 closed the Plutus
  reference-script path: `show_babbage_script_ref` renders Plutus
  V1/V2/V3 reference scripts as upstream `SJust PlutusScript
  PlutusV{1,2,3} ScriptHash "<hex>"` with Blake2b-224 over
  (language-tag byte ++ script bytes). R575 closed the Plutus
  witness-set script path: `show_alonzo_witness_set` renders the
  `atwrScriptTxWits` map as `fromList [(ScriptHash "<hex>",
  PlutusScript PlutusV{N} ScriptHash "<hex>"),...]` sorted by
  script-hash byte-lex order. R576 closed the Conway treasury-field
  path: non-zero `ctbrTreasuryDonation` renders as `Coin <n>` and
  `Some` `ctbrCurrentTreasuryValue` as `SJust (Coin <n>)` via new
  `show_coin` and `show_strict_maybe_coin` helpers. R577 closed the
  Conway `VotingProcedures` map path: non-empty
  `ctbrVotingProcedures` renders as upstream `VotingProcedures
  {unVotingProcedures = fromList [(Voter, fromList [(GovActionId,
  VotingProcedure)])]}` via new `show_conway_vote`,
  `show_conway_voter` (5 variants), `show_conway_gov_action_id`,
  `show_conway_voting_procedure`, `show_anchor`, and `show_url`
  helpers. R578 closed the native-script rendering path: both
  `show_babbage_script_ref` (reference scripts) and
  `show_alonzo_script_witnesses` (witness-set scripts) now accept
  `NativeScript` values via new `show_native_script` /
  `show_timelock_raw` helpers covering all 6 upstream `TimelockRaw`
  variants (`TimelockSignature`, `TimelockAllOf`, `TimelockAnyOf`,
  `TimelockMOf`, `TimelockTimeStart`, `TimelockTimeExpire`), with
  the outer MemoBytes hash computed as `Blake2b-256` over the
  canonical native-script CBOR. R579 closed the bootstrap-witness
  path: `show_alonzo_witness_set` now renders non-empty bootstrap
  witnesses as `atwrBootAddrTxWits = fromList [BootstrapWitness
  {bwKey, bwSignature, bwChainCode, bwAttributes}]`. The witness set
  is now boundary-free across all 5 carrier fields (vkey, native,
  Plutus, data, redeemer, bootstrap). Documented byte-parity caveat:
  upstream `Ord BootstrapWitness` uses `bootstrapWitKeyHash` (Byron
  AddressInfo Blake2b-224); yggdrasil sorts by canonical `(pubkey,
  sig, chain_code, attrs)` tuple lex — single-witness cases byte-
  equivalent, multi-witness pending a future round. R580 closed the
  simple-variant `ProposalProcedures` rendering path:
  `show_conway_proposal_procedures` renders the OSet shell, the
  `ProposalProcedure` record (with `pProcReturnAddr` decoded from
  yggdrasil's 29-byte reward-account bytes through
  `RewardAccount::from_bytes`), and the 4 simple `GovAction` variants
  (`InfoAction`, `NoConfidence`, `HardForkInitiation`,
  `NewConstitution`). R581 closed `TreasuryWithdrawals` via a
  `Map AccountAddress Coin` Show keyed by the typed
  `RewardAccount` directly. R582 closed `UpdateCommittee` via new
  `show_stake_credential` + `show_unit_interval` helpers and
  member-map iteration. R583 closed `ParameterChange` for the empty
  PParamsUpdate path: `show_conway_pparams_update` renders the full
  30-field `ConwayPParams` record with all SNothing values (and
  `cppProtocolVersion = NoUpdate`). Non-empty updates report
  field-name-bearing TxGenError pending per-type Show ports
  (`CoinPerByte`, `EpochInterval`, `NonNegativeInterval`, `Prices`,
  `OrdExUnits`, `PoolVotingThresholds`, `DRepVotingThresholds`,
  `CostModels`). All 7 GovAction variants now render for the
  empty-update path. R584 wired 16 scalar PParamsUpdate fields
  through `show_pparam_compact_coin` (8 Coin-family fields render
  as `SJust (CompactCoin {unCompactCoin = N})`) and
  `show_pparam_word` (8 plain Word fields render as `SJust N`).
  R585 wired 8 interval PParamsUpdate fields through
  `show_pparam_epoch_interval` (4 EpochInterval as `SJust
  (EpochInterval N)`) and `show_pparam_ratio_interval` (4
  UnitInterval/NonNegativeInterval as `SJust (num % den)`). R586
  wired 3 more composite PParamsUpdate fields: `cppPrices`
  (Prices record combining yggdrasil's split `price_mem` +
  `price_step`), `cppMaxTxExUnits`, and `cppMaxBlockExUnits`
  (`OrdExUnits` → ExUnits Show). R587 wired the
  `cppPoolVotingThresholds` (5-field record) and
  `cppDRepVotingThresholds` (10-field record) Show paths. R588
  closed the final composite field `cppCostModels`, splitting
  yggdrasil's `BTreeMap<u8, Vec<i64>>` by language tag (0/1/2 →
  PlutusV1/V2/V3 valid; other tags → unknown) into the upstream
  `_costModelsValid` + `_costModelsUnknown` two-map shape. **All
  30/30 Conway PParamsUpdate fields now render** — the
  PParamsUpdate Show surface is complete for the Conway era. R589
  closed the full Haskell `Show (ByteString)` mnemonic-escape
  coverage gap: `show_haskell_bytestring` now emits the full GHC
  `showLitChar` table (`\a`/`\b`/`\t`/`\n`/`\v`/`\f`/`\r` short
  aliases for 0x07-0x0D, `\SO` with H-lookahead disambiguation
  for 0x0E, multi-letter mnemonics for the rest of 0x00-0x1F,
  `\DEL` for 0x7F, plus the existing `\NNN` decimal escapes for
  0x80-0xFF). ByteString Show is now byte-equivalent to upstream
  for every byte. R590 closed the bootstrap-witness multi-witness
  byte-parity gap: new `bootstrap_witness_key_hash` ports upstream
  `bootstrapWitKeyHash` (Blake2b-224 over SHA3-256 over the 6-byte
  Byron AddressInfo prefix `[0x83 0x00 0x82 0x00 0x58 0x40]` plus
  key + chain_code + attributes) and drives the
  `show_alonzo_bootstrap_witnesses` sort. The multi-witness
  ordering is now byte-equivalent to upstream `Ord BootstrapWitness
  = comparing bootstrapWitKeyHash`. The remaining tx-generator
  blocker is upstream-binary soak evidence — every documented
  byte-parity gap inside yggdrasil is now closed.
**Scope:** ~5–8 rounds per tool. **Exit:** each
reaches `implemented_needs_11_0_1_evidence` in `parity-matrix.json`.

### A5 — cardano-submit-api structured rejection enum  (`TECH-DEBT.md` §"validation error")
Phase 1 (raw-bytes carrier) landed pre-R594. R594 shipped the
Phase-2 type-level scaffold: `TxValidationErrorInCardanoMode`
era-tagged enum + `TxValidationEra` discriminator + `EraApplyTxError`
payload. R595 added the Phase-2.5 scaffold for the
Shelley LEDGER predicate-failure variant set: `ShelleyLedgerPredFailure`
4-variant enum (UtxowFailure / DelegsFailure /
ShelleyWithdrawalsMissingAccounts / ShelleyIncompleteWithdrawals)
with `tag()`, `constructor()`, `raw_inner()`, and a Display impl
marking the rendering as raw-cbor pending per-variant payload
decoders. R596 (2026-05-21) shipped the first typed payload
decoder: `Withdrawals::from_cbor` decodes the tag-2 payload
(`Map AccountAddress Coin`) into `BTreeMap<RewardAccount, u64>`
via the existing `yggdrasil-ledger` `Decoder` + `RewardAccount`
codec. R597 (2026-05-21) wired the typed payload into the variant
itself (`ShelleyWithdrawalsMissingAccounts(Withdrawals)`) and
shipped the second typed decoder: `IncompleteWithdrawals::from_cbor`
for tag-3 `NonEmptyMap AccountAddress (Mismatch RelEQ Coin)`. New
generic `Mismatch<T>` struct + `MismatchRelation` enum + `CoinShow`
helper render upstream's custom `Show (Mismatch r a)` as
`Mismatch (RelEQ) {supplied: Coin <n>, expected: Coin <n>}`.
`IncompleteWithdrawals` enforces the NonEmpty invariant at decode
time. `ShelleyLedgerPredFailure::Display` now emits typed shapes
for tags 2 and 3, and continues marking tags 0/1 as raw-cbor
pending the UTXOW/DELEGS sub-rule decoders. R598 (2026-05-21) shipped the `ShelleyUtxowPredFailure` scaffold
(tag-0 sub-rule of the LEDGER tree): 11-variant enum mirroring
upstream `Cardano.Ledger.Shelley.Rules.Utxow` with `from_cbor`
decoder handling the simple payloads — tag 6
(`MissingTxBodyMetadataHash`), tag 7 (`MissingTxMetadata`), tag 8
(`ConflictingMetadataHash` as a typed `Mismatch<TxAuxDataHash>`),
and tag 9 (`InvalidMetadata` with no payload). Tags 0/1/2/3/4/5/10
(NonEmptySet/NonEmpty/sub-rule payloads) carry raw inner CBOR
pending their per-variant decoders. New `TxAuxDataHash` newtype
mirrors upstream's 32-byte metadata-hash SafeHash shape.
Phase-2.5+ remaining work: NonEmptySet decoder for the
ScriptHash/KeyHash variants, `ShelleyUtxoPredFailure` (tag-4
nested sub-rule), `ShelleyDelegsPredFailure` (tag-1 of the LEDGER
tree), wiring the typed decoder into `ShelleyLedgerPredFailure`,
then mirroring the predicate-failure tree for Allegra/Mary/Alonzo/
Babbage/Conway eras (Conway adds 4+ governance-specific variants).
**Exit:** operators can pattern-match typed rejection variants
without a CBOR re-walk.

### A6 — Workspace + documentation hygiene
Post-reorganization cleanup guardrails:
- **Workspace members:** closed. The root `Cargo.toml` now explicitly lists
  the shipped `crates/node/cardano-node` binary crate plus all 9 Wave 5
  `crates/node/*` support crates (`block-producer`, `config`, `genesis`,
  `ntc-server`, `ntn-server`, `plutus-eval`, `runtime`, `sync`, `tracer`),
  so `cargo check-all`/`lint`/`test-all` can cover their own targets.
  Current `cargo metadata --no-deps` evidence: 34 workspace packages total,
  including 10 `crates/node/*` packages and 13 `crates/tools/*` packages.
  Keep future extracted crates explicit in `[workspace].members`.
- **`.rs` comment sweep:** live placement comments were normalized after the
  Wave 5 crate split; keep future comments on `crates/node/<sub-crate>/...`
  paths and leave upstream Haskell URLs unchanged.
- **Parity-data files:** `docs/parity-matrix.json` now uses
  `scripts/...` for operator scripts; keep
  `check-parity-matrix.py` green after future path edits.
- **Historical-doc paths:** round-stamped historical narratives now use the
  post-split `crates/node/<sub-crate>/...` paths where they mention local Rust
  files; keep upstream Haskell `cardano-node/...` URLs unchanged.
- **Filetree descriptions:** `.claude/filetree/manifest.json` was bootstrapped
  2026-05-17 with auto-derived descriptions; refine the weak ones incrementally
  via the `cardano-filetree-maintainer` skill / `filetree-reviewer` agent.

---

## Category B — operator-soak gated

Code may be complete; closure needs a long-running rehearsal an automated
environment cannot perform.

### B1 — cardano-tracer full Network.Mux semantics + conformance soak
11 of 12 sub-items shipped (`TECH-DEBT.md` §"cardano-tracer Mux Layer 2/3").
Remaining: full per-mini-protocol queue limits + scheduler fairness, then a
24h+ soak forwarding live traces to a real `cardano-tracer` endpoint.
**Closes with:** the soak harness + a clean 24h run.

### B2 — cardano-submit-api integration soak (R345)
Functional binary exists; needs a drop-in byte-equivalence soak vs the
upstream `cardano-submit-api`. **Closes with:** operator soak →
`verified_11_0_1`.

### B3 — db-truncater integration soak (R351)
Functional; needs integration verification vs upstream `db-truncater`.
**Closes with:** operator soak → `verified_11_0_1`.

### B4 — EKG-parity metrics consolidation  (`TECH-DEBT.md` §"EKG-parity metrics")
`install_prometheus_exporter` has no live consumer; best wired when a sister
tool drives it. **Closes with:** an integration driver + the ~30 `NodeMetrics`
update sites bridged.

### B5 — Production-readiness operator gates
`MANUAL_TEST_RUNBOOK.md` §2–9 mainnet endurance rehearsal (24h+) and the §6.5
parallel-fetch sign-off (`scripts/parallel_blockfetch_soak.sh`)
before flipping the default `max_concurrent_block_fetch_peers` from 1 to 2.

---

## Category C — running-Haskell-node gated

These require a running upstream Haskell `cardano-node` (or pre-captured wire
fixtures) for byte-level forensic comparison. Blocked in any environment
without `.reference-haskell-cardano-node/install/`. Author a `parity-plan`
and delegate to the `haskell-reference-auditor` agent before any fix.

### C1 — Gap BO: preprod TPraos VRF parity (slot ~429,460)
VRF check fails in the Shelley `d=1` federation period. Candidates: overlay
slot mis-classification, active-genesis-delegate VRF check, or TPraos nonce
drift. **Closes with:** per-block VRF input/seed/key diff vs upstream
`classifyOverlaySlot` + `pbftVrfChecks`.

### C2 — Gap BP: preview Plutus V2 cost-budget overrun (slot ~1,462,057)
CEK overruns the CPU budget by ≈0.0185% on a real V2 script. Workaround:
`YGG_SKIP_PHASE2=1` (sync-only; never on a block producer). **Closes with:**
per-builtin step-cost trace diff vs upstream `Cek/Internal.hs::stepAndMaybeSpend`.

### C3 — R178-followup: Conway HFC LSQ response envelope
cardano-cli's HFC decoder expects a different Conway-era LSQ response envelope
than yggdrasil's current `[1, body]` shape. **Closes with:** captured upstream
Conway-era wire fixtures + aligned `encode_query_if_current_match`.

### C4 — Performance: 2× Haskell sync throughput
Yggdrasil ~2,321 slot/s vs Haskell ~5,296 slot/s (0.44×). Needs governor
warm/hot promotion of snapshot peers for multi-peer BlockFetch, batched
Ed25519 verify, pipelined CBOR decode, allocator tuning. **Closes with:** a
side-by-side preview soak vs the Haskell node.

---

## Verification matrix

| Item | Closes when | External dependency |
|---|---|---|
| A1 feature flags | flags conditionally compile; `lint-no-default` green | none |
| A2 cardano-cli ports | each subcommand byte-equivalent to upstream | none (vendored reference) |
| A3 db-synthesizer R2–R3 | Praos forge path plus stake-equivalent synthesized ChainDB | none |
| A4 skeleton tools | each → `implemented_needs_11_0_1_evidence` | none for tx-generator; kes-agent still needs socket-protocol fixtures |
| A5 submit-api errors | typed rejection variants | none |
| A6 hygiene | `cargo metadata --no-deps` lists 34 packages, including all 10 node packages; gates green | none |
| B1 tracer Mux | 24h+ trace-forward soak clean | operator soak |
| B2 submit-api soak | byte-equivalent vs upstream | operator soak |
| B3 db-truncater soak | integration verified vs upstream | operator soak |
| B4 EKG metrics | metrics flow through the global facade | a sister-tool driver |
| B5 operator gates | runbook §2–9 + §6.5 sign-off | 24h+ mainnet rehearsal |
| C1 Gap BO | VRF diff resolved | running Haskell node |
| C2 Gap BP | per-builtin cost diff resolved | running Haskell node |
| C3 R178-followup | envelope aligned to upstream | Conway wire fixtures |
| C4 perf 2× | preview soak ≥ 2× Haskell | side-by-side Haskell node |

## How to use this doc

Work proceeds in the project's R-arc rhythm: one bounded slice per round, the
four cargo gates green between rounds, one `docs/operational-runs/` doc per
round, and the "proceed" human-in-the-loop checkpoint (see the
`continuous-agent-loop` skill). Prefer Category A items first — they are fully
executable and unblock parts of B/C. Update this file whenever an item moves:
closed items graduate to `PARITY_SUMMARY.md` / `PARITY_PROOF.md` and their
`parity-matrix.json` entry.
