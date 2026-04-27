---
title: Monitoring
layout: default
parent: User Manual
nav_order: 7
---

# Monitoring

A production node needs three observability surfaces: metrics for dashboards and alerting, structured traces for debugging, and a health endpoint for orchestrator probes.

## Prometheus metrics

Enable with `--metrics-port`:

```bash
$ yggdrasil-node run ... --metrics-port 12798
```

The node binds an HTTP server on `127.0.0.1:12798` exposing:

- **`GET /metrics`** — Prometheus text exposition format.
- **`GET /metrics/json`** — JSON snapshot of the same counters.
- **`GET /health`** — JSON liveness probe (`status`, `uptime_seconds`, `blocks_synced`, `current_slot`).
- **`GET /debug`**, **`GET /debug/metrics`**, **`GET /debug/metrics/prometheus`**, **`GET /debug/health`** — upstream-style aliases.

Bind is intentionally to `127.0.0.1` only. To scrape from a remote Prometheus, run a reverse proxy (nginx, Caddy) or use SSH tunnelling.

### Counters and gauges

Yggdrasil emits 35+ counters and gauges. Selected highlights:

#### Sync

| Metric                                    | Type    | Description |
|-------------------------------------------|---------|-------------|
| `yggdrasil_blocks_synced`                 | counter | Total blocks applied during this process lifetime. |
| `yggdrasil_current_block_number`          | gauge   | Latest block number applied. |
| `yggdrasil_current_slot`                  | gauge   | Slot of the latest applied block. |
| `yggdrasil_checkpoint_slot`               | gauge   | Slot of the most recent ledger checkpoint. |
| `yggdrasil_rollbacks`                     | counter | RollBackward events received. |
| `yggdrasil_stable_blocks_promoted`        | counter | Volatile → immutable promotions. |
| `yggdrasil_reconnects`                    | counter | Sync session reconnects. |
| `yggdrasil_batches_completed`             | counter | Verified batches applied. |

#### Mempool

| Metric                                  | Type    | Description |
|-----------------------------------------|---------|-------------|
| `yggdrasil_mempool_tx_count`            | gauge   | Current transactions in the mempool. |
| `yggdrasil_mempool_bytes`               | gauge   | Current mempool byte total. |
| `yggdrasil_mempool_tx_added`            | counter | Successfully admitted transactions. |
| `yggdrasil_mempool_tx_rejected`         | counter | Rejected transactions. |

#### Connection manager

| Metric                                      | Type  | Description |
|---------------------------------------------|-------|-------------|
| `yggdrasil_cm_full_duplex_conns`            | gauge | Full-duplex peer count. |
| `yggdrasil_cm_duplex_conns`                 | gauge | Duplex (uni- + bi-directional) peer count. |
| `yggdrasil_cm_unidirectional_conns`         | gauge | One-way peer count. |
| `yggdrasil_cm_inbound_conns`                | gauge | Currently accepted inbound. |
| `yggdrasil_cm_outbound_conns`               | gauge | Currently established outbound. |
| `yggdrasil_inbound_connections_accepted`| counter | Cumulative inbound accept count. |
| `yggdrasil_inbound_connections_rejected`| counter | Inbound connections rejected by rate limit. |

#### BlockFetch workers (Phase 6)

| Metric                                          | Type    | Description |
|-------------------------------------------------|---------|-------------|
| `yggdrasil_blockfetch_workers_registered`       | gauge   | Per-peer fetch workers currently active. |
| `yggdrasil_blockfetch_workers_migrated_total`   | counter | Cumulative warm-to-hot migrations into the worker pool. |

A healthy multi-peer setup has `registered` ≈ number of hot peers (usually 2 with `max_concurrent_block_fetch_peers = 2`) and `migrated_total` strictly increasing across the run.

#### Process

| Metric                | Type  | Description |
|-----------------------|-------|-------------|
| `yggdrasil_uptime_seconds`| gauge | Process uptime. |

### Sample Prometheus scrape config

```yaml
- job_name: yggdrasil
  scrape_interval: 15s
  static_configs:
    - targets: ['127.0.0.1:12798']
      labels:
        node_role: relay
        network: mainnet
```

### Grafana

