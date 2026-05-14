# Technical debt tracker

Operator-visible items that work today but want consolidation. Each
entry names the owning subsystem, the current state, the desired
end state, and the rough scope of the consolidation PR.

## EKG-parity metrics: two emission paths

**Owner:** observability (Wave 6 PR 16 + follow-on)

**State today.** Two code paths emit the 15 EKG-parity metric names
declared at `crates/observability/yggdrasil-metrics/src/lib.rs`:

1. **`yggdrasil_metrics::install_prometheus_exporter`** —
   `PrometheusBuilder::new().with_http_listener` plus
   `metrics::gauge!`/`counter!`/`histogram!` pre-registrations against
   each canonical name. Standalone HTTP listener on the configured
   port (default 12798). **No live consumer in the binary today.**
2. **`MetricsSnapshot::to_ekg_parity_prometheus_text`** — appended to
   the legacy `yggdrasil_*` Prometheus text inside the binary's
   `metrics_server.rs` route handler. **This is the path operators
   actually see at `GET /metrics`.**

Both paths render the same 15 names from the same `MetricsSnapshot`
values today; drift risk is in the field-mapping table, not the
underlying counters.

**Desired end state.** A single owner — `yggdrasil_metrics` — drives
the registry. The binary's `serve_metrics` calls into
`yggdrasil_metrics::handle.render()` for the EKG-parity block and
concatenates with the legacy `yggdrasil_*` text. `to_ekg_parity_prometheus_text`
gets retired. `NodeMetrics` update sites optionally call `metrics::*`
gauges so the values are emitted *through* the facade rather than
mirrored on every scrape.

**Scope.** ~30 NodeMetrics update sites in `yggdrasil_node_tracer`
either bridged via a tick-loop (cheaper) or rewritten as direct
`metrics::*` calls (cleaner). The HTTP listener in
`install_prometheus_exporter` either gets disabled (binary owns the
port) or moved to a sibling port `12799` (operators scrape both).
Removing `to_ekg_parity_prometheus_text` removes ~110 lines of
field-mapping logic.

## cardano-tracer Mux Layer 2/3 (R502)

**Owner:** observability (Wave 6 PR 17)

**State today.** Layer 1 (the `TraceObject` CBOR codec at
`crates/node/tracer/src/trace_forwarder.rs`) is fully implemented
and unit-tested against pinned upstream-shape wire bytes. Layers
2/3 — the `Trace.Forward.Protocol.TraceObject` mini-protocol
state machine and the `Network.Mux` SDU framing + handshake — are
documented but unimplemented; the `## Layered design` block in
`trace_forwarder.rs` explains the gap.

**Desired end state.** A `Layer<S>` for `tracing-subscriber` that
forwards every `tracing::Event` over the cardano-tracer Unix
socket. Wave 6 PR 17 adds the `tracing-opentelemetry` workspace
dep plus the Mux 2/3 protocol implementation. SPOs who run a
sibling `cardano-tracer` process get drop-in trace forwarding.

**Scope.** Multi-day. Conformance verification against a live
`cardano-tracer` binary, Mux SDU framing, handshake protocol
state machine, integration test against the vendored upstream
binary.

## Wave 3 / Wave 5 feature flags: declared but not gating

**Owner:** packaging (Wave 3 PR 5, Wave 5 PR 7+)

**State today.** Feature flags are declared on every workspace
crate (`yggdrasil-ledger/plutus`, `yggdrasil-consensus/experimental-genesis`,
`yggdrasil-network/{ntn, ntc, serde-traces}`, `yggdrasil-storage/{lmdb, mem-only}`,
`yggdrasil-plutus/{secp256k1, bls12-381}`, `yggdrasil-node-block-producer/forge`,
binary's `yggdrasil-node/{forge, plutus, ntc-socket, tracer-forwarder}`)
but no Rust code uses `#[cfg(feature = "...")]` to gate anything yet.
The flags are documentation in Cargo.toml form.

**Desired end state.** Each flag actually conditionally compiles the
code paths it names. Relay-only builds (`forge` off) compile out
the block-producer crate entirely. Slim builds (`plutus` off)
compile out the Plutus evaluator. WASM-stub builds become buildable.

**Scope.** Per-flag PRs. `forge` is the cleanest first target since
the block-producer crate is already its own crate boundary.

## yggdrasil-cardano-cli library-only crate has no `[[bin]]`

**Owner:** packaging (sister-tools layout)

**State today.** Every other sister tool under `crates/tools/`
ships a `[[bin]] name = "<upstream-name>"` so `cargo install
--path crates/tools/<tool>` produces an operator-named binary.
`crates/tools/cardano-cli/` is library-only — the binary surface
is hosted by `yggdrasil-node`'s `cardano-cli` subcommand
(C-arc partial port).

**Desired end state.** Once the C-arc port reaches `CLI-MVS`
verified parity, a standalone `[[bin]] name = "cardano-cli"`
ships from `crates/tools/cardano-cli/` so operators can install
just the CLI without the runtime binary.

**Scope.** Gated on C-arc Phase F + R298+ migration roadmap;
not actionable as a standalone PR until the C-arc lands.

## Tracking conventions

- New tech-debt entries follow this header structure: **Owner**,
  **State today**, **Desired end state**, **Scope**.
- An entry is closed (deleted from this file) when the
  consolidation PR lands.
- Issues that are blocking ship — broken tests, security bugs,
  parity-matrix violations — DO NOT belong here; they go to
  Issues / SECURITY.md / the strict-mirror gate.
