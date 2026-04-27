---
title: Docker
layout: default
parent: User Manual
nav_order: 6.5
---

# Running with Docker

A multi-stage `Dockerfile` and `docker-compose.yml` are included at the repository root. This chapter covers a relay deployment via `docker compose`. For block production via Docker, see the credential-mounting section near the end.

## Prerequisites

- Docker Engine 20.10+ or Docker Desktop.
- Docker Compose v2 (`docker compose ...`, not the legacy `docker-compose`).
- ~16 GB free RAM and ~500 GB SSD for a mainnet relay. The same size applies inside a container.

## Quickstart — mainnet relay

```bash
$ git clone https://github.com/yggdrasil-node/Cardano-node yggdrasil
$ cd yggdrasil
$ docker compose up -d
$ docker compose logs -f
```

That's it. The compose file defines:

- A named volume `yggdrasil-db` for the chain database (survives container recreates).
- Port `3001/tcp` exposed on all interfaces for inbound NtN.
- Port `12798/tcp` exposed on `127.0.0.1` only for Prometheus metrics.
- A 30-second healthcheck against `/health` with a 120-second startup grace period.
- Resource limits: 8 GB reservation, 16 GB limit.

The container builds from source on first run (5–15 minutes). Subsequent `docker compose up` reuses the built image.

## Image structure

Two stages:

1. **Builder** (`rust:1.95-bookworm`) — compiles `yggdrasil-node` with `--release --locked`.
2. **Runtime** (`debian:bookworm-slim`) — copies the binary, vendored presets, and operator scripts. Runs as a non-root user (`yggdrasil`, UID 1000) under `tini` for proper signal forwarding.

The runtime image weighs in around 100 MB. The builder image is discarded after build (it carries the full Rust toolchain).

## Environment variables

| Variable          | Default                       | Purpose |
|-------------------|-------------------------------|---------|
| `YG_DB_PATH`      | `/var/lib/yggdrasil/db`       | Chain DB root inside the container. |
| `YG_CONFIG_PATH`  | `/etc/yggdrasil`              | Custom config mount point. |

## Custom configuration

To use your own config and topology instead of the bundled mainnet preset:

```yaml
# docker-compose.override.yml
services:
  yggdrasil:
    volumes:
      - ./my-config.json:/etc/yggdrasil/config.json:ro
      - ./my-topology.json:/etc/yggdrasil/topology.json:ro
    command:
      - run
      - --config=/etc/yggdrasil/config.json
      - --topology=/etc/yggdrasil/topology.json
      - --database-path=/var/lib/yggdrasil/db
      - --port=3001
      - --host-addr=0.0.0.0
      - --metrics-port=12798
```

`docker compose` automatically merges `docker-compose.override.yml` on top of `docker-compose.yml` when both are present. The override is gitignored so you can customise without polluting the repo.

## Switching network

For preprod or preview, change the `--network` flag:

```yaml
command:
  - run
  - --network=preprod
  - --database-path=/var/lib/yggdrasil/db
  - --port=3001
  - --host-addr=0.0.0.0
  - --metrics-port=12798
```

Use a separate volume per network — chain databases are not interchangeable.

## Block production via Docker

A block producer needs four credential files mounted read-only:

```yaml
services:
  yggdrasil-producer:
    image: yggdrasil-node:latest
    container_name: yggdrasil-producer
    restart: unless-stopped
    # NO public port exposure for a producer — only outbound to relays.
    volumes:
      - producer-db:/var/lib/yggdrasil/db
      - ./keys:/var/lib/yggdrasil/keys:ro
      - ./topology.json:/etc/yggdrasil/topology.json:ro
    command:
      - run
      - --network=mainnet
      - --database-path=/var/lib/yggdrasil/db
      - --topology=/etc/yggdrasil/topology.json
      - --metrics-port=12798
      - --shelley-kes-key=/var/lib/yggdrasil/keys/kes.skey
      - --shelley-vrf-key=/var/lib/yggdrasil/keys/vrf.skey
      - --shelley-operational-certificate=/var/lib/yggdrasil/keys/node.opcert
      - --shelley-operational-certificate-issuer-vkey=/var/lib/yggdrasil/keys/cold.vkey

volumes:
  producer-db:
    driver: local
```

Key safety notes:

- Mount the keys directory **read-only** (`:ro`).
- Set host-side permissions: `chmod 0400 keys/*.skey && chown 1000:1000 keys/*.skey` (UID 1000 matches the in-container `yggdrasil` user).
- Do not commit keys to the image. Always mount at runtime.
- Do not publish ports (`ports:` block omitted) — the producer should only reach the network through your relays.
- Configure `topology.json` with `diffusionMode: InitiatorOnlyDiffusionMode` for each local-root.

## Inspecting a running container

```bash
$ docker compose exec yggdrasil yggdrasil-node status \
    --database-path /var/lib/yggdrasil/db
$ docker compose exec yggdrasil curl -s http://127.0.0.1:12798/metrics | head
$ docker compose exec yggdrasil curl -s http://127.0.0.1:12798/health
```

## Stopping cleanly

```bash
$ docker compose stop          # SIGTERM, 60-second grace; tini forwards to yggdrasil-node
$ docker compose down          # also removes the container
$ docker compose down -v       # additionally removes the named volume — destroys the chain DB
```

## Logs

By default, container logs go to the Docker logging driver:

```bash
$ docker compose logs -f --tail=100 yggdrasil
```

For production deployments, configure a logging driver (`json-file` with rotation, `journald`, or a remote aggregator). Example:

```yaml
services:
  yggdrasil:
    logging:
      driver: json-file
      options:
        max-size: "100m"
        max-file: "10"
```

## Updates

When a new release is published:

```bash
$ git pull --ff-only
$ docker compose build --pull   # rebuild from updated source
$ docker compose up -d          # rolling restart
$ docker compose logs -f --tail=200
```

The chain DB volume is preserved across image rebuilds. See [Maintenance]({{ "/manual/maintenance/" | relative_url }}) for the cross-version upgrade procedure.

## Where to go next

- [Monitoring]({{ "/manual/monitoring/" | relative_url }}) — wire the metrics endpoint into Prometheus.
- [Block Production]({{ "/manual/block-production/" | relative_url }}) — KES rotation procedure (applies to Docker too — rotate keys outside the container, restart it).
- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — backups and disk health.
