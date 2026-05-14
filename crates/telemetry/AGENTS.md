# yggdrasil-telemetry — observability scaffold (Wave 2)

## Scope

Workspace-wide observability primitives:

- `LogFormat` enum (`HaskellJson`, `Pretty`, `Otel`) — selected by
  the binary CLI `--log-format` flag. Default is `HaskellJson` so
  Haskell-cardano-node operators' existing log-shipper configs
  (Promtail / fluentd / vector) work unchanged.
- `TracingConfig` struct — CLI values flow into this and the
  Wave 6 PR 14 `init_subscriber()` function consumes it.
- `trace_fields` constants — single source of truth for span /
  event field names. Renaming `slot` here propagates to every
  emit-site without a code sweep.

## Rules — Non-Negotiable

- **No `tracing` crate dependency in Wave 2.** This crate is the
  scaffold; Wave 6 PR 14 adds `tracing`, `tracing-subscriber`,
  `tracing-opentelemetry`, OTLP, and writes the actual subscriber
  builder.
- **`HaskellJson` is the default.** Operator drop-in compatibility
  is the v1.0 stability promise (see `docs/COMPATIBILITY.md`,
  Wave 10).
- **Field name renames are semver-major.** `slot`, `epoch`,
  `block_hash`, `ns`, `peer` are externally observable via
  Loki / Grafana queries.

## Naming parity

Synthesis crate (no upstream mirror). Upstream's equivalents are
`iohk-monitoring-framework` (`contra-tracer` + EKG + Katip);
Yggdrasil collapses the corresponding Rust-side conventions into
one place. Declared via the `## Naming parity` block in `src/lib.rs`
and allowlisted in `docs/strict-mirror-audit.tsv` — Wave 2 PR 4
scaffold.

## R-arc tracking

Wave 2 PR 4 (scaffold) → Wave 6 PR 14 (`init_subscriber` + workspace
deps) → Wave 6 PR 15 (Haskell-JSON FormatEvent) → Wave 6 PR 17
(cardano-tracer forwarder layer, R502).
