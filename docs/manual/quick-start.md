---
title: Quick Start
layout: default
parent: User Manual
nav_order: 3
---

# Quick Start

This is the fastest path from "binary built" to "node syncing mainnet". Five commands.

If you have not built the binary yet, do [Installation]({{ "/manual/installation/" | relative_url }}) first.

## 1. Pick a database path

Choose a directory on a fast SSD with at least 200 GB free. The chain plus ledger snapshots will grow there.

```bash
$ export YG_DB=/var/lib/yggdrasil/db
$ mkdir -p "$YG_DB"
```

## 2. Validate the configuration

```bash
$ yggdrasil-node validate-config --network mainnet --database-path "$YG_DB"
```

Expected output: a report with zero errors. Warnings about "storage not initialized" or "no peer snapshot" are normal for a fresh node.

If `validate-config` reports a `HashMismatch` for any genesis file, **stop**. The vendored config under `node/configuration/mainnet/` is corrupt or out of date. Reclone the repository.

## 3. Start the node

```bash
$ yggdrasil-node run --network mainnet --database-path "$YG_DB" --metrics-port 12798
```

Interpret the output:

- `Net.Bootstrap` traces show the initial peer dial.
- `ChainDB.AddBlockEvent` traces fire as blocks are validated and persisted.
- `Node.Recovery.Checkpoint` traces fire periodically as the ledger snapshot is checkpointed.

The first sync from genesis to current tip takes:

- **Mainnet**: 24–60 hours depending on hardware. The Byron era replays in minutes; Shelley+ takes most of the time.
- **Preprod**: 2–6 hours.
- **Preview**: 30 min – 2 hours.

## 4. Watch progress

In a second terminal:

```bash
$ yggdrasil-node status --database-path "$YG_DB"
```

This prints the on-disk sync position, block count, checkpoint state, and ledger counts (UTxO entries, registered pools, DReps).

Or, if you enabled `--metrics-port`:

```bash
$ curl -s http://127.0.0.1:12798/metrics | grep -E "^yggdrasil_(blocks_synced|current_slot|block_number)"
yggdrasil_blocks_synced 412987
yggdrasil_current_slot 117425831
yggdrasil_block_number 11293441
```

The current mainnet tip slot is published at [pooltool.io](https://pooltool.io/) or [explorer.cardano.org](https://explorer.cardano.org/). When `yggdrasil_current_slot` reaches that value, your node is fully caught up.

## 5. Submit a transaction (optional)

Once synced, you can use the node as a transaction submission endpoint. With `cardano-cli` from upstream:

```bash
$ cardano-cli transaction submit \
    --tx-file my-signed-tx.txsigned \
    --socket-path "$YG_DB/node.sock"
```

Or from Yggdrasil's own CLI (Unix only):

```bash
$ yggdrasil-node submit-tx --tx-hex 84a300...01ff --network mainnet
```

## Stop the node cleanly

`Ctrl-C` (SIGINT) or `kill -TERM <pid>` triggers graceful shutdown:

1. Inbound accept loop stops accepting new connections.
2. In-flight inbound sessions get up to 5 seconds to complete.
3. Outbound peers receive `ControlMessage::Terminate`.
4. The connection manager drains warm and hot peers via `release_outbound_connection`.
5. The current ledger checkpoint is flushed to disk.

After clean shutdown, restarting from the same `--database-path` resumes from the checkpoint — only the volatile suffix is replayed (typically the last K=2160 blocks).

## What just happened

Your node:

- Loaded the mainnet genesis files from `node/configuration/mainnet/`.
- Verified each genesis file hash matches the value pinned in the config.
- Connected to the bootstrap peers listed in `topology.json`.
- Negotiated the NtN handshake (versions 13/14) with each peer.
- Started ChainSync, BlockFetch, KeepAlive, TxSubmission2, and PeerSharing mini-protocols on the multiplexer.
- Pulled chain history from genesis, verified every block, applied the ledger transition, and persisted to disk.
- Started accepting inbound connections (default-disabled unless you set `inbound_listen_addr`).
- Started the peer governor, refreshing peer state every governor tick.

## Where to go next

- For a real production deployment, continue to [Configuration]({{ "/manual/configuration/" | relative_url }}) to tune peer counts, log destinations, and storage paths for your environment.
- To run the node under systemd, see [Running a Node]({{ "/manual/running/" | relative_url }}).
- For Prometheus scraping and dashboard setup, see [Monitoring]({{ "/manual/monitoring/" | relative_url }}).
- To produce blocks as a stake pool, see [Block Production]({{ "/manual/block-production/" | relative_url }}).
