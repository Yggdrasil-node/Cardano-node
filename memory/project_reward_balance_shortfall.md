# Reward Balance Shortfall — Investigation & Resolution

## Status: CLOSED (2026-05-10)

## Symptom

Preview node replay at slot 1,038,602 failed with:

```
withdrawal exceeds reward balance: requested 360529557, available 360528672
```

Difference: **885 lovelace**.

## Root Cause

`compute_stake_snapshot()` in `crates/ledger/src/stake.rs` only walked
`Address::Base` UTxOs when building the stake snapshot.  It silently ignored
`Address::Pointer` UTxOs — a Shelley-era address type that references the
staking credential via a `(slot, tx_index, cert_index)` pointer to its
registration certificate rather than embedding the credential hash directly.

Credential `67ecde6546b25c003e9637c653be2f58237586ce91a4ed2e5ae1bd7c` held
**7,971 ADA** in pointer-address UTxOs (registered at slot 725220, tx_index 0,
cert_index 0).  These were invisible to the snapshot, causing its staked ADA
to be under-counted, which in turn produced a reward that was 885 lovelace
short of the amount the withdrawal expected.

Upstream reference: `addShelleyInstantStake` / `resolveShelleyInstantStake` in
`.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-shelley/src/Cardano/Ledger/Shelley/State/Stake.hs`
walks both `StakeRefBase` and `StakeRefPtr` UTxOs.  Yggdrasil was only walking
`StakeRefBase`.

## Fix

**Files changed:**

| File | Change |
|---|---|
| `crates/ledger/src/state/stake_credentials.rs` | Added `registration_ptr: Option<(u64,u64,u64)>` to `StakeCredentialState`; new `register_with_ptr()` method; extended CBOR encode/decode to array(4) (backward-compatible) |
| `crates/ledger/src/state.rs` | Updated `register_stake_credential` signature; added `tx_index: u64` param to `apply_certificates_and_withdrawals_with_future`; cert loop now uses `enumerate()` to track `cert_index` |
| `crates/ledger/src/stake.rs` | `compute_stake_snapshot()` now builds `ptr_to_cred: BTreeMap<(u64,u64,u64), StakeCredential>` reverse-lookup and resolves pointer-address UTxOs via it; two regression tests added |
| `crates/ledger/src/state/eras/{shelley,allegra,mary,alonzo,babbage,conway}.rs` | Tx loop changed to `enumerate()` pattern; `tx_index as u64` threaded to cert call |
| `crates/ledger/src/state/tests.rs` | 10 direct call sites updated with `tx_index = 0` |

**Regression tests:**
- `pointer_address_utxo_contributes_to_stake_snapshot` — verifies 5 ADA base + 7,971 ADA pointer = total
- `pointer_address_unregistered_ptr_is_skipped` — dangling pointer contributes 0

**All five verification gates pass:** `cargo fmt --all -- --check` ✓, `cargo check-all` ✓, `cargo test-all` ✓ (5,360 passing / 0 failing), `cargo lint` ✓, `python3 scripts/check-strict-mirror.py --fail-on-violation` 0 violations ✓.

## Verification Pending

Clean preview replay past slot 1,038,602 to confirm the reward shortfall no
longer occurs.  Requires building the release binary and running a fresh
preview sync; tracked as an operator-side gate.
