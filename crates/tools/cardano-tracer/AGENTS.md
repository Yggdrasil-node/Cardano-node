# Guidance for the pure-Rust port of upstream `cardano-tracer`.

**Status:** `partial` (post-R474 closeout — trace-forwarder
TraceObject + DataPoint sub-protocols both fully wired through
Type → Codec → Acceptor → Run + Configuration + Utils +
ForwardSink + Acceptors/{Server, Client, Utils, Run} + supervisor).
The R430 closure marked the structural completion of the
trace-forwarder pipe + per-node Prometheus / EKG-equivalent
endpoints. The R452-R459 arc closed the DataPoint **acceptor**
sub-protocol; the R471-R473 arc closed the DataPoint
**forwarder** sub-protocol (R474 added the end-to-end integration
smoke). The R460-R470 follow-on arcs closed the per-connection
mux integration (R460), Logs Rotator IO orchestration (R461-R463),
runMetricsServers aggregator (R464), per-connection HandleRegistry
hooks (R465), supervisor shutdown helpers (R466-R467), TLS
termination via axum-server-rustls (R468), and the cardano-tracer-
side `DataPointRequestor` registry plumbing (R469-R470). Remaining
gaps are documented carve-outs (EKG ReqResp sub-protocol, RTView
web UI, trace-forwarder handshake-over-socket codec, TraceObject
CBOR upstream-byte-equivalence) — each surfaced via a
`*_status()` helper for programmatic introspection. Scope band:
**LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/cardano-tracer/` (93 `.hs` files).

## Mini-arc scope

Standalone trace-forwarder + log + metrics aggregator. Phase A.5 mini-arc R360-R385 (26 rounds, LARGE). RTView web UI carve-out approved (no Rust analog for ThreePenny GUI). R367 adds tracing-appender for log rotation; R371 adds axum for Prometheus metrics endpoint.

## Current functional surface (post-R474)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Concrete supervisor dispatch via `run::run_cardano_tracer`:
  reads tracer-config.json, spawns the Acceptors supervisor on a
  multi-thread tokio runtime (R427).
- ✅ Trace-forwarder TraceObject sub-protocol acceptor + initiator
  drivers (R417-R421) over Unix-pipe transport (R416).
- ✅ **Trace-forwarder DataPoint sub-protocol** acceptor + initiator
  drivers (R452-R457): Type + Codec + Acceptor + Configuration +
  Utils + Run + DataPointRequestor STM coordination primitive.
- ✅ Acceptors quartet: `Server`, `Client`, `Utils`, `Run` — full
  per-connection lifecycle (R423-R426). R458 extended the
  per-connection mux to multiplex HANDSHAKE + TRACE_OBJECTS +
  DATA_POINTS concurrently via `tokio::join!`; both sub-protocols
  share the connection-level brake flag.
- ✅ `acceptors::utils::prepare_data_point_requestor` (R458) ships
  the real per-connection `DataPointRequestor` factory.
- ✅ Per-node `MetricsStore` registry + Prometheus + EKG-equivalent
  HTML endpoints (R411-R414).
- ✅ `metrics_help.json` parser (R415).
- ❌ EKG ReqResp sub-protocol — synthesis carve-out (`ekg-forward`
  Hackage package not vendored). See
  `acceptors::server::run_ekg_acceptor_status`.
- ✅ **DataPoint sub-protocol forwarder side** (R471-R473) —
  `crates/network/src/{data_point_forwarder,data_point_run_forwarder}.rs`
  ships the cardano-node-analog forwarder driver + DataPointStore +
  `forward_data_points_{init,resp}` mux entries. The trace-forward
  2-sided port is structurally complete at the protocol level.
- ❌ Trace-forwarder handshake-over-socket codec — defers
  RemoteSocket TCP path (operator uses Unix-pipe transport).
  See `acceptors::server::do_listen_to_forwarder_socket_status`.
- ❌ TraceObject CBOR upstream-byte-equivalence — current codec
  is a 6-field array synthesis (`crates/tools/cardano-tracer/src/logging.rs`
  R437 carve-out); upstream's `Cardano.Logging.TraceObject` Serialise
  instance lives in the cardano-logging Hackage package which is
  not vendored locally. Operationally: yggdrasil ↔ yggdrasil
  round-trips work; yggdrasil ↔ upstream-cardano-node interop
  would need the upstream Serialise port.
- ✅ **TLS termination** (R468) — `http_server::serve_router_with_tls`
  ships axum-server-rustls integration with R408's
  `load_pem_certs` / `load_pem_key` helpers, defaulting to the
  ring CryptoProvider. Carve-out documented at
  `http_server::tls_bind_plan_status` (R429) closed.
- ✅ **Logs Rotator** (R461-R463) —
  `handlers::logs::rotator::run_logs_rotator` ships the full IO
  orchestration (`runLogsRotator`, `launchRotator`, `checkRootDir`,
  `checkLogs`, `checkIfCurrentLogIsFull`) + file-write soak (R463).
  Wired into `do_run_cardano_tracer` supervisor alongside
  `run_acceptors` via `tokio::spawn` with supervisor-level brake
  flag.
- ✅ **runMetricsServers** (R464) — Prometheus + Monitoring HTTP
  endpoint aggregator wired into the supervisor.
- ✅ **per-connection HandleRegistry** (R465) — deregister hook
  fires on connection close.
- ✅ **DataPointRequestors registry plumbing** (R469-R470) —
  `askNodeName` helper + Acceptors-side spawn-body plumbing.
- ❌ RTView web UI — permanent synthesis carve-out (no Rust
  analog for upstream's ThreePenny GUI / Haskell-specific FRP).

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-cardano-tracer

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh cardano-tracer --help
node/scripts/run-tools.sh cardano-tracer --version

# Or invoke the binary directly:
target/release/cardano-tracer --help
```

