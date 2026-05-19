# Round 513 - full project parity audit with real preview block-producer plan

**Date:** 2026-05-19
**Area:** workspace parity / operator audit
**Upstream reference:** IntersectMBO/cardano-node 11.0.1
**Status:** Partial. Static and reference-artifact gates passed. Generated
preview block-producer credentials validate, producer startup was exercised,
and the generated pool registration/delegation transaction is confirmed
on-chain. Leader election, forged/adopted block evidence, and Haskell
tip-comparison windows remain blocked until the pool is active in epoch 1304.

## Summary

The audit target remains `IntersectMBO/cardano-node 11.0.1`. The Haskell
reference tree was refreshed from the policy tag and the release binary was
rebuilt. The four Cargo gates and the parity-flow validators passed against
the refreshed reference tree.

The initial execution environment did not provide operator-supplied preview
credential paths:

```text
KES_SKEY_PATH=
VRF_SKEY_PATH=
OPCERT_PATH=
```

No filesystem scan for signing-key material was performed. Generated preview
harness credentials were not substituted for the real preview pool
credentials. Later in the same audit, a generated preview credential bundle was
created outside the repository, validated, used for startup evidence, and then
registered/delegated on preview after the operator funded its generated payment
address.

This round added an operator runner for the blocked step:
`crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh`.
The runner validates the real preview credentials first, starts
`yggdrasil-node run --network preview` directly, and fails if required
producer-startup, forge, adoption, Haskell tip-comparison, or error-absence
evidence is missing. It does not call `preview_producer_harness.sh` and does not
read generated harness credential paths.

The runner's optional Haskell comparison path uses
`crates/node/yggdrasil-node/scripts/compare_tip_to_haskell.sh` at
`TIP_COMPARE_CHECKPOINTS=900,3600,21600` when `HASKELL_SOCK` is set. Setting
`REQUIRE_TIP_COMPARISON=1` fails the run unless `HASKELL_SOCK` is set, every
configured checkpoint fits inside `RUN_SECONDS`, and each due comparison runs
and passes.

## Completed Commands

```sh
bash scripts/setup-reference.sh --force
```

Result: exit 0. The reference install reports:

```text
cardano-node 11.0.1 - linux-x86_64 - ghc-9.6
git rev 97036a66bcf8c89f687ae57a048eecc0389977ef
```

```sh
cargo fmt --all -- --check
cargo build --release -p yggdrasil-node
cargo check-all
cargo lint
cargo test-all
```

Results: all exit 0 on the final audit diff. `cargo test-all` completed unit
tests, integration tests, and doctests successfully; the tracer forwarder
doctests remain the expected three ignored doctests.

```sh
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
```

Results:

```text
parity matrix clean: 22 entries validated against .reference-haskell-cardano-node (reference tag 11.0.1)
strict-mirror: 0 violations (clean)
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c consistent across pin source, fixture tree, and docs; 2 corpora validated.
reference artifacts clean: cardano-node 11.0.1 install + 9 binaries + 3 network share dirs validated.
```

```sh
target/release/yggdrasil-node validate-config --network preview --non-producing-node
```

Result: exit 0. Key fields:

```text
network_magic: 2
node_role.role: non-producing
node_role.block_producer_credentials: absent
resolved_startup_peer_count: 171
peer_snapshot.status: loaded
peer_snapshot.big_ledger_peer_count: 173
storage.status: not-initialized
warning: storage directories are not initialized; a deployment preflight cannot validate restart recovery yet
```

## Attempted Auxiliary Checks

```sh
python3 .claude/scripts/filetree.py check
```

Result: exit 1. The manifest is stale before this audit record is added. A
fresh rerun flags this round's new preview real-pool runner and audit report,
prior operational-run files from rounds 505-512, and multiple already-tracked
files as stale. This is a documentation-manifest maintenance issue, not
evidence of a new parity failure.

Follow-up:

```sh
python3 .claude/scripts/filetree.py scan --write
python3 .claude/scripts/filetree.py check
```

Result: `scan --write` added missing entries for the new preview real-pool
runner, this audit report, and prior operational-run records. The follow-up
check still exits 1, but the remaining findings are `STALE` entries requiring
description/metadata review; the `NEW` file findings are cleared.

After the later preview-runner checkpoint update, manifest metadata was accepted
only for this audit's touched files. A fresh filetree check still exits 1, but
no longer reports this audit's runner, report, README/runbook/manual updates,
smoke test, drift-helper, CI-template/doc entries, or the DMQ test-helper fix as
stale. Remaining stale entries are older unrelated workspace changes
(`Cargo.lock`, block-producer slice files, db-synthesizer slice files, and older
manual chapters).

```sh
crates/node/yggdrasil-node/scripts/check_upstream_drift.sh --json
```

Initial result: exit 3. The script still looked for
`crates/node/yggdrasil-node/src/upstream_pins.rs`; the current pin source is
`crates/node/config/src/upstream_pins.rs`.

Follow-up fix in this round: `check_upstream_drift.sh` now reads the config
crate pin source. A smoke regression test covers that path:

```sh
cargo test -p yggdrasil-node --test smoke upstream_drift_script_uses_config_crate_pin_source
cargo test -p yggdrasil-node --test smoke
```

Results: focused test passed, then the full node smoke test passed.

Post-fix full gate refresh:

```sh
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
```

Results: all exit 0 on the drift-helper/report diff. The full test run
includes the new smoke guard and keeps the expected three ignored tracer
forwarder doctests.

During the final `cargo test-all` refresh, `yggdrasil-dmq-node` initially
exposed a parallel test-fixture collision in
`crates/tools/dmq-node/src/configuration.rs`: two JSON fixture tests created the
same `/tmp/yggdrasil-dmq-test-<pid>-<stamp>.json` path and one parser observed
trailing bytes from another fixture. The root cause was test-only temporary file
name reuse, not a runtime parser regression. The helper now appends an atomic
per-process sequence and uses `create_new(true)`.

Focused verification after that fix:

```sh
cargo test -p yggdrasil-dmq-node --lib configuration::tests:: -- --test-threads=16
```

Result: exit 0, `10 passed / 0 failed`.

Final full gate refresh after the DMQ test-helper fix:

```sh
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
```

Results: all exit 0. `cargo test-all` again completed unit tests, integration
tests, and doctests successfully with the expected three ignored tracer
forwarder doctests.

Post-fix drift result: exit 0 at `2026-05-19T03:39:52Z`, `total=9`,
`drifted=7`, `unreachable=0`. The pinned audit baseline intentionally remains
`cardano-node 11.0.1`; drift is informational until a coordinated pin advance is
performed.

Operator path hygiene follow-up: the runbook and script `--help` / usage
examples were checked for stale `node/scripts/...` commands and updated to the
actual `crates/node/yggdrasil-node/scripts/...` paths. Verification:

