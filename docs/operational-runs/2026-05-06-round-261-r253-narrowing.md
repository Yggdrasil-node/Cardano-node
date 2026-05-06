## Round 261 — R253 root-cause narrowing for preprod TPraos VRF gap

Date: 2026-05-06
Branch: main
Type: Static parity verification (no behaviour change)

### Goal

Narrow R253's three candidate root causes for the R249 preprod
"invalid VRF proof" failure at slot ~429,460. Each candidate is
ruled in or out by a static check against
`.reference-haskell-cardano-node/`.

### Candidates (R259 baseline)

R259 narrowed R253 from "anywhere in the VRF/overlay path" to:

1. **gen_delegs activation timing** — `effective_gen_delegs()`
   returns the wrong source or wrong content at slot 429460+.
2. **Block header VRF parsing** — `block_issuer_vkey`,
   `block_vrf_vkey`, `leader_vrf.proof`, `nonce_vrf.proof` decoded
   from the on-wire bytes differently than upstream.
3. **IETF VRF draft compatibility** — `yggdrasil_crypto::vrf::
   VrfVerificationKey::verify` produces different bytes than
   upstream `Cardano.Crypto.VRF.Praos.verify`.

### Candidate 3 ruled out

Ran `cargo test -p yggdrasil-crypto --test upstream_vectors -- vrf`:

```
test embedded_ver03_vrf_vectors_match_full_vendored_corpus ... ok
test embedded_vrf_vectors_match_vendored_standard_examples ... ok
test embedded_ver13_vrf_vectors_match_full_vendored_corpus ... ok
test upstream_praos_vrf_vector_files_are_present_and_well_formed ... ok
test result: ok. 4 passed; 0 failed
```

All 4 vector tests against the vendored
`cardano-base/cardano-crypto-praos/test_vectors/` corpus
(SHA `7a8a991945d4...`) pass for both ietfdraft03 (Shelley/Alonzo
TPraos) and ietfdraft13 (Babbage+ Praos) modes. Yggdrasil's pure-Rust
VRF crypto is byte-identical to upstream's Haskell implementation.

### Candidate 1 partially ruled out (JSON-parse leg)

Added new test
`crates/node/src/genesis/tests.rs::parse_real_preprod_shelley_genesis_gen_delegs_matches_upstream`
which:

- Loads `node/configuration/preprod/shelley-genesis.json` (verified
  `diff`-clean against upstream `install/share/preprod/shelley-genesis.json`).
- Runs `load_shelley_genesis_bootstrap` to parse the `genDelegs` map.
- Asserts the resulting `BTreeMap<GenesisHash, _>` contains exactly
  7 entries in upstream-`Set.elemAt` order (lexicographic by
  28-byte hash) with the expected genesis-key hashes.

Test passes. **The JSON-parse path correctly decodes preprod's
genesis-delegate map byte-for-byte.** Combined with R259's
`tpraos_overlay_matches_upstream_classifyoverlayslot_preprod_429460_window`
test (which verified overlay classification matches upstream
selection across the 429460-429540 active-overlay window), this
rules out the static side of candidate 1.

### Candidate 1' (sub-condition still open)

The dynamic side of candidate 1 is **not** yet ruled out: a
`GenesisDelegateCert` certificate between slot 86400 and 429460
on preprod's chain would schedule a `future_gen_delegs` update
(`(activation_slot, genesis_hash) -> new_delegation`) which
`apply_scheduled_genesis_delegations(current_slot)` activates
at the appropriate slot. If yggdrasil's apply path for that cert
is wrong — wrong activation_slot computation, wrong delegation
content, wrong scheduling cadence in `future_gen_delegs` — the
active map at slot 429460 would diverge from upstream.

This sub-candidate needs:

- Either a synthetic fixture test that constructs a
  `GenesisDelegateCert` apply scenario and asserts the resulting
  map matches upstream `Cardano.Ledger.Shelley.Rules.Deleg.GenesisDelegate`.
- Or a forensic dump of preprod's actual ChainDB between slots
  86400 and 429460 via
  `.reference-haskell-cardano-node/install/bin/db-analyser`
  to enumerate which (if any) GenesisDelegateCerts fired and
  compare yggdrasil's apply behaviour against upstream's.

### Candidate 2 still open

Block header VRF parsing has not been independently verified. The
extraction sites (`block_issuer_vkey`, `block_vrf_vkey`,
`leader_vrf.proof`, `nonce_vrf.proof`) are simple field
accessors on the parsed Shelley `HeaderBody` struct; the
byte-correctness depends on the Shelley/Allegra/Mary
`HeaderBody` CBOR decoder. R259's diagnostic enrichment will
disambiguate this on the next preprod sync past slot 429460
via the `WrongGenesisColdKey` / `VrfKeyMismatch` /
`InvalidVrfProof` error discriminant in the trace.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 845 passed, 0 failed (+1 vs R260)
```

### What this enables

R253 now has only **two open sub-candidates** instead of three:
candidate (1') GenesisDelegateCert apply path, and candidate (2)
header CBOR parsing. Both are bounded; both can be addressed in
later focused rounds with either a synthetic fixture (1') or a
captured failing block (2).

Each ruled-out candidate prevents wasted investigation:
- Candidate 3 (VRF crypto) — would have been days of byte-trace
  comparison against upstream IETF draft implementations.
- Candidate 1 JSON-parse — would have been hours of hex-dump
  comparison.

### References

- R259 diagnostic enrichment: `2026-05-06-round-259-tpraos-overlay-vrf-diagnostics.md`
- VRF vector pin test: `crates/crypto/tests/upstream_vectors.rs`
  (4 tests against cardano-base SHA `7a8a991945d4...`)
- Genesis parse test: `node/src/genesis/tests.rs::parse_real_preprod_shelley_genesis_gen_delegs_matches_upstream`
- Overlay classification test: `node/src/sync.rs::tpraos_overlay_matches_upstream_classifyoverlayslot_preprod_429460_window`
- Upstream forensic harness:
  `.reference-haskell-cardano-node/install/bin/db-analyser`
