# Temporary Rebuild Resume Plan

This file is temporary context for resuming after a devcontainer rebuild. Remove
it once the environment is rebuilt and the work is either committed or replaced
by permanent docs.

## Current Goal

Make the development environment reliable for Yggdrasil, a pure Rust Cardano
node, with clean separation between:

- NtN/P2P: TCP networking for relays/block producers and peer connections.
- NtC: Unix socket access for local query, local tx submission, and
  `cardano-cli`-style tooling.

Do not tag a release or publish public distribution artifacts yet. Preview sync
is not fully green; the current blocker is ledger/Plutus parity, not a
devcontainer socket or P2P port problem.

## Devcontainer Changes In Progress

Files changed:

- `.devcontainer/devcontainer.json`
- `.devcontainer/post-create.sh`
- `docs/CONTRIBUTING.md`

Expected devcontainer behavior after rebuild:

- Forwarded TCP ports:
  - `3001`: default Yggdrasil NtN peer port
  - `13001`: preview relay NtN local P2P rehearsal
  - `12798`: docker-compose metrics
  - `19002`: preview producer metrics
  - `9001`, `9099`, `9101`: runbook metrics ports
- Exported environment:
  - `CARDANO_NODE_SOCKET_PATH=/workspaces/Cardano-node/tmp/preview-producer/run/preview-producer.sock`
  - `YGG_PREVIEW_PRODUCER_SOCKET=/workspaces/Cardano-node/tmp/preview-producer/run/preview-producer.sock`
  - `YGG_PREVIEW_RELAY_SOCKET=/workspaces/Cardano-node/tmp/preview-producer/run/preview-relay.sock`
  - `YGG_PREVIEW_CONFIG=/workspaces/Cardano-node/tmp/preview-producer/config/preview-producer.json`
  - `YGG_PREVIEW_TOPOLOGY=/workspaces/Cardano-node/tmp/preview-producer/config/topology-fast.json`
- `post-create.sh` prepares:
  - `tmp/preview-producer/run`
  - `tmp/preview-producer/logs`
  - `tmp/preview-producer/db`
  - `tmp/preview-producer/config`
  - `tmp/preview-producer/keys`
  - mode `0775`

Validation already run before rebuild:

```sh
bash -n .devcontainer/post-create.sh
python3 - <<'PY'
import json, re, pathlib
p = pathlib.Path('.devcontainer/devcontainer.json')
text = '\n'.join(line for line in p.read_text().splitlines()
                 if not re.match(r'^\s*//', line))
json.loads(text)
print('devcontainer jsonc parses after stripping full-line comments')
PY
```

## Important Clarification

`SocketPath` is for node-to-client Unix socket traffic. It does not make P2P
available. P2P/node-to-node availability depends on TCP listener config and
container port forwarding. For host/container reachability, bind relay or
producer listeners to `0.0.0.0:<port>`; `127.0.0.1:<port>` is only in-container
loopback.

## Other Uncommitted Work To Preserve

Current dirty files before rebuild:

- `.devcontainer/devcontainer.json`
- `.devcontainer/post-create.sh`
- `crates/ledger/src/cbor.rs`
- `crates/ledger/src/state.rs`
- `docs/CONTRIBUTING.md`
- `node/src/sync.rs`

Do not discard these without checking diffs.

Relevant code changes already made:

- `crates/ledger/src/cbor.rs`
  - `extract_block_tx_byte_spans()` unwraps HFC envelopes shaped
    `[era_index, inner_block]` before extracting transaction body/witness spans.
  - Test added: `extract_block_tx_byte_spans_unwraps_hfc_envelope`.
- `node/src/sync.rs`
  - Ledger recovery now replays stored `raw_cbor` through node-level multi-era
    decode plus raw span extraction, preserving on-wire transaction bytes during
    startup recovery.
- `crates/ledger/src/state.rs`
  - Babbage/Conway block apply only checks produced output reference scripts
    when `tx_is_valid` is true, while still checking `collateral_return`.
  - This did not fully clear the live preview blocker.

## Preview Runtime State

Known config paths:

```sh
tmp/preview-producer/config/preview-producer.json
tmp/preview-producer/config/topology-fast.json
/workspaces/Cardano-node/tmp/preview-producer/db/producer
```

Last known recovered chain point:

- Slot: `730728`
- Era: Babbage
- Epoch: 8

