---
title: Maintenance
layout: default
parent: User Manual
nav_order: 10
---

# Maintenance

Long-running production responsibilities. None are urgent in week one, all matter eventually.

## Backups

What to back up:

| Item                                | Frequency | Where it lives                                         |
|-------------------------------------|-----------|--------------------------------------------------------|
| **Cold key**                        | once      | `cold.skey` — keep offline, multiple physical copies   |
| **VRF key**                         | once      | `vrf.skey` on the block producer                       |
| **KES key**                         | per rotation | `kes.skey` on the block producer                    |
| **OpCert**                          | per rotation | `node.opcert` on the block producer                 |
| **OpCert issue counter**            | per rotation | `cold.counter` (kept with the cold key)             |
| **Pool registration metadata**      | once      | `pool.cert`, `delegation.cert`, `pool.metadata.json`   |
| **Yggdrasil chain database**        | optional  | `<storage_dir>/`. Resyncable from the network if lost. |
| **Configuration files**             | on change | `config.json`, `topology.json`                         |

The chain database is technically expendable — if you lose it, a re-sync from genesis takes 1–3 days on mainnet. But on a block producer where downtime costs missed slots, retain a recent snapshot.

### Chain database snapshot procedure

```bash
# systemctl stop yggdrasil
$ tar -czf /backup/yggdrasil-db-$(date +%Y%m%d).tar.gz -C /var/lib/yggdrasil db
# systemctl start yggdrasil
```

Stop is required because the database is being written to. A 5-second graceful shutdown is sufficient; the snapshot itself takes minutes depending on the size of the chain DB.

For a hot snapshot without downtime, use a filesystem-level snapshot (LVM, ZFS, btrfs):

```bash
# lvcreate -L 50G -s -n yggdrasil-snap /dev/vg0/yggdrasil-data
# tar -czf /backup/yggdrasil-db-$(date +%Y%m%d).tar.gz -C /mnt/yggdrasil-snap db
# lvremove -f /dev/vg0/yggdrasil-snap
```

Volatile state mid-snapshot is acceptable because volatile data is journalled — restart from a snapshotted DB will replay the volatile suffix from the most recent checkpoint.

### Restore-test procedure

Once a month, restore a backup to a scratch machine and confirm the node starts and reaches tip. Untested backups are not backups.

## Garbage collection and pruning

The node maintains its own GC for the chain DB:

- **Immutable region**: never trimmed in normal operation. Stable blocks are append-only.
- **Volatile region**: pruned to slots after the most-recent immutable boundary.
- **Ledger snapshots**: capped at `--max-ledger-snapshots` (default 10). Older snapshots are deleted when a new one is taken.

If the immutable region grows too large for your disk, you have two options:

1. **Add storage.** The chain only grows; eventually you will need more disk.
2. **Move to a larger SSD** with a fresh resync. Mainnet currently uses ~150 GB and grows ~10 GB/month.

### Manual cleanup commands

For deliberate cleanup (e.g. before a major upgrade), Yggdrasil's `ChainDb` exposes:

- `garbage_collect` — full GC pass.
- `gc_immutable_before_slot(slot)` — drop immutable data before a slot. Use with care; this is destructive.
- `gc_volatile_before_slot(slot)` — drop volatile data before a slot.
- `compact` — rewrite chunks to reclaim space.

These are not currently exposed as CLI commands. Open an issue if you need them — they exist as internal storage primitives.

## KES rotation procedure

KES keys expire. On mainnet, `slotsPerKESPeriod = 129600` slots ≈ 36 hours, and the maximum number of evolutions is 62, so a single KES key is valid for **up to 90 days**. Rotate before expiry to avoid missed slots.

### Rotation steps

1. **Compute the current KES period.**

   ```bash
   $ current_slot=$(yggdrasil-node query chain-tip --socket /var/lib/yggdrasil/db/node.sock | jq .slot)
   $ kes_period=$(( current_slot / 129600 ))
   $ echo "Current KES period: $kes_period"
   ```

2. **On the block producer, generate a new KES key.**

   ```bash
   $ cardano-cli node key-gen-KES \
       --verification-key-file kes-new.vkey \
       --signing-key-file kes-new.skey
   $ chmod 0400 kes-new.skey
   ```

3. **On the air-gapped cold-key machine, issue a new OpCert.**

   Copy `kes-new.vkey` and `cold.counter` to the cold-key machine. Then:

   ```bash
   $ cardano-cli node issue-op-cert \
       --kes-verification-key-file kes-new.vkey \
       --cold-signing-key-file cold.skey \
       --operational-certificate-issue-counter-file cold.counter \
       --kes-period $kes_period \
       --out-file node-new.opcert
   ```

   The counter file is auto-incremented. Save the updated counter back to safe storage.

