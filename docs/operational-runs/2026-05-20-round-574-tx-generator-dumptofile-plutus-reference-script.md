---
title: "Round 574 tx-generator DumpToFile Plutus reference scripts"
parent: Reference
---

# Round 574 tx-generator DumpToFile Plutus reference scripts

Date: 2026-05-20

## Scope

This round lifts the Plutus `script_ref` boundary in
`show_babbage_script_ref`. Previously, any Babbage/Conway transaction
output carrying a reference script would fail
`SubmitMode::DumpToFile` with `does not yet support reference
scripts`. After this round, Plutus V1/V2/V3 reference scripts render
as upstream `SJust PlutusScript PlutusV{1,2,3} ScriptHash "<hex>"`,
mirroring `Show (AlonzoScript era)`'s custom Show.

Native reference scripts (`Script::Native`) remain on explicit
`TxGenError` boundary until the Timelock Show is ported.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Scripts.hs:475-478`
  (`Show (AlonzoScript era)` custom Show: `"PlutusScript " ++ show
  (plutusScriptLanguage plutus) ++ " " ++ show (hashScript @era s)`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/Language.hs:224-229`
  (`data Language = PlutusV1 | PlutusV2 | PlutusV3 | PlutusV4
  deriving Show`).

## Changes

- Replaced the `Some(_) => TxGenError` arm in
  `show_babbage_script_ref` with proper rendering for
  `Script::PlutusV1`, `Script::PlutusV2`, `Script::PlutusV3` paths.
- Added `plutus_script_hash` helper: Blake2b-224 over
  `(language-tag-byte ++ script_bytes)` with tags 0x01/0x02/0x03 for
  PlutusV1/V2/V3, mirroring upstream `hashScript` for Plutus scripts.
- `Script::Native` continues to return a typed `TxGenError` with the
  message "does not yet support native reference scripts".
- Added 3 focused unit tests:
  - `dumptofile_babbage_script_ref_renders_snothing_and_plutus_versions`
    — exercises SNothing baseline plus V1/V2/V3 rendering, and
    asserts that identical script bytes under V1 vs V3 produce
    different script hashes (because the language-tag prefix
    differs).
  - `dumptofile_babbage_script_ref_rejects_native_script` — pins the
    native-reject error message so future Timelock-Show work has a
    clear regression boundary.
  - `dumptofile_plutus_script_hash_matches_language_prefix_domain` —
    direct hash-domain invariant: `plutus_script_hash(1, bytes) ==
    Blake2b-224([0x01, ...bytes])`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (23 tests, +3
  from R573)
- `cargo test -p yggdrasil-tx-generator` (206 lib tests + 5
  CLI/golden, +3 from R573 baseline)

## Remaining

- Render native reference scripts (`Script::Native`) — needs the
  Timelock Show port (`NativeScript x` → `NativeScript <...>` Haskell
  Show).
- Render Plutus V1/V2/V3 script-witness bytes inside the witness set
  (`show_alonzo_witness_set` `atwrScriptTxWits`).
- Render native scripts and bootstrap witnesses inside the witness
  set.
- Render Conway governance procedures
  (`ctbrVotingProcedures`, `ctbrProposalProcedures`,
  `ctbrCurrentTreasuryValue`, `ctbrTreasuryDonation`).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for byte
  parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