```sh
rg -n '(^|[[:space:]`#])node/scripts/' \
  README.md docs/MANUAL_TEST_RUNBOOK.md docs/manual/block-production.md \
  crates/node/yggdrasil-node/scripts/*.sh
for f in crates/node/yggdrasil-node/scripts/*.sh; do bash -n "$f"; done
cargo test -p yggdrasil-node --test smoke
crates/node/yggdrasil-node/scripts/check_upstream_drift.sh --json
```

Results: no stale `node/scripts/...` command references remained in those
operator surfaces; shell syntax checks passed; `smoke.rs` passed
`9 passed / 0 failed`; drift JSON generation exited 0.

Resume-time focused verification at `2026-05-19T04:34:07Z`:

```sh
cargo fmt --all -- --check
for f in crates/node/yggdrasil-node/scripts/*.sh; do bash -n "$f"; done
git diff --check
cargo test -p yggdrasil-node --test smoke
cargo test -p yggdrasil-dmq-node --lib configuration::tests:: -- --test-threads=16
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
```

Results: all exit 0. Node smoke tests report `9 passed / 0 failed`; DMQ
configuration tests report `10 passed / 0 failed`; parity/reference validators
again report a clean `11.0.1` reference, zero strict-mirror violations, clean
fixture manifest, and a validated reference-artifact install.

Relay-only preflight validation for the non-preview networks:

```sh
target/release/yggdrasil-node validate-config --network preprod --non-producing-node
target/release/yggdrasil-node validate-config --network mainnet --non-producing-node
```

Results: both exit 0. Preprod reports `network_magic: 1`,
`node_role.role: non-producing`, `block_producer_credentials: absent`,
`resolved_startup_peer_count: 97`, and a loaded peer snapshot with
`big_ledger_peer_count: 96`. Mainnet reports
`network_magic: 764824073`, `node_role.role: non-producing`,
`block_producer_credentials: absent`, `configured_fallback_peer_count: 2`,
`resolved_startup_peer_count: 650`, and a loaded peer snapshot with
`big_ledger_peer_count: 647`. Both warn only that storage directories are not
initialized, so restart recovery cannot be validated by preflight alone.

Bounded relay-only runtime diagnostics for preprod/mainnet:

```sh
RELAY_ONLY=1 \
RUN_SECONDS=60 \
YGG_BIN=target/release/yggdrasil-node \
DB_DIR=/tmp/ygg-preprod-relay-helper-audit-db \
LOG_DIR=/tmp/ygg-preprod-relay-helper-audit \
crates/node/yggdrasil-node/scripts/run_preprod_real_pool_producer.sh

RELAY_ONLY=1 \
RUN_SECONDS=60 \
EXPECT_HOT_PEERS=0 \
YGG_BIN=target/release/yggdrasil-node \
DB_DIR=/tmp/ygg-mainnet-relay-helper-audit-db \
LOG_DIR=/tmp/ygg-mainnet-relay-helper-audit \
METRICS_PORT=19103 \
crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
```

Results: both exit 0. The helper scripts now pass `--non-producing-node`
whenever `RELAY_ONLY=1`; a smoke regression test
(`real_pool_relay_only_scripts_force_non_producing_node`) covers that guard.
Preprod evidence from
`/tmp/ygg-preprod-relay-helper-audit/preprod-real-pool-20260519-064606.log`:

```text
Startup.NodeRole ... blockProducerCredentials=absent ... nonProducingNode=true role=non-producing
bootstrap peer connected attempt=1 peer=3.248.56.78:3001
sync complete ... finalPoint=BlockPoint(SlotNo(92460), HeaderHash(39f5338545f6075b...)) ... totalBlocks=350
```

Mainnet evidence from
`/tmp/ygg-mainnet-relay-helper-audit/mainnet-real-pool-20260519-064732.log`:

```text
Startup.NodeRole ... blockProducerCredentials=absent ... nonProducingNode=true role=non-producing
bootstrap peer connected attempt=1 peer=3.77.115.8:3001
sync complete ... finalPoint=Origin ... totalBlocks=0
```

No `Startup.BlockProducer`, `block producer credentials loaded`,
`block producer loop started`, or `invalid VRF proof` lines were present in the
bounded preprod/mainnet relay logs. These are short relay-only diagnostics, not
the planned long preprod/mainnet relay soaks or section 6.5 sign-off.

Local upstream Haskell preview relay startup smoke:

```sh
RUN_ROOT=/tmp/ygg-haskell-preview-relay-smoke \
PORT=13001 \
.reference-haskell-cardano-node/install/run-node.sh preview
```

Initial attempt without `RUN_ROOT` placed the Haskell node socket under the
workspace-mounted `.reference-haskell-cardano-node/install/run/preview/socket/`
tree and exited with `Network.Socket.bind: unsupported operation (Not
supported)`. The generated launcher template in `scripts/setup-reference.sh`
now supports `RUN_ROOT` so local reference sockets and ChainDBs can be placed on
a native Unix-socket-capable filesystem without changing the default install
layout.

Bounded retry result: the process reached `socket-ready` with
`/tmp/ygg-haskell-preview-relay-smoke/preview/socket/node.socket`, then was
terminated cleanly. Key Haskell log evidence in
`/tmp/ygg-haskell-preview-relay-smoke-20260519T044012Z.log`:

```text
Topology: Peer snapshot containing ledger peers recorded at SlotNo 110741160 loaded.
Opened db with immutable tip at genesis (origin) and tip genesis (origin)
CreatedLocalSocket ... "/tmp/ygg-haskell-preview-relay-smoke/preview/socket/node.socket"
ListeningLocalSocket ... "/tmp/ygg-haskell-preview-relay-smoke/preview/socket/node.socket"
LocalSocketUp ... "/tmp/ygg-haskell-preview-relay-smoke/preview/socket/node.socket"
ServerSocketUp ... "0.0.0.0:13001"
```

This proves the local upstream preview relay launcher can provide the
`HASKELL_SOCK` prerequisite for later tip comparisons. It does not prove any
Yggdrasil producer or tip-parity acceptance criterion by itself.

## Bounded Parallel BlockFetch Diagnostic

The full section 6.5 sign-off requires 6-hour and 24-hour preprod/mainnet
windows, so it was not completed in this round. A short preview diagnostic was
run through the same harness to capture current worker-activation behavior:

```sh
YGG_BIN=target/release/yggdrasil-node \
NETWORK=preview \
MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
RUN_SECONDS=120 \
SAMPLE_INTERVAL_S=30 \
COMPARE_INTERVAL_S=999999 \
START_DEADLINE_S=120 \
REQUIRE_WORKERS=1 \
REQUIRE_PROGRESS=1 \
crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh
```

Result: exit 1. The node started, connected to preview, synced blocks, and
shut down cleanly, but the worker activation assertion failed:

```text
ERROR: worker pool never reached EXPECT_WORKERS=2 (max observed 0)
```

Artifacts landed under `/tmp/ygg-blockfetch-soak-xCZ0vr/`. Key metrics:

```text
start: yggdrasil_blocks_synced=0,   yggdrasil_current_slot=0,    yggdrasil_active_peers=0, yggdrasil_blockfetch_workers_registered=0, yggdrasil_blockfetch_workers_migrated_total=0
final: yggdrasil_blocks_synced=500, yggdrasil_current_slot=9980, yggdrasil_active_peers=1, yggdrasil_blockfetch_workers_registered=0, yggdrasil_blockfetch_workers_migrated_total=0
```

Relevant log evidence:

```text
bootstrap peer connected attempt=1 peer=18.185.163.167:3001
sync complete batchesCompleted=12 finalPoint=BlockPoint(SlotNo(10980), HeaderHash(8e25b2b67a77852f...)) reconnectCount=0 stableBlockCount=118 totalBlocks=550 totalRollbacks=1
```

Interpretation: this diagnostic confirms preview relay liveness/progress for a
short window, but it does not satisfy section 6.5 because multi-peer BlockFetch
did not activate and no Haskell tip comparison was enabled.

Follow-up topology check: the vendored preprod topology and the current
Operations Book `preprod/topology.json` for the 11.0.1 environment both contain
one bootstrap peer (`preprod-node.play.dev.cardano.org:3001`) and empty
`localRoots` / `publicRoots` access-point lists. That matches the runbook's
warning that §6.5 cannot activate multi-peer BlockFetch from the stock topology
before ledger-derived peers are available. A real §6.5 sign-off therefore needs
an operator-supplied multi-relay topology, or a run that reaches the
`useLedgerAfterSlot` peer-discovery window, plus the required Haskell tip
comparison socket.

Continuation guard added: `parallel_blockfetch_soak.sh` now supports
`REQUIRE_TIP_COMPARISON=1`, rejects sign-off attempts without `HASKELL_SOCK`,
rejects comparison intervals that cannot fit inside `RUN_SECONDS`, and fails
the final summary path if no Haskell tip comparison passed. This does not
complete §6.5; it prevents a future long BlockFetch soak from being recorded as
sign-off evidence without Haskell comparison coverage.

Focused verification after this guard:

```sh
bash -n crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh \
  crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
cargo fmt --all -- --check
cargo test -p yggdrasil-node --test smoke
git diff --check
```

Results: all exit 0. The node smoke suite now reports
`17 passed / 0 failed`, including the new `REQUIRE_TIP_COMPARISON` coverage for
`parallel_blockfetch_soak.sh`.

Current broad gate refresh after the BlockFetch sign-off guard, recorded at
`2026-05-19T05:22:00Z`:

```sh
cargo check-all
cargo lint
cargo test-all
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
git diff --check
```

Results: all exit 0 on the current audit diff. `cargo test-all` completed unit
tests, integration tests, and doctests successfully; the only ignored tests
remain the expected three tracer-forwarder doctests. The parity matrix,
strict-mirror, fixture-manifest, and reference-artifact validators remain clean
against the `11.0.1` reference install.

## Generated Preview Credential Follow-up

After the operator asked to create the missing keys, a local preview credential
bundle was generated with the vendored `cardano-cli` from the 11.0.1 reference
install. These credentials are real key files and an operational certificate,
but they are newly generated and are **not** an active registered/delegated
preview stake pool. They therefore cover credential-loading and startup
evidence only; they do not satisfy leader-election, forged-block, or adopted
block acceptance criteria.

Credential bundle:

```text
CRED_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z
KES_PERIOD=868
KES_SKEY_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/kes.skey
VRF_SKEY_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/vrf.skey
OPCERT_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/node.cert
ENV_FILE=/tmp/ygg-preview-generated-bp-20260519T052515Z/env.sh
```

Public pool identifier derived from the generated cold verification key:

```text
pool_id_bech32=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q
pool_id_hex=1b0b5ad0c1cb69a647579e72d300ba1a23ff5cd52bc48d6820b5bd7c
```

Additional preview registration-support material was generated after the
completion audit to reduce the active-pool setup blocker. These artifacts are
outside the repository and are **not** evidence of on-chain registration:

```text
REGISTRATION_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z/registration
PAYMENT_ADDRESS=addr_test1qz20nlxh6wuyt2549dw55ge0ecn0l36qv4qsshd5ckpmnj7yceehdm5tjxp2rhtvq5xhp7r7e2q6za9semjp9sjx6veqjme05e
STAKE_ADDRESS=stake_test1urzvvumka69erq4pm4kq2rtslplv4qdpwjcvaeqjcfrdxvs5vr7lk
```

Generated registration-support files:

```text
payment.vkey / payment.skey
stake.vkey / stake.skey
payment.addr / stake.addr
stake.reg.cert
stake.deleg.cert
pool.reg.cert
```

The unsigned certificates use the local preview genesis values
`keyDeposit=2000000`, `poolDeposit=500000000`, and `minPoolCost=340000000`.
At creation time no transaction had been funded, signed, submitted, or observed
on-chain, so the generated pool was initially unregistered and inactive.

Funding check at `2026-05-19T05:50:26Z`: a temporary Haskell preview relay was
started with
`RUN_ROOT=/tmp/ygg-haskell-preview-funding-check PORT=13002 .reference-haskell-cardano-node/install/run-node.sh preview`.
Using its socket, the generated payment address was queried with:

```sh
.reference-haskell-cardano-node/install/bin/cardano-cli conway query utxo \
  --testnet-magic 2 \
  --socket-path /tmp/ygg-haskell-preview-funding-check/preview/socket/node.socket \
  --address "$(cat /tmp/ygg-preview-generated-bp-20260519T052515Z/registration/payment.addr)" \
  --output-json \
  --out-file /tmp/ygg-preview-generated-bp-20260519T052515Z/registration/funding-utxo-20260519T055026Z.json
```

Initial result: exit 0, `utxo_count=0`, `lovelace_total=0`. This confirmed the
generated payment address was unfunded at that checkpoint.

After the operator funded the address from the preview faucet, Koios preview
API evidence at `2026-05-19T05:54:55Z` showed one unspent UTxO:

```text
funding_tx_in=c99cf846037397f594c51ec0ca92e9f12d56edde188b3c58c3561b801ee70e74#0
funding_lovelace=10000000000
funding_block_height=4295068
funding_epoch=1302
```

The pool registration transaction was then built offline from the generated
bundle using the epoch-1302 preview protocol parameters and submitted through
the Koios preview transaction-submit endpoint as CBOR. Submission response:
HTTP 202 with the expected transaction hash.

```text
registration_work_dir=/tmp/ygg-preview-generated-bp-20260519T052515Z/registration/offline-submit-20260519T055951Z
registration_tx_id=e7a492ca7c8419d326db92606a8d55aa9db50f317d309b8dce26740a64e1c03a
registration_fee=185389
registration_change=9497814611
registration_deposit=502000000
submit_response=/tmp/ygg-preview-generated-bp-20260519T052515Z/registration/offline-submit-20260519T055951Z/koios-submit-response-20260519T060033Z.txt
```

Post-submit Koios evidence at `2026-05-19T06:01:12Z` confirmed the transaction
on-chain:

```text
block_hash=7016541e0ceea8226c2e3fb689d538c04bdd0e4f20aece352ff799fdacfc73ca
block_height=4295085
absolute_slot=112514454
epoch=1302
epoch_slot=21654
```

`pool_registrations` and `pool_updates` show
`pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q` registered by the
same transaction with `active_epoch_no=1304`; `account_updates` shows the stake
address registration and pool delegation in that transaction. The preview tip
at the same check was epoch `1302`, epoch slot `21714`, so the generated pool is
registered/delegated but not yet active. With preview `epochLength=86400`, epoch
1304 starts around `2026-05-21T00:00:29Z` UTC from the observed tip.

Live status refresh at `2026-05-19T06:13:09Z`:

```text
pool_update_rows=1
pool_update_type=registration
pool_active_epoch_no=1304
pool_update_tx=e7a492ca7c8419d326db92606a8d55aa9db50f317d309b8dce26740a64e1c03a
tip_epoch=1302
tip_epoch_slot=22384
tip_abs_slot=112515184
tip_block_height=4295107
seconds_to_epoch_1304=150416
epoch_1304_eta_utc=2026-05-21T00:00:05Z
```

Cleanup follow-up added
`crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh` as the
readiness gate for this exact state. With `POOL_ID` and optional `CRED_DIR`, it
queries Koios preview `pool_updates` plus `tip`, prints active-epoch status and
the credential environment, and exits `3` when `REQUIRE_ACTIVE=1` but the pool
is still pending activation. Current run result:

```text
active_epoch_no=1304
current_epoch_no=1302
status=pending
exit_code_with_REQUIRE_ACTIVE_1=3
```

A follow-up operator helper was added at
`crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh` for the
next handoff. Once the generated `PAYMENT_ADDRESS` is funded and a synced
preview node socket is available, it queries the address UTxO, builds a balanced
Conway transaction with certificate order stake registration → pool
registration → stake delegation, signs with payment/stake/cold keys, and
submits only when `SUBMIT=1` is set. The helper is preview-only
(`NETWORK_MAGIC=2`) and does not by itself prove registration.

Cleanup follow-up: the helper now also supports the exact offline path used for
the confirmed transaction. With `OFFLINE_BUILD=1`, `TX_IN`, `INPUT_LOVELACE`,
and `PROTOCOL_PARAMS_FILE`, it builds a raw transaction with iterative minimum
fee calculation, emits signed CBOR artifacts, and can submit to the preview
Koios endpoint with `KOIOS_SUBMIT=1`.

Focused verification for that helper at `2026-05-19T05:44:18Z`:

```sh
bash -n crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh \
  crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh \
  crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh
NETWORK_MAGIC=1 crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh
cargo fmt --all -- --check
cargo test -p yggdrasil-node --test smoke
git diff --check
```

Results: all expected checks passed. The explicit non-preview invocation exits
2 with `NETWORK_MAGIC must be 2`. The node smoke suite now reports
`21 passed / 0 failed`.

Focused verification after adding the offline/Koios registration helper path,
recorded at `2026-05-19T06:10:17Z`:

```sh
cargo fmt --all -- --check
bash -n crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh
bash -n crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
bash -n crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh
cargo test -p yggdrasil-node --test smoke
git diff --check
```

Results: all exit 0. The full node smoke suite now reports
`22 passed / 0 failed`, including the offline/Koios path checks.

Focused verification after adding the activation-status helper:

```sh
bash -n crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh
POOL_ID=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q \
  CRED_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z \
  crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh
POOL_ID=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q \
  REQUIRE_ACTIVE=1 \
  crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh
```

Results: syntax check exits 0; status query exits 0 with `status=pending`;
`REQUIRE_ACTIVE=1` exits 3 while current epoch remains below active epoch.

Activation-status refresh at `2026-05-19T06:24:49Z`:

```text
pool_id=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q
registration_tx=e7a492ca7c8419d326db92606a8d55aa9db50f317d309b8dce26740a64e1c03a
update_type=registration
active_epoch_no=1304
current_epoch_no=1302
current_epoch_slot=22997
current_block_height=4295135
seconds_until_active=149803
active_eta_utc=2026-05-21T00:01:16+00:00
status=pending
```

Follow-up cleanup at `2026-05-19T06:32:32Z`: the activation helper's
`producer_command` handoff now prints the full sign-off shape, including
`HASKELL_SOCK=/tmp/ygg-haskell-preview/preview/socket/node.socket`,
`RUN_SECONDS=21600`, `TIP_COMPARE_CHECKPOINTS=900,3600,21600`,
`EXPECT_FORGE_EVENTS=1`, `EXPECT_ADOPTED_EVENTS=1`, and
`REQUIRE_TIP_COMPARISON=1`. This avoids accidentally starting the active-epoch
resume with the default 600-second diagnostic window.

Follow-up hardening at `2026-05-19T06:37:14Z`: the producer runner now prefers
the vendored `.reference-haskell-cardano-node/install/bin/cardano-cli` for
Haskell tip comparison when it exists, and `REQUIRE_TIP_COMPARISON=1` now
requires `tip_comparisons_run` to equal `tip_comparisons_expected`, not merely
nonzero. The summary artifact records both counts so the 15m/60m/6h evidence
can be audited directly.

Follow-up hardening at `2026-05-19T06:43:43Z`: when `HASKELL_SOCK` is supplied,
the producer runner now performs a preflight
`cardano-cli query tip --testnet-magic 2` before starting the producer. This
fails fast for missing, stale, or wrong-network Haskell sockets instead of
waiting until the first 15-minute comparison checkpoint.

Haskell preview socket readiness check at `2026-05-19T06:50:12Z`:

```sh
RUN_ROOT=/tmp/ygg-haskell-preview PORT=13003 \
  .reference-haskell-cardano-node/install/run-node.sh preview \
  >/tmp/ygg-haskell-preview-readiness-20260519T064941Z.log 2>&1 &
CARDANO_NODE_SOCKET_PATH=/tmp/ygg-haskell-preview/preview/socket/node.socket \
  .reference-haskell-cardano-node/install/bin/cardano-cli query tip \
    --testnet-magic 2
```

Result: the socket at `/tmp/ygg-haskell-preview/preview/socket/node.socket`
became queryable and returned a preview tip:

```json
{
  "epoch": 0,
  "era": "Alonzo",
  "slotInEpoch": 0,
  "slotsToEpochEnd": 86400,
  "syncProgress": "0.00"
}
```

The readiness node was shut down after the query; no `cardano-node` or
`yggdrasil-node` processes remained.

Follow-up hardening at `2026-05-19T06:53:25Z`: the activation helper now
selects the latest Koios `pool_updates` row by `block_time` and
`active_epoch_no` instead of relying on array order. The live status query after
that change still reports the generated pool as `status=pending` with
`active_epoch_no=1304`.

Fresh static/reference gate refresh at `2026-05-19T07:00:22Z` after the
activation-helper and runner hardening:

```sh
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
```

Results: all commands exit 0. `cargo test-all` finishes with the expected
three ignored `yggdrasil_node_tracer` doctests. The reference validators confirm
`docs/parity-matrix.json` still targets `11.0.1`, strict mirror has
`0 violations`, the `cardano-base` fixture manifest pin is consistent, and the
vendored reference install reports `cardano-node 11.0.1` with the required
nine binaries and three network share directories.

Credential and activation refresh at `2026-05-19T07:02:56Z`:

```sh
target/release/yggdrasil-node validate-config \
  --network preview \
  --shelley-kes-key /tmp/ygg-preview-generated-bp-20260519T052515Z/kes.skey \
  --shelley-vrf-key /tmp/ygg-preview-generated-bp-20260519T052515Z/vrf.skey \
  --shelley-operational-certificate /tmp/ygg-preview-generated-bp-20260519T052515Z/node.cert

CRED_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z \
POOL_ID=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q \
  crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh
```

`validate-config` exits 0 and reports:

```json
{
  "role": "block-producer",
  "non_producing_node": false,
  "block_producer_credentials": "complete",
  "credential_fields_present": [
    "ShelleyKesKey",
    "ShelleyVrfKey",
    "ShelleyOperationalCertificate"
  ],
  "credential_fields_missing": []
}
```

The activation check still reports `status=pending` with
`active_epoch_no=1304`, `current_epoch_no=1302`, and
`active_eta_utc=2026-05-21T00:00:08+00:00` as of the latest Koios refresh at
`2026-05-19T07:44:01Z` (`current_block_height=4295325`,
`current_epoch_slot=27762`, `seconds_until_active=145038`).

Active-pool sign-off wrapper added at `2026-05-19T07:08:49Z`:

```sh
CRED_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z \
POOL_ID=pool1rv9445xped56v36hneedxq96rg3l7hx490zg66pqkk7hcrtl26q \
  crates/node/yggdrasil-node/scripts/run_preview_active_pool_signoff.sh
```

The wrapper performs the epoch-1304 resume sequence in one command:

1. Runs `preview_pool_activation_status.sh` with `REQUIRE_ACTIVE=1`.
2. Uses an existing queryable `HASKELL_SOCK`, or starts the vendored Haskell
   preview launcher under `/tmp/ygg-haskell-preview`.
3. Waits for `cardano-cli query tip --testnet-magic 2`.
4. Delegates to `run_preview_real_pool_producer.sh` with
   `RUN_SECONDS=21600`, `TIP_COMPARE_CHECKPOINTS=900,3600,21600`,
   `EXPECT_FORGE_EVENTS=1`, `EXPECT_ADOPTED_EVENTS=1`, and
   `REQUIRE_TIP_COMPARISON=1`.

Current pre-activation verification: help exits 0, Bash syntax check exits 0,
and the real generated-pool invocation exits `3` at the active-status gate with
`status=pending` without starting Haskell or Yggdrasil. Focused smoke coverage
for the wrapper reports `2 passed / 0 failed`.

Follow-up hardening at `2026-05-19T07:13:45Z`: the sign-off wrapper now waits
for the Haskell preview node's `syncProgress` to reach
`HASKELL_SYNC_MIN_PERCENT` (default `99.00`) before starting Yggdrasil. This
keeps the 15-minute comparison checkpoint from being consumed by a Haskell node
that just started from Origin. Operators can still pass a pre-synced
`HASKELL_SOCK`; otherwise the wrapper starts the vendored preview relay and
waits up to `HASKELL_SYNC_TIMEOUT_S` (default `7200`) before failing.

Follow-up hardening at `2026-05-19T07:17:56Z`: the sign-off wrapper now
validates `HASKELL_SYNC_MIN_PERCENT` as a numeric 0-100 percentage before
running the activation gate or starting any long-lived processes. A bad value
such as `abc` exits 2 with a concise error.

Preprod relay-only refresh at `2026-05-19T07:28:39Z`:

```sh
RELAY_ONLY=1 \
RUN_SECONDS=180 \
YGG_BIN=target/release/yggdrasil-node \
LOG_DIR=/tmp/ygg-preprod-relay-20260519T072458Z \
DB_DIR=/tmp/ygg-preprod-relay-20260519T072458Z-db \
  crates/node/yggdrasil-node/scripts/run_preprod_real_pool_producer.sh
```

Result: exit 0. The script observed relay-only mode, connected to a preprod
bootstrap peer, found no `invalid VRF proof`, and reported:

```text
[ok] relay-only preprod verification checks passed
log=/tmp/ygg-preprod-relay-20260519T072458Z/preprod-real-pool-20260519-092458.log
```

Log evidence:

```text
Startup.NodeRole ... blockProducerCredentials=absent nonProducingNode=true role=non-producing
Net.PeerSelection ... bootstrap peer connected attempt=1 peer=54.194.143.142:3001
Node.Sync ... sync complete batchesCompleted=22 finalPoint=BlockPoint(SlotNo(106460), HeaderHash(137bdc081a713cf8...)) reconnectCount=0 totalBlocks=1050 totalRollbacks=1
```

This is bounded relay-only evidence, not the full endurance soak requested for
final operator sign-off.

Mainnet relay-only refresh at `2026-05-19T07:33:36Z`:

```sh
RELAY_ONLY=1 \
RUN_SECONDS=180 \
EXPECT_HOT_PEERS=1 \
YGG_BIN=target/release/yggdrasil-node \
METRICS_PORT=36583 \
LOG_DIR=/tmp/ygg-mainnet-relay-20260519T073336Z \
DB_DIR=/tmp/ygg-mainnet-relay-20260519T073336Z-db \
  crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
```

Result: exit 1. The run stayed relay-only and connected to a mainnet bootstrap
peer twice, but the final metrics assertion reported
`yggdrasil_active_peers=0 < EXPECT_HOT_PEERS=1`. The log shows the sync session
was established, dropped, re-established, then dropped again before any block
batch was accepted:

```text
Startup.NodeRole ... blockProducerCredentials=absent ... nonProducingNode=true role=non-producing
Net.PeerSelection ... bootstrap peer connected attempt=1 peer=34.214.24.177:3001
Net.ConnectionManager.Remote ... verified sync session established fromPoint=Origin ... reconnectCount=0
ChainSync.Client ... chainsync connectivity lost; reconnecting currentPoint=Origin error=mux error: all egress senders closed
Net.ConnectionManager.Remote ... verified sync session re-established fromPoint=Origin ... reconnectCount=1
Node.Sync ... sync complete batchesCompleted=0 finalPoint=Origin reconnectCount=1 totalBlocks=0 totalRollbacks=0
```

No `Startup.BlockProducer`, credential-load, producer-loop, forge/adoption, or
`invalid VRF proof` lines were observed. This refresh confirms the mainnet path
remains non-producing, but it does not satisfy a mainnet relay soak or active
peer health criterion.

Mainnet hot-peer metric cleanup at `2026-05-19T07:54:11Z`: the helper's
`EXPECT_HOT_PEERS` assertion now sums `yggdrasil_active_peers` and
`yggdrasil_active_big_ledger_peers`. Mainnet startup can use bootstrap and
snapshot big-ledger peers, so checking only the non-big-ledger gauge could
misclassify a healthy big-ledger session as zero active peers. The new smoke
test `mainnet_relay_hot_peer_check_counts_big_ledger_peers` failed before the
change and passed after it.

Corrected rerun:

```sh
RELAY_ONLY=1 \
RUN_SECONDS=180 \
EXPECT_HOT_PEERS=1 \
YGG_BIN=target/release/yggdrasil-node \
METRICS_PORT=60695 \
LOG_DIR=/tmp/ygg-mainnet-relay-fixed-20260519T075411Z \
DB_DIR=/tmp/ygg-mainnet-relay-fixed-20260519T075411Z-db \
  crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
```

Result: exit 1. The halfway metric check passed with
`active peers total=1 >= 1 (yggdrasil_active_peers=1
yggdrasil_active_big_ledger_peers=0)`, but the final check failed with
`active peers total=0`. The log shows repeated reconnects to
`3.78.8.215:3001` from `Origin`, each followed by
`chainsync connectivity lost; reconnecting ... all egress senders closed`, and
the run ended at `totalBlocks=0`. This leaves the latest mainnet bounded
diagnostic failed for session durability, not for credential safety or the
previous metric-classification issue.

Mainnet Origin-intersection fix at `2026-05-19T08:11:15Z`: the runtime now
sends `MsgFindIntersect [Origin]` before `MsgRequestNext` instead of treating
`Origin` as a no-op cursor position. The regression test
`runtime_resume_sync_sends_find_intersect_even_from_origin` failed before this
change (`expected origin MsgFindIntersect before RequestNext, got
MsgRequestNext`) and passed after it.

Rebuilt release binary:

```sh
cargo build -p yggdrasil-node --release
```

Corrected mainnet rerun with the rebuilt binary:

```sh
RELAY_ONLY=1 \
RUN_SECONDS=180 \
EXPECT_HOT_PEERS=1 \
YGG_BIN=target/release/yggdrasil-node \
METRICS_PORT=38185 \
LOG_DIR=/tmp/ygg-mainnet-relay-origin-intersect-20260519T081115Z \
DB_DIR=/tmp/ygg-mainnet-relay-origin-intersect-20260519T081115Z-db \
  crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
```

Result: exit 0. Both active-peer checks reported
`active peers total=1 >= 1`; the run stayed relay-only and synced from Origin
through mainnet slot `648`:

```text
Startup.NodeRole ... blockProducerCredentials=absent ... nonProducingNode=true role=non-producing
Net.PeerSelection ... bootstrap peer connected attempt=1 peer=34.214.24.177:3001
Net.ConnectionManager.Remote ... verified sync session established fromPoint=Origin ... reconnectCount=0
Node.Sync ... sync complete batchesCompleted=14 finalPoint=BlockPoint(SlotNo(648), HeaderHash(5f7fdca85e34aba8...)) reconnectCount=0 totalBlocks=650 totalRollbacks=1
```

No `Startup.BlockProducer`, credential-load, producer-loop, forge/adoption, or
`invalid VRF proof` lines were observed. This is bounded relay-only evidence,
not the full mainnet endurance soak.

Final focused validation after the activation-helper and runner hardening:

```sh
cargo fmt --all -- --check
bash -n crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh \
  crates/node/yggdrasil-node/scripts/preview_pool_activation_status.sh \
  crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh \
  crates/node/yggdrasil-node/scripts/run_preview_active_pool_signoff.sh \
  crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
cargo test -p yggdrasil-node --test smoke
python3 .claude/scripts/filetree.py accept-current
python3 .claude/scripts/filetree.py render
python3 .claude/scripts/filetree.py check
git diff --check
```

Results: all exit 0. The full node smoke suite reports
`28 passed / 0 failed`, and filetree check reports all non-exempt entries match
accepted metadata.

The KES period was computed from preview `shelley-genesis.json`
(`systemStart=2022-10-25T00:00:00Z`, `slotLength=1`,
`slotsPerKESPeriod=129600`, `maxKESEvolutions=62`) at generation time. The
credential directory was created outside the repository with `0700`
permissions; signing keys and the opcert/env file were set to `0600`.

Generation command shape:

```sh
.reference-haskell-cardano-node/install/bin/cardano-cli node key-gen \
  --cold-verification-key-file "$CRED_DIR/cold.vkey" \
  --cold-signing-key-file "$CRED_DIR/cold.skey" \
  --operational-certificate-issue-counter-file "$CRED_DIR/cold.counter"
.reference-haskell-cardano-node/install/bin/cardano-cli node key-gen-KES \
  --verification-key-file "$CRED_DIR/kes.vkey" \
  --signing-key-file "$CRED_DIR/kes.skey"
.reference-haskell-cardano-node/install/bin/cardano-cli node key-gen-VRF \
  --verification-key-file "$CRED_DIR/vrf.vkey" \
  --signing-key-file "$CRED_DIR/vrf.skey"
.reference-haskell-cardano-node/install/bin/cardano-cli node issue-op-cert \
  --kes-verification-key-file "$CRED_DIR/kes.vkey" \
  --cold-signing-key-file "$CRED_DIR/cold.skey" \
  --operational-certificate-issue-counter-file "$CRED_DIR/cold.counter" \
  --kes-period "$KES_PERIOD" \
  --out-file "$CRED_DIR/node.cert"
```

Generated credential validation:

```sh
target/release/yggdrasil-node validate-config \
  --network preview \
  --shelley-kes-key /tmp/ygg-preview-generated-bp-20260519T052515Z/kes.skey \
  --shelley-vrf-key /tmp/ygg-preview-generated-bp-20260519T052515Z/vrf.skey \
  --shelley-operational-certificate /tmp/ygg-preview-generated-bp-20260519T052515Z/node.cert
```

Result: exit 0. The validation report at
`/tmp/ygg-preview-generated-bp-20260519T052515Z/validate-config.json` reports
`network_magic: 2`, `node_role.role: block-producer`,
`node_role.non_producing_node: false`,
`node_role.block_producer_credentials: complete`, all three Shelley credential
fields present, and no credential fields missing.

Continuation refresh at `2026-05-19T05:35:39Z` reran the same
`validate-config --network preview --shelley-*` command via
`/tmp/ygg-preview-generated-bp-20260519T052515Z/env.sh`. It again exited 0 and
wrote
`/tmp/ygg-preview-generated-bp-20260519T052515Z/validate-config-refresh-20260519T053539Z.json`
with `network_magic: 2`, `role: block-producer`, `non_producing_node: false`,
and `block_producer_credentials: complete`.

Generated credential startup run:

```sh
KES_SKEY_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/kes.skey \
VRF_SKEY_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/vrf.skey \
OPCERT_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/node.cert \
YGG_BIN=target/release/yggdrasil-node \
LOG_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z/run \
DB_DIR=/tmp/ygg-preview-generated-bp-20260519T052515Z/db \
SOCKET_PATH=/tmp/ygg-preview-generated-bp-20260519T052515Z/ygg.sock \
METRICS_PORT=19022 \
RUN_SECONDS=90 \
METRICS_SNAPSHOT_INTERVAL_S=30 \
EXPECT_FORGE_EVENTS=0 \
EXPECT_ADOPTED_EVENTS=0 \
crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
```

Result: exit 0. Summary:

```text
metrics_snapshots: 5
tip_comparisons_run: 0
leaders: 0
forged: 0
adopted: 0
not_adopted: 0
```

Key generated-run log evidence:

```text
Startup.BlockProducer ... block producer credentials loaded ... opcertKesPeriod=868 opcertSequenceNumber=0
Node.BlockProduction ... block producer loop started ... slotLengthSecs=1
bootstrap peer connected attempt=1 peer=18.117.34.199:3001
sync complete ... finalPoint=BlockPoint(SlotNo(8980), HeaderHash(c37ab0e0742c3f30...)) ... totalBlocks=450 totalRollbacks=1
```

No `invalid VRF proof` was observed in the generated-credential run. No leader,
forge, adopted-block, or Haskell tip-comparison evidence was produced; at that
time the generated pool was not yet registered or active.

Additional guard after this follow-up: `run_preview_real_pool_producer.sh`
tightened `EXPECT_FORGE_EVENTS=1` so it now requires distinct leader-election,
forged-local-block, and forged-block adoption-judgement log evidence. The
stricter `EXPECT_ADOPTED_EVENTS=1` gate still separately requires an adopted
forged block.

Focused verification after the generated-key and forge-evidence guard changes:

```sh
cargo fmt --all -- --check
bash -n crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh \
  crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh
cargo test -p yggdrasil-node --test smoke
git diff --check
```

Results: all exit 0. The node smoke suite now reports
`18 passed / 0 failed`.

Post-generated-key broad gate refresh at `2026-05-19T05:33:43Z`:

```sh
cargo check-all
cargo lint
cargo test-all
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
pgrep -af '(^|/)(yggdrasil-node|cardano-node)( |$)' || true
```

Results: all exit 0. `cargo test-all` completed unit tests, integration tests,
and doctests successfully; only the expected three tracer-forwarder doctests
were ignored. The parity matrix, strict-mirror, fixture-manifest, and
reference-artifact validators remain clean, and no stale `yggdrasil-node` or
`cardano-node` processes were running after the audit attempts.

## Real Preview Producer And Relay Guards

The real preview producer runner was added but only exercised through
non-secret, preflight-safe checks because the actual preview pool credential
paths were unavailable.

```sh
bash -n crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
for f in \
  crates/node/yggdrasil-node/scripts/run_preprod_real_pool_producer.sh \
  crates/node/yggdrasil-node/scripts/run_mainnet_real_pool_producer.sh
do
  bash -n "$f"
done
cargo test -p yggdrasil-node --test smoke preview_real_pool_producer_script
cargo test -p yggdrasil-node --test smoke
cargo test -p yggdrasil-node --test smoke real_pool_relay_only_scripts_force_non_producing_node
```

Results:

```text
bash -n: exit 0
preview_real_pool_producer_script_*: 3 passed / 0 failed
smoke.rs: 15 passed / 0 failed
real_pool_relay_only_scripts_force_non_producing_node: 1 passed / 0 failed
```

Credential environment status at the time of the runner check:

```text
KES_SKEY_PATH=unset
VRF_SKEY_PATH=unset
OPCERT_PATH=unset
```

The smoke guard verifies `--help` documents `--network preview` and the three
required credential environment variables plus `HASKELL_SOCK`,
`TIP_COMPARE_CHECKPOINTS`, `REQUIRE_TIP_COMPARISON`, `METRICS_DIR`, and
`METRICS_SNAPSHOT_INTERVAL_S`; it also verifies the script rejects a missing
`KES_SKEY_PATH` file before starting the node, that tip-comparison failure
aborts the producer run while the loop is under `set +e`, and that a
credentialed run fails unless it captures at least one Prometheus metrics
snapshot from the configured metrics port. It also checks that `RUN_SECONDS`,
`METRICS_PORT`, and `METRICS_SNAPSHOT_INTERVAL_S` are validated as positive
integers before a run starts. `EXPECT_FORGE_EVENTS`,
`EXPECT_ADOPTED_EVENTS`, and `REQUIRE_TIP_COMPARISON` are validated as `0`/`1`
flags, and `REQUIRE_TIP_COMPARISON=1` now fails before startup unless every
configured `TIP_COMPARE_CHECKPOINTS` value falls within `RUN_SECONDS`. After
`validate-config` exits 0, the runner parses its JSON report and requires
`node_role.role: block-producer`, `non_producing_node: false`,
`block_producer_credentials: complete`, all three Shelley credential fields
present, and no credential fields missing before the producer process starts.
When `EXPECT_FORGE_EVENTS=1`, the runner now requires distinct leader-election,
forged-local-block, and forged-block adoption-judgement log evidence; the
stricter `EXPECT_ADOPTED_EVENTS=1` gate separately requires an adopted forged
block.
On successful completion the runner writes
`preview-real-pool-summary-<run-id>.txt` beside the validation JSON and node log;
the summary records the validation/log/metrics artifact paths, metrics snapshot
count, tip-comparison count, configured checkpoints, and leader/forge/adoption
event counts.

Final static refresh after the preview runner validation-report and summary
guards were added:

```sh
cargo fmt --all -- --check
python3 scripts/check-parity-matrix.py
python3 scripts/check-strict-mirror.py --fail-on-violation
python3 scripts/check-fixture-manifest.py
python3 scripts/check-reference-artifacts.py
cargo check-all
cargo lint
cargo test-all
```

Results: all exit 0. `cargo test-all` completed unit tests, integration tests,
and doctests successfully; the only ignored tests remain the expected three
tracer-forwarder doctests. The final full-node smoke suite in this audit reports
`15 passed / 0 failed` at that checkpoint.

Fresh blocker verification after the completion-audit checklist was added:

```sh
YGG_BIN=target/release/yggdrasil-node \
env -u KES_SKEY_PATH -u VRF_SKEY_PATH -u OPCERT_PATH \
crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
```

Result: exit 1 before node startup:

```text
ERROR: missing KES_SKEY_PATH file:
```

## Remaining Blocked Producer Evidence

The generated preview pool is now registered/delegated on preview, but Koios
reports `active_epoch_no=1304` while the observed network tip is still epoch
`1302`. The active registered/delegated producer soak therefore remains blocked
until the active epoch window begins and a Haskell preview socket is available
for the required tip comparisons.

```sh
target/release/yggdrasil-node validate-config \
  --network preview \
  --shelley-kes-key "$KES_SKEY_PATH" \
  --shelley-vrf-key "$VRF_SKEY_PATH" \
  --shelley-operational-certificate "$OPCERT_PATH"

target/release/yggdrasil-node run \
  --network preview \
  --database-path /tmp/ygg-preview-real-bp-db \
  --socket-path /tmp/ygg-preview-real-bp.sock \
  --metrics-port 19002 \
  --shelley-kes-key "$KES_SKEY_PATH" \
  --shelley-vrf-key "$VRF_SKEY_PATH" \
  --shelley-operational-certificate "$OPCERT_PATH"

HASKELL_SOCK=/tmp/cardano-preview.sock \
TIP_COMPARE_CHECKPOINTS=900,3600,21600 \
REQUIRE_TIP_COMPARISON=1 \
crates/node/yggdrasil-node/scripts/run_preview_real_pool_producer.sh
```

Because an active registered/delegated preview producer did not run, this round
still has no evidence for:

- KES/opcert consistency against an active preview pool;
- leader election;
- local block forging;
- adopted forged block;
- 15-minute, 60-minute, or 6-hour tip comparison against a local Haskell
  preview relay.

No `invalid VRF proof`, credential, KES expiry, or block-adoption errors were
observed in the generated-credential startup run, but that run is not active
pool adoption evidence.

## Completion Audit Checklist

Objective restated: execute the full project parity audit against
`IntersectMBO/cardano-node 11.0.1`, with static/reference evidence recorded and
real preview block-producer validation performed only when real credential paths
are supplied.

| Requirement | Evidence | Status |
| --- | --- | --- |
| Reference target is `IntersectMBO/cardano-node 11.0.1`. | `bash scripts/setup-reference.sh --force` exit 0; installed reference reports `cardano-node 11.0.1` and git rev `97036a66bcf8c89f687ae57a048eecc0389977ef`. | Verified |
| Release binary is available for operator checks. | `cargo build --release -p yggdrasil-node` exit 0. | Verified |
| Static Cargo gates pass. | `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and final `cargo test-all` all exit 0; only the expected three tracer-forwarder doctests are ignored. | Verified |
| Parity-flow validators pass. | `check-parity-matrix.py`, `check-strict-mirror.py --fail-on-violation`, `check-fixture-manifest.py`, and `check-reference-artifacts.py` all exit 0. | Verified |
| Preview real-pool runner uses execution-time credentials, not generated harness credentials. | `run_preview_real_pool_producer.sh` requires `KES_SKEY_PATH`, `VRF_SKEY_PATH`, and `OPCERT_PATH`, runs `validate-config --network preview`, asserts the validate report confirms block-producer mode with complete Shelley credentials, then runs `yggdrasil-node run --network preview` directly. | Verified by script review and smoke tests |
| Generated preview harness remains reference/relay material only. | README, manual runbook, block-production chapter, and node AGENTS guidance route producer parity to `run_preview_real_pool_producer.sh`. | Verified by docs review |
| Generated preview credential paths are available. | Created `/tmp/ygg-preview-generated-bp-20260519T052515Z/{kes.skey,vrf.skey,node.cert}` plus an env helper at `/tmp/ygg-preview-generated-bp-20260519T052515Z/env.sh`. | Verified for generated credentials |
| Generated preview registration-support material is available. | Created `/tmp/ygg-preview-generated-bp-20260519T052515Z/registration/{payment.skey,stake.skey,payment.addr,stake.addr,stake.reg.cert,stake.deleg.cert,pool.reg.cert}` using preview genesis deposits/cost. No transaction was submitted. | Verified for generated setup only |
| Generated preview registration transaction helper exists. | `crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh` builds/signs the preview-only registration transaction from the generated bundle and requires `SUBMIT=1` before submission. Smoke coverage checks help text, preview-only gating, certificate order, and required witnesses. | Verified as setup helper |
| Generated payment address has preview funds. | Koios preview `address_info`/`address_utxos` showed `c99cf846037397f594c51ec0ca92e9f12d56edde188b3c58c3561b801ee70e74#0` with `10000000000` lovelace. | Verified |
| Generated pool registration/delegation transaction is submitted and confirmed. | Transaction `e7a492ca7c8419d326db92606a8d55aa9db50f317d309b8dce26740a64e1c03a` returned HTTP 202 from Koios submit and is confirmed at block height `4295085`; `pool_updates` shows registration, and `account_updates` shows stake registration/delegation. | Verified |
| Operator active preview pool credential paths are available. | Generated credential paths are available and now registered/delegated on preview, but the pool is not active until epoch `1304`. | Pending active epoch |
| Generated preview `validate-config` succeeds with KES/VRF/OpCert. | `target/release/yggdrasil-node validate-config --network preview --shelley-* /tmp/ygg-preview-generated-bp-20260519T052515Z/...` exits 0 and reports complete block-producer credentials. | Verified for generated credentials |
| Producer logs show `Startup.BlockProducer`, `block producer credentials loaded`, and `block producer loop started`. | The generated-credential 90s run logs all three startup signals and captures five metrics snapshots. | Verified for generated startup only |
| Active-pool window shows leader election, local forged block, and adopted forged block. | Generated-credential run reports `leaders=0`, `forged=0`, `adopted=0`; no active registered pool evidence collected. | Blocked |
| Haskell preview relay tip comparisons pass at 15m, 60m, and 6h. | `run_preview_real_pool_producer.sh` enforces all configured checkpoints when `REQUIRE_TIP_COMPARISON=1`; `run_preview_active_pool_signoff.sh` can start/validate a Haskell preview socket and waits for `syncProgress` before delegating. No active producer run has occurred yet. | Blocked |
| Local Haskell preview relay can provide `HASKELL_SOCK`. | `RUN_ROOT=/tmp/ygg-haskell-preview-relay-smoke PORT=13001 .reference-haskell-cardano-node/install/run-node.sh preview` reached socket-ready at `/tmp/ygg-haskell-preview-relay-smoke/preview/socket/node.socket`; launcher now supports `RUN_ROOT` for Unix-socket-capable run dirs. | Verified |
| Preprod/mainnet stay relay-only for this audit. | No preprod/mainnet producer credentials were used in this round; both networks passed `validate-config --non-producing-node` with `block_producer_credentials: absent`. The latest preprod helper diagnostic passed, and the latest mainnet helper diagnostic stayed non-producing and passed after the Origin-intersection runtime fix. | Verified relay-only safety |
| Bounded preprod relay-only diagnostic passes. | `RELAY_ONLY=1 RUN_SECONDS=180 YGG_BIN=target/release/yggdrasil-node ... run_preprod_real_pool_producer.sh` exits 0; log shows non-producing role, bootstrap peer connection, checkpoint progression, and `totalBlocks=1050`. | Verified diagnostic |
| Bounded mainnet relay-only diagnostic passes. | The helper now sums non-big-ledger and big-ledger active-peer gauges, and the runtime sends `MsgFindIntersect [Origin]` before the first `MsgRequestNext`. The rebuilt `RELAY_ONLY=1 RUN_SECONDS=180 EXPECT_HOT_PEERS=1 ... run_mainnet_real_pool_producer.sh` exits 0, reports `active peers total=1 >= 1`, and syncs from Origin through mainnet slot `648` with `totalBlocks=650`. | Verified diagnostic |
| Preprod/mainnet relay endurance soaks are complete. | Preprod and mainnet now both have bounded 180s relay-only diagnostics, but no current multi-hour endurance relay run was completed. | Not verified |
| Section 6.5 BlockFetch soaks are complete. | Only a bounded preview diagnostic ran; worker assertion failed (`max observed 0`). | Not verified |
| No parity docs/matrix are promoted without evidence. | No parity matrix entries were promoted. | Verified |

## Documentation And Matrix Impact

No parity matrix entries were promoted in this round. Static/reference gates
are current as of this audit, but the requested real preview producer
acceptance criteria are not verified. Operator docs were updated so preview
producer parity runs point at the real-credential runner while the generated
preview harness is retained only for wallet/cert reference material and relay
smoke.

At or after preview epoch `1304`, rerun the credential-validation command with
the generated `KES_SKEY_PATH`, `VRF_SKEY_PATH`, and `OPCERT_PATH` first. Only
after that succeeds should the producer soak, Haskell preview relay comparison,
preprod/mainnet relay-only soaks, and section 6.5 BlockFetch soaks be recorded
as operational evidence.

## Continuation Completion Audit

Continuation check at `2026-05-19T05:08:47Z` restated the active goal as these
deliverables:

- maintain the upstream target at `IntersectMBO/cardano-node 11.0.1`;
- keep the static Cargo and parity-flow gates green on the current audit diff;
- provide dated documentation evidence for the audit commands and outcomes;
- validate and run a real preview block producer only from
  `KES_SKEY_PATH`, `VRF_SKEY_PATH`, and `OPCERT_PATH`;
- require producer startup, leader/forge/adoption evidence, and Haskell tip
  comparisons before claiming the producer acceptance criteria;
- avoid promoting parity matrix entries without direct evidence.

Prompt-to-artifact mapping:

| Prompt requirement | Current artifact / command evidence | Audit result |
| --- | --- | --- |
| Audit target remains `11.0.1`. | Reference setup evidence above records `cardano-node 11.0.1` and git rev `97036a66bcf8c89f687ae57a048eecc0389977ef`; `check-reference-artifacts.py` passed in the final static refresh. | Covered |
| Static gates and parity validators are current. | Final refresh above records `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, `cargo test-all`, and all four parity-flow validators exiting 0. | Covered |
| Documentation evidence is dated and command-oriented. | This report records commands, result summaries, log snippets, blocked evidence, and the checklist above. | Covered |
| Generated preview credentials are available. | Created `/tmp/ygg-preview-generated-bp-20260519T052515Z/{kes.skey,vrf.skey,node.cert}` and `/tmp/ygg-preview-generated-bp-20260519T052515Z/env.sh`. | Covered for generated credentials |
| Generated preview registration inputs are available. | Created the payment/stake keys, payment and stake addresses, stake registration/delegation certificates, and pool registration certificate under `/tmp/ygg-preview-generated-bp-20260519T052515Z/registration`. | Covered for generated setup only |
| Generated preview registration transaction path is repeatable. | `register_preview_generated_pool.sh` now supports both synced-local-socket build/submission and the offline explicit-UTxO + `KOIOS_SUBMIT=1` path used for the confirmed transaction; documented in `docs/MANUAL_TEST_RUNBOOK.md`. | Covered |
| Generated preview active-epoch readiness is repeatable. | `preview_pool_activation_status.sh` checks Koios pool status/tip, prints `seconds_until_active`, and exits 3 when `REQUIRE_ACTIVE=1` before epoch `1304`. | Covered, pending active epoch |
| Active-pool sign-off handoff is repeatable. | `run_preview_active_pool_signoff.sh` gates on active registration, starts or validates Haskell preview, waits for Haskell sync progress, then delegates to the real producer runner with 15m/60m/6h tip comparison plus forge/adoption gates. Current pre-activation invocation exits 3 at the status gate. | Covered, pending active epoch |
| Generated payment address is funded. | Koios preview evidence shows `10000000000` lovelace at the generated payment address before registration. | Covered |
| Generated preview pool is registered/delegated. | Koios preview evidence confirms registration/delegation transaction `e7a492ca7c8419d326db92606a8d55aa9db50f317d309b8dce26740a64e1c03a`, with pool registration active from epoch `1304`. | Covered, activation pending |
| Active registered/delegated preview credentials are available. | Pool registration/delegation is confirmed, but the current network tip is epoch `1302`; active pool evidence begins at epoch `1304`. | Pending active epoch |
| Preview producer `validate-config` runs with KES/VRF/OpCert. | Generated credential validation exits 0 and reports complete block-producer credentials; the same credential paths are now registered/delegated on-chain, with active epoch pending. | Covered, activation pending |
| Producer runtime evidence is collected. | Generated 90s run records `Startup.BlockProducer`, credential load, producer loop start, bootstrap connection, sync progress, and five metrics snapshots. Active-pool leader/forge/adoption evidence remains absent. | Partially covered |
| Relay-only non-preview evidence is collected. | Bounded preprod relay-only run exits 0 with non-producing role, preprod bootstrap connection, and `totalBlocks=1050`. Bounded mainnet relay-only run exits 0 after the Origin-intersection fix, stays non-producing, reports `active peers total=1 >= 1`, and syncs through slot `648` with `totalBlocks=650`. Neither result replaces the requested endurance soaks. | Partially covered |
| Haskell tip comparisons run at the configured checkpoints. | The runner and sign-off wrapper enforce the Haskell socket, sync-progress wait, and exact checkpoint count, but no active producer run has occurred yet. | Blocked |
| Parity matrix is not promoted without producer evidence. | `git status --short docs/parity-matrix.json` and `git diff -- docs/parity-matrix.json` produced no output. | Covered |
| Workspace diff is syntactically clean. | `git diff --check` exited 0. | Covered |
| No stale node processes remain from audit attempts. | `pgrep -af '(^|/)(yggdrasil-node|cardano-node)( |$)' || true` produced no running node processes. | Covered |

Conclusion: the active goal is not complete. The implemented runner, generated
credential bundle, confirmed preview registration/delegation, docs, and
static/reference evidence are ready, but active preview block-producer
validation, Haskell comparisons, and adoption evidence remain blocked until
epoch 1304 begins and the active-pool sign-off wrapper can run through the
leader/forge/adoption and 15m/60m/6h comparison gates.
