---
title: Troubleshooting
layout: default
parent: User Manual
nav_order: 11
---

# Troubleshooting

A catalogue of common error messages and their resolutions. If your issue isn't here, file a GitHub issue with the trace output.

## Startup failures

### `HashMismatch` for a genesis file

```
GenesisLoadError::HashMismatch {
    path: ".../shelley-genesis.json",
    expected: "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
    actual:   "8d9f47..."
}
```

**Cause**: The genesis file does not match the expected Blake2b-256 hash from the config.

**Resolution**:

1. Verify you have not edited any genesis JSON files.
2. Re-clone the repository to get a fresh copy of the vendored configs.
3. If you are using a custom `--config`, confirm it points to the correct genesis files for the chosen network.

This check is intentional — a wrong genesis file silently corrupts every subsequent ledger state.

### `OpCert signature verification failed against issuer cold key`

**Cause**: The OpCert was issued by a different cold key than the one configured.

**Resolution**:

1. Confirm `--shelley-operational-certificate-issuer-vkey` points to the correct cold-key vkey.
2. Re-run `cardano-cli node issue-op-cert` with the cold key whose vkey you have configured.
3. Restart with the new OpCert.

### `KES period <n> outside valid OpCert window`

**Cause**: The current chain slot is past the OpCert's validity window.

