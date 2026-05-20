---
title: "Round 578 tx-generator DumpToFile native-script Timelock Show"
parent: Reference
---

# Round 578 tx-generator DumpToFile native-script Timelock Show

Date: 2026-05-20

## Scope

This round lifts the native-script boundary in both DumpToFile paths
that previously rejected `Script::Native`:

- `show_babbage_script_ref` (reference scripts on `BabbageTxOut`),
  previously rejected with "does not yet support native reference
  scripts".
- `show_alonzo_script_witnesses` (native scripts inside
  `atwrScriptTxWits`), previously rejected at the
  `show_alonzo_witness_set` gate with "does not yet support native
  scripts or bootstrap witnesses".

Bootstrap witnesses remain the last `TxGenError` boundary inside the
witness set.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/allegra/impl/src/Cardano/Ledger/Allegra/Scripts.hs:170-263`
  (`TimelockRaw`, `Timelock`, stock-derived Show).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Scripts.hs:475-478`
  (`Show (AlonzoScript era)` — `NativeScript x` ⇒ `"NativeScript " ++
  show x`).
- `.reference-haskell-cardano-node/deps/cardano-base/cardano-slotting/src/Cardano/Slotting/Slot.hs:35-41`
  (`SlotNo` Show via Quiet).

## Changes

- Added `show_native_script` rendering the full `MkTimelock <raw>
  (blake2b_256: SafeHash "<hex>")` envelope. The outer hash is
  Blake2b-256 over the canonical NativeScript CBOR (`encode_cbor`
  output matches upstream's `EncCBOR TimelockRaw` Sum encoding).
- Added `show_timelock_raw` for the 6 upstream `TimelockRaw`
  variants:
  - `TimelockSignature (KeyHash {unKeyHash = "<hex>"})`
  - `TimelockAllOf (StrictSeq {fromStrict = fromList [<MkTimelock>,...]})`
  - `TimelockAnyOf (StrictSeq {fromStrict = fromList [<MkTimelock>,...]})`
  - `TimelockMOf <n> (StrictSeq {fromStrict = fromList [<MkTimelock>,...]})`
  - `TimelockTimeStart (SlotNo <n>)`
  - `TimelockTimeExpire (SlotNo <n>)`
- Wired the `Script::Native` branch of `show_babbage_script_ref` to
  emit `SJust NativeScript MkTimelock ...`, replacing the prior
  rejection.
- Extended `show_alonzo_script_witnesses` to include native-script
  entries: `(ScriptHash "<hex>",NativeScript MkTimelock ...)` where
  the hash key is `yggdrasil_ledger::native_script_hash` (Blake2b-224
  over `[0x00, ...cbor]`). Entries continue to sort by script-hash
  byte-lex order.
- Narrowed `show_alonzo_witness_set` rejection to bootstrap
  witnesses only.
- Converted the prior native-script rejection tests to acceptance
  tests; added `dumptofile_show_native_script_variants` exercising
  all 6 Timelock variants.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (34 tests, +1
  from R577)
- `cargo test -p yggdrasil-tx-generator` (217 lib tests + 5
  CLI/golden, +1 from R577 baseline)

## Remaining

- Render bootstrap witnesses (Byron-era) inside the witness set.
- Render Conway `ProposalProcedures` OSet entries — needs `GovAction`
  Show (7 variants: ParameterChange, HardForkInitiation,
  TreasuryWithdrawals, NoConfidence, UpdateCommittee, NewConstitution,
  InfoAction) plus `AccountAddress` decoding for the
  `pProcReturnAddr` field.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for byte
  parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