Grafana dashboards built for upstream `cardano-node` will work against Yggdrasil with one substitution: replace the `cardano_node_metrics_*` prefix with `yggdrasil_`. The metric semantics are aligned where the upstream metric exists, with a couple of names differing where Yggdrasil added new instrumentation (e.g. the `blockfetch_workers_*` family is Yggdrasil-specific).

## Structured tracing

The trace dispatcher writes namespace-scoped events. The namespace is dotted, e.g. `Net.BlockFetch.Worker`, `ChainDB.AddBlockEvent.AddedBlock`, `Mempool.Eviction`, `Node.BlockProduction`. Per-namespace settings control:

- **Severity threshold** — `Debug`, `Info`, `Notice`, `Warning`, `Error`, `Critical`.
- **Backends** — list of destinations (`Stdout MachineFormat`, `Forwarder`, etc.).
- **`maxFrequency`** — Hz cap on emission per namespace.
- **`detail`** — `DMinimal`, `DNormal`, `DDetailed`, `DMaximum`.

Configure in `config.json`:

```jsonc
{
  "TraceOptions": {
    "": {
      "severity": "Notice",
      "backends": ["Stdout HumanFormatColoured"]
    },
    "ChainDB": {
      "severity": "Info",
      "detail": "DDetailed"
    },
    "Net.BlockFetch": {
      "severity": "Info",
      "maxFrequency": 5.0
    },
    "Node.Recovery.Checkpoint": {
      "severity": "Info",
      "maxFrequency": 1.0
    }
  }
}
```

The empty-string key is the root default. Longest-prefix wins.

### Forwarder backend

For aggregation across many nodes, configure the Forwarder backend with a Unix socket destination. The wire format is CBOR-encoded trace events compatible with upstream `cardano-tracer`, so you can plug a single tracer in front of Haskell and Yggdrasil nodes interchangeably.

```jsonc
{
  "TraceOptions": {
    "": {
      "backends": ["Forwarder"]
    }
  },
  "TraceOptionForwarder": {
    "address": {
      "filePath": "/var/run/cardano-tracer.sock"
    },
    "mode": "Initiator"
  }
}
```

## Health endpoint

```bash
$ curl -s http://127.0.0.1:12798/health
{"status":"ok","uptime_seconds":86412,"blocks_synced":523109,"current_slot":117425831}
```

Use this for Kubernetes liveness probes, load-balancer health checks, etc.

A Kubernetes example:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 12798
  initialDelaySeconds: 30
  periodSeconds: 30
readinessProbe:
  httpGet:
    path: /health
    port: 12798
  initialDelaySeconds: 60
  periodSeconds: 15
```

## Suggested alerts

A starting alerting baseline:

| Alert                          | Expression                                      | Severity |
|--------------------------------|------------------------------------------------|----------|
| Node down                      | `up{job="yggdrasil"} == 0`                     | critical |
| Slot lag > 600                 | `(time() - yggdrasil_current_slot * 1) > 600`  | warning  |
| Slot lag > 3600                | as above, threshold 3600                       | critical |
| Frequent reconnects            | `rate(yggdrasil_reconnects[5m]) > 1`           | warning  |
| Excessive rollbacks            | `rate(yggdrasil_rollbacks[10m]) > 0.1`         | warning  |
| Stuck migration                | `yggdrasil_blockfetch_workers_registered < hot_peer_count` | warning |
| Mempool growing unbounded      | `yggdrasil_mempool_bytes > 10485760`           | warning  |
| Inbound rate-limit hits        | `rate(yggdrasil_inbound_connections_rejected[5m]) > 0.5` | info |

Adjust thresholds based on your network and traffic profile.

## What "synced" means

Practical synced check:

```promql
abs(yggdrasil_current_slot - <expected_mainnet_tip>) < 60
```

Where `<expected_mainnet_tip>` comes from a trusted second source (e.g. another node, an explorer's API). Within 60 slots (~20 minutes) of upstream tip is operationally synced for most purposes; within 10 slots is "block-production ready".

## Where to go next

- [Block Production]({{ "/manual/block-production/" | relative_url }}) — extend monitoring to track forge-loop events.
- [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}) — interpret common error traces.
