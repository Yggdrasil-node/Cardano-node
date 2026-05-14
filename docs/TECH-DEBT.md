# Technical debt tracker

Operator-visible items that work today but want consolidation. Each
entry names the owning subsystem, the current state, the desired
end state, and the rough scope of the consolidation PR.

## EKG-parity metrics: field-mapping consolidated (formerly dual paths)

**Owner:** observability (Wave 6 PR 16 follow-on consolidation)

**State today.** The 110-line field-mapping table now lives in ONE
place — `yggdrasil_metrics::render_ekg_parity_prometheus_text` —
via the `yggdrasil_metrics::EkgParitySource` trait. The binary's
`MetricsSnapshot::to_ekg_parity_prometheus_text` is a one-line
delegation to that function plus an `impl EkgParitySource for
MetricsSnapshot` block (ten 1:1 field accessors). The
`/metrics` route's concatenation behaviour is unchanged from the
operator perspective; only the rendering ownership moved.

**Remaining secondary debt.** `yggdrasil_metrics::install_prometheus_exporter`
still spawns its own HTTP listener (originally meant as a parallel
Prometheus scrape endpoint). It currently has NO live consumer in
the binary — `serve_metrics` owns the port. Sister tools (cardano-
tracer, future cardano-submit-api) that want their own scrape
endpoint can call `install_prometheus_exporter`; the binary
continues to use the `render_ekg_parity_prometheus_text` path so
both legacy `yggdrasil_*` and EKG names appear on the same port.

**Desired end state for the secondary item.** The binary's
`NodeMetrics` update sites call `metrics::*` macros so values flow
through the global `metrics` facade. `install_prometheus_exporter`
no longer binds its own port (operator chooses the port via
`--metrics-port`). `to_ekg_parity_prometheus_text` becomes a thin
adapter over `handle.render()`. Histogram-shaped metrics
(`blockProcessingTime`) become real Prometheus histograms.

**Scope of the remaining item.** ~30 `NodeMetrics` update sites in
`yggdrasil_node_tracer` either bridged via a tick-loop (cheaper)
or rewritten as direct `metrics::*` calls (cleaner). Best done
once a sister tool actually consumes the `install_prometheus_exporter`
surface so the integration has a real driver.

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
