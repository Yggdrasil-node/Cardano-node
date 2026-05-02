# Round 246 - preview Plutus well-formedness parity

Date: 2026-05-02

## Summary

R246 closes the preview replay blocker that surfaced as
`MalformedReferenceScripts` near Babbage slot `730728`, then exposed a
later Plutus validation mismatch around slot `831387`.

The fix keeps the upstream well-formedness rule active while aligning
the local Plutus path with upstream `PlutusBinary` handling and the
observed Babbage/Conway `TxInfo`/CEK details:

- On-chain script bytes are treated as raw `PlutusBinary` bytes at the
  evaluator boundary: a CBOR bytestring containing the Flat UPLC program.
  Ledger call sites pass protocol version so language gates remain active.
- Babbage/Conway reference inputs are sorted by `ShelleyTxIn`, matching
  upstream `Set.toList` ordering before ScriptContext construction.
- CEK `ExMemoryUsage` for non-constant runtime values is `1`, not `0`.
- Pre-Conway upper-only validity intervals encode the upper bound with
  inclusive `PV1.to`; Conway/PV9+ uses the strict upper-bound encoding.

## Local Impact

- `crates/ledger/src/plutus_validation.rs` now normalizes
  `resolved_reference_inputs` deterministically before node-side
  ScriptContext encoding.
- `crates/plutus/src/cost_model.rs` matches upstream CEK memory
  accounting for `VDelay`, `VLamAbs`, builtins, and constructors.
- `node/src/plutus_eval.rs` makes validity-interval encoding
  protocol-version aware.
- Local `AGENTS.md` files in ledger, Plutus, and node paths now document
  the parity constraints so future changes do not reintroduce the
  preview failure.

## Focused Verification

```text
cargo fmt
cargo test -p yggdrasil-plutus flat::tests --lib
cargo test -p yggdrasil-plutus cost_model --lib
cargo test -p yggdrasil-ledger cbor::tests::extract_block_tx_byte_spans --lib
cargo test -p yggdrasil-ledger witness_validation --test integration
cargo test -p yggdrasil-node plutus_eval --lib
cargo test -p yggdrasil-node sync:: --lib
cargo build -p yggdrasil-node --release
```

All passed.

Full workspace gates:

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
checkpoint=SlotNo(830594)
volatile replay blocks=128
ok final tip=BlockPoint(SlotNo(834713), HeaderHash(ecf07479870a29bf...))
```

The replay advanced past the previous Babbage failure point with no
`MalformedReferenceScripts` and no ledger decode error. A debug refscan
can overflow the default process stack on the very deep observed script;
the same replay passes with `ulimit -s 65536`, and the release replay
passes without a stack adjustment.

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
