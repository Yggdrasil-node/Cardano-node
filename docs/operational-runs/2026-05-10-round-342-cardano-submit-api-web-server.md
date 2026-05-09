---
title: 'R342: cardano-submit-api web server — raw-tokio HTTP listener + tx_submit_app dispatch'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-342-cardano-submit-api-web-server/
---

# Round 342 — cardano-submit-api web server

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R341`](2026-05-10-round-341-cardano-submit-api-trace-instances.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R342 lands the HTTP server core for cardano-submit-api. Two production
modules graduate from R335 stub-only to working web server:

1. **`rest/web.rs`** — `HttpRequest` + `HttpResponse` types (response
   constructors for 202/400/404/405/413/503), `parse_request` (scans
   Content-Length / Content-Type / Transfer-Encoding with chunked-
   rejection + 32 KiB `MAX_REQUEST_BYTES` cap), `HttpResponse::encode`
   (emits RFC 7230 wire format with `Connection: close`),
   `run_settings` (TCP listener tracing `EndpointListeningOnPort` and
   spawning per-connection handlers).
2. **`web.rs`** — `run_tx_submit_server` outer supervisor mirroring
   upstream `runTxSubmitServer`; `tx_submit_app` dispatch closure
   routing `POST /api/submit/tx` to `tx_submit_post` and emitting
   404/405 for off-path / wrong-method requests; `tx_submit_post`
   placeholder returning 503 with byte-equivalent `TxSubmitFail` JSON
   body for non-empty bodies and 400 `TxSubmitEmpty` for empty bodies.

The raw-tokio TCP approach matches the project's existing
`node/src/metrics_server.rs` pattern; **no axum / hyper / tower /
warp dependency is added.** The `tokio` workspace dep (already present
for the rest of yggdrasil) is added to cardano-submit-api's Cargo.toml.

## Wire-format parity

`HttpResponse::encode` produces RFC 7230 single-shot responses:

```
HTTP/1.1 <status> <status_text>\r\n
Content-Type: <content_type>\r\n
Content-Length: <body.len()>\r\n
Connection: close\r\n
\r\n
<body>
```

`Connection: close` is unconditional — no keep-alive, no pipelining.
This matches the `metrics_server.rs` pattern and is sufficient for
tx-submit clients (curl, cardano-rosetta, etc.).

`tx_submit_post`'s 503 body is byte-equivalent to upstream Aeson:

```json
{"tag":"TxSubmitFail","contents":{"tag":"TxCmdSocketEnvError","contents":{"message":"LocalTxSubmission integration lands at R343"}}}
```

R343 will replace the placeholder `TxCmdSocketEnvError` with real
`TxCmdTxSubmitValidationError` / `TxCmdTxSubmitConnectionError`
results from `crates/network/src/local_tx_submission_client.rs`. The
JSON shape (tag/contents/tag/contents) stays identical.

## Carve-outs

- **`Network.Wai.Handler.Warp.runSettingsSocket` /
  `Data.Streaming.Network.bindPortTCP`**: replaced by
  `tokio::net::TcpListener::bind`. Documented in `rest/web.rs`
  strict-mirror docstring.
- **`Servant.Application`**: replaced by the `Handler` type alias
  (`Arc<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>`).
  `Servant.serve (Proxy :: Proxy TxSubmitApi) (toServant handlers)`
  collapses into the path-prefix dispatch in `tx_submit_app`.
  Documented in `web.rs` strict-mirror docstring.
- **`Cardano.Api.submitTxToNodeLocal`** + multi-era `FromSomeType`
  CBOR decode table: deferred to R343 LocalTxSubmission wiring.
- **Chunked transfer-encoding**: parser rejects with 400 Bad Request
  (`UnsupportedTransferEncoding`); cardano-submit-api clients always
  send `Content-Length`, so this is not a parity defect.

## Diff inventory

- `crates/cardano-submit-api/Cargo.toml` — `tokio = { workspace = true }`
  added to `[dependencies]`; `tokio = { workspace = true, features =
  ["test-util"] }` added to `[dev-dependencies]`.
- `crates/cardano-submit-api/src/rest/web.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/web.rs` — full implementation
  (was: 13-line stub).
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed; `next_milestone` advanced
  R342 → R343.
- `Cargo.lock` — tokio + tokio-macros vendored.

## Test inventory

| Section                                             | New tests | Total |
|-----------------------------------------------------|-----------|-------|
| `rest/web.rs::HttpResponse::encode`                 | 4         |       |
| `rest/web.rs::parse_request`                        | 11        |       |
| `rest/web.rs::run_settings` (#[tokio::test])        | 2         |       |
| `web.rs::resolve_bind_addr`                         | 2         |       |
| `web.rs::tx_submit_post`                            | 2         |       |
| `web.rs::tx_submit_app`                             | 1         |       |
| `web.rs::tx_submit_post_traces_failed_event`        | 1         |       |
| **Round contribution**                              | **+23**   |       |
| Crate total                                         |           | 132   |

Workspace contribution: 5,076 → 5,099 (+23).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,099 passed
cargo lint                                          # clean
python3 scripts/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 scripts/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 scripts/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 132 tests pass
```

