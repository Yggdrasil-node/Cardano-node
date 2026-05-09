---
title: 'R345: cardano-submit-api comparison harness — operator-runnable soak vs upstream'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-345-cardano-submit-api-comparison-harness/
---

# Round 345 — cardano-submit-api comparison harness

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R344`](2026-05-10-round-344-cardano-submit-api-prometheus-metrics.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R345 ships the comparison-harness script for byte-for-byte verification
of `yggdrasil-cardano-submit-api` against the upstream Haskell binary
across the canonical request surface. The script is **operator-
runnable, not CI-runnable** because it requires:

- A live cardano-node (for the NtC LocalTxSubmission backend).
- The vendored upstream `cardano-submit-api` binary running.
- Both binaries connected to the same socket.

CI cannot satisfy any of these. Promotion of the parity-matrix entry
to `verified_11_0_1` (R346 closeout) is gated on an operator running
this script and reporting an empty diff.

## Diff inventory

- `node/scripts/compare_submit_api_to_upstream.sh` — new 175-line
  bash script. POSTs canonical inputs (empty body, malformed CBOR)
  to both endpoints, diffs HTTP status + body. Scrapes /metrics
  from both, diffs the `# HELP` / `# TYPE` shape (counter values
  legitimately differ between concurrent binaries — only the
  exposition shape matters for parity).

## Procedure

```bash
# Terminal 1: bring up upstream cardano-node + cardano-submit-api on preview.
.reference-haskell-cardano-node/install/bin/cardano-node run \
  --config .reference-haskell-cardano-node/install/share/preview/config.json \
  ...

.reference-haskell-cardano-node/install/bin/cardano-submit-api \
  --config /etc/submit-api-upstream.json \
  --socket-path /tmp/preview/socket/node.socket \
  --testnet-magic 2 \
  --port 18090

# Terminal 2: yggdrasil-cardano-submit-api against the same socket.
cargo run --release --bin cardano-submit-api -- \
  --config /etc/submit-api-yggdrasil.json \
  --socket-path /tmp/preview/socket/node.socket \
  --testnet-magic 2 \
  --port 18091 \
  --metrics-port 18182

# Terminal 3: comparison harness.
node/scripts/compare_submit_api_to_upstream.sh
```

Expected output on success:

```
--- empty body ---
  upstream:  HTTP 400 body={"tag":"TxSubmitEmpty"}
  yggdrasil: HTTP 400 body={"tag":"TxSubmitEmpty"}
  ✓ identical (HTTP 400)
--- malformed CBOR ---
  upstream:  HTTP 400 body={"tag":"TxSubmitFail",...}
  yggdrasil: HTTP 400 body={"tag":"TxSubmitFail",...}
  ✓ identical (HTTP 400)
--- upstream /metrics ---
# HELP tx_submit_fail Number of failed tx submissions
# TYPE tx_submit_fail counter
tx_submit_fail 2
# HELP tx_submit Number of successful tx submissions
# TYPE tx_submit counter
tx_submit 0
--- yggdrasil /metrics ---
# HELP tx_submit Number of successful tx submissions
# TYPE tx_submit counter
tx_submit 0
# HELP tx_submit_fail Number of failed tx submissions
# TYPE tx_submit_fail counter
tx_submit_fail 2
--- /metrics shape diff (counter values stripped) ---
  ✓ counter shape (HELP + TYPE) identical

All endpoints byte-identical. Phase A.2 closeout (R346) can
promote sister-tool.cardano-submit-api parity-matrix entry
from 'partial' to 'verified_11_0_1'.
```

(Note: counter line ordering may legitimately differ between the two
binaries — upstream emits in registry-insertion order, Yggdrasil emits
in the order of the `render_prometheus` `format!` literal. The HELP
+ TYPE shape diff strips line ordering and only checks that both
include the same metadata for both counters.)

## Failure modes the harness catches

| Failure | Symptom | Likely cause |
|---|---|---|
| HTTP status mismatch | `STATUS DIVERGED: 400 != 503` | Yggdrasil's NtC connect reachability differs (e.g. wrong socket path) |
| Empty body shape mismatch | `BODY DIVERGED: TxSubmitEmpty != ...` | Yggdrasil's empty-body branch in `tx_submit_post` regressed |
| Malformed CBOR error wrapper differs | `BODY DIVERGED:` | Yggdrasil's TxCmdError → JSON serialization regressed (likely a serde tag/content config change) |
| /metrics counter set differs | shape diff non-empty | New counter added in one binary but not the other |
| Either endpoint unreachable | `... unreachable on port` | One binary failed to bind or crashed |

## Carve-out: inner-reason-bytes for malformed CBOR

The malformed-CBOR test case (`\xff\xff\xff\xff`) hits cardano-node's
mempool validation, which returns `MsgRejectTx { reason: <era-specific
CBOR> }`. The reason bytes can legitimately differ between binaries
because:

- Upstream binds against its own `Cardano.Api.TxValidationErrorInCardanoMode`
  surface and renders the reason via `Show` instance.
- Yggdrasil hex-encodes the reason bytes verbatim into a string.

The wrapper shape (`{"tag":"TxSubmitFail","contents":{"tag":"TxCmdTxSubmit
ValidationError",...}}`) MUST match; the inner `contents` string is a
documented divergence (recorded in `cli/types.rs` strict-mirror
docstring as the R340+ TODO).

The harness reports this category as a non-fatal warning: `(note: inner
reason string may legitimately differ between binaries — the wrapper
shape must still match.)`.

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton                                                           | done        |
| R339  | Foundations                                                        | done        |
| R340  | Type bridges                                                       | done        |
| R341  | Trace surface                                                      | done        |
| R342  | Web server                                                         | done        |
| R343  | LocalTxSubmission wiring                                           | done        |
| R344  | Prometheus metrics                                                 | done        |
| R345  | Comparison harness (operator-runnable soak)                        | **this**    |
| R346  | Closeout: parity-matrix `verified_11_0_1` (gated on operator soak) | next        |

## Notes for future readers

The decision to ship the harness as a bash script in `node/scripts/`
(rather than as a `cargo test` integration test) was made because:

1. **Live-network requirement.** A `cargo test` couldn't bring up
   the upstream binary or attach to a real cardano-node socket from
   inside the test harness; the test would always be `#[ignore]`'d
   in CI.
2. **Operator-facing format.** Operators who want to verify the
   binary swap in their own environment expect a runnable script,
   not a test target.
3. **Existing pattern.** `node/scripts/compare_tip_to_haskell.sh`
   uses the same shape for the chain-tip comparison; staying
   consistent reduces operator cognitive load.

If a future round introduces a CI-runnable simulator (e.g. a mocked
cardano-node socket that returns scripted MsgAcceptTx/MsgRejectTx),
the bash script can be retained as the operator-facing form and a
companion `cargo test --ignored` integration test added for CI
regression coverage.