**Resolution**: Rotate KES key and reissue OpCert per [Maintenance — KES rotation]({{ "/manual/maintenance/" | relative_url }}#kes-rotation-procedure).

### `Storage uninitialized`

**Symptom**: First-run warning from `validate-config`.

**Resolution**: This is normal on a fresh setup. `run` will initialise the database on first start.

## Sync issues

### Sync stalls with no progress

**Symptom**: `yggdrasil_current_slot` stops advancing.

**Diagnose**:

```bash
$ journalctl -u yggdrasil --since "5 minutes ago" | grep -E "Net.|ChainDB|Sync"
```

Look for:

- **Repeated reconnect events** → peers unhealthy or local network issue.
- **`MsgIntersectNotFound` followed by genesis replay** → see "ChainSync resets to genesis" below.
- **No events at all** → the runtime is hung. Check for thread-pool exhaustion via `htop`/`top`.

### ChainSync resets to genesis after every disconnect

**Symptom**: After a peer disconnect, the node re-syncs from slot 0.

**Cause**: Pre-`6e8c8f9` regression where `MsgFindIntersect` was not issued after reconnect, defaulting peer read-pointer to Origin.

**Resolution**: Upgrade to current `main`. Already fixed.

### `keep-alive timeout`

**Symptom**: Outbound connections drop every ~97 seconds.

**Cause**: KeepAlive heartbeat not being sent. Pre-`d3f1c2a` codec bug where `MsgKeepAliveResponse` and `MsgDone` had swapped tags.

**Resolution**: Upgrade. Already fixed.

### `BlockFromFuture` rejecting valid blocks

**Symptom**: Trace error rejecting blocks whose declared slot is in the future.

**Cause**: Local clock is behind real time.

**Resolution**: Sync system clock (chrony, ntpd, systemd-timesyncd). Verify with `chronyc tracking` (drift should be < 100 ms).

## Connection issues

### `Connection refused` to all peers

**Cause**: Local network blocking outbound, or all configured peers down.

**Resolution**:

1. Test connectivity manually: `nc -z backbone.cardano.iog.io 3001`.
2. Check firewall: `iptables -L -n | grep DROP`.
3. Verify topology — bootstrap peers should be IOG-operated and very rarely down.

### Inbound rate limiting hits

**Symptom**: `yggdrasil_inbound_connections_rejected_total` increasing.

**Cause**: Total inbound connections at `accepted_connections_limit_hard` (default 512).

**Resolution**:

- For a relay serving many peers, raise `AcceptedConnectionsLimit.hardLimit` and `softLimit`.
- For a block producer, this should never happen — your firewall should reject inbound at the IP layer.

### `0 outbound connections established`

**Symptom**: Governor reports `outbound_count = 0` after several ticks.

**Diagnose**:

- Verify config points to reachable peers.
- Check the handshake namespace traces for `Refuse` messages: `journalctl -u yggdrasil | grep Handshake`.
- Confirm `--network-magic` matches the network of the configured peers.

## Ledger / consensus errors

### `ProtocolVersionTooHigh` on incoming block

**Cause**: A peer is on a newer protocol version than `MaxKnownMajorProtocolVersion`.

**Resolution**:

- If a hard fork has just landed on mainnet, bump `MaxKnownMajorProtocolVersion` (e.g. from 10 to 11).
- Alternatively, upgrade to a newer Yggdrasil release that bumps the default.

### `BlockBodyHashMismatch` from a peer

**Symptom**: Trace shows a peer-sent block whose body hash doesn't match the header.

**Cause**: Peer is misbehaving (or there's a wire-codec bug on Yggdrasil's side).

**Resolution**:

- Yggdrasil treats this as peer-attributable: the peer is demoted and reconnect attempted.
- If you see this from many peers, file a bug — likely a Yggdrasil decode regression.

### `OcertCounterTooOld`

**Cause**: A peer block carries an OpCert sequence number lower than the most recent observed for that pool.

**Resolution**:

- Yggdrasil is enforcing OpCert monotonicity. The peer's block is invalid, and the peer is demoted.
- If your **own** node logs this against your pool, you have an OpCert misconfiguration (issued counter went backwards). Fix the counter file.

## Block production issues

### `TraceForgedInvalidBlock` (Critical)

**Cause**: A block your node forged failed self-validation. This should never happen in healthy operation — it indicates a bug.

**Action**:

1. Capture the trace context.
2. Check for any local data corruption.
3. File a high-priority bug with the trace output.
4. Until resolved, run with the operational-certificate disabled (relay-only mode) to avoid further bad forges.

### `TraceSlotIsImmutable`

**Cause**: Forge loop ticked for a slot that is at or behind the current chain tip.

**Resolution**:

- This is informational — the node was leader for a slot that has already been claimed.
- If frequent, your node is consistently behind tip. Check sync health.

### Missed leader slots

**Symptom**: Pool tracker shows blocks were expected but not produced.

**Diagnose**:

- Check `journalctl` for `TraceNodeIsLeader` events at the expected slots.
- If the events fired but the block didn't make it to the chain: peer connectivity issue at production time. Adoption traces (`TraceAdoptedBlock` / `TraceDidntAdoptBlock`) tell you whether your block was adopted by the relays.
- If the events did not fire: VRF check declined, which is normal — leader election is probabilistic.

### `Could not load KES key`

**Cause**: File missing, wrong permissions, or wrong format.

**Resolution**:

```bash
# ls -l /var/lib/yggdrasil/keys/
# stat -c '%a %U:%G %n' /var/lib/yggdrasil/keys/kes.skey
# Should be: 400 yggdrasil:yggdrasil
```

If correct, verify the file is a valid text-envelope KES key (starts with `{"type":"KesSigningKey_ed25519_kes_2^6", ...}`).

## Storage / I/O

### `Disk full`

**Action**:

1. `df -h` — confirm.
2. Identify large directories: `du -h -d 1 /var/lib/yggdrasil`.
3. Reduce `max_ledger_snapshots` in the config and restart — older snapshots will be deleted.
4. Move the database to a larger disk and update `--database-path`. Sync resumes.

### `IO error: input/output error` from the database

**Cause**: Hardware-level disk problem.

**Action**:

1. Stop the node immediately.
2. `dmesg | tail` and `smartctl -a /dev/<your-disk>` to diagnose.
3. If the disk is failing, replace it. Restore the most recent chain DB backup.
4. If no backup, resync from genesis on the new disk.

### Slow sync compared to peers

**Diagnose**:

- `iostat -x 5 6` — `%util` should be high during sync (you're disk-bound during initial sync, that's expected).
- `top` — CPU usage. If consistently 100% on one core, the validator is the bottleneck.
- Compare against `cardano-node` Haskell on similar hardware. Yggdrasil targets parity but performance characteristics may differ.

## Performance issues

### High memory usage

Yggdrasil targets **6–8 GB resident** during steady-state mainnet operation. If you see > 16 GB:

- Check `yggdrasil_mempool_bytes` — if it's unbounded, see "Mempool growing" below.
- Profile with `heaptrack` if you have a development build.
- File an issue with the resident-set-size and `top` output.

### Mempool growing unbounded

**Cause**: Tx admission rate exceeds eviction rate (block-application rollovers).

**Mitigation**:

- Mempool has a hard cap (default ~4 MB total). Beyond that, new transactions are rejected.
- If you see rejects from your own submission attempts, the network is congested — back off and retry.

### Forge loop misses slots

**Diagnose**:

- High CPU at slot tick time — look for competing processes.
- High disk I/O — block production must read mempool snapshot quickly.
- Network latency to relays — block adoption requires propagation through the relays.

A healthy block producer has `htop` showing < 50% sustained CPU and < 5 ms median I/O response.

## Logs and diagnostics

### Capture state for a bug report

```bash
$ yggdrasil-node --version > /tmp/diag.txt
$ yggdrasil-node validate-config --network mainnet --database-path /var/lib/yggdrasil/db >> /tmp/diag.txt 2>&1
$ yggdrasil-node status --database-path /var/lib/yggdrasil/db >> /tmp/diag.txt 2>&1
$ journalctl -u yggdrasil --since "1 hour ago" --no-pager > /tmp/journal.txt
$ ls -la /var/lib/yggdrasil/db > /tmp/db-listing.txt
```

Attach `diag.txt`, `journal.txt`, and `db-listing.txt` to the issue.

### Increase trace verbosity for a single namespace

Edit `config.json`:

```jsonc
"TraceOptions": {
  "Net.BlockFetch": { "severity": "Debug", "detail": "DMaximum" }
}
```

Reload (restart node). Capture for the duration needed, then revert to avoid log volume.

## Where to go next

- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — preventative procedures.
- [Glossary]({{ "/manual/glossary/" | relative_url }}) — term definitions.
- File issues at the [GitHub repository](https://github.com/yggdrasil-node/Cardano-node/issues).
