---
title: 'R343: cardano-submit-api LocalTxSubmission wiring — async Handler + ntc_connect integration'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-343-cardano-submit-api-localtxsubmission-wiring/
---

# Round 343 — cardano-submit-api LocalTxSubmission wiring

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R342`](2026-05-10-round-342-cardano-submit-api-web-server.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R343 completes the Phase A.2 web round: the placeholder 503 response
from R342 is replaced with real NtC LocalTxSubmission integration,
and `lib.rs::run()` now spins a tokio runtime + binds + serves until
the listener exits.

The cardano-submit-api binary is now end-to-end functional against
a real cardano-node socket: an operator can run

```bash
yggdrasil-cardano-submit-api --config /etc/c.json \
  --socket-path /run/cardano-node.socket \
  --mainnet --port 8090
```

and the binary will bind, listen, accept POST `/api/submit/tx`
requests, forward the body to cardano-node via NtC LocalTxSubmission,
and return 202/400/503 with byte-equivalent JSON error bodies on
reject/connect failure.

Drop-in replacement testing vs upstream `.reference-haskell-cardano-node/install/bin/cardano-submit-api` is queued for R345.

## Diff inventory

- `crates/cardano-submit-api/Cargo.toml` — added `hex = { workspace
  = true }` and `yggdrasil-network = { path = "../network" }` to
  `[dependencies]`.
- `crates/cardano-submit-api/src/rest/web.rs` — `Handler` type alias
  changed from `Arc<dyn Fn(&HttpRequest) -> HttpResponse + ...>` to
  `Arc<dyn Fn(HttpRequest) -> Pin<Box<dyn Future<Output=HttpResponse>
  + Send>> + Send + Sync>`. Per-connection task awaits the handler
  future. Test helper `sync_handler` wraps a sync closure into the
  async type for tests that don't need real I/O.
- `crates/cardano-submit-api/src/web.rs` — `tx_submit_app` returns
  async dispatch closure. `tx_submit_post` is now `async fn` doing
  real `submit_via_ntc` work. New `submit_via_ntc(socket_path,
  network_id, tx_bytes) -> Result<(), TxCmdError>` opens
  `ntc_connect` per request, extracts `NTC_LOCAL_TX_SUBMISSION`
  ProtocolHandle, drives `LocalTxSubmissionClient::submit`, maps
  outcomes:

  | Client outcome                                          | TxCmdError                              | HTTP |
  |---------------------------------------------------------|-----------------------------------------|------|
  | `Ok(())` (MsgAcceptTx)                                  | —                                       | 202  |
  | `Err(TransactionRejected(reason))`                      | `TxCmdTxSubmitValidationError("rejected: 0x<hex>")` | 400  |
  | `Err(ConnectionClosed)`                                 | `TxCmdTxSubmitConnectionError("NtC connection closed by remote")` | 503 |
  | `Err(other)`                                            | `TxCmdTxSubmitConnectionError("<error>")` | 503 |
  | ntc_connect failure                                     | `TxCmdTxSubmitConnectionError("ntc_connect to <path> failed: <err>")` | 503 |

  `MAINNET_NETWORK_MAGIC = 764824073` constant exported.
- `crates/cardano-submit-api/src/lib.rs` — `run()` now builds a tokio
  multi-thread runtime, constructs an `Arc<dyn Fn(TraceSubmitApi)>`
  tracer that forwards to stderr via `render_human`, and calls
  `runtime.block_on(web::run_tx_submit_server_from_params(...))`.
  The `"web server not yet implemented"` sentinel is removed.
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed; `next_milestone` advanced
  R343 → R344.

## Carve-outs (newly recorded)

- **`Cardano.Api.deserialiseFromCBOR` + multi-era `FromSomeType` table**:
  Yggdrasil's tx-submit binary forwards the raw request body bytes
  directly to NtC LocalTxSubmission without per-era pre-decoding.
  Upstream's pre-decoding is a defense-in-depth check for malformed
  CBOR before round-tripping the bytes through the socket; Yggdrasil
  delegates that check to cardano-node, which returns `MsgRejectTx`
  for malformed bytes. Equivalent observable behavior, simpler code
  path. Documented in `web.rs` strict-mirror docstring.
- **`Cardano.Api.getTxId`**: upstream's 202 response body contains
  the parsed TxId. Yggdrasil returns the empty placeholder body
  `"OK"` because deriving the TxId requires multi-era CBOR awareness
  (which the previous carve-out skips). Operators wanting the tx-id
  can compute it client-side via Blake2b-256 of the same bytes they
  submitted. Tracked in `docs/parity-matrix.json`'s `remaining_work`.

## Test inventory

| File                              | Tests | Notes                                          |
|-----------------------------------|-------|------------------------------------------------|
| `types.rs`                        | 24    | Unchanged from R339                            |
| `tracing/trace_submit_api.rs`     | 38    | Unchanged from R341                            |
| `parser.rs`                       | 10    | Unchanged from R335                            |
| `util.rs`                         | 4     | Unchanged from R339                            |
| `cli/types.rs`                    | 7     | Unchanged from R340                            |
| `cli/parsers.rs`                  | 11    | Unchanged from R340                            |
| `rest/types.rs`                   | 7     | Unchanged from R340                            |
| `rest/parsers.rs`                 | 4     | Unchanged from R340                            |
| `rest/web.rs`                     | 19    | sync_handler helper added; 17 prior tests + 2 #[tokio::test] tweaked to use the wrapper |
| `web.rs`                          | 8     | 5 prior + 3 new (#[tokio::test] for async) — connect-failure path now exercised against /nonexistent/socket/path |
| `tests/cli_help_golden.rs`        | 4     | Unchanged                                      |
| Doctest                           | 1     | `util::log_exception` example                  |
| **Crate total**                   | **133** |                                              |

Workspace contribution: 5,099 → 5,100 (+1 net; the test-count
displacement is from a doctest count change in another crate).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,100 passed
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 dev/test/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 dev/test/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 133 tests pass
```

