# Monitoring with the New Tracing System

`cardano-node` 10.2+ uses the new tracing system. The legacy direct EKG/Prometheus
endpoint on port 12798 is no longer supported. The modern stack is:

```
relays + block producer ──► cardano-tracer ──► Prometheus ──► Grafana
                                │
                                exposes /metrics per node
```

`cardano-tracer` is a separate process that aggregates traces and metrics from one
or more nodes over Unix sockets, and exposes a single Prometheus scrape endpoint.

Authoritative source: `references/sources/monitoring-prometheus-grafana.md`,
`references/sources/monitoring-overview.md`, and the new-tracing-system pages
(`references/sources/new-tracing-quick-start.md`,
`references/sources/new-tracing-cardano-tracer.md`,
`references/sources/new-tracing-metrics-migration.md`).

## What to monitor

| Metric | Why it matters |
|---|---|
| Sync progress / slot height | A node behind the chain can't mint |
| Block production | Are you winning and minting your assigned slots? |
| Remaining KES periods | Node stops forging when KES expires |
| Memory and CPU | Sustained high usage is a warning sign |
| Disk space | Chain DB grows continuously |
| Process liveness | Is the node process running? |
| Peer connections (hot/warm/cold) | Too few hot peers degrades propagation |
| Block propagation | Blocks must reach the network in time |
| Unexpected error rate | Spike = investigate |

---

## Step 1 — Build cardano-tracer

Built alongside cardano-node.

```bash
# Cabal
cabal build cardano-tracer
cabal install cardano-tracer --installdir=$HOME/.local/bin --overwrite-policy=always

# Nix
nix build github:IntersectMBO/cardano-node#cardano-tracer
cp result/bin/cardano-tracer $HOME/.local/bin/
```

---

## Step 2 — Configure each node

Edit each node's `config.json`:

```json
{
  "UseTraceDispatcher": true,
  "TraceOptionNodeName": "relay-1",
  "TraceOptions": {
    "": {
      "severity": "Notice",
      "detail": "DNormal",
      "backends": [
        "EKGBackend",
        "Forwarder"
      ]
    }
  }
}
```

- `TraceOptionNodeName` must be unique per node (`relay-1`, `relay-2`,
  `block-producer`, etc.) — this becomes the URL path component and Prometheus
  label.
- Per-severity overrides go in the same `TraceOptions` map keyed by tracer name.

Add the tracer socket flag to node startup:
```bash
cardano-node run \
    ... \
    --tracer-socket-path-connect /run/cardano/tracer.sock
```

**Caution:** Only enable the `Forwarder` backend when `cardano-tracer` is running
and reachable. Without a consumer, traces are buffered in RAM; sustained
disconnection grows the buffer.

---

## Step 3 — Configure cardano-tracer

On the monitoring host, create `/etc/cardano/tracer-config.json`:

```json
{
  "networkMagic": 764824073,
  "network": {
    "tag": "AcceptAt",
    "contents": "/run/cardano/tracer.sock"
  },
  "logging": [
    {
      "logRoot": "/var/log/cardano-tracer",
      "logMode": "FileMode",
      "logFormat": "ForMachine"
    }
  ],
  "rotation": {
    "rpFrequencySecs": 3600,
    "rpKeepFilesNum": 14,
    "rpLogLimitBytes": 104857600,
    "rpMaxAgeHours": 24
  },
  "hasPrometheus": {
    "epHost": "127.0.0.1",
    "epPort": 12789
  }
}
```

`networkMagic` for mainnet: `764824073`. For preprod: `1`. For preview: `2`.

Run as a systemd service:
```ini
[Unit]
Description=Cardano Tracer
After=network-online.target
Wants=network-online.target

[Service]
User=cardano
Type=simple
ExecStart=/usr/local/bin/cardano-tracer --config /etc/cardano/tracer-config.json
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
```

When a node and the tracer run on different hosts, forward the tracer socket
over SSH:
```bash
ssh -L /run/cardano/tracer.sock:/run/cardano/tracer.sock cardano@tracer-host
```

Or use socat for a persistent forward.

---

## Step 4 — Prometheus

Install Prometheus and scrape the tracer's endpoint. `/etc/prometheus/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'cardano'
    metrics_path: '/relay-1/metrics'
    static_configs:
      - targets: ['127.0.0.1:12789']
        labels:
          node: relay-1

  - job_name: 'cardano-bp'
    metrics_path: '/block-producer/metrics'
    static_configs:
      - targets: ['127.0.0.1:12789']
        labels:
          node: block-producer
```

The path component matches each node's `TraceOptionNodeName`.

---

## Step 5 — Grafana

Install Grafana, add the Prometheus data source pointing at `http://127.0.0.1:9090`.

Import a community Cardano dashboard. Good starting points:
- The dashboard JSON shipped in the Guild Operators repo
- IOG's official dashboards from the cardano-tracer repo
- Community dashboards on grafana.com (search "cardano-node")

---

## Step 6 — Alerts

Configure Alertmanager and route to your channel of choice (email, PagerDuty,
Slack, Discord, Telegram).

Essential alerts:

```yaml
groups:
  - name: cardano-node
    rules:
      - alert: KESKeyExpiringSoon
        expr: cardano_node_metrics_remainingKESPeriods_int < 15
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "KES rotation needed on {{ $labels.node }}"

      - alert: NodeFallenBehind
        expr: time() - cardano_node_metrics_slotInEpoch_int > 600
        for: 10m
        labels:
          severity: warning

      - alert: LowHotPeers
        expr: cardano_node_metrics_connectedPeers_int < 3
        for: 15m
        labels:
          severity: warning

      - alert: DiskSpaceLow
        expr: node_filesystem_avail_bytes{mountpoint="/"} < 20 * 1024 * 1024 * 1024
        for: 30m
        labels:
          severity: warning
```

(Metric names may differ slightly between versions; check the tracer's
`/metrics` output for the exact names in your deployment.)

---

## Real-time CLI: gLiveView

For ad-hoc inspection without the full stack, use Guild Operators'
[gLiveView](https://cardano-community.github.io/guild-operators/Scripts/gliveview/):

```bash
cd $CNODE_HOME/scripts
./gLiveView.sh
```

It autodetects relay vs. block producer mode and shows:
- Epoch / slot / block height / sync %
- Hot/warm/cold peer counts
- KES expiry countdown (BP only)
- CPU/memory/disk
- Block production stats (BP only)

Good for quick SSH-in checks. No alerting, no history.

---

## Network visibility: openBlockPerf

[openBlockPerf](https://github.com/cardano-foundation/openblockperf) is an
opt-in tool from the Cardano Foundation. Participating relays collect block
propagation timing from across the network, and contributors get back metrics
showing how their own blocks were experienced by other operators. Useful for:
- Spotting propagation issues with your relays' upstream
- Benchmarking your geographic placement
- Contributing data to network-health research
