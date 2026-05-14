# Compatibility & Stability Contract

> **Status:** drafted for v1.0. Until v1.0 ships, anything in this
> document is provisional. Operators should expect Tier-3
> ("Experimental") surfaces to move; Tier-1 surfaces should stay
> stable from v1.0 onward.

This document declares which Yggdrasil surfaces are externally
observable and what stability promise each carries. The goal is
operator drop-in compatibility with Haskell `cardano-node` 11.0.1:
an SPO running `cardano-node` should be able to swap binaries and
keep their existing systemd unit, Promtail/Loki/fluentd shipper
configs, Grafana dashboards, and Alertmanager rules unchanged.

Authorities consulted by this contract:

- `Cargo.toml::workspace.package.rust-version` — MSRV.
- `docs/parity-matrix.json::reference.tag` — upstream policy tag.
- `crates/node/yggdrasil-node/src/cli.rs` — CLI surface.
- `crates/observability/yggdrasil-tracing/src/lib.rs::trace_fields` —
  log-field source-of-truth (Wave 6 PR 14).
- `crates/observability/yggdrasil-metrics/src/lib.rs::register_all` —
  metric-name source-of-truth (Wave 6 PR 16).

## Tier 1 — Stable from v1.0 (semver-major to change)

Operators can rely on these. A breaking change requires a major-version
bump and a deprecation window (see §"Deprecation policy" below).

### Command-line interface (`yggdrasil-node`)

The following top-level flags are stable from v1.0:

| Flag | Type | Default | Notes |
| --- | --- | --- | --- |
| `--config <path>` | path | (required) | Loads JSON config; key schema below. |
| `--database-path <dir>` | path | from config | ChainDB root directory. |
| `--socket-path <path>` | path | from config | NtC Unix socket. |
| `--port <u16>` | port | from config | NtN TCP port (default `3001`). |
| `--metrics-port <u16>` | port | `12798` | Prometheus scrape port. |
| `--topology <path>` | path | from config | Topology JSON; root + ledger peer roots. |
| `--shelley-kes-key <path>` | path | — | Block-producer KES secret. |
| `--shelley-vrf-key <path>` | path | — | Block-producer VRF secret. |
| `--shelley-operational-certificate <path>` | path | — | Operator opcert. |
| `--byron-signing-key <path>` | path | — | Legacy Byron signing key. |
| `--byron-delegation-certificate <path>` | path | — | Legacy Byron delegation. |
| `--help` / `--version` | flag | — | Standard. |

### Configuration JSON keys

The keys present in the per-network config files at
`crates/node/yggdrasil-node/configuration/{mainnet,preprod,preview}/config.json`
are stable from v1.0. Adding new optional keys with sensible defaults
is *not* a break; removing or renaming an existing key is.

### On-disk database format

The ChainDB layout (`immutable/`, `volatile/`, `ledger/`) and its
major-version format identifier are stable from v1.0. The ledger
snapshot format will get a major-version bump if the on-disk
serialisation changes (currently retained `serde_cbor` for the
block format pending the audit-M-4 migration to `ciborium`).

### Prometheus metric names (EKG-parity subset)

Metric names listed under `register_all()` in
`crates/observability/yggdrasil-metrics/src/lib.rs` that begin with
the `cardano.node.metrics.` prefix mirror the Haskell EKG schema
and are **stable from v1.0**. The top 15 mappings are documented
in `crates/observability/yggdrasil-metrics/AGENTS.md`. Examples:

- `cardano.node.metrics.slotNum.int`
- `cardano.node.metrics.blockNum.int`
- `cardano.node.metrics.density.real`
- `cardano.node.metrics.slotsMissedNum.int`
- `cardano.node.metrics.txsInMempool.int`
- `cardano.node.metrics.connectedPeers.int`
- `cardano.node.metrics.blocksForgedNum.int`
- `cardano.node.metrics.nodeIsLeader.int`

Histogram bucket boundaries are Tier 2 (see below).

### Structured-log JSON schema

The Haskell-JSON log format emitted by default
(`--log-format=haskell-json`, set via `LogFormat::HaskellJson` in
`crates/observability/yggdrasil-tracing/src/lib.rs`) carries these
**non-negotiable** fields:

