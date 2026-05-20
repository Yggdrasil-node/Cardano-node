# Guidance for the pure-Rust port of upstream `cardano-submit-api`.

**Status:** `partial` (post-R344 functional binary with metrics; integration soak + closeout remain — operator-time gates). Scope band: **MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/cardano-submit-api/` (14 `.hs` files).

## Mini-arc rounds (R335-R346)

| Round | Scope                                                                                                  | Status |
|-------|--------------------------------------------------------------------------------------------------------|--------|
| R335  | Skeleton: 14-file mirror tree + clap-bypass parser with byte-equivalent --help/--version golden tests   | done   |
| R339  | Foundations: `types.rs` (TxSubmitPort, RawCborDecodeError, EnvSocketError, TxCmdError, TxSubmitWebApiError), `util.rs` (log_exception), `tracing/trace_submit_api.rs` data-only enum + render_human | done   |
| R340  | Type bridges: `cli/types.rs` (ConfigFile, GenesisFile, SocketPath, ConsensusModeParams, NetworkId, TxSubmitNodeParams, TxSubmitCommand) + `cli/parsers.rs` (into_command) + `rest/types.rs` (WebserverConfig + to_socket_addr) + `rest/parsers.rs` (from_args) | done   |
| R341  | Trace surface complete: TraceSubmitApi::for_machine / as_metrics / namespace_for; Namespace enum with segments / severity / metrics_doc; ALL_NAMESPACES const; MetricUpdate enum; Severity enum | done   |
| R342  | Web server: HttpRequest/HttpResponse + parse_request + run_settings (raw tokio TCP); web.rs supervisor scaffolding with placeholder 503 | done   |
| R343  | LocalTxSubmission wiring: async Handler refactor; submit_via_ntc opens ntc_connect per request, drives LocalTxSubmissionClient::submit, maps outcomes to TxCmdError; lib.rs::run() spins tokio runtime + serves the listener | done   |
| R344  | Metrics.hs Prometheus surface: `metrics.rs` ships `MetricsRegistry` (lock-free `AtomicU64` `tx_submit` + `tx_submit_fail` counters with `register_metrics_server` doing port-occupied retry up to `MAX_PORT_OFFSET=1000` adjacent ports), `web.rs::run_tx_submit_server_from_params` spawns `register_metrics_server` on `params.metrics_port` + wraps the operator tracer via `make_metrics_aware_tracer` so `TraceSubmitApi::ApplicationTxSubmitPostResult` events increment counters. 13 metrics tests pin the path. | done |
| R345  | Integration soak: drop-in replacement vs upstream binary at .reference-haskell-cardano-node/install/bin/cardano-submit-api; diff response wire output for matching tx-submission requests | scheduled (operator-time) |
| R346  | Closeout: parity-matrix promotion `partial → verified_11_0_1`; AGENTS.md operational guide finalized; CHANGELOG closeout entry | scheduled (gated on R345) |

## Current functional surface (post-R343)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ argv → `TxSubmitCommand` validation (mandatory flags
  `--config`, `--socket-path`, `--mainnet|--testnet-magic` enforced).
- ✅ HTTP listener binds + accepts on the configured `--listen-address`
  + `--port` (default `127.0.0.1:8090`).
- ✅ `EndpointListeningOnPort` traced to stderr on bind.
- ✅ `POST /api/submit/tx` handler decodes the request body, opens an
  NtC `LocalTxSubmission` connection to the configured socket path,
  forwards the body bytes, returns:
  - `202 Accepted` `"OK"` on `MsgAcceptTx`
  - `400 Bad Request` `{"tag":"TxSubmitFail","contents":{"tag":"TxCmdTxSubmitValidationError","contents":"rejected: 0x<hex>"}}` on `MsgRejectTx`
  - `503 Service Unavailable` `{"tag":"TxSubmitFail","contents":{"tag":"TxCmdTxSubmitConnectionError","contents":"<err>"}}` on connect/protocol failure
- ✅ `EndpointSubmittedTransaction` / `EndpointFailedToSubmitTransaction`
  traced per outcome.
- ✅ Off-path requests → 404; non-POST on `/api/submit/tx` → 405.
- ✅ Request size cap 32 KiB (`MAX_REQUEST_BYTES`); chunked
  transfer-encoding rejected with 400; oversized bodies → 413.
- ✅ `/metrics` Prometheus endpoint (R344) — `register_metrics_server`
  binds the configured `--metrics-port`, serves `text/plain; charset=utf-8`
  exposition format with `tx_submit` + `tx_submit_fail` counters.
- ✅ Port-occupied retry (R344) — tries `starting_port..starting_port+1000`;
  on exhaustion traces `TraceSubmitApi::MetricsServerPortNotBound`
  and returns cleanly (mirrors upstream's "disable endpoint" semantic).
- ❌ TxId in 202 success body — deferred (depends on multi-era CBOR
  decode; tracked in parity-matrix `remaining_work`).
- ❌ End-to-end soak vs upstream binary — lands at R345.

## Build + run

```bash
# Build.
cargo build --release -p yggdrasil-cardano-submit-api

