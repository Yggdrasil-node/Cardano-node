---
title: "Round 590 tx-generator DumpToFile bootstrap-witness key-hash sort"
parent: Reference
---

# Round 590 tx-generator DumpToFile bootstrap-witness key-hash sort

Date: 2026-05-20

## Scope

This round closes the documented byte-parity caveat from R579:
multi-witness bootstrap-witness ordering. yggdrasil now ports the
upstream `bootstrapWitKeyHash` hash domain and uses it as the sort
key in `show_alonzo_bootstrap_witnesses`, so the output is
byte-equivalent to upstream `Ord BootstrapWitness = comparing
bootstrapWitKeyHash` for any number of witnesses.

With this round, **every documented byte-parity gap inside
yggdrasil's tx-generator DumpToFile renderer is closed.** The only
remaining gate is operator-soak comparison against the upstream
binary.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Keys/Bootstrap.hs:108-146`
  (`Ord BootstrapWitness = comparing bootstrapWitKeyHash`, with
  `bootstrapWitKeyHash` implemented as `Blake2b-224 . SHA3-256 .
  (prefix ++ key ++ chain_code ++ attributes)`).
- The 6-byte prefix `[0x83, 0x00, 0x82, 0x00, 0x58, 0x40]` is the
  CBOR-shaped Byron AddressInfo header: list-of-3-token, addrType
  byte = 0, list-of-2-token, type byte = 0,
  bytestring-len-64-token.

## Changes

- Added `bootstrap_witness_key_hash` helper in
  `crates/tools/tx-generator/src/script/core.rs`. Composition:
  - `prefix = [0x83, 0x00, 0x82, 0x00, 0x58, 0x40]` (6 bytes)
  - `buf = prefix ++ public_key (32 B) ++ chain_code (32 B) ++ attributes (variable)`
  - `sha3 = SHA3-256(buf)` (`yggdrasil_crypto::sha3_256`)
  - `result = Blake2b-224(sha3)` (`yggdrasil_crypto::hash_bytes_224`)
- Updated `show_alonzo_bootstrap_witnesses` to sort by this hash
  via `sort_by_key`. Removed the byte-parity caveat from the
  function doc.
- Updated the R579 multi-witness sort regression test to verify
  hash-based ordering: the test now computes both witness hashes
  and asserts the rendered order matches the hash comparator
  (regardless of input order).
- Added `dumptofile_bootstrap_witness_key_hash_matches_upstream_domain`
  unit test that directly verifies the hash composition:
  `Blake2b-224(SHA3-256(PREFIX ++ key ++ cc ++ attrs))`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (56 tests, +1
  from R589)
- `cargo test -p yggdrasil-tx-generator` (239 lib tests + 5
  CLI/golden, +1 from R589 baseline)

## Remaining

- Capture upstream-binary comparison evidence (Benchmark soak
  against the upstream `tx-generator` binary) — operator-soak
  gated, requires a runnable upstream binary environment.