| Field | Type | Source |
| --- | --- | --- |
| `at` | RFC3339 sub-second | Timestamp; matches Haskell `Katip` `at`. |
| `ns` | array of strings | Namespace, e.g. `["cardano.node.ChainDB"]`. |
| `data` | object | Payload-specific event fields. |
| `sev` | `Debug|Info|Notice|Warning|Error|Critical|Alert|Emergency` | Severity; matches `crates/node/yggdrasil-node/src/trace_forwarder.rs::TraceSeverity::as_str`. |
| `thread` | string | OS thread ID. |

Fields `host` and `app` are emitted when set; their absence is not a
break. Renaming any of the five non-negotiable fields is semver-major.

### `/health` endpoint shape

The `GET /health` endpoint on `--metrics-port` returns a stable JSON
shape from v1.0 onward. The keys `current_slot`, `current_block_number`,
`current_era`, and `tip_hash` are stable; the order of keys is not.
Additional keys may be added in minor releases.

## Tier 2 — Stable from v1.0, extensible

Additions are allowed in minor releases; removals or renames require
a major bump.

- **CLI subcommand surface.** Adding a new subcommand or a new flag
  on an existing subcommand is a minor-version change. Removing or
  renaming an existing subcommand or required flag is major.
- **Prometheus histogram bucket boundaries.** Operators graphing
  histograms against fixed bucket buckets will see existing buckets
  preserved; adding finer buckets is a minor.
- **EKG-parity counter / gauge additions.** New `cardano.node.metrics.*`
  names are a minor; removing or renaming an existing name is major.

## Tier 3 — Experimental (warn-and-break OK in minor with one-release grace)

Operators should treat these as best-effort. They may move between
releases with one release of deprecation warning.

- `--otlp-endpoint <url>` — OpenTelemetry OTLP collector forwarding.
  Schema lives at `crates/observability/yggdrasil-tracing` (Wave 6).
- `--tracer-socket <path>` — `cardano-tracer` Unix-socket forwarder.
  Mux Layer 2/3 is being verified against a live tracer
  (Wave 6 PR 17, R502).
- All `--debug-*` flags.
- The `GET /metrics/json` debug route (Prometheus text remains stable).
- All metric names prefixed `yggdrasil_*` — emitted in parallel with
  the `cardano.node.metrics.*` EKG-parity names until v1.0; **dropped
  at v1.0**.
- The `Pretty` and `Otel` `--log-format` values. The `HaskellJson`
  value is Tier 1.

## Tier 4 — Internal (no guarantee, no deprecation window)

These exist for internal use; they may change in any release without
notice and operators should not depend on them.

- The library crate API surface of every workspace member
  (`yggdrasil-crypto`, `yggdrasil-ledger`, …). Workspace crates are
  `publish = false` and are not consumed as `cargo install`
  dependencies; downstream embedders pin a git SHA and accept the
  trade-off.
- Cargo feature flags. Default features keep the operator-visible
  build byte-identical, but individual features may be renamed or
  consolidated.
- The `target/` directory layout, build profile names beyond the
  documented `release` / `release-debug` / `dist` / `bench`, and any
  files under `tmp/`.
- All AGENTS.md content, R-arc round identifiers, and internal
  validation scripts (`scripts/check-*.py`,
  `scripts/audit-strict-mirror.py`).

## Deprecation policy

When a Tier 1 or Tier 2 surface is deprecated:

1. The release that introduces the deprecation MUST emit a
   `WARN` log on every node startup citing the deprecated
   surface, its replacement, and the planned removal release.
2. The deprecation MUST be documented in `CHANGELOG.md` under the
   release's `## Deprecated` heading.
3. The actual removal MUST wait at least one minor release after
   the deprecation warning shipped, and MUST happen in a major
   release if the surface is Tier 1.

## What is *not* stable

These categories are explicitly outside the stability contract:

- **Performance characteristics.** A minor release may regress
  throughput or latency relative to the previous release; we ship
  bench results in CI (`.github/workflows/bench.yml`, Wave 9 PR 28)
  but do not promise monotonic improvement.
- **Memory layout / RSS.** ChainDB compaction, mempool admission
  budgets, and ledger-snapshot retention windows may evolve between
  releases.
- **Container image layout.** The
  `crates/node/yggdrasil-node/Dockerfile`-built image is
  operator-convenience; build flags, base image, and exposed
  filesystem paths may change.

## Source of truth

This document is the source of truth for stability claims. If any
other doc (a `README.md`, an `AGENTS.md`, an R-arc operational-runs
log) contradicts it, this document wins. Updates to this document
require a focused PR with a clear changelog entry; bundled changes
are explicitly rejected.