The binary is named `cardano-tracer` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
now that the R411-R474 arc closure shipped the supervisor +
trace-forwarder pipe + per-node Prometheus / EKG endpoints + TLS
termination. The TraceObject CBOR upstream-byte-equivalence
caveat: yggdrasil ↔ yggdrasil interop works; yggdrasil ↔
upstream-cardano-node interop would need the upstream
`Cardano.Logging.TraceObject` Serialise instance ported (the
cardano-logging Hackage package is not vendored locally).

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `cardano-tracer` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `node/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Per the R326-R459 plan + R411-R430 arc:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ Phase A.5 mini-arc R360-R385 (initial 26 rounds) — typed
  configuration, runtime-state types, Time + Severity + Notifications,
  Logs / Journal placeholders, Handlers / System path resolution.
- ✅ Phase B (R386-R398) — Notifications subsystem (Email + Send +
  Settings + Timer + Utils) + dep audits (lettre, maud, axum/tower/
  rustls-pemfile).
- ✅ Phase C (R399-R410) — TraceObject 6-field inline port,
  HandleRegistry upgrade, Logs / Utils, Metrics / Utils + Prometheus
  + Monitoring server scaffolding.
- ✅ R411-R430 arc — Phase 1 (R411-R415): EKG-equivalent MetricsStore;
  Phase 2 (R416-R426): trace-forwarder mini-arc + Acceptors leaves;
  Phase 3 (R427-R428): supervisor entry + closure documentation;
  Phase 4 (R429-R430): TLS integration plan + parity-matrix promotion.
- ✅ R452-R459 arc — DataPoint sub-protocol acceptor side: Type +
  Codec (R452-R453), Acceptor driver (R454), Configuration (R455),
  Utils + DataPointRequestor (R456), Run/Acceptor aggregator
  (R457), Acceptors-server.rs + client.rs integration (R458),
  closeout (R459). Closes R423 + R424 deferrals.
- ✅ R460-R474 follow-on arc — DataPoint sub-protocol forwarder
  side (R471-R473) + closing R459's advisor flags: per-connection
  mux integration smoke (R460), Logs Rotator IO orchestration
  (R461-R463), runMetricsServers aggregator (R464), per-connection
  HandleRegistry deregister hook (R465), supervisor shutdown
  helpers + write_to_sink + read_from_sink_non_blocking
  (R466-R467), TLS termination via axum-server-rustls (R468),
  DataPointRequestors registry plumbing into Acceptors spawn body
  (R469-R470), end-to-end DataPoint acceptor+forwarder
  integration test (R474).
- 🟡 Surviving follow-ons (each tracked via a `*_status()`
  helper):
  - EKG ReqResp sub-protocol — synthesis carve-out
    (`ekg-forward` Hackage package not vendored).
  - Trace-forwarder handshake-over-socket codec — RemoteSocket
    TCP path (operator uses Unix-pipe transport in production).
  - TraceObject CBOR upstream-byte-equivalence — current codec
    is a 6-field array synthesis. Upstream's
    `Cardano.Logging.TraceObject` Serialise instance lives in
    the cardano-logging Hackage package not vendored locally.
  - RTView web UI — permanent synthesis carve-out (no Rust
    analog for upstream's ThreePenny GUI / Haskell-specific FRP).

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-cardano-tracer

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/cardano-tracer --help) \
     <(target/debug/cardano-tracer --help)
diff <(.reference-haskell-cardano-node/install/bin/cardano-tracer --version) \
     <(target/debug/cardano-tracer --version)
# (empty diffs expected — byte-equivalent)
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
