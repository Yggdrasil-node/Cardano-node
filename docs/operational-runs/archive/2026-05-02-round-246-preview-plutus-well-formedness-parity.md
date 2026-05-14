# Round 246 - preview Plutus well-formedness parity

Date: 2026-05-02

## Summary

R246 closes the preview replay blocker that surfaced as
`MalformedReferenceScripts` near Babbage slot `730728`, then exposed a
later Plutus validation mismatch at slot `840719`. Additional preview
replay exposed and fixed two follow-up parity issues: Plutus `serialiseData`
CBOR shape for non-empty lists/constructor fields, and legacy Shelley
stake-registration certificate witness/redeemer collection.

The fix keeps the upstream well-formedness rule active while aligning
the local Plutus path with upstream `PlutusBinary` handling and the
observed Babbage/Conway `TxInfo`/CEK details:

- On-chain script bytes are treated as raw `PlutusBinary` bytes at the
  evaluator boundary: a CBOR bytestring containing the Flat UPLC program.
  Ledger call sites pass protocol version so language gates remain active.
- Babbage/Conway reference inputs are sorted by `ShelleyTxIn`, matching
  upstream `Set.toList` ordering before ScriptContext construction.
- CEK `ExMemoryUsage` for non-constant runtime values is `1`, not `0`.
- Plutus `Integer` values are arbitrary precision across Flat decoding,
  CBOR `PlutusData`, and integer builtins, matching upstream Haskell
  `Integer` behavior instead of fixed-width `i128`.
- Pre-Conway upper-only validity intervals encode the upper bound with
  inclusive `PV1.to`; Conway/PV9+ uses the strict upper-bound encoding.
- `serialiseData` now follows upstream `PlutusCore.Data`: compact
  constructor tags 121-127, extended tags 1280-1400, general tag 102,
  64-byte bounded bytes chunks, and non-empty Plutus lists/constructor
  field lists using the Haskell-list indefinite-array shape.
- Legacy `AccountRegistration` certificates do not require credential
  witnesses or Certifying redeemers; Conway deposit-bearing registration
  certificates still do.

## Local Impact

- `crates/ledger/src/plutus_validation.rs` now normalizes
  `resolved_reference_inputs` deterministically before node-side
  ScriptContext encoding.
- `crates/ledger/src/plutus.rs` now represents `PlutusData::Integer` as
  arbitrary-precision `BigInt` and round-trips CBOR tags 2/3 without
  fixed-width overflow.
- `crates/plutus/src/types.rs`, `flat.rs`, and `builtins.rs` now use
  arbitrary-precision `BigInt` for UPLC integer constants, Flat integer
  decode, and integer builtin arithmetic.
- `crates/plutus/src/cost_model.rs` matches upstream CEK memory
  accounting for `VDelay`, `VLamAbs`, builtins, constructors, and
  arbitrary-precision integer word sizing.
- `node/src/plutus_eval.rs` makes validity-interval encoding
  protocol-version aware and includes opt-in failure diagnostics for
  replay investigations via `YGGDRASIL_PLUTUS_TRACE_FAILURES`.
- `crates/ledger/src/plutus.rs` now emits upstream-compatible
  `serialiseData` CBOR for live script data, including the observed
  preview payload shape.
- `crates/ledger/src/plutus_validation.rs` and
  `crates/ledger/src/witnesses.rs` no longer over-collect legacy
  `AccountRegistration` as a script-bearing certificate purpose.
- `node/src/runtime.rs` now preserves boundary-aware recovered
  `StakeSnapshots` and current-epoch `pool_block_counts` during runtime
  resume so a mid-epoch restart does not under-credit rewards at the next
  epoch boundary.
- Local `AGENTS.md` files in ledger, Plutus, and node paths now document
  the parity constraints so future changes do not reintroduce the
  preview failure.

## Focused Verification

```text
cargo fmt
cargo fmt --all -- --check
cargo test -p yggdrasil-plutus flat::tests --lib
cargo test -p yggdrasil-plutus builtins::tests::serialise_data --lib
cargo test -p yggdrasil-ledger cbor::tests::extract_block_tx_byte_spans --lib
cargo test -p yggdrasil-ledger plutus::tests --lib
cargo test -p yggdrasil-ledger plutus_validation::tests::validate_certifying_script_skips_legacy_registration_without_redeemer --lib
cargo test -p yggdrasil-ledger witnesses::tests --lib
cargo test -p yggdrasil-ledger witness_validation --test integration
cargo test -p yggdrasil-node sync:: --lib
cargo test -p yggdrasil-node runtime::tests::runtime_recovery_preserves_current_epoch_block_counts --lib
cargo build -p yggdrasil-node --release
```

All passed.

Full workspace gates after the latest R246 patches:

```text
cargo check-all
cargo test-all
cargo lint
```

All passed.

## Preview Replay Evidence

Release replay against the existing preview producer database and config:

```text
cargo run --manifest-path tmp/refscan/Cargo.toml --release -- \
  tmp/preview-producer/db/producer \
  tmp/preview-producer/config/preview-producer.json
```

Result:

```text
plutus evaluator enabled
tip=BlockPoint(SlotNo(887833), HeaderHash(...))
checkpoint=SlotNo(881160)
volatile replay blocks=255
ok final tip=BlockPoint(SlotNo(887833), HeaderHash(...))
```

After the legacy certificate redeemer/witness fix, the same refscan path
advanced again:

```text
checkpoint=SlotNo(898474)
volatile replay blocks=128
ok final tip=BlockPoint(SlotNo(901725), HeaderHash(...))
```

Live bounded preview sync then advanced past the Plutus/certificate
failures and reached a ledger checkpoint at slot `1038614`. The next
observed stop is not Plutus:

```text
slot=1038978
tx=ffe6f2f14fe4743872f13863eb2e64898928b68415bce7180abe8d69325b580c
error=WithdrawalExceedsBalance
requested=360529557
available=1092724
```

Diagnosis: the run that produced the current temp preview database had
already resumed from a mid-epoch checkpoint while dropping the recovered
current-epoch pool block counts. At the next epoch boundary, rewards were
under-credited and the later full-drain withdrawal failed. Runtime resume
now preserves recovered `pool_block_counts` together with
`StakeSnapshots`, but the existing `tmp/preview-producer/db/producer`
contains post-boundary checkpoints written before that fix. A clean
replay, or a rollback to a pre-boundary checkpoint with matching sidecars,
is required to verify progress past slot `1038978` on that database.

## Upstream References

- Babbage well-formed reference script rule:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs>
- Alonzo Plutus validity interval encoding:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Plutus/TxInfo.hs>
- Conway Plutus validity interval encoding:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/TxInfo.hs>
- Babbage `TxInfo` reference-input ordering:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxInfo.hs>
- Plutus CEK runtime memory accounting:
  <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs>
- Plutus builtin integer semantics:
  <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs>
- Plutus `Data`/`serialiseData` encoding:
  <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Data.hs>
- Shelley certificate script-witness extraction:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs>
- Conway withdrawal full-drain rule:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Certs.hs>
- Shelley reward update / new-epoch ordering:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/NewEpoch.hs>
