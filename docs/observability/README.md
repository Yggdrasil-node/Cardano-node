# Yggdrasil observability — operator quick start

Wave 10 PR 29 ships drop-in observability configs so SPOs migrating
from upstream `cardano-node` 11.0.1 keep their existing dashboards
and alerts. Three artifacts land here:

| File | Role | Wave |
| --- | --- | --- |
| [`grafana-dashboards/yggdrasil-node-overview.json`](grafana-dashboards/yggdrasil-node-overview.json) | Grafana dashboard — slot/block tip, era, density, peers, mempool, forge counters, rollback rate. | 10 PR 29 |
| [`loki/promtail.example.yml`](loki/promtail.example.yml) | Promtail scrape config — parses the Haskell-Katip JSON shape from each Yggdrasil binary's journal output into Loki labels. | 10 PR 29 |
| [`alertmanager/yggdrasil-rules.yml`](alertmanager/yggdrasil-rules.yml) | Prometheus alerting rules — chain-behind-tip, missed slots, cannot-forge, rollback rate, mempool backlog. | 10 PR 29 |

## First five minutes

1. **Confirm the node is emitting metrics.** Default scrape target
   is `http://<host>:12798/metrics`. The response should contain
   BOTH the legacy `yggdrasil_*` lines and the new EKG-parity
   `cardano_node_metrics_*` lines:

   ```bash
   curl -s http://localhost:12798/metrics | grep -c '^cardano_node_metrics_'
   # Expected: 15 (one line per EKG-parity name; see
   # `crates/observability/yggdrasil-metrics/src/lib.rs::ALL_NAMES`)
   ```

2. **Confirm the node is emitting Haskell-Katip JSON logs.** Default
   `--log-format` is `haskell-json`. The schema fields are
   declared Tier-1 stable in [`docs/COMPATIBILITY.md`](../COMPATIBILITY.md):

   ```bash
   journalctl -u yggdrasil-node -o json --no-pager -n 1 \
     | jq '. | {at, ns, sev, thread, host, app}'
   ```

   Every record carries `at` (RFC3339 sub-second), `ns` (namespace
   array), `data` (per-event fields), `sev` (severity), `thread`
   (OS thread name/ID). `host` and `app` are emitted when set.

3. **Import the Grafana dashboard.** In Grafana:
   `Dashboards → New → Import → Upload JSON file →
    yggdrasil-node-overview.json → Prometheus datasource`.
   The dashboard auto-discovers `cardano_node_metrics_*` series
   from the configured datasource.

4. **Point Promtail at the journal.** Copy
   `loki/promtail.example.yml` to `/etc/promtail/config.yml`,
   edit `clients[].url` to your Loki instance, restart Promtail.
   The pipeline_stages drop empty / unparseable lines so Loki
   doesn't ingest raw stdout from a misconfigured binary.

5. **Wire the alerts.** Copy
   `alertmanager/yggdrasil-rules.yml` into your Prometheus
   `rule_files` directory and reload the Prometheus config
   (`kill -HUP $(pidof prometheus)`).

## Metric-name reference

Source-of-truth identifiers live at
[`crates/observability/yggdrasil-metrics/src/lib.rs::names`](../../crates/observability/yggdrasil-metrics/src/lib.rs).

The 15 canonical names:

| Symbol | Type | Source |
| --- | --- | --- |
| `cardano.node.metrics.slotNum.int` | gauge | NodeMetrics.current_slot |
| `cardano.node.metrics.blockNum.int` | gauge | NodeMetrics.current_block_number |
| `cardano.node.metrics.density.real` | gauge | derived: blocks_synced / uptime_seconds |
| `cardano.node.metrics.slotsMissedNum.int` | counter | (forge-side, pending) |
| `cardano.node.metrics.txsInMempool.int` | gauge | NodeMetrics.mempool_tx_count |
| `cardano.node.metrics.mempoolBytes.int` | gauge | NodeMetrics.mempool_bytes |
| `cardano.node.metrics.connectedPeers.int` | gauge | NodeMetrics.active_peers |
| `cardano.node.metrics.peersFromNodeKernel.int` | gauge | NodeMetrics.known_peers |
| `cardano.node.metrics.currentEra.int` | gauge | NodeMetrics.current_era |
| `cardano.node.metrics.blockProcessingTime.real` | gauge | (histogram, pending) |
| `cardano.node.metrics.forks.int` | counter | NodeMetrics.rollbacks |
| `cardano.node.metrics.nodeIsLeader.int` | counter | (forge-side, pending) |
| `cardano.node.metrics.nodeCannotForge.int` | counter | (forge-side, pending) |
| `cardano.node.metrics.blocksForgedNum.int` | counter | (forge-side, pending) |
| `cardano.node.metrics.aboutToLeadSlotLast.int` | gauge | (forge-side, pending) |

Prometheus converts `.` → `_` in metric names during scrape, so the
on-wire names operators see at the `/metrics` endpoint and in
Grafana / Loki / Alertmanager queries are:
`cardano_node_metrics_slotNum_int`, `cardano_node_metrics_blockNum_int`, etc.

Five metrics emit `0` today because the underlying NodeMetrics field
isn't tracked yet (forge-side counters, slot-missed tracker,
block-processing-time histogram). The names + dashboards are stable
under [`docs/COMPATIBILITY.md`](../COMPATIBILITY.md) Tier-1 — live
values land as their respective subsystems wire through `metrics::*`.

## See also

- [`docs/COMPATIBILITY.md`](../COMPATIBILITY.md) — what surfaces are stable across releases.
- [`docs/manual/release-verification.md`](../manual/release-verification.md) — cosign / SLSA / SBOM verification recipes.
- [`crates/telemetry/src/haskell_json.rs`](../../crates/telemetry/src/haskell_json.rs) — the JSON schema's source of truth.
- [`crates/observability/yggdrasil-metrics/src/lib.rs`](../../crates/observability/yggdrasil-metrics/src/lib.rs) — the metric-name constants' source of truth.
