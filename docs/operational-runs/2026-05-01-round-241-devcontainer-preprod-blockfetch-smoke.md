# Round 241 - Devcontainer preprod BlockFetch smoke

Date: 2026-05-01
Phase: E.2 operator evidence hardening
Scope: rebuilt devcontainer toolchain validation plus short preprod
parallel BlockFetch smoke, no consensus/runtime behavior change

### Summary

The rebuilt devcontainer has the operator tooling needed for the
remaining runbook gates:

- Rust/Cargo 1.95.0 from `rust-toolchain.toml`
- `cardano-cli 10.16.0.0`
- ShellCheck 0.9.0
- actionlint 1.7.8

The canonical §6.5 harness was then run against preprod for a short
startup/metrics smoke with `--max-concurrent-block-fetch-peers 2`.
Because the vendored and official Operations Book preprod topology both
ship a single bootstrap DNS name and empty local roots, the smoke used a
temporary topology outside the repository with the official
`preprod-node.play.dev.cardano.org:3001` access point as a local root
with valency 2. DNS resolution produced multiple concrete peer
addresses, allowing the governor to migrate multiple BlockFetch workers
without editing the tracked reference topology.

### Command

```sh
RUN_DIR=/tmp/ygg-blockfetch-smoke-20260501T112607Z/run \
YGG_BIN=/workspaces/Cardano-node/target/release/yggdrasil-node \
NETWORK=preprod \
TOPOLOGY=/tmp/ygg-blockfetch-smoke-20260501T112607Z/preprod-topology.json \
MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
EXPECT_WORKERS=2 \
RUN_SECONDS=180 \
SAMPLE_INTERVAL_S=15 \
START_DEADLINE_S=120 \
node/scripts/parallel_blockfetch_soak.sh
```

### Result

```text
network: preprod
network_magic: 1
max_concurrent_block_fetch_peers: 2
expected_workers: 2
run_seconds: 180
blocks_synced: 0 -> 1150
current_slot: 0 -> 108460
reconnects: 0 -> 0
max_workers_registered: 6
workers_registered_final: 6
workers_migrated_total: 6
fetch_avg_per_batch: 7.196s
apply_avg_per_batch: 0.217s
tip_compare_passes: 0
```

No `fetch worker channel closed` or `fetch worker dropped response`
traces were present in the node log. Haskell tip comparison was not run
because no `HASKELL_SOCK` was available in this short smoke.

Artifacts for this run are under:

- `/tmp/ygg-blockfetch-smoke-20260501T112607Z/run/logs/summary.txt`
- `/tmp/ygg-blockfetch-smoke-20260501T112607Z/run/logs/yggdrasil-node.log`
- `/tmp/ygg-blockfetch-smoke-20260501T112607Z/run/metrics/`

### Verification

```sh
cargo check-all
cargo test-all
cargo lint
```

All three workspace gates were green after the smoke. The repository
worktree was clean before adding this evidence record.

### Status impact

- Short preprod multi-peer smoke: PASS.
- This does not replace the required §6.5 operator sign-off:
  preprod knob=2 6h with Haskell comparison, preprod knob=4 24h soak,
  and mainnet knob=2 24h relay-only rehearsal remain operator-time
  gates before changing the default `max_concurrent_block_fetch_peers`.
