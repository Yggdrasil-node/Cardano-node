# Guidance for the pure-Rust port of upstream `cardano-tracer`.

**Status:** `partial` (post-R411-R430 arc — trace-forwarder
TraceObject sub-protocol fully wired through Type → Codec →
Acceptor → Run + Configuration + Utils + ForwardSink + Acceptors/{Server, Client, Utils, Run} + supervisor).
The R430 closure marks the structural completion of the
trace-forwarder pipe + per-node Prometheus / EKG-equivalent
endpoints. Remaining gaps are documented carve-outs (EKG
ReqResp sub-protocol, DataPoint sub-protocol, RTView web UI,
trace-forwarder handshake codec, TraceObject CBOR codec, TLS
termination integration) — each surfaced via a `*_status()`
helper for programmatic introspection. Scope band: **LARGE**.

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

## Current functional surface (post-R430)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Concrete supervisor dispatch via `run::run_cardano_tracer`:
  reads tracer-config.json, spawns the Acceptors supervisor on a
  multi-thread tokio runtime (R427).
- ✅ Trace-forwarder TraceObject sub-protocol acceptor + initiator
  drivers (R417-R421) over Unix-pipe transport (R416).
- ✅ Acceptors quartet: `Server`, `Client`, `Utils`, `Run` — full
  per-connection lifecycle (R423-R426).
- ✅ Per-node `MetricsStore` registry + Prometheus + EKG-equivalent
  HTML endpoints (R411-R414).
- ✅ `metrics_help.json` parser (R415).
- ❌ EKG ReqResp sub-protocol — synthesis carve-out (`ekg-forward`
  Hackage package not vendored). See
  `acceptors::server::run_ekg_acceptor_status`.
- ❌ DataPoint sub-protocol — vendored, port deferred to a follow-on
  arc. See `acceptors::server::run_data_points_acceptor_status`.
- ❌ Trace-forwarder handshake codec — defers RemoteSocket TCP path.
  See `acceptors::server::do_listen_to_forwarder_socket_status`.
- ❌ TraceObject CBOR codec — depends on `trace-dispatcher`
  upstream port. R424's stub decoder returns empty list.
- ❌ TLS termination — R408's `load_pem_certs` / `load_pem_key`
  helpers ship; the `axum-server-rustls` integration recipe is in
  `http_server::tls_bind_plan_status` (R429).
- ❌ Logs Rotator — see `run::run_logs_rotator_status`.
- ❌ RTView web UI — synthesis carve-out per the R326-R459 plan.

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
once concrete dispatch lands at `R361+`.

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
- 🟡 Follow-on arcs (post-R430): EKG ReqResp sub-protocol synthesis,
  DataPoint sub-protocol port, trace-forwarder handshake codec,
  TraceObject CBOR codec, Logs Rotator full impl, axum-server TLS
  bind integration. Each follow-on advances a `*_status()`-tracked
  carve-out.

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
