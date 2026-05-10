---
title: 'R398: dependency audit + TracerEnv decision (R398-R410 cardano-tracer arc prep)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-398-dep-audit-tracerenv-decision/
---

# Round 398 — Dependency audit + TracerEnv decision (R398-R410 cardano-tracer arc prep)

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R397`](#) (MetaTrace.hs port — TracerTrace 25-variant enum).
**Plan:** Sister-Tools Pure-Rust Port (R326-R459), Phase A.5 cardano-tracer arc — R398-R410 sub-arc designed by the Plan agent on 2026-05-10.

## Summary

R398 is a documentation-only round preparing the next 12 rounds
(R399-R410) of cardano-tracer subsystem build-out. Three architectural
decisions were identified by the advisor + planning agent as blockers
for ~10 deferred carve-outs across `crates/cardano-tracer/`:

1. **D1 — `lettre` SMTP client** for `Notifications/Email::sendEmail`
   (closes `SmtpSendStatus`, R388).
2. **D2 — HTTP server choice (`axum` over raw-tokio)** for
   `Metrics/{Prometheus, Monitoring, TimeseriesServer, Servers}`
   + the JSON `renderListOfConnectedNodes` exposure (closes
   `RenderHtmlStatus`, `ComputeRoutesStatus`).
3. **D3 — `TraceObject` upgrade strategy (Option A: inline 6-field
   port)** for `Logs/{File, TraceObjects}`.

Plus a sub-decision on the `TracerEnv` 14-field record approach —
adopt option (b): tactical direct-arg pass-through per helper, defer
full record port until `Cardano.Logging` + `Cardano.Timeseries` vendor.

This round runs the dependency-justification work + logs the
TracerEnv decision; it does NOT bump `[workspace.dependencies]`.
Workspace entries land at R403 (lettre), R406 (axum + maud +
rustls-pemfile).

## D1 audit — `lettre` 0.11

**Status:** approved for R403 land; pin to rustls-only feature.

**Recommended Cargo.toml entry (R403):**

```toml
lettre = { version = "0.11", default-features = false, features = ["smtp-transport", "tokio1-rustls", "builder"] }
```

**Why these features:**
- `smtp-transport` — the actual SMTP client. Required.
- `tokio1-rustls` — pure-Rust TLS via `rustls`. **MANDATORY**: avoids
  the default `tokio1-native-tls` feature which would pull in
  `native-tls` → blocked by `deny.toml:90`.
- `builder` — Mail message builder API used by upstream's
  `simpleMail'` constructor.

**License:** MIT/Apache-2.0 dual.

