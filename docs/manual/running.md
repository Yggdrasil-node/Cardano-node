---
title: Running a Node
layout: default
parent: User Manual
nav_order: 6
---

# Running a Node

Once you have a working binary and a config you trust, the next step is making the node a long-running service that survives reboots, restarts cleanly on signals, and rotates logs.

## Foreground (development)

```bash
$ yggdrasil-node run --network mainnet --database-path /var/lib/yggdrasil/db
```

Press `Ctrl-C` (SIGINT) to shut down gracefully. Output goes to stdout in human-readable format with severity colours when stdout is a TTY.

For machine-readable JSON output, set `TraceOptions` `Stdout` backend to `MachineFormat` (see [Monitoring]({{ "/manual/monitoring/" | relative_url }})).

## systemd (production)

Recommended for any deployment that should auto-restart and survive reboots.

Create `/etc/systemd/system/yggdrasil.service`:

```ini
[Unit]
Description=Yggdrasil Cardano Node
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=yggdrasil
Group=yggdrasil
WorkingDirectory=/var/lib/yggdrasil
ExecStart=/usr/local/bin/yggdrasil-node run \
    --network mainnet \
    --database-path /var/lib/yggdrasil/db \
    --metrics-port 12798 \
    --port 3001
Restart=always
RestartSec=5
LimitNOFILE=65536
TimeoutStopSec=60
KillSignal=SIGINT
SuccessExitStatus=0

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
ReadWritePaths=/var/lib/yggdrasil

# Standard output / error → journal
StandardOutput=journal
StandardError=journal
SyslogIdentifier=yggdrasil

[Install]
WantedBy=multi-user.target
```

Apply and enable:

```bash
# systemctl daemon-reload
# systemctl enable yggdrasil
# systemctl start yggdrasil
# journalctl -u yggdrasil -f
```

Notes on the unit file:

- **`KillSignal=SIGINT`**. Yggdrasil installs a `tokio::signal::ctrl_c` handler that triggers graceful shutdown. systemd's default `SIGTERM` is also handled, but `SIGINT` matches the development behaviour.
- **`TimeoutStopSec=60`**. Graceful shutdown drains in-flight peers; 60 s gives ample headroom. The actual drain typically completes in under 10 s.
- **`Restart=always`**. The node will recover automatically from any uncaught panic. With proper logging, this should be a rare event — investigate every restart.

## Logging destinations

The trace dispatcher supports three backends:

- **`Stdout HumanFormatColoured`** — ANSI-coloured human text (default for TTY).
- **`Stdout HumanFormatUncoloured`** — plain human text (default for non-TTY).
- **`Stdout MachineFormat`** — line-delimited JSON, suitable for log aggregators.
- **`Forwarder`** — CBOR-encoded events to a Unix socket. Compatible with upstream `cardano-tracer`.
- **`PrometheusSimple`** — metric export (works alongside the `--metrics-port` HTTP endpoint).
- **`EKGBackend`** — recognised but currently a no-op (placeholder for future EKG bridge).

To switch stdout to JSON, edit `config.json`:

```jsonc
{
  "TraceOptions": {
    "": {
      "severity": "Notice",
      "backends": ["Stdout MachineFormat"]
    }
  }
}
```

## Log rotation

If you use `Stdout MachineFormat` and pipe to a file (rather than systemd's journal), wire up `logrotate`. Example `/etc/logrotate.d/yggdrasil`:

```
/var/log/yggdrasil/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
}
```

`copytruncate` is preferred over `create` because Yggdrasil does not currently re-open log files on `SIGHUP`. With `copytruncate`, logrotate copies the file out, then truncates the original — the node keeps writing without interruption.

If you use `journalctl`, the journal manages rotation itself via `/etc/systemd/journald.conf` (`SystemMaxUse=`, `SystemKeepFree=`).

## Graceful shutdown internals

When the node receives SIGINT or SIGTERM:

1. The accept loop stops admitting new inbound connections.
2. The connection manager runs `timeout_tick(now)` once more.
3. Phase 1: outbound peers receive `ControlMessage::Terminate` via `apply_control_close`.
4. Phase 2: warm/hot CM-managed peers are released through `release_outbound_connection`.
5. Phase 3: inbound `JoinSet` is drained with a 5-second deadline; remaining tasks are aborted.
6. The current ledger checkpoint is flushed to disk.
7. The node exits with status 0.

If the deadline expires before phase 3 completes, the offending tasks are force-aborted but the database state is still consistent — the volatile suffix is journalled before each block apply, and the immutable region only advances after stable-block promotion.

## Restart resilience

After a clean or unclean shutdown, restarting from the same `--database-path`:

1. Loads the most recent ledger checkpoint.
2. Replays the volatile suffix (typically the last K=2160 blocks) to bring the in-memory state to the last persisted tip.
3. Resumes ChainSync from the loaded tip via `MsgFindIntersect`.
4. Continues normal operation.

Total restart-to-syncing time on mainnet is typically under 60 seconds.

For an automated restart-resilience verification, see [`node/scripts/restart_resilience.sh`](https://github.com/yggdrasil-node/Cardano-node/blob/main/node/scripts/restart_resilience.sh) and runbook §4.

## Updating the binary

To upgrade Yggdrasil while preserving the chain database:

```bash
$ cd /path/to/yggdrasil
$ git fetch origin
$ git checkout main
$ git pull --ff-only
$ cargo build --release --bin yggdrasil-node
# systemctl stop yggdrasil
# install -o root -g root -m 755 target/release/yggdrasil-node /usr/local/bin/yggdrasil-node
# systemctl start yggdrasil
# journalctl -u yggdrasil -f
```

The chain database under `--database-path` is forward-compatible with newer node binaries within a major version. Across major versions, see release notes for migration steps.

## Multiple instances on one host

Each node needs:

- A unique `--database-path`.
- A unique `--port` (or none, if outbound-only).
- A unique `--metrics-port`.
- A unique NtC socket path (`local_socket_path` in config, default `<storage_dir>/node.sock`).

Drop in a second systemd unit `yggdrasil-preprod.service` with different paths and start it alongside the mainnet one.

## Where to go next

- [Monitoring]({{ "/manual/monitoring/" | relative_url }}) — wire up Prometheus and dashboards.
- [Block Production]({{ "/manual/block-production/" | relative_url }}) — turn this relay into a stake pool.
- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — backups, garbage collection, upgrades.