Manual smoke test (binds to ephemeral port):

```bash
cargo run --release --bin cardano-submit-api -- \
  --config /tmp/c.json --socket-path /run/cardano-node.socket \
  --mainnet --port 0 --metrics-port 0
# Expect: parser succeeds, validation succeeds, then "web server not
# yet implemented" sentinel error from lib.rs::run() (since R342's
# integration into run() still returns the sentinel).
```

The HTTP listener integration into `lib.rs::run()` (replacing the
sentinel error) lands at R343 alongside LocalTxSubmission wiring.

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | done        |
| R340  | Type bridges: cli/types, cli/parsers, rest/types, rest/parsers     | done        |
| R341  | Trace surface: for_machine, as_metrics, Namespace tables           | done        |
| R342  | Web server: HttpRequest/Response, parse_request, run_settings,     | **this**    |
|       | tx_submit_app, tx_submit_post placeholder                          |             |
| R343  | LocalTxSubmission wiring + multi-era CBOR decode + lib.rs::run()   | next        |
| R344  | Metrics.hs Prometheus surface (port-occupied retry)                | scheduled   |
| R345  | Integration: end-to-end soak vs upstream binary                    | scheduled   |
| R346  | Closeout: AGENTS.md + CHANGELOG + parity-matrix `verified_11_0_1`  | scheduled   |

## Notes for future readers

The decision to use raw tokio TCP rather than `axum` was driven by
three factors:

1. **Project consistency.** `node/src/metrics_server.rs` already
   uses raw tokio TCP for the in-process Prometheus endpoint; using
   the same pattern here keeps the operational story uniform.
2. **Dep budget.** axum brings ~5 transitive deps (hyper, tower, http,
   http-body, mime). For a single-endpoint HTTP server, that's a high
   surface-area cost.
3. **Path-routing simplicity.** cardano-submit-api has exactly one
   route (`POST /api/submit/tx`); the dispatch closure in
   `tx_submit_app` is 8 lines. axum's router would be 3 lines but
   imports a far larger framework.

If a future round needs richer routing / middleware (e.g. for
cardano-tracer's RTView web UI), this decision is reversible — the
`Handler` type alias is intentionally simple and could be replaced
with `axum::Router` without breaking callers.

`MAX_REQUEST_BYTES = 32 KiB` was chosen because:
- Cardano max tx size: ~16 KiB (Conway era; smaller in earlier eras).
- Headers budget: ~2 KiB conservative.
- 2× multiplier for safety margin.

If a future round needs to handle bigger payloads (e.g. multi-tx
batching or witness sets), bump the constant; the parser's TooLarge
path is already tested.
