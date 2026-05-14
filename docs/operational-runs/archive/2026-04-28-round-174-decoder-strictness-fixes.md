## Round 174 — Decoder strictness fixes (R171/R172/R173 follow-up)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Sweep through the recent dispatcher additions (R171, R172, R173)
for hidden bugs.  Found and fixed three subtle issues in the CBOR
decoders for set / `Maybe Set` payloads where over-permissive
checks could silently mis-parse malformed wire bytes.

### Issues fixed

1. **`decode_pool_hash_set` accepted any CBOR tag**, not just 258.
   The pre-fix code did `if dec.peek_major() == Some(6) { dec.tag()?; }`,
   which strips the tag without verifying it's the canonical
   CIP-21 set tag (258).  A malformed payload with tag 30
   (UnitInterval), tag 24 (CBOR-in-CBOR), or any other tag would
   have its tag silently stripped and then the next byte parsed
   as an array length.  Tightened to require tag 258 specifically;
   non-258 tags now surface as a `CborDecodeError` with a clear
   message.

2. **`decode_stake_credential_set` had the same issue** — accepted
   any tag in the optional 258 wrapper.  Same tightening applied
   for parity.

3. **`decode_maybe_pool_hash_set` over-matched on the `Nothing`
   shortcut**.  The pre-fix code used
   `if dec.peek_major() == Some(7)` which matches CBOR major
   type 7 — that's not just `null` (`0xf6`).  Major 7 also
   covers `undefined` (`0xf7`), the simple values `false`/`true`,
   half/single/double-precision floats, and the `break`
   stop-code.  Any of these in the payload position would
   silently shortcut to `Nothing` instead of erroring.  Switched
   to the existing precise `peek_is_null()` accessor (matches
   only `0xf6`).

### Code change

`node/src/local_server.rs`:

- `decode_pool_hash_set`: replace `dec.tag()?;` with explicit
  `tag_number == 258` check + descriptive error.
- `decode_stake_credential_set`: same tightening.
- `decode_maybe_pool_hash_set`: replace `peek_major == Some(7)`
  with `peek_is_null()` for the `Nothing` shortcut.  Also
  generalised the error message from "GetPoolState Maybe
  payload" to "Maybe (Set PoolKeyHash) payload" since R173 reuses
  the helper for `GetStakeSnapshots`.

### Regression tests (+3)

- `decode_pool_hash_set_rejects_non_258_tag` — feeds tag 30 and
  expects an error mentioning "expected tag 258".
- `decode_stake_credential_set_rejects_non_258_tag` — same
  shape, parity check.
- `decode_maybe_pool_hash_set_rejects_undefined` — feeds CBOR
  `undefined` (`0xf7`) and expects an error (rather than the
  pre-R174 silent `Nothing`).

Test count progression: 4726 → **4729**.

The existing positive-path tests
(`decode_pool_hash_set_accepts_tagged_set_form`,
`decode_pool_hash_set_accepts_untagged_array_form`,
`decode_maybe_pool_hash_set_accepts_*`) all continue to pass —
the tightening doesn't change behaviour for valid inputs.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4729  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), the parity sweep continues to work — the
tightened decoders accept every valid wire-form input the
existing dispatchers handle, and only the previously-silent
malformed-input paths now surface as errors:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 1960, "epoch": 0, "era": "Alonzo", ... }

$ cardano-cli query utxo --whole-utxo --testnet-magic 2
{ "e3ca57e8...#0": { "address": "...", "datum": null, "datumhash": null, "value": { "lovelace": 100000000000000 } }, ... }

$ /metrics summary
yggdrasil_blocks_synced 99
yggdrasil_current_era 4
yggdrasil_active_peers 1
```

Sync rate unchanged at ~14 blk/s.

### Why this matters

Pre-R174, a malformed cardano-cli or third-party LSQ client
sending a tag-30 wrapper or a CBOR `undefined` could trigger
silent decoder mis-behaviour: yggdrasil would either parse
garbage as a pool-hash set (likely producing zero matches and
returning empty results that look correct) or shortcut a `Just
<set>` query to `Nothing` (returning all pools instead of the
filtered subset).  Neither is exploitable in any obvious way —
LSQ runs over a Unix socket so the threat model is local
clients, not adversarial network input — but the silent
mis-parse would mask client bugs and complicate debugging.

Strict decoders surface those bugs immediately.

### Open follow-ups (unchanged from R173)

1. Live stake-snapshot plumbing into `LedgerStateSnapshot` (R163
   + R173 follow-up — the proper fix for the `[0, 0, 0]`
   placeholder data in `GetStakeSnapshots` and the empty map in
   `GetStakeDistribution`).
2. `GetGenesisConfig` ShelleyGenesis serialisation.
3. Apply-batch duration histogram (R169 follow-up).
4. Multi-session peer accounting (R168 follow-up).
5. Pipelined fetch + apply (R166 follow-up).
6. Deep cross-epoch rollback recovery (R167 follow-up).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — three decoder-tightening edits + three new regression tests.
- Upstream reference: CIP-21 (CBOR set tag 258); RFC 8949 §3.4
  (CBOR major types).
- Previous round:
  [`docs/operational-runs/2026-04-28-round-173-stake-snapshots-tag18.md`](2026-04-28-round-173-stake-snapshots-tag18.md).