Status command:

```sh
target/release/yggdrasil-node status \
  --config tmp/preview-producer/config/preview-producer.json \
  --database-path /workspaces/Cardano-node/tmp/preview-producer/db/producer
```

Start command pattern:

```sh
mkdir -p tmp/preview-producer/logs tmp/preview-producer/run
stamp=$(date -u +%Y%m%d-%H%M%S)
log="tmp/preview-producer/logs/preview-producer-batch128-mp4-pipelined-${stamp}.log"
ln -sf "$(basename "$log")" tmp/preview-producer/logs/latest-continuous.log
setsid target/release/yggdrasil-node run \
  --config tmp/preview-producer/config/preview-producer.json \
  --topology tmp/preview-producer/config/topology-fast.json \
  --batch-size 128 \
  --max-concurrent-block-fetch-peers 4 \
  --metrics-port 19002 \
  > "$log" 2>&1 < /dev/null &
echo $! > tmp/preview-producer/run/producer.pid
```

Log checks:

```sh
rg -n "InvalidBlock|ledger decode error|MalformedReferenceScripts|malformed reference|sync failed|panic|rollback before end of pipelined|no direct BlockFetch|ChainSync.Client.*connectivity lost" \
  tmp/preview-producer/logs/latest-continuous.log | tail -140 || true

rg -n "Recovery.Checkpoint|recovered ledger state|verified sync session|switching sync session|BlockFetch.Client.CompletedBlockFetch|verified sync batch|slot=" \
  tmp/preview-producer/logs/latest-continuous.log | tail -160 || true
```

## Current Live Blocker

After recovery to slot `730728`, preview sync currently fails with:

```text
ledger decode error: malformed reference script(s):
[[8d,73,f1,25,39,54,66,f1,d6,85,70,44,7e,4f,4b,87,cd,63,3c,67,28,f3,80,2b,2d,cf,ca,20],
 [d4,2e,f9,52,43,04,ec,04,44,a5,0d,4a,39,9d,18,d9,27,47,03,92,93,70,64,64,71,dc,b4,28]]
```

Likely next investigation:

- `node/src/plutus_eval.rs::CekPlutusEvaluator::is_script_well_formed()` calls
  `decode_script_bytes(script_bytes).is_ok()`.
- The local Flat/UPLC decoder may be rejecting valid on-chain reference scripts.
  Upstream accepted the preview block, so a local false negative here breaks
  sync parity.
- Be careful: upstream does have `MalformedReferenceScripts`, so do not simply
  remove the rule without documenting why. A pragmatic intermediate fix may be
  to avoid using the incomplete CEK parser for admission-time well-formedness
  while keeping actual script evaluation strict.

Relevant files:

- `node/src/plutus_eval.rs`
- `crates/plutus/src/flat.rs`
- `crates/ledger/src/witnesses.rs`
- `crates/ledger/src/state.rs`

## Commands To Re-run After Rebuild

```sh
git status --short
bash -n .devcontainer/post-create.sh
cargo fmt
cargo test -p yggdrasil-ledger cbor::tests::extract_block_tx_byte_spans --lib
cargo test -p yggdrasil-ledger witness_validation --test integration
cargo test -p yggdrasil-node sync:: --lib
cargo build -p yggdrasil-node --release
```

If the Plutus/reference-script fix changes behavior, also run focused node
tests around `plutus_eval` and any new ledger tests added for the rule.

## AGENTS.md Updates Still Needed

Before finalizing any code changes, update operational notes in:

- `node/src/AGENTS.md`
  - raw-aware recovery/replay and ChainSync/BlockFetch sync notes
- `crates/ledger/AGENTS.md`
  - HFC envelope span extraction and reference-script well-formedness note
- `crates/network/AGENTS.md`
  - ChainSync pipelined `MsgRequestNext` note if the current file does not
    already mention it

Keep these entries concise and actionable.

## Resume Order

1. Confirm devcontainer rebuild picked up `remoteEnv`, forwarded ports, and
   runtime directories.
2. Verify status command can inspect the existing preview database.
3. Continue the malformed reference script investigation.
4. Re-run focused tests and release build.
5. Restart preview sync and confirm it progresses past slot `730728`.
6. Only after preview sync and the required test gates are green, consider docs,
   README, GitHub Pages, commit, tag, and release work.
