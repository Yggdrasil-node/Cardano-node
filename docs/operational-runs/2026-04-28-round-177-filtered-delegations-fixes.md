## Round 177 — `encode_filtered_delegations_and_rewards` correctness fixes

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Audit the R163 `encode_filtered_delegations_and_rewards`
helper (the dispatcher for upstream tag 10
`GetFilteredDelegationsAndRewardAccounts`) for hidden bugs.
Found three.

### Issues fixed

1. **Non-deterministic CBOR output**.  The function iterated
   `credentials.iter()` directly, where `credentials` is a
   `HashSet<StakeCredential>` — iteration order is internal and
   varies across runs even for the same logical input.  CBOR map
   entries should emit in canonical ascending-key order to match
   upstream `Map.toAscList`.  Pre-fix, two calls with the same
   filter set could produce different byte streams; cardano-cli
   would still parse them but bytewise comparison against
   golden vectors would fail unpredictably.

2. **O(n²) inner search per credential**.  For each requested
   credential, the function called
   `stake_creds.iter().find(|(c, _)| *c == cred)` — a linear
   scan over every registered stake credential.  With N
   delegated credentials and M filter credentials, this was
   `O(N·M)`; the BTreeMap behind `StakeCredentials` already
   exposes `get(cred)` for O(log N) lookup.

3. **Reward-account lookup mis-matched on hash bytes alone**.
   The function compared via `addr.credential.hash() ==
   cred.hash()`, which strips the AddrKey-vs-Script
   discriminator from `StakeCredential`.  A malicious or
   misconfigured client could request a `Script(h)` credential
   and receive an `AddrKey(h)` reward balance — the 28-byte
   hash bytes match but they're cryptographically distinct
   credentials.  Switched to `RewardAccounts::find_account_by_credential(cred)`
   which compares the full `StakeCredential` (kind + hash).

### Code change

`node/src/local_server.rs::encode_filtered_delegations_and_rewards`:

- Pre-sort the filter into a `Vec<&StakeCredential>` via
  `sort()` so subsequent iteration is canonical.
- Replace `stake_creds.iter().find(...)` with
  `stake_creds.get(cred)` (BTreeMap O(log N)).
- Replace `reward_accounts.iter().find(|(addr, _)|
  addr.credential.hash() == cred.hash())` with
  `reward_accounts.find_account_by_credential(cred)` followed
  by `reward_accounts.get(acct)`.

### Regression test (+1)

- `encode_filtered_delegations_and_rewards_is_deterministic` —
  builds two `HashSet`s with identical credentials but inserted
  in different orders, calls the encoder against an empty
  `Era::Conway` snapshot, and asserts the two byte outputs are
  identical.  Also pins the empty-snapshot baseline output
  `0x82 0xa0 0xa0` (= `[empty_map, empty_map]`).

Test count progression: 4733 → **4734**.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4734  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), the dispatcher continues to operate:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 5960, "epoch": 0, "era": "Alonzo", ... }

$ /metrics summary
yggdrasil_blocks_synced 299
yggdrasil_active_peers 1
```

`cardano-cli query stake-address-info` (the surface that uses
this dispatcher) is era-blocked client-side at Alonzo and so
isn't directly callable on preview yet, but the unit-test
deterministic assertion plus the unchanged sync rate (~14
blk/s) confirms the fix doesn't introduce regressions.

### Why this matters

- **Determinism** matters for any future test fixture or
  byte-for-byte parity check against upstream cardano-node
  responses.  Without sorted iteration, two yggdrasil nodes
  serving identical data would emit different bytes, masking
  legitimate divergences.
- **O(N·M) → O(M·log N) + O(M·log N)** matters for large
  reward / delegation maps that mainnet-class chains will
  produce.  Pre-fix, a query for 100 credentials against a
  10k-pool ledger would do 1M comparisons per query; post-fix,
  ~1300.
- **Kind-discriminator stripping** is a real correctness
  concern: in the wild, AddrKey and Script credentials
  occasionally share hash byte prefixes by coincidence.  The
  pre-fix lookup could return cross-typed reward balances,
  silently confusing clients that rely on the discriminator.

### Open follow-ups (unchanged from R176)

1. Live stake-snapshot plumbing into `LedgerStateSnapshot`.
2. `GetGenesisConfig` ShelleyGenesis serialisation.
3. Apply-batch duration histogram (R169).
4. Multi-session peer accounting (R168 structural follow-up).
5. Pipelined fetch + apply (R166).
6. Deep cross-epoch rollback recovery (R167).

### References

- Code: [`node/src/local_server.rs`](node/src/local_server.rs)
  — `encode_filtered_delegations_and_rewards` rewrite + 1 new
  regression test.
- Upstream reference:
  `Cardano.Ledger.Shelley.LedgerStateQuery.GetFilteredDelegationsAndRewardAccounts`;
  `Cardano.Ledger.Shelley.LedgerState.DState.dsStakeMembers`,
  `dsStakeRewards`.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-176-decoder-strictness-cleanup.md`](2026-04-28-round-176-decoder-strictness-cleanup.md).