**Transitive surface (estimated from crates.io 2026-05-10):** ~30
transitive deps. Notable additions:
- `rustls` (already on the workspace radar via the deferred axum entry)
- `webpki-roots` (Mozilla CA bundle for TLS validation)
- `hyper-util` (transitive via lettre's tokio integration)
- `base64`, `email-encoding`, `email_address` (mail-format helpers)
- `idna` (international email-address validation)
- `quoted_printable` (MIME encoding)

**deny.toml verification (planned at R403):** `cargo deny check`
confirms no `openssl` / `openssl-sys` / `native-tls` in the resolved
tree. The audit happens at R403 against the actual `Cargo.lock` — not
speculative — per workspace policy.

**Rejected alternatives:**
- Hand-rolled SMTP client: massive scope (RFC 5321 + RFC 3207 STARTTLS
  + SASL AUTH + DSN + headers), security surface, parity risk.
- Skip SMTP entirely: cardano-tracer never matches upstream's email-
  notification surface; operator would have to configure an external
  SMTP relay manually for every deployment.

## D2 audit — `axum` 0.7 (over raw-tokio)

**Status:** approved for R406 land; pin to rustls-only feature.

**Why `axum` and not raw-tokio (the cardano-submit-api precedent):**

| Concern | cardano-submit-api (raw-tokio) | cardano-tracer (axum) |
|---|---|---|
| Endpoint count | 1 (`POST /api/submit/tx`) | 4 servers × multi-route each |
| Method dispatch | none (single POST) | GET + POST per server |
| Content negotiation | none | `Accept: application/json` vs HTML vs `application/openmetrics-text` |
| TLS termination | none | per-server `epForceSSL` + `tlsSettingsChain` (cert + chain + key) |
| Per-node dynamic routing | none | `/<slug>` per connected node |
| Static-asset 404 fallback | none | EKG monitoring page bundles `*.html`/`*.css`/`*.js`/`*.png` |

The TLS per-server + content-negotiation requirements are the
deciding factors. Hand-rolling rustls integration four times (one per
server) means re-implementing an RFC-correct HTTPS handshake ladder
each time. axum + tower expose that as a single config knob via
`axum-server-rustls` (or the equivalent hyper-rustls direct integration).

**Recommended Cargo.toml entries (R406):**

```toml
axum = { version = "0.7", default-features = false, features = ["http1", "tokio", "json"] }
hyper = { version = "1", features = ["http1", "server"] }
tower = { version = "0.5", features = ["util"] }
rustls-pemfile = "2"
```

**License:** all MIT.

**Transitive surface (estimated):**
- `axum` 0.7: brings `hyper` 1.x, `tower` 0.5, `http` 1.x, `bytes`,
  `serde_json` (already dep).
- `rustls-pemfile` 2: zero new transitive deps (depends only on
  `rustls-pki-types` which is already pulled by lettre's
  `tokio1-rustls` feature).
- Combined with lettre's `rustls`, the unique TLS surface is ~10
  crates total.

**deny.toml verification (planned at R406):** Same as D1 — pin
default-features off; verify no native-tls pulls; test against actual
Cargo.lock.

**Rejected alternative:**
- Raw `tokio::net::TcpListener` matching `cardano-submit-api/src/rest/web.rs`:
  rejected because (a) 4-server complexity, (b) per-route HTTPS
  termination, (c) ~hundreds of lines of routing/parsing per endpoint
  vs axum's declarative router.

## D2-prime audit — `maud` 0.27 (HTML templating sub-decision)

**Status:** approved for R406 land alongside axum; alternative
hand-rolled inline renderer kept as fallback.

**Why `maud`:**
- Zero transitive deps (proc-macro only).
- License: MIT.
- Renders the upstream `Text.Blaze.Html`-equivalent
  `renderListOfConnectedNodes` page (~30 lines of HTML).

**Recommended Cargo.toml entry (R406):**

```toml
maud = "0.27"
```

**Rejected alternative:**
- Hand-rolled inline renderer: viable since the HTML page is small
  (≤ 100 LOC), but maud's compile-time template syntax catches typos
  + auto-escapes user content + zero runtime cost. The trade is
  worth the tiny transitive footprint.

**Fallback if maud audit fails:** hand-rolled inline renderer in
`crates/cardano-tracer/src/handlers/metrics/utils.rs::render_list_of_connected_nodes_html`.

## D3 audit — `TraceObject` 6-field inline port (no new deps)

**Status:** approved; ships at R399 with **zero new workspace deps**.

**Sub-decision:** Option A — inline port over Option B (vendor
trace-dispatcher) over Option C (defer entirely).

**Why Option A:**
- Bounded round (R399); 6-field struct.
- Unblocks `Logs/File.hs` (75 lines), `Logs/TraceObjects.hs` (85
  lines), functional `Logs/Journal/Systemd.hs`, AND the trace-forwarder
  mini-protocol acceptors.
- No vendor commitment.

**Why not Option B:**
- Vendoring `trace-dispatcher` is multi-quarter scope (`Cardano.Logging`
  has its own dependency tree: severity ladders, namespace types,
  several already partially mirrored at `crates/cardano-tracer/src/severity.rs`).
- Eliminates the carve-out class permanently, but at far higher cost
  than the immediate port.

**Why not Option C:**
- Blocks too much. `Logs/File.hs` + `Logs/TraceObjects.hs` +
  acceptors all stay deferred indefinitely.

**Field set (at R399):**
- `to_human: Option<String>` — mirror of `toHuman :: Maybe Text`.
- `to_machine: String` — mirror of `toMachine :: Text`.
- `to_severity: SeverityS` — already exported from `crate::severity`.
- `to_namespace: Vec<String>` — mirror of `toNamespace :: [Text]`.
- `to_thread_id: String` — mirror of `toThreadId :: Text`.
- `to_timestamp_ms: i64` — Unix-epoch milliseconds (matches
  `crate::time::get_time_ms` convention; replaces upstream `UTCTime`).

**Migration:** the existing unit-struct placeholder at
`crates/cardano-tracer/src/handlers/logs/journal/no_systemd.rs:45`
becomes a `pub use crate::logging::TraceObject` re-export. Existing
caller (only `write_trace_objects_to_journal` in this file) stays
no-op.

## TracerEnv 14-field record decision

**Decision:** option (b) — **tactical direct-arg pass-through** at
R407 over option (a) full TracerEnv 14-field record port.

**Rationale:**

| Approach | Pros | Cons |
|---|---|---|
| (a) Full TracerEnv port | Mirrors upstream record-syntax 1:1; downstream sites take a single `TracerEnv` arg matching upstream. | Six fields use unported types (`AcceptedMetrics`, `DataPointRequestors`, `Trace IO TracerTrace`, `[TraceObject] -> IO ()`, `TimeseriesHandle`, `Cardano.Logging.MetricsHelp`); each is its own carve-out chain. R393's existing `TracerEnv` struct is the foundation, but extending it now to be the canonical record means committing to the dependency chain. |
| **(b) Direct-arg pass-through** | Each helper takes only the slice of state it actually needs (`Option<&Path>` for state-dir, `Arc<RwLock<BTreeMap<NodeId, NodeName>>>` for connected-node-name lookup, etc.). Bounded per-helper. Mirrors the R383 `Handlers/System.hs` pattern that already shipped. | Helper signatures diverge from upstream's `TracerEnv`-arg convention; downstream porting work needs explicit per-call adaptation. Re-port to (a) when `Cardano.Logging` + `Cardano.Timeseries` vendor. |

**Per the advisor's R407 recommendation:** option (b) is the safer
choice for a bounded R407-R410 metrics-server arc. The full record
port lands when the unported field-types' carve-outs close (multi-
quarter horizon).

