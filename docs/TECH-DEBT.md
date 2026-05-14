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

**State today.** Layer 1 (`TraceObject` CBOR codec) is fully
implemented and unit-tested against pinned upstream-shape wire
bytes. Phase 2.B of Wave 6 PR 17 added the codec halves of
Layers 2 and 3:

- `crates/node/tracer/src/trace_forwarder/mux.rs` — `Network.Mux`
  SDU header codec (encode + decode of the 8-byte big-endian
  timestamp + dir-and-protonum + length header). Mini-protocol
  number constants for Handshake (0), EKG (1), TraceObject (2),
  DataPoint (3) per upstream `Cardano.Tracer.Acceptors.*`.
- `crates/node/tracer/src/trace_forwarder/mini_protocol.rs` —
  `Trace.Forward.Protocol.TraceObject` CBOR codec for
  `MsgTraceObjectsRequest` / `MsgDone` / `MsgTraceObjectsReply`.
  Round-trip pinned against the upstream wire shape; `Request`
  and `Done` are byte-exact, `Reply` encodes the prefix
  byte-exactly and concatenates each `TraceObject`'s Layer 1
  CBOR.

What's still missing:

- The Mux state-machine driver (ingress queue, egress queue,
  per-mini-protocol scheduler, handshake driver, bearer-task
  lifecycle).
- ~~An `AF_UNIX SOCK_STREAM` bearer adapter.~~ **Landed in commit
  `ee7d496`** — `crates/node/tracer/src/trace_forwarder/bearer.rs`
  ships `Bearer<S>` generic over any `tokio::io::AsyncRead +
  AsyncWrite + Unpin + Send` transport, with `read_sdu` /
  `write_sdu` + 4 round-trip tests pinned against
  `tokio::io::DuplexStream` in-memory pipes.
- The cardano-tracer-specific Handshake mini-protocol negotiator
  (mini-protocol num 0).
- A `Layer<S>` adapter for `tracing-subscriber` that walks every
  `tracing::Event` into a `TraceObject` and emits it through the
  Mux stack to a configurable Unix socket.
- ~~TraceObject Layer 1 **decoder** (today only the encoder ships;
  `mini_protocol.rs` errors on a non-empty inbound `Reply` until
  the decoder lands).~~ **Landed in commit `f0bc5a9`** —
  `TraceObject::from_cbor_bytes` is now the inverse of `to_cbor`;
  `mini_protocol::decode_message` walks non-empty replies via
  `Decoder::raw_value()` + the Layer-1 decoder; full round-trip
  test pinned in `mini_protocol_tests::nonempty_reply_round_trip`.
- Conformance test against the vendored
  `.reference-haskell-cardano-node/install/bin/cardano-tracer`
  binary — needs `scripts/setup-reference.sh` without
  `--sources-only` so the install tarball materialises.

**Desired end state.** A `Layer<S>` for `tracing-subscriber` that
forwards every `tracing::Event` over the cardano-tracer Unix
socket. SPOs who run a sibling `cardano-tracer` process get
drop-in trace forwarding.

**Scope.** Multi-day. Bearer adapter, state-machine driver,
Handshake negotiator, decoder side of TraceObject, conformance
verification against a live `cardano-tracer` binary.

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

## cardano-submit-api validation error: structured mapping (Phase 1 — raw-bytes carrier landed)

**Owner:** sister-tools (Wave 6 PR 18 / cardano-submit-api web round, Phase 4.A)

**State today.** `TxCmdError::TxCmdTxSubmitValidationError` now carries
a `TxSubmitValidationError` struct (in
`crates/tools/cardano-submit-api/src/types.rs`) holding BOTH the raw
CBOR-encoded era-specific `ApplyTxError` payload AND a string
rendering. The custom `Serialize` impl on `TxSubmitValidationError`
emits only the rendered string so the upstream JSON wire shape
(`{"tag":"...","contents":"<rendered>"}`) stays byte-equivalent.
The `LocalTxSubmissionClientError::TransactionRejected(reason)` path
in `web.rs` now plumbs the raw reject bytes through.

**Remaining work** (Phase 2 — structured-enum decoder, deferred):
the rendered string is still a hex-dump of the reject bytes; the
structured `ApplyTxError` enum mirroring upstream's per-era variant
sum (`FeeTooSmall`, `ValueNotConservedUTxO`, `OutsideValidityInterval`,
`BadInputsUTxO`, `OutputTooSmall`, `WrongNetwork`, …; multiplied by
6 eras Shelley→Conway) is not yet built. Once it lands, the
renderer becomes a `Display` impl on the structured form and
operators can pattern-match on individual rejection variants
without a CBOR re-walk.

**Scope of Phase 2.** ~400 lines of new enum (per-era ApplyTxError
sum + UtxoErr/UtxowErr/DelegsErr sub-sums + Conway governance
errors), the per-era CBOR decoders that map the wire shape to
the typed enum, and the `Display` impl that re-renders. Best
landed when a sister tool actually needs to pattern-match (e.g.,
tx-generator surfacing typed rejection reasons in its 1-hour TPS
soak summary).

**Tracking.** Also referenced from `docs/parity-matrix.json` under
the cardano-submit-api entry's `next_milestone` field; the doc-
comment in `types.rs` points here.

## Tracking conventions

- New tech-debt entries follow this header structure: **Owner**,
  **State today**, **Desired end state**, **Scope**.
- An entry is closed (deleted from this file) when the
  consolidation PR lands.
- Issues that are blocking ship — broken tests, security bugs,
  parity-matrix violations — DO NOT belong here; they go to
  Issues / SECURITY.md / the strict-mirror gate.
