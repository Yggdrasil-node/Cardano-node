# Guidance for the pure-Rust port of upstream `tx-generator`.

**Status:** `partial` (post-R577 Conway VotingProcedures map DumpToFile rendering).
The old cardano-cli CLI-MVS prerequisite is closed; concrete work here is now
the tx-generator Script / GeneratorTx / Submission implementation arc
plus upstream comparison evidence. Scope band: **LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/bench/tx-generator/`
(46 `.hs` files).

## Mini-arc scope

Transaction-stream load generator for benchmarking. The active arc
starts from the vendored `Command.hs`, `Setup/*`, and
`GeneratorTx/Submission.hs` surfaces, then finishes with an end-to-end
soak against a yggdrasil node on preview. The Calibrate sub-tree
carve-out (Compiler.hs, Benchmarking/Script/*, PureExample) remains an
approved synthesis area from the sister-tools plan.

## Current Functional Surface

- Shipped: `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- Shipped: `<binary> --version` byte-equivalent to upstream.
- Shipped R533: `Command.hs` parser surface. `command.rs` mirrors the
  upstream `Command` sum type and `commandParser` grammar for `json`,
  `json_highlevel`, `compile`, `selftest`, and `version`.
- Shipped R533: `parser::Args` now carries typed `command::Command`
  instead of raw passthrough.
- Shipped R534: `Setup/TestnetDiscovery.hs` surface. `setup/testnet_discovery.rs`
  discovers `cardano-testnet` output directories, reads node port files,
  builds localhost `targetNodes`, and deep-merges discovered connection
  settings over user JSON config.
- Shipped R534: `json_highlevel --testnet-config-dir DIR` now reads the
  high-level config JSON and performs testnet discovery before reaching
  the command-execution sentinel.
- Shipped R535: `Setup/NixService.hs` high-level config surface.
  `setup/nix_service.rs` parses `NixServiceOptions`, owns upstream
  `NodeDescription`, projects `txGenTxParams` / `txGenConfig` /
  `txGenPlutusParams`, and applies `nodeConfig` / cardano-tracer CLI
  override rules.
- Shipped R535: `json_highlevel` and `compile` now read and validate
  high-level config JSON before reaching their command-execution
  sentinel; `discover_testnet_config` now returns typed
  `NixServiceOptions` like upstream.
- Shipped R536: `Compiler.hs` high-level script generation surface.
  `compiler.rs` emits typed `Action` scripts from `NixServiceOptions`,
  including fixed signing-key envelopes, genesis import, collateral
  setup, split planning, benchmark submission mode selection, and the
  upstream split/fee helper arithmetic.
- Shipped R536: `Benchmarking/Script/Types.hs` action/generator IR
  surface. `script/types.rs` serializes the generated script with
  upstream ObjectWithSingleField-style action, generator, submit-mode,
  pay-mode, and script-budget wrappers.
- Shipped R536: `compile FILEPATH` is functional and writes the
  generated script JSON to stdout; `json_highlevel` compiles its final
  options before reaching the runtime-execution sentinel.
- Shipped R537: `Benchmarking/Script/Aeson.hs` script JSON surface.
  `script/aeson.rs` parses low-level script files and
  `script/types.rs` now decodes upstream ObjectWithSingleField-style
  `Action`, `Generator`, submit-mode, pay-mode, protocol-parameter,
  and script-budget wrappers.
- Shipped R537: `json FILEPATH` now reads and validates low-level
  script JSON before reaching the runtime-execution sentinel.
- Shipped R538: `Benchmarking/Script/Env.hs` state surface.
  `script/env.rs` owns the upstream `Env`, `ProtocolParameterMode`,
  `Error`, wallet/key/protocol placeholders, and accessor semantics
  used by action execution.
- Shipped R538: `Benchmarking/Script/Action.hs` dispatch surface.
  `script/action.rs` executes deterministic state-only actions
  (`SetNetworkId`, `SetSocketPath`, `InitWallet`,
  `SetProtocolParameters`, `ReadSigningKey`, `DefineSigningKey`,
  `AddFund`, `Delay`, `LogMsg`, `Reserved`, and benchmark-control
  checks) and returns explicit runtime-pending errors for protocol,
  query, transaction-generation, and submission actions.
- Shipped R538: `json FILEPATH` now calls `run_script`, so low-level
  scripts execute their supported state prefix before failing at the
  first missing async/runtime boundary.
- Shipped R539: `Benchmarking/Script/Core.hs` state-helper surface.
  `script/core.rs` now owns upstream-shaped `withEra`,
  `setProtocolParameters`, signing-key loading/definition,
  fund insertion, delay, benchmark-control checks, local-connect-info
  carrier, protocol-parameter mode resolution, `submitAction`
  boundary, `initWallet`, version tracing, and `reserved`.
- Shipped R539: `Benchmarking/Script/Action.hs` now mirrors the
  upstream split more closely: dispatch remains in `script/action.rs`,
  while Core-owned action bodies live in `script/core.rs`.
- Shipped R540: `Benchmarking/Script/Core.hs` node-to-client query
  surface. `queryEra` and `queryRemoteProtocolParameters` now build the
  upstream LocalStateQuery envelopes (`QueryHardFork GetCurrentEra` and
  `QueryIfCurrent GetCurrentPParams`), drive the NtC socket on Unix,
  preserve era-native protocol-parameter CBOR in
  `protocol-parameters-queried.json`, and keep non-Unix builds on an
  explicit Unix-socket boundary.
- Shipped R541: `Benchmarking/GeneratorTx/SizedMetadata.hs`
  transaction-metadata sizing surface. `generator_tx/sized_metadata.rs`
  ports the upstream `mkMetadata` chunking algorithm, metadata cost step
  assumptions, and CBOR map/bytes test pins; `Script/Core.toMetadata`
  now preflights `NtoM` metadata payload sizes before the remaining
  transaction-build sentinel.
- Shipped R542: `Cardano.TxGenerator.Internal.Fifo`,
  `Cardano.TxGenerator.Fund`, `Cardano.TxGenerator.FundQueue`, and
  `Cardano.Benchmarking.Wallet` surfaces. Wallet state now uses the
  upstream paired-list FIFO queue behavior, `Fund` equality/order is
  keyed by `TxIn`, and wallet source/preview semantics match upstream
  before the remaining transaction-assembly slice consumes them.
- Shipped R543: `Cardano.TxGenerator.Utils` pure value-splitting
  surface. `tx_generator/utils.rs` ports `inputsToOutputsWithFee`,
  `includeChange`, and the upstream `mkTxIn` parser; `Script/Core` now
  preflights `Split`, `SplitN`, and `NtoM` wallet value splitting
  before the remaining transaction-build sentinel.
- Shipped R544: `Cardano.TxGenerator.UTxO` output-builder surface.
  `tx_generator/utxo.rs` ports `ToUTxO`, `ToUTxOList`,
  `makeToUTxOList`, key-address derivation, and the key-witness
  `mkUTxOVariant` path across Shelley-family output encodings. Script
  outputs stay with the Plutus witness-builder slice.
- Shipped R545: `Benchmarking/Script/Core.hs` pay-mode and collateral
  preflight. `script/core.rs` now ports `selectCollateralFunds` and the
  key-output half of `interpretPayMode`, traces upstream-shaped
  `Split`, `SplitN`, and `NtoM` output-address messages before value
  splitting, and keeps `PayToScript` on an explicit
  `makePlutusContext` / `mkUTxOScript` boundary.
- Shipped R546: `Cardano.TxGenerator.UTxO.mkUTxOScript` output and
  fund construction. `tx_generator/utxo.rs` now builds Plutus script
  enterprise addresses, datum-hash outputs for Alonzo and Babbage-family
  eras, era/language support errors, and script-witnessed funds with no
  signing key; `PayToScript` now waits only on `makePlutusContext`.
- Shipped R547: static-budget `makePlutusContext` path.
  `setup/plutus.rs` ports upstream `.plutus` text-envelope loading and
  bundled `scripts-fallback` resolution; `tx_generator/plutus_context.rs`
  ports detailed-schema `readScriptData` plus `scriptDataModifyNumber`;
  `script/core.rs` now resolves `PayToScript` static budgets into
  `mkUTxOScript` builders carrying real datum/redeemer/execution-unit
  witness data. R556 extends the static path with upstream-shaped
  `preExecutePlutusScript` checking; R557 wires upstream-shaped
  AutoScript budget fitting through `plutusAutoScaleBlockfit`.
- Shipped R548: `Cardano.TxGenerator.Tx` key-spend transaction
  construction. `tx_generator/tx.rs` ports
  `sourceToStoreTransaction`, `sourceToStoreTransactionNew`,
  `sourceTransactionPreview`, `genTx`, and `txSizeInBytes` for
  Shelley-family key-witnessed inputs. Generated transactions are real
  `MultiEraSubmittedTx` values with vkey witnesses over the body hash,
  metadata auxiliary-data hashes, collateral input fields, and tx-id-
  based generated-fund storage. `wallet.rs` now carries upstream
  `createAndStore`, `mangle`, and `mangleWithChange` helpers. R555
  extends this same mirror to Plutus script-spending witnesses.
- Shipped R549: `Benchmarking.Script.Core.submitInEra` finite
  key-spend runtime wiring. `script/core.rs` now evaluates `Split`,
  `SplitN`, `NtoM`, `Sequence`, and `Take (Cycle ...)` into real
  generated transactions, mutates source/destination wallets through
  upstream-shaped source/store semantics, previews `NtoM` tx size
  traces, supports `DiscardTX`, and submits finite streams over NtC
  LocalTxSubmission for `LocalSocket`. R559 adds the Allegra selftest
  `DumpToFile` renderer; benchmark mode remains blocked on the
  `GeneratorTx.Submission` client/scheduler slice.
- Shipped R550: `Benchmarking.Command.runCommand` high-level execution
  path. `json_highlevel FILE` now parses/discovers config, applies
  node/tracer overrides, prints initial/final option snapshots, runs
  upstream `quickTestPlutusDataOrDie`-style datum/redeemer preflight,
  compiles with `compileOptions`, and passes the generated script to
  `run_script`. The explicit `version` subcommand now emits the same
  version fixture as top-level `--version`.
- Shipped R551: `Benchmarking.Script.Action.startProtocol` now reads
  the node config with JSON/YAML fallback via `yggdrasil-node-config`,
  rejects non-Cardano protocol configs, sets protocol/genesis carriers,
  derives upstream-shaped `Testnet NetworkMagic` state, and initializes
  benchmark tracers instead of stopping at the old
  `mkConsensusProtocol` sentinel.
- Shipped R552: `Cardano.TxGenerator.Genesis` and
  `Generator.SecureGenesis` are wired. `startProtocol` verifies and
  loads Shelley genesis initial funds through `yggdrasil-node-genesis`;
  `SecureGenesis` now finds the matching genesis UTxO by key-derived
  address, spends the genesis pseudo-input with a GenesisUTxO witness,
  applies `txParamFee` / `txParamTTL`, and stores the generated payment
  fund in the target wallet.
- Shipped R553: `Benchmarking.Script.Selftest` no-output-file path.
  `script/selftest.rs` ports the upstream static action list and the
  `selftest` command now runs the full DiscardTX self-test script
  against bundled upstream protocol parameters. R559 extends this path
  to `selftest FILEPATH` with an Allegra Haskell `Show (Tx)` renderer.
- Shipped R554: `RoundRobin` / `OneOf` upstream-TODO error-shape
  parity. Upstream `Core.hs` intentionally crashes with
  `return $ foldr1 Streaming.interleaves gList` and
  `todo: implement Quickcheck style oneOf generator`; the Rust
  `Script/Core` mirror now returns those exact `TxGenError` strings
  instead of local placeholder wording.
- Shipped R555: Plutus script-spend transaction assembly. `genTx`
  now accepts ledger protocol parameters, includes Plutus V1/V2/V3
  scripts, datums, redeemers, and `script_data_hash` in Alonzo-family
  transactions, signs collateral keys, and lets finite `DiscardTX`
  streams spend script funds with static budgets. R556 extends this
  path with pre-execution checking for static budgets.
- Shipped R556: `Cardano.TxGenerator.Setup.Plutus.preExecutePlutusScript`.
  `setup/plutus.rs` now decodes Plutus V1/V2/V3 scripts through the
  shared pure-Rust Flat decoder, builds the upstream dummy
  `ScriptContext` shapes, runs the CEK evaluator with active cost
  models and per-transaction limits, and returns measured
  `ExecutionUnits`. `Script/Core.makePlutusContext` now honors
  `StaticScriptBudget(..., withCheck=true)` and rejects mismatched
  stated budgets like upstream.
- Shipped R557: `Cardano.TxGenerator.PlutusContext` auto-budget fitting.
  `tx_generator/plutus_context.rs` now ports `PlutusAutoBudget`,
  `PlutusBudgetFittingStrategy`, `plutusAutoBudgetMaxOut`,
  `plutusAutoScaleBlockfit`, `plutusBudgetSummary`, and the upstream
  binary-search boundary. `Script/Core.makePlutusContext` now resolves
  `AutoScript` budgets, writes `plutus-budget-summary.json`, and parses
  `maxBlockExecutionUnits` from JSON protocol parameters.
- Shipped R558: `Benchmarking.Script.Core.previewNtoMTransaction`
  summary projection. Successful `NtoM` previews now trace the
  projected transaction size and upstream-shaped `Maybe Coin` fee text,
  update `projectedTxSize` / `projectedTxFee` in the environment budget
  summary when one exists, and refresh `plutus-budget-summary.json`.
- Shipped R559: `Benchmarking.Script.Core.submitInEra` Allegra
  `DumpToFile` selftest rendering. `SubmitMode::DumpToFile` now
  evaluates finite streams and writes newline-prefixed Haskell
  `ShelleyTx ShelleyBasedEraAllegra` records for the deterministic
  selftest path.
- Shipped R560: Shelley-family transaction body `StrictSeq` fields now
  follow upstream `cardano-ledger-binary` variable-length encoding
  (definite through 23 elements, indefinite above). This closes the
  R559 generated-transaction drift: the upstream-captured selftest
  setup stages and final 4,000-record `DumpToFile` stream now match
  Rust byte-for-byte.
- Shipped R561: `Cardano.Benchmarking.Types` and
  `Cardano.Benchmarking.TpsThrottle` mirrors now provide the upstream
  request/ack/sent/unavailable counters, submission error policy, and
  TMVar-style TPS watermark semantics that `GeneratorTx.Submission`
  and `walletBenchmark` consume.
- Shipped R562: `Cardano.Benchmarking.LogTypes` and
  `Cardano.Benchmarking.GeneratorTx.SubmissionClient` mirrors now
  provide upstream-shaped submission trace/summary types plus the pure
  request/response state machine for ack handling, tx-id announcement,
  tx-body lookup, unavailable accounting, and per-thread stats.
- Shipped R563: `Cardano.Benchmarking.GeneratorTx.Submission` now
  provides upstream-shaped `SubmissionParams`, `ReportRef`,
  `SubmissionThreadReport`, report publication helpers,
  `mkSubmissionSummary`, `StreamState`, `SharedTxStream`, and
  `txStreamSource` over the R561 TPS throttle and R562
  `SubmissionClient` source boundary.
- Shipped R564: `Cardano.Benchmarking.GeneratorTx.SubmissionClient`
  now drives the typed `yggdrasil_network` TxSubmission2 client
  through `run_tx_submission_client`, translating server tx-id/body
  requests into the upstream-shaped local request state and sending
  tx-id replies, tx-body replies, or `MsgDone` on the wire. A muxed
  loopback test covers `MsgInit -> RequestTxIds -> RequestTxs ->
  MsgDone`.
- Shipped R565: `Cardano.Benchmarking.GeneratorTx.walletBenchmark`
  now resolves IPv4 target nodes, proposes upstream NtN V14
  initiator-only version data, spawns one TxSubmission2 worker per
  target, spawns the TPS feeder, exposes shutdown/summary control, and
  has a peer-accept/peer-connect loopback test that submits generated
  transactions through the negotiated TxSubmission2 mini-protocol.
- Shipped R566: `Benchmarking.Script.Core.submitInEra` now wires
  `SubmitMode::Benchmark` into the R565 `wallet_benchmark` control,
  stores a real `AsyncBenchmarkControl` in `Script/Env.hs`, keeps the
  Tokio runtime alive across `Submit` and `WaitBenchmark`, and has a
  script-core loopback test covering generated transaction submission
  plus summary tracing.
- Shipped R567/R568: `SubmitMode::DumpToFile` now renders
  Haskell-shaped Shelley, Mary, and Alonzo key-witnessed streams in
  addition to the byte-equivalent Allegra selftest fixture. Unsupported
  optional fields, multi-asset Mary/Alonzo values, Plutus-bearing Alonzo
  witnesses, and non-vkey witnesses remain explicit `TxGenError`
  boundaries instead of approximate `Show` output.
- Shipped R569: `SubmitMode::DumpToFile` now renders Babbage
  key-witnessed streams via `show_babbage_tx_for_dump`. The body emits
  the upstream 16-field `BabbageTxBodyRaw` record (including
  `btbrCollateralInputs`, `btbrReferenceInputs`, `btbrCollateralReturn`,
  `btbrTotalCollateral`), wraps outputs as `Sized {sizedValue = (addr,
  val, datum, refScript), sizedSize = N}` 4-tuples with `NoDatum` /
  `DatumHash (SafeHash ...)` and `SNothing` constructors, and reuses
  `AlonzoTxWitsRaw` for the witness set. Inline datums, reference
  scripts, and the remaining Plutus-bearing Babbage shapes stay on
  explicit `TxGenError` boundaries.
- Shipped R577: `show_conway_tx_for_dump` now renders non-empty
  `ctbrVotingProcedures` map as upstream `VotingProcedures
  {unVotingProcedures = fromList [(Voter, fromList [(GovActionId,
  VotingProcedure)])]}`. New helpers `show_conway_vote` (VoteNo /
  VoteYes / Abstain), `show_conway_voter` (5 variants:
  CommitteeVoter/DRepVoter as `(KeyHashObj (KeyHash {unKeyHash = ...}))`
  or `(ScriptHashObj (ScriptHash ...))`, StakePoolVoter as `(KeyHash
  {unKeyHash = ...})`), `show_conway_gov_action_id` (record with
  `TxId {unTxId = SafeHash}` and `GovActionIx {unGovActionIx}`),
  `show_conway_voting_procedure` (record with `vProcVote` and
  `vProcAnchor :: StrictMaybe Anchor`), `show_anchor` (record with
  `anchorUrl :: Url` and `anchorDataHash :: SafeHash`), and `show_url`
  (record-newtype `Url {urlToText = ...}`). 5 focused unit tests cover
  Vote variants, all 5 Voter variants, GovActionId record shape,
  VotingProcedure with and without anchor, and full VotingProcedures
  map rendering (empty and non-empty cases). ProposalProcedures map
  with its GovAction 7+ variants remains on explicit TxGenError.
- Shipped R576: `show_conway_tx_for_dump` now accepts non-zero
  `ctbrTreasuryDonation` and `Some` `ctbrCurrentTreasuryValue`. New
  helpers `show_coin` (`Coin <n>` mirroring upstream `Show Coin` via
  `Quiet Coin`) and `show_strict_maybe_coin` (`SNothing` or `SJust
  (Coin <n>)` with the `Coin` wrapped in parens for showsPrec 11
  inside `SJust`). `VotingProcedures` and `ProposalProcedures` map
  rendering are deferred to dedicated rounds (rich nested types:
  `Voter`, `GovActionId`, `VotingProcedure`, `Vote`, `Anchor`,
  `ProposalProcedure`, `GovAction` with 7+ variants).
- Shipped R575: `show_alonzo_witness_set` now renders non-empty
  Plutus V1/V2/V3 script-witness bytes as upstream `atwrScriptTxWits =
  fromList [(ScriptHash "<hex>", PlutusScript PlutusV{N} ScriptHash
  "<hex>"), ...]` matching `Show (Map ScriptHash (AlonzoScript era))`.
  Entries sort by script-hash byte-lex order (mirroring upstream
  `Data.Map toAscList`). Reuses R574's `plutus_script_hash`. Native
  scripts and bootstrap witnesses remain `TxGenError` until the
  Timelock Show is ported. 4 focused unit tests cover empty
  script-witness map (regression guard), single PlutusV2 entry,
  multi-version (V1+V2) byte-lex sort order, and native-script
  rejection error.
- Shipped R574: `show_babbage_script_ref` now renders Plutus
  reference scripts as upstream `SJust PlutusScript PlutusV{1,2,3}
  ScriptHash "<hex>"`, matching upstream `Show (AlonzoScript era)`
  (custom Show that emits `"PlutusScript " ++ show language ++ " " ++
  show (hashScript @era s)`). Script hash domain: `Blake2b-224 over
  (language-tag-byte ++ script_bytes)`, tags 0x01/0x02/0x03 for
  PlutusV1/V2/V3. New `plutus_script_hash` helper. Native reference
  scripts (`Script::Native`) remain `TxGenError` until the Timelock
  Show is ported. 3 focused unit tests cover SNothing/V1/V2/V3
  rendering with cross-language hash distinctness, native-script
  rejection, and the hash-domain invariant.
- Shipped R573: `show_babbage_datum` now renders inline datums
  (`DatumOption::Inline(PlutusData)`) as upstream `Datum (BinaryData
  "<latin1-escaped-cbor>")`, using `show_haskell_bytestring` over the
  PlutusData's canonical CBOR. Inline datums no longer block the
  Babbage/Conway `DumpToFile` path; reference scripts and Plutus
  script-witness bytes remain `TxGenError` boundaries. 3 focused unit
  tests cover `NoDatum`, `DatumHash`, simple-integer inline datum,
  and nested-Constr inline datum.
- Shipped R572: `show_alonzo_witness_set` now renders non-empty
  `plutus_data` and `redeemers` instead of returning `TxGenError`. New
  helpers `show_plutus_data` (matching upstream stock-derived `Show
  (PV1.Data)`: `Constr <i> [...]`, `Map [...]`, `List [...]`, `I <n>`,
  `B <bytestring>`), `show_haskell_bytestring` (Latin1 byte-string
  Show — printable ASCII inline, `"` and `\` escaped, `\n`/`\t`/`\r`
  named escapes, `\NNN` decimal escapes for bytes >= 0x80 with `\&`
  separator before following digits), `show_alonzo_tx_dats` (full
  `MkTxDats (TxDatsRaw {unTxDatsRaw = fromList [(SafeHash
  "<hex>",MkData I 42 (blake2b_256: SafeHash "<hex>"))...]}
  (blake2b_256: SafeHash "<hex>"))` envelope with sorted DataHash
  ordering and definite-length set-tag CBOR over each datum's
  canonical CBOR for the outer hash), `show_alonzo_redeemers` (full
  `MkRedeemers (RedeemersRaw {unRedeemersRaw = fromList
  [(AlonzoSpending (AsIx {unAsIx = N}),(...Data...,ExUnits {exUnitsMem
  = ..., exUnitsSteps = ...}))]} (blake2b_256: SafeHash "<hex>"))`
  envelope with `(tag, index)` sorted ordering and definite-length
  array-of-`[tag,index,data,ex_units]` CBOR for the outer hash), plus
  `show_alonzo_plutus_purpose` (tag→constructor: 0
  `AlonzoSpending`, 1 `AlonzoMinting`, 2 `AlonzoCertifying`, 3
  `AlonzoRewarding`) and `show_alonzo_ex_units`. 7 focused unit tests
  cover the renderer surface: integer signs, byte-string escapes
  (printable, non-ASCII, escape-boundary, backslash/quote escapes),
  List, Map+Constr nesting, single Spending redeemer, multi-redeemer
  `(tag, index)` sort, single-datum TxDats. Native scripts, bootstrap
  witnesses, and Plutus V1/V2/V3 script witnesses still return
  `TxGenError` until their downstream mirrors land. Note: the
  bytestring renderer matches structural Haskell `Show (ByteString)`
  without the full mnemonic-escape set (`\NUL`, `\SOH`, ...,
  `\DEL`); upstream-binary soak evidence will close that for byte
  parity.
- Shipped R571: `show_mary_value` now renders non-empty `MultiAsset`
  bundles for Mary/Alonzo/Babbage/Conway transaction outputs, mirroring
  upstream `Show (MaryValue)` and `Show (MultiAsset)`:
  `MaryValue (Coin N) (MultiAsset (fromList [(PolicyID {policyID =
  ScriptHash "<hex>"},fromList [("<asset-hex>",<qty>),...]),...]))`.
  Entry order tracks `BTreeMap` byte-lex which matches upstream
  `Data.Map toAscList` ordering on `PolicyID` (Ord via `ScriptHash`
  Hash bytes) and `AssetName` (Ord via `ShortByteString` bytes). The
  multi-asset boundary is now lifted from `show_mary_tx_out`,
  `show_alonzo_tx_out`, and `show_babbage_tx_out` automatically (those
  wrappers were only forwarding the rejection). Output renderers in
  Mary/Alonzo/Babbage/Conway DumpToFile paths will pick this up once
  gen_tx supports producing multi-asset outputs.
- Shipped R570: `SubmitMode::DumpToFile` now renders Conway
  key-witnessed streams via `show_conway_tx_for_dump`. The body emits
  the upstream 19-field `ConwayTxBodyRaw` record — renamed
  `ctbrSpendInputs` (vs `btbrInputs`), combined `ctbrVldt`
  `ValidityInterval`, `ctbrCerts` carried as an `OSet {osSSeq =
  StrictSeq ..., osSet = ...}` (Conway moved off `StrictSeq`),
  `btbrUpdate` dropped, plus the four governance fields
  `ctbrVotingProcedures = VotingProcedures {unVotingProcedures = ...}`,
  `ctbrProposalProcedures = OSet {...}`, `ctbrCurrentTreasuryValue =
  SNothing`, `ctbrTreasuryDonation = Coin 0`. Outputs reuse
  `show_babbage_tx_out_list` (Conway shares `BabbageTxOut`), witnesses
  reuse `show_alonzo_witness_set` (Conway `TxWits = AlonzoTxWits`), and
  the envelope reads `ShelleyTx ShelleyBasedEraConway (AlonzoTx
  ...)`. The `show_tx_for_dump` match is now exhaustive across all
  `MultiEraSubmittedTx` variants. Inline datums, reference scripts,
  non-empty governance procedures, non-zero treasury donations, and the
  remaining Plutus-bearing Conway shapes stay on explicit `TxGenError`
  boundaries.
- Pending: low-level `json FILE` and
  high-level `json_highlevel FILE` now run supported script actions,
  including finite key-spend Submit actions, and stop only at the next
  explicit runtime parity boundary.
- Pending: extend `DumpToFile` Show rendering into Plutus-bearing
  Babbage / Conway transactions (inline datums, reference scripts,
  Plutus witness sets, governance procedures) and capture
  upstream-binary soak evidence for Benchmark scripts.

## Build + Run

```bash
# Build (release).
cargo build --release -p yggdrasil-tx-generator

# Run via the universal launcher (recommended).
scripts/run-tools.sh tx-generator --help
scripts/run-tools.sh tx-generator --version

# Or invoke the binary directly:
target/release/tx-generator --help
```

The binary is named `tx-generator` (matching upstream exactly).
Operators can swap upstream's binary for the yggdrasil one in their
automation once concrete dispatch and upstream comparison evidence land.

## Rules

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `tx-generator` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies from
  crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream ships a
  new release with different help output, refresh the fixtures + bump
  the relevant SHA pin in `crates/node/config/src/upstream_pins.rs` as
  a coordinated round.

## Round Roadmap

This crate's full implementation remains an A4 sister-tool build-out:

- Shipped: skeleton (R327 + R335-pattern bulk skeleton at R335-R336).
- Shipped: Command parser (R533): `Command.hs` `Command`,
  `TestnetConfig`, and command-parser grammar.
- Shipped: Testnet discovery (R534): `Setup/TestnetDiscovery.hs`
  path conventions, node discovery, JSON deep-merge, and runtime
  `json_highlevel --testnet-config-dir` preparation.
- Shipped: Nix-service options (R535): `Setup/NixService.hs`
  high-level JSON shape, target-node parsing, config/tracer override
  helpers, and tx-generator parameter projections.
- Shipped: Compiler/script generation (R536): `Compiler.hs`
  `compileOptions` plus the `Script/Types.hs` IR needed for generated
  scripts; `compile` now emits generated action JSON.
- Shipped: Script JSON parsing (R537): `Script/Aeson.hs`
  `parseScriptFileAeson`, `scanScriptFile`, JSON round-trip checking,
  and low-level `json FILEPATH` script validation.
- Shipped: Script state/action execution (R538): `Script.hs`
  `runScript` boundary plus `Script/Env.hs` state/accessors and
  `Script/Action.hs` deterministic state-only action dispatch.
- Shipped: Script/Core state helpers (R539): `Script/Core.hs`
  non-network state helpers and explicit runtime boundaries moved into
  a strict mirror file.
- Shipped: Script/Core NtC query behavior (R540): `queryEra` /
  `queryRemoteProtocolParameters` use upstream LocalStateQuery wire
  shapes and write queried protocol-parameter evidence.
- Shipped: GeneratorTx sized metadata (R541): `SizedMetadata.hs`
  `mkMetadata` and cost assumptions feed `Script/Core.toMetadata` for
  `NtoM` additional-size payloads.
- Shipped: Fund/FundQueue/Wallet runtime (R542): upstream FIFO-backed
  wallet storage, fund accessors, source, and preview behavior.
- Shipped: TxGenerator Utils/value splitting (R543):
  `inputsToOutputsWithFee`, `includeChange`, `mkTxIn`, and
  `Script/Core.submitInEra` value preflight for `Split`, `SplitN`, and
  `NtoM`.
- Shipped: TxGenerator UTxO output builders (R544):
  `ToUTxO`, `ToUTxOList`, `makeToUTxOList`, key-address derivation,
  and key-witnessed `mkUTxOVariant` output/fund construction.
- Shipped: Script/Core pay-mode/collateral preflight (R545):
  `selectCollateralFunds`, key-output `interpretPayMode`, and
  upstream-shaped address trace points for `Split`, `SplitN`, and
  `NtoM` before the transaction-build sentinel.
- Shipped: TxGenerator script UTxO output builders (R546):
  `mkUTxOScript`, Plutus script address hashing, datum hashes, and
  script-witnessed generated funds without signing keys.
- Shipped: static Plutus context (R547): `Setup/Plutus.hs`
  text-envelope/fallback-script loading, `PlutusContext.hs`
  detailed-schema script-data parsing and first-number mutation helper,
  plus static-budget `makePlutusContext` wiring for `PayToScript`.
- Shipped: key-spend transaction construction (R548):
  `Cardano.TxGenerator.Tx` source/store/preview functions plus
  signed Shelley-family `genTx` for key-witnessed inputs, and
  `Benchmarking.Wallet` create/store mangling helpers.
- Shipped: finite `submitInEra` runtime wiring (R549):
  `Script/Core.submitInEra` now evaluates finite key-spend generators,
  updates wallets, supports `DiscardTX`, and drives `LocalSocket`
  through NtC LocalTxSubmission.
- Shipped: high-level command execution (R550):
  `Benchmarking.Command.runCommand` now drives `json_highlevel` through
  config discovery/mangling, Plutus data preflight, `compileOptions`,
  and `run_script`; `version` subcommand is concrete.
- Shipped: StartProtocol env wiring (R551):
  `Benchmarking.Script.Action.startProtocol` now loads node config,
  sets protocol/genesis/network/tracer state, and lets high-level runs
  advance to the next concrete script/runtime boundary.
- Shipped: SecureGenesis runtime (R552):
  `Cardano.TxGenerator.Genesis` now spends Shelley genesis initial
  funds into wallet-managed payment funds, with hash-verified genesis
  loading during `startProtocol`.
- Shipped: selftest DiscardTX execution (R553):
  `Benchmarking.Script.Selftest` now builds and runs the upstream
  static self-test action list without an output file.
- Shipped: RoundRobin/OneOf upstream-TODO parity (R554):
  `Benchmarking.Script.Core` now preserves the exact upstream
  unimplemented error text for both constructors.
- Shipped: script-spend transaction assembly (R555):
  `Cardano.TxGenerator.Tx.genTx` now builds script-spend witness sets
  and script-integrity hashes for static-budget Plutus funds.
- Shipped: Plutus pre-execution checking (R556):
  `Cardano.TxGenerator.Setup.Plutus.preExecutePlutusScript` now
  pre-runs static-budget scripts with the shared CEK evaluator and
  `Benchmarking.Script.Core.makePlutusContext` honors `withCheck`.
- Shipped: Plutus auto-budget fitting (R557):
  `Cardano.TxGenerator.PlutusContext` now fits loop redeemers with the
  upstream binary-search strategy and `Benchmarking.Script.Core` writes
  `plutus-budget-summary.json` for `AutoScript`.
- Shipped: NtoM preview budget-summary projection (R558):
  `Benchmarking.Script.Core.previewNtoMTransaction` now feeds the
  projected serialized transaction size and calculated fee back into the
  Plutus budget summary before dumping it.
- Shipped: Allegra selftest DumpToFile rendering (R559):
  `Benchmarking.Script.Core.submitInEra` now writes upstream-shaped
  newline-prefixed `ShelleyTx ShelleyBasedEraAllegra` records for
  `selftest FILEPATH`.
- Shipped: StrictSeq selftest byte-equivalence (R560):
  Shelley-family transaction body output/certificate sequences now use
  upstream variable-length CBOR, closing the R559 30-output split drift;
  all selftest setup stages and the final 4,000-record stream compare
  byte-for-byte against the vendored upstream binary.
- Shipped: Benchmarking.Types/TpsThrottle foundation (R561):
  `benchmarking/types.rs` and `benchmarking/tps_throttle.rs` port the
  upstream benchmark counter wrappers, submission error policy, and
  TPS watermark gate used by `GeneratorTx.Submission`.
- Shipped: LogTypes/SubmissionClient core (R562):
  `benchmarking/log_types.rs` and
  `generator_tx/submission_client.rs` port the upstream
  submission-summary/tracing carriers and the requestTxIds/requestTxs
  state machine that the later network loop will drive.
- Shipped: GeneratorTx.Submission stream source (R563):
  `generator_tx/submission.rs` ports the upstream report refs,
  submission summaries, stream state, and `txStreamSource` bridge over
  the TPS throttle and `TxSource` boundary.
- Shipped: SubmissionClient TxSubmission2 wire driver (R564):
  `generator_tx/submission_client.rs` now bridges the upstream-shaped
  request state into the typed `yggdrasil_network` TxSubmission2
  client and has muxed loopback coverage through `TxSubmissionServer`.
- Shipped: walletBenchmark NtN control/connect layer (R565):
  `generator_tx.rs` now owns `wallet_benchmark`, target IPv4
  resolution, V14 NtN proposal construction, worker/feeder spawning,
  shutdown, and summary collection with real peer-connect loopback
  coverage.
- Shipped: Script/Core Benchmark env-control wiring (R566):
  `SubmitMode::Benchmark` now evaluates generated transactions,
  launches `wallet_benchmark`, stores a real `AsyncBenchmarkControl`
  with its runtime in `Env`, and `WaitBenchmark` traces the summary.
- Shipped: Shelley/Mary/Alonzo key-witnessed DumpToFile rendering
  (R567/R568): `SubmitMode::DumpToFile` now accepts Shelley, Mary, and
  Alonzo key-witnessed streams, preserving upstream constructor names
  and body/witness hashes while rejecting unsupported optional fields
  explicitly.
- Shipped: Babbage key-witnessed DumpToFile rendering (R569):
  `SubmitMode::DumpToFile` accepts Babbage key-witnessed streams with
  the 16-field `BabbageTxBodyRaw` record, `Sized` output wrappers,
  `NoDatum` / `DatumHash` datum shape, and upstream `ShelleyTx
  ShelleyBasedEraBabbage (AlonzoTx ...)` envelope. Reference scripts
  and inline datums stay on explicit `TxGenError` boundaries.
- Shipped: Conway key-witnessed DumpToFile rendering (R570):
  `SubmitMode::DumpToFile` accepts Conway key-witnessed streams with
  the 19-field `ConwayTxBodyRaw` record (governance-aware:
  `ctbrSpendInputs` rename, `ctbrVldt`, `ctbrCerts` OSet, dropped
  `btbrUpdate`, plus `ctbrVotingProcedures` / `ctbrProposalProcedures`
  / `ctbrCurrentTreasuryValue` / `ctbrTreasuryDonation`), reusing
  `show_babbage_tx_out_list` for outputs and `show_alonzo_witness_set`
  for witnesses, and emits the `ShelleyTx ShelleyBasedEraConway
  (AlonzoTx ...)` envelope. The match in `show_tx_for_dump` is now
  exhaustive across `MultiEraSubmittedTx`.
- Shipped: Mary multi-asset value DumpToFile rendering (R571):
  `show_mary_value` now produces the upstream `MaryValue (Coin N)
  (MultiAsset (fromList [(PolicyID {...},fromList [(...,qty)])]))`
  Show output for non-empty multi-asset bundles. Lifts the multi-asset
  boundary across the Mary, Alonzo, Babbage, and Conway `tx_out`
  renderers in one round.
- Shipped: Plutus-bearing TxDats + Redeemers DumpToFile rendering
  (R572): `show_alonzo_witness_set` now renders non-empty
  `plutus_data` and `redeemers` via `show_plutus_data`,
  `show_haskell_bytestring`, `show_alonzo_tx_dats`,
  `show_alonzo_redeemers`, `show_alonzo_plutus_purpose`, and
  `show_alonzo_ex_units`, including blake2b_256 hashes computed from
  the upstream-canonical CBOR shape (`tag 258` + array for TxDats,
  array-of-`[tag,index,data,ex_units]` for Redeemers) and `(tag,
  index)`-sorted redeemer ordering matching upstream `Map PlutusPurpose
  AsIx era` traversal. Native scripts, bootstrap witnesses, and Plutus
  V1/V2/V3 script witnesses still return `TxGenError`.
- Shipped: Inline-datum DumpToFile rendering (R573):
  `show_babbage_datum` now renders `DatumOption::Inline(PlutusData)`
  as upstream `Datum (BinaryData "<latin1-escaped-cbor>")` using
  `show_haskell_bytestring` over the PlutusData's canonical CBOR.
- Shipped: Plutus reference-script DumpToFile rendering (R574):
  `show_babbage_script_ref` now renders Plutus V1/V2/V3 reference
  scripts as upstream `SJust PlutusScript PlutusV{1,2,3} ScriptHash
  "<hex>"` with Blake2b-224 over (language tag ++ script bytes).
  Native reference scripts remain on `TxGenError`.
- Shipped: Plutus witness-set script DumpToFile rendering (R575):
  `show_alonzo_witness_set` now renders non-empty Plutus V1/V2/V3
  script-witness bytes as `atwrScriptTxWits = fromList [(ScriptHash
  "<hex>",PlutusScript PlutusV{N} ScriptHash "<hex>"),...]`. Entries
  sort by script-hash byte-lex order.
- Shipped: Conway treasury-field DumpToFile rendering (R576):
  `show_conway_tx_for_dump` now accepts non-zero
  `ctbrTreasuryDonation` and `Some` `ctbrCurrentTreasuryValue` via
  new `show_coin` and `show_strict_maybe_coin` helpers.
  `VotingProcedures` and `ProposalProcedures` map rendering remain
  `TxGenError` until their dedicated rounds.
- Shipped: Conway VotingProcedures map DumpToFile rendering (R577):
  `show_conway_voting_procedures` renders the nested
  `Map Voter (Map GovActionId VotingProcedure)` shape with
  upstream-shaped Vote / Voter / GovActionId / VotingProcedure /
  Anchor / Url helpers.
- Next: native-script reference rendering (Timelock Show),
  native-script and bootstrap-witness rendering in the witness set,
  Conway `ProposalProcedures` map (GovAction 7+ variants), and
  upstream-binary soak in strict-mirror-sized slices.
- Closeout: when all subcommands are functional, parity-matrix entry
  advances `partial -> verified_11_0_1`. Operators can then swap
  upstream binary for the yggdrasil binary without script changes.

## Comparison With Upstream

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-tx-generator

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --help) \
     <(target/debug/tx-generator --help)
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --version) \
     <(target/debug/tx-generator --version)
# (empty diffs expected; byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete command implementations land.
- Keep the per-tool migration status in sync with
  `docs/COMPLETION_ROADMAP.md` and `docs/parity-matrix.json`.
- If upstream ships a new release: refresh the help/version fixtures,
  advance the relevant SHA pin in `upstream_pins.rs`, and re-run the
  full cargo gate.