## Round-by-round breakdown (R398-R410)

| R | Surface | Dependency | Closes carve-out |
|---|---|---|---|
| **R398** (this round) | Doc-only audit + decisions | none | n/a |
| R399 | TraceObject 6-field inline port | D3 (no deps) | unblocks R400-R401, R411+ |
| R400 | `Logs/File.hs` writeTraceObjectsToFile | D3 + R371 HandleRegistry | partial (needs R402 createOrUpdateEmptyLog) |
| R401 | `Logs/TraceObjects.hs` traceObjectsHandler + deregisterNodeId | D3 + R400 + R383 TracerEnv slice | partial |
| R402 | `Logs/Utils.hs` createEmptyLogRotation + createOrUpdateEmptyLog | R371 HandleRegistry + R396 modify_registry | **closes `LogRotationStatus` (R390)** |
| R403 | lettre wired | D1 | **closes `SmtpSendStatus` (R388)** |
| R404 | makeAndSendNotification orchestration | R403 | partial (R407 closes remaining) |
| R405 | initEventsQueues | R404 | **closes `InitEventsQueuesStatus` (R385)** |
| R406 | axum router skeleton + maud HTML renderer | D2 | partial; **closes `RenderHtmlStatus` (R391)** |
| R407 | TracerEnv direct-arg pass-through (option b) | none | **closes `ComputeRoutesStatus` (R391)** |
| R408 | Metrics/Prometheus.hs | D2 + R407 | partial |
| R409 | Metrics/Monitoring.hs | D2 + R407 | partial |
| R410 | Metrics/{TimeseriesServer + Servers} orchestration | D2 + R407 | partial (TimeseriesHandle remains carved out) |

**Round-count check:** 13 rounds (R398-R410). Original Phase A.5
`cardano-tracer` arc was R360-R385 = 26 rounds. R397 closeout brings
us to R411-R415 absorbing Run.hs supervisor + integration-test
rounds (per the plan's reserved buffer). Phase B.1 db-truncater
moves from R386 to R416, with cumulative end-of-arc target shifted
from R459 to R464 (+5 rounds) — within the operator's ±10-round
risk-buffer per the plan acceptance.

## Risk register additions

| Risk | Surface | Mitigation | Rollback |
|---|---|---|---|
| lettre transitive deps drag in `native-tls` or `openssl-sys`, violating `deny.toml:88-91` | R398 audit / R403 land | Pin `default-features = false` + explicit `tokio1-rustls` feature; `cargo deny check` clean before R398 closes. | Defer D1; keep `SmtpSendStatus` carve-out; document operator workaround (external SMTP relay). |
| axum transitive deps drag in `native-tls` via hyper / hyper-tls | R398 audit / R406 land | Same pattern as D1; pair with `axum-server` + rustls feature OR `hyper-rustls` direct. | Pivot to raw-tokio + manual rustls; ~hundreds of lines per endpoint. |
| TracerEnv 14-field record balloons R407 scope | R407 | Pre-decided option (b) here; full record port deferred. | Stay with R383's per-helper `Option<&Path>` pattern; no closeout deadline. |
| `maud` HTML renderer transitive deps unaudited | R406 sub-decision | Audit alongside lettre + axum; alternative hand-rolled inline renderer (~100 LOC). | Hand-roll inline. |
| `rustls-pemfile` API surface differs from `tlsSettingsChain`'s cert+chain+key shape | R406 | Test cert loading against vendored mainnet `tracer-config.json` paths during R406. | Wrap rustls-pemfile in a thin adapter mirroring upstream Certificate record. |
| `Cardano.Timeseries.Component` not vendored (TimeseriesHandle) | R410 | Keep `TimeseriesHandle` as `crate::environment` placeholder; metrics/timeseries_server.rs ships routing-only with In-Memory placeholder. | Defer R410 timeseries entirely. |
| 5-round overshoot of original R385 closeout target | R398-R410 vs original plan's R386 | Re-number Phase A.5 closeout to R415; bump Phase B.1 db-truncater to R416+. End-of-arc target shifts R459 → R464. | Compress R408-R410 into 2 rounds (Monitoring + TimeseriesServer combined); reduces overshoot to +3. |

## Verification gates

R398 is a documentation-only round; the standard 5 cargo gates
(`fmt`, `check-all`, `test-all`, `lint`, `check-strict-mirror`) +
3 parity validators (`check-parity-matrix`, `check-fixture-manifest`,
`check-reference-artifacts`) all pass unchanged from R397. No code
changes; workspace tests held at 5,676.

## Deliverables

This round adds:

1. This operational-runs entry (you are here).
2. Three new "Sister-tools port arc — R398 dep audit" entries in
   `docs/DEPENDENCIES.md` covering lettre + axum (with maud +
   rustls-pemfile + tower + hyper transitives) + TraceObject
   inline-port decision.
3. CHANGELOG entry under `[Unreleased]` summarizing R398.
4. parity-matrix entry sister-tool.cardano-tracer advanced:
   `next_milestone R398 → R399`.
