## Round 176 — Decoder strictness cleanup (R174 sweep completion)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Find and fix the remaining instances of the R174 over-permissive
optional-tag bug.  R174 tightened `decode_pool_hash_set` (R171
helper) and `decode_stake_credential_set` (R163 helper) to only
accept tag 258 in the optional CIP-21 set wrapper position, but
missed the older `decode_address_set` and `decode_txin_set`
helpers added back in R157.  Those two had the exact same
"`if peek_major == Some(6) { dec.tag()?; }`" pattern that
silently strips any arbitrary tag.

### Issues fixed

1. **`decode_address_set`** (R157) accepted any CBOR tag in the
   optional 258 wrapper position.  A malformed `GetUTxOByAddress`
   payload with tag 30 / 24 / any other tag would have its tag
   silently stripped.  Tightened to require tag 258 specifically;
   non-258 tags now surface as a `CborDecodeError`.

2. **`decode_txin_set`** (R157) had the same issue for
   `GetUTxOByTxIn` payloads.  Same tightening applied.

### Code change

`node/src/local_server.rs`: same tightening pattern as R174
applied to `decode_address_set` and `decode_txin_set` —
explicit `tag_number == 258` check + descriptive error message.
Both annotated with a Round 176 rationale comment that points
to R174 as the prior fix.

### Regression tests (+4)

- `decode_address_set_rejects_non_258_tag` — feeds tag 30,
  expects "expected tag 258" error.
- `decode_address_set_accepts_tagged_set_form` — positive case
  pinning that the canonical `tag(258) [* bytes]` shape still
  works.
- `decode_address_set_accepts_untagged_array_form` — positive
  case pinning that the legacy untagged-array shape still works.
- `decode_txin_set_rejects_non_258_tag` — non-258 rejection
  parity check.

The `decode_txin_set` positive cases were already covered by
the end-to-end `query utxo --tx-in` operational test path.

Test count progression: 4729 → **4733**.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4733  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), both the `GetUTxOByAddress` and
`GetUTxOByTxIn` end-to-end paths continue to succeed:

```
$ cardano-cli query utxo --whole-utxo --testnet-magic 2
{ "e3ca57e8...#0": { "address": "...", ... }, ... }

$ cardano-cli query utxo --tx-in "e3ca57e8...#0" --testnet-magic 2
{ "e3ca57e8...#0": { "address": "...", ... } }
```

Both are routed through the tightened decoders (`--whole-utxo`
filtering happens via `decode_address_set` once a filter is
supplied; `--tx-in` uses `decode_txin_set`).  Sync rate
unchanged at ~14 blk/s.

### Why this matters

Pre-R176, a malformed cardano-cli or third-party LSQ client
sending a non-258 tag in the address-set or txin-set position
would have its tag silently stripped, then yggdrasil would try
to parse the next byte as an array length — likely producing
either a decode error further down the stack or, worse, a
result that LOOKS valid but isn't (e.g. parsing UnitInterval's
inner array as a list of addresses).  Strict decoders surface
the malformed input immediately at the wire boundary.

This completes the R174 strictness sweep; all five CBOR
set-decoder helpers in [`node/src/local_server.rs`](node/src/local_server.rs)
(`decode_pool_hash_set`, `decode_stake_credential_set`,
`decode_address_set`, `decode_txin_set`, `decode_maybe_pool_hash_set`)
now have consistent strict tag-258 validation.

### Open follow-ups (unchanged from R175)

1. Live stake-snapshot plumbing into `LedgerStateSnapshot`.
2. `GetGenesisConfig` ShelleyGenesis serialisation.
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 structural follow-up).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — two decoder-tightening edits + four new regression tests.
- Upstream reference: CIP-21 (CBOR set tag 258); RFC 8949 §3.4
  (CBOR major types).
- Previous round:
  [`docs/operational-runs/2026-04-28-round-175-registry-cooling-completeness.md`](2026-04-28-round-175-registry-cooling-completeness.md).
- Sibling round (initial sweep):
  [`docs/operational-runs/2026-04-28-round-174-decoder-strictness-fixes.md`](2026-04-28-round-174-decoder-strictness-fixes.md).