4. **Copy the new OpCert and the updated counter back to the block producer.**

5. **Atomically replace the old files.**

   ```bash
   # mv kes-new.skey /var/lib/yggdrasil/keys/kes.skey
   # mv node-new.opcert /var/lib/yggdrasil/keys/node.opcert
   # chown yggdrasil:yggdrasil /var/lib/yggdrasil/keys/kes.skey /var/lib/yggdrasil/keys/node.opcert
   # chmod 0400 /var/lib/yggdrasil/keys/kes.skey
   ```

6. **Restart the node.**

   ```bash
   # systemctl restart yggdrasil
   # journalctl -u yggdrasil -f
   ```

   Confirm in the logs:
   - `loaded block-producer credentials`
   - `OpCert verified against issuer cold key`
   - `KES period <new_period> within valid window`

7. **Verify by waiting for a leader slot.** If your pool has a leader slot in the next epoch, confirm the block was forged with the new KES key.

### Rotation timing

Rotate when the **current KES period is at most 50** within the OpCert validity window — leaving room for unforeseen delays. Calendar-based: every 60 days is conservative.

A `cron` reminder for a 60-day cadence:

```cron
0 9 1 */2 *  /usr/local/bin/notify-kes-rotation-due
```

## Yggdrasil version upgrades

Within a major version (`0.x`), chain DB and config are forward-compatible. Across major versions, see the release notes.

### Standard procedure

```bash
$ cd /path/to/yggdrasil
$ git fetch origin
$ git log --oneline HEAD..origin/main      # review changes
$ git pull --ff-only
$ cargo build --release --bin yggdrasil-node
$ ./target/release/yggdrasil-node --version

$ cargo test-all                           # local sanity check
$ cargo lint                               # lint clean
```

Then deploy:

```bash
# systemctl stop yggdrasil
# install -o root -g root -m 755 target/release/yggdrasil-node /usr/local/bin/yggdrasil-node
# yggdrasil-node validate-config --network mainnet --database-path /var/lib/yggdrasil/db
# systemctl start yggdrasil
# journalctl -u yggdrasil -f
```

### Upgrade order across machines

For pool operators:

1. **Upgrade one relay first.** Watch for 24 hours.
2. **Upgrade other relays.**
3. **Upgrade the block producer last.** Schedule outside leader slots if possible (use `cardano-cli query leadership-schedule` against your pool key).

A failed upgrade on a relay is recoverable — the network has redundancy. A failed upgrade on the block producer during a leader slot loses block rewards.

### Rolling back

If an upgrade misbehaves:

```bash
$ git checkout <previous-tag-or-sha>
$ cargo build --release --bin yggdrasil-node
# systemctl stop yggdrasil
# install -m 755 target/release/yggdrasil-node /usr/local/bin/yggdrasil-node
# systemctl start yggdrasil
```

The chain DB is forward-compatible within major versions, so you can downgrade without resyncing. If a new release introduces a DB-format change, the release notes will say so explicitly and rollback may require a fresh sync.

## Disk health

Monitor:

- **`/var/lib/yggdrasil` free space.** Alert at < 50 GB free.
- **SSD wear.** `smartctl -a /dev/nvme0` shows `Percentage Used` (lower is better).
- **I/O errors.** `dmesg` and journal for `nvme:` or `sd:` errors.

Yggdrasil writes the volatile region heavily — measured at ~50 MB/hour during steady-state mainnet operation. A consumer SSD can sustain this for years; a cheap thumb drive cannot.

## Network health

Monitor:

- **Outbound connection count** (`yggdrasil_cm_outbound_conns`). Should be at or near `governor_target_active`.
- **Hot peer count** (derived from governor traces). Should match the configured target.
- **Reconnect rate** (`rate(yggdrasil_reconnects_total[10m])`). High values suggest peer churn or local connectivity issues.
- **TCP retransmit rate** (`netstat -s | grep retransmit`). High values suggest packet loss.

## Time synchronisation

Yggdrasil is sensitive to wall-clock skew. The `FutureBlockCheckConfig` rejects blocks whose declared slot is too far in the future relative to the local clock. A clock that drifts forward by tens of seconds will reject valid peer blocks.

Configure `chrony` or `systemd-timesyncd` against multiple time sources. Verify:

```bash
$ chronyc tracking
Reference ID    : XXX (...)
Stratum         : 2
Ref time (UTC)  : ...
System time     : 0.000012345 seconds slow of NTP time
```

Anything more than ±100 ms requires investigation.

## Where to go next

- [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}) — when something goes wrong.
- [Manual Test Runbook]({{ "/MANUAL_TEST_RUNBOOK/" | relative_url }}) — formal validation procedures including parallel-fetch §6.5.