# Run via the universal launcher.
scripts/run-tools.sh cardano-submit-api --help
scripts/run-tools.sh cardano-submit-api --version

# Live test against the upstream cardano-node socket on preview testnet
# after starting `.reference-haskell-cardano-node/install/run-node.sh preview`.
export CARDANO_NODE_SOCKET_PATH=/path/to/live/upstream/node.socket
target/release/cardano-submit-api \
  --config configuration/preview/submit-api-config.json \
  --socket-path "$CARDANO_NODE_SOCKET_PATH" \
  --testnet-magic 2 --port 8090
# In another terminal:
curl -X POST http://127.0.0.1:8090/api/submit/tx \
  -H 'Content-Type: application/cbor' --data-binary @sample_tx.cbor
```

## Architecture

```
lib.rs::run()
├── parser::parse_args(argv)              -> Args
├── cli::parsers::into_command(&args)     -> TxSubmitCommand
└── tokio runtime → web::run_tx_submit_server_from_params(tracer, params)
    ├── webserver_config.to_socket_addr() -> std::net::SocketAddr
    ├── tx_submit_app(tracer, protocol, network_id, socket_path) -> Handler (async)
    └── rest::web::run_settings(tracer, addr, handler)
        ├── tokio::net::TcpListener::bind(addr)
        ├── trace EndpointListeningOnPort
        └── per-connection: parse_request → handler.await → response.encode
            └── tx_submit_post(...)
                ├── empty body → TxSubmitEmpty
                └── submit_via_ntc(socket_path, network_id, body)
                    ├── ntc_connect(socket_path, network_magic, false)
                    ├── extract NTC_LOCAL_TX_SUBMISSION ProtocolHandle
                    └── LocalTxSubmissionClient::submit(body)
```

Key types:

- `Handler = Arc<dyn Fn(HttpRequest) -> Pin<Box<dyn Future<Output = HttpResponse> + Send>> + Send + Sync>`
- `Tracer = Arc<dyn Fn(TraceSubmitApi) + Send + Sync>`

## Carve-outs (NOT ported, by design)

- `TxSubmitApi` / `TxSubmitApiRecord` / `CBORStream` Servant types →
  raw-tokio HTTP routing in `tx_submit_app`.
- `Cardano.CLI.Environment.EnvCli` → Yggdrasil parser is environment-blind.
- `LogFormatting` / `MetaTrace` typeclasses → inherent methods on
  TraceSubmitApi + the Namespace enum.
- `Network.Wai.Handler.Warp.runSettingsSocket` / `bindPortTCP` →
  `tokio::net::TcpListener::bind` + raw HTTP/1.1 in `rest::web::run_settings`.
- `Servant.Application` → `Handler` type alias + path-prefix dispatch.
- `Cardano.Api.deserialiseFromCBOR` + multi-era `FromSomeType` table
  (`AsTx AsShelleyEra` / ... / `AsTx AsConwayEra`) → raw bytes pass
  through to NtC LocalTxSubmission (cardano-node returns MsgRejectTx
  for malformed bytes; equivalent observable behavior).
- `Cardano.Api.getTxId` (returning the TxId on accept) → empty `"OK"`
  response body; operators compute Blake2b-256 client-side. Future
  enhancement riding on multi-era CBOR support.

## Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `cardano-submit-api` is
  the acceptance gate for the closeout round.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `crates/node/config/src/upstream_pins.rs` as a coordinated round.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-cardano-submit-api  # 133 tests pass at R343

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/cardano-submit-api --help) \
     <(target/release/cardano-submit-api --help)
diff <(.reference-haskell-cardano-node/install/bin/cardano-submit-api --version) \
     <(target/release/cardano-submit-api --version)
# (empty diffs expected — byte-equivalent)

# 4. Live tx-submission diff against upstream binary (R345 round-doc procedure):
#    Bring up upstream cardano-node + upstream cardano-submit-api on port 8090;
#    bring up yggdrasil-cardano-submit-api on port 8091;
#    POST the same sample tx to both endpoints and diff the responses.
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration round numbers in sync with the
  authoritative plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