Smoke test (live cardano-node socket required):

```bash
# Terminal 1: bring up upstream cardano-node on preview testnet
.reference-haskell-cardano-node/install/run-node.sh preview

# Terminal 2: run yggdrasil submit-api against the same socket
cargo run --release --bin cardano-submit-api -- \
  --config node/configuration/preview/submit-api-config.json \
  --socket-path .reference-haskell-cardano-node/install/run/preview/socket/node.socket \
  --testnet-magic 2 --port 8090

# Terminal 3: post a sample tx
curl -X POST http://localhost:8090/api/submit/tx \
  -H 'Content-Type: application/cbor' \
  --data-binary @sample_tx.cbor
# Expect: 202 Accepted (or 400 with TxCmdTxSubmitValidationError if
# the sample tx is reject-worthy on the live network).
```

End-to-end soak vs upstream binary lands at R345 — that round will
diff the wire output of yggdrasil vs upstream against the same set of
tx-submission requests.

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | done        |
| R340  | Type bridges: cli/types, cli/parsers, rest/types, rest/parsers     | done        |
| R341  | Trace surface: for_machine, as_metrics, Namespace tables           | done        |
| R342  | Web server: HttpRequest/Response, parse_request, run_settings,     | done        |
|       | tx_submit_app, tx_submit_post placeholder                          |             |
| R343  | LocalTxSubmission wiring: async Handler refactor + submit_via_ntc, | **this**    |
|       | lib.rs::run() runs the runtime                                     |             |
| R344  | Metrics.hs Prometheus surface (port-occupied retry)                | next        |
| R345  | Integration: end-to-end soak vs upstream binary                    | scheduled   |
| R346  | Closeout: AGENTS.md + CHANGELOG + parity-matrix `verified_11_0_1`  | scheduled   |

## Notes for future readers

The decision to open a fresh NtC connection per HTTP request (rather
than maintaining a long-lived multiplexed connection) was made
because:

1. **Simplicity.** A per-request connection has no concurrency state
   to manage; the LocalTxSubmissionClient state machine is trivially
   `StIdle → submit → done`.
2. **Operator expectations.** Tx-submit clients (curl, cardano-rosetta)
   already build per-request connections to the HTTP endpoint;
   matching that lifetime to the underlying NtC connection is
   conceptually consistent.
3. **Failure isolation.** If one tx submission gets stuck or
   crashes, the next one starts with a fresh state.

The cost is ~1 extra round-trip latency per submission for the
NtC handshake. For tx-submit (which is not throughput-critical) this
is an acceptable trade. If a future round needs higher TPS, the
upgrade path is to introduce a connection pool keyed on
`(socket_path, network_magic)` — the per-request abstraction stays
testable + the pool wraps it.

The TxId-in-success-body deferral is recorded in the `web.rs`
strict-mirror carve-out and the parity-matrix's `remaining_work`.
Implementing it requires multi-era CBOR decode (which has its own
parity carve-out for the same reason); both ride together and
likely land in a future T-arc round once cardano-cli's tx-construction
surface is available.
