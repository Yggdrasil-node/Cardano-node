---
title: "Round 575 tx-generator DumpToFile Plutus witness-set scripts"
parent: Reference
---

# Round 575 tx-generator DumpToFile Plutus witness-set scripts

Date: 2026-05-20

## Scope

This round lifts the Plutus V1/V2/V3 script-witness boundary in
`show_alonzo_witness_set`. Previously, any Alonzo/Babbage/Conway
witness set containing Plutus script bytes would fail
`SubmitMode::DumpToFile` with `does not yet support native or Plutus
scripts`. After this round, Plutus script witnesses render as
upstream `atwrScriptTxWits = fromList [(ScriptHash "<hex>",
PlutusScript PlutusV{N} ScriptHash "<hex>"),...]` matching
`Show (Map ScriptHash (AlonzoScript era))`.

Native scripts and bootstrap witnesses inside the witness set remain
on explicit `TxGenError` boundary until the Timelock Show is ported.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxWits.hs`
  (`atwrScriptTxWits :: Map ScriptHash (AlonzoScript era)`)
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Scripts.hs:475-478`
  (custom `Show (AlonzoScript era)` used per script value)

## Changes

- Narrowed the witness-set rejection: it now only fires for
  `native_scripts` or `bootstrap_witnesses`, not for Plutus scripts.
- Added `show_alonzo_script_witnesses` building the
  `fromList [...]` body. Each Plutus script becomes one entry keyed
  by its `ScriptHash` (R574's `plutus_script_hash`) and valued by
  the upstream `Show (AlonzoScript era)` form `PlutusScript
  PlutusV{N} ScriptHash "<hex>"`.
- Sorted entries by script-hash byte-lex order (mirroring upstream
  `Data.Map toAscList`).
- Added 4 focused unit tests:
  - empty script-witness map (regression guard for the `fromList []`
    path)
  - single PlutusV2 entry — pins the exact rendered text
  - multi-version (V1+V2) byte-lex sort order verification
  - native-script rejection error message regression guard

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (27 tests, +4
  from R574)
- `cargo test -p yggdrasil-tx-generator` (210 lib tests + 5
  CLI/golden, +4 from R574 baseline)

## Remaining

- Render native reference scripts (`Script::Native`) and
  native-script witnesses inside the witness set — needs the
  Timelock Show port.
- Render bootstrap witnesses (Byron-era) for completeness.
- Render Conway governance procedures
  (`ctbrVotingProcedures`, `ctbrProposalProcedures`,
  `ctbrCurrentTreasuryValue`, `ctbrTreasuryDonation`).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage
  (`\NUL` ... `\DEL`) for byte parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
