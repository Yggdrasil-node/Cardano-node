---
title: Block Production
layout: default
parent: User Manual
nav_order: 8
---

# Block Production

This chapter is for stake pool operators (SPOs). It covers the additional setup needed to turn a relay node into a block-producing node.

If you do not operate a stake pool, skip this chapter.

## Architecture

A typical stake pool deployment runs **multiple machines**:

```
        ┌─────────────────┐
        │  Block producer │  (private — no inbound from public internet)
        │  (this guide)   │
        └────────┬────────┘
                 │  outbound NtN to your relays only
        ┌────────┴─────────┐
        │                  │
   ┌────▼─────┐       ┌────▼─────┐
   │ Relay 1  │       │ Relay 2  │  (public — inbound from anywhere)
   └──────────┘       └──────────┘
```

The block producer holds the KES, VRF, and operational-certificate keys. The relays do not. Compromising a public-internet-exposed relay must not give the attacker the keys to forge blocks.

This chapter covers the **block producer** node only. Relay configuration is the standard setup from the previous chapters.

## Required credentials

A block producer needs four files:

1. **Operational certificate cold key** — `cold.skey` and `cold.vkey`. Generated once, kept offline.
2. **VRF key pair** — `vrf.skey` and `vrf.vkey`. Generated once, lives on the block producer.
3. **KES key pair** — `kes.skey` and `kes.vkey`. Rotated every 90 days (mainnet `slotsPerKESPeriod = 129600`, max periods = 62).
4. **Operational certificate** — `node.opcert`. Signs the KES verification key with the cold key. Reissued each KES rotation.

The cold key is the registered pool's identity. It signs the operational certificate but is otherwise never online. The KES key is what actually signs blocks during the validity window of the OpCert.

### Generating credentials

Yggdrasil uses the same key formats as upstream `cardano-cli`. To create credentials:

```bash
# 1. Cold key (one-time, on an air-gapped machine)
$ cardano-cli node key-gen \
    --cold-verification-key-file cold.vkey \
    --cold-signing-key-file cold.skey \
    --operational-certificate-issue-counter-file cold.counter

# 2. VRF key (one-time, on the block producer)
$ cardano-cli node key-gen-VRF \
    --verification-key-file vrf.vkey \
    --signing-key-file vrf.skey

# 3. KES key (rotated every 90 days)
$ cardano-cli node key-gen-KES \
    --verification-key-file kes.vkey \
    --signing-key-file kes.skey

# 4. Operational certificate (reissued each KES rotation)
$ cardano-cli node issue-op-cert \
    --kes-verification-key-file kes.vkey \
    --cold-signing-key-file cold.skey \
    --operational-certificate-issue-counter-file cold.counter \
    --kes-period <current-period> \
    --out-file node.opcert
```

`<current-period>` is the current KES period number. Compute as:

```
period = floor(current_slot / slotsPerKESPeriod)
```

Check current slot with `yggdrasil-node status` or via a query.

### File permissions

```bash
# chmod 0400 cold.skey vrf.skey kes.skey
# chown yggdrasil:yggdrasil cold.skey vrf.skey kes.skey
# chmod 0644 *.vkey node.opcert
```

The cold key should not normally live on the block producer — store it offline and only touch it for OpCert reissue.

### Preview-only generated harness

For fast preview-network runtime testing, the repository includes a harness that generates upstream `cardano-cli` text-envelope credentials and self-contained Yggdrasil configs under the ignored `tmp/` tree:

```bash
$ cargo build --release -p yggdrasil-node
$ FORCE=1 node/scripts/preview_producer_harness.sh generate
$ node/scripts/preview_producer_harness.sh wallet
$ node/scripts/preview_producer_harness.sh certs
$ node/scripts/preview_producer_harness.sh validate
$ RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-relay
$ RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-producer
$ RUN_SECONDS=300 MIN_SLOT_ADVANCE=1000 node/scripts/preview_producer_harness.sh endurance-producer
```

The default output directory is `tmp/preview-producer/`. It contains:

- `keys/` — cold key, VRF key, KES key, OpCert, and issue counter.
- `wallet/` — preview payment/stake signing keys and addresses for funding and delegation.
- `certs/` — stake registration, stake delegation, pool registration, pool id, and registration summary.
- `config/preview-relay.json` — preview relay config with local inbound serving.
- `config/preview-producer.json` — preview producer-mode config with generated credentials.
- `run/run-preview-relay.sh` and `run/run-preview-producer.sh` — convenience launchers.

The generated cold key is **not registered on-chain** until the certificate transaction is submitted. The producer smoke test proves credential loading, OpCert validation, preview bootstrap connection, metrics, sync, and forge-loop startup. Actual block adoption requires preview tADA, stake-pool registration, delegation, and enough active stake to win leader slots. With the default zero pledge, the funding address needs the preview stake-key deposit plus pool deposit and transaction fees before registration can be submitted.

If you build the preview registration transaction manually with `cardano-cli transaction build-raw`, pass the certificate files in this order: `stake-registration.cert`, `pool-registration.cert`, then `stake-delegation.cert`. The delegation certificate depends on the pool already being present in the transaction certificate sequence.

Use `endurance-producer` after the startup smoke when you need evidence that sync continues over the full bounded run. It samples Prometheus metrics for the whole `RUN_SECONDS` window and fails unless `yggdrasil_current_slot` advances by at least `MIN_SLOT_ADVANCE`.

## Configuring Yggdrasil for block production

Add to `config.json`:

```jsonc
{
  "ShelleyKesKey": "/var/lib/yggdrasil/keys/kes.skey",
  "ShelleyVrfKey": "/var/lib/yggdrasil/keys/vrf.skey",
  "ShelleyOperationalCertificate": "/var/lib/yggdrasil/keys/node.opcert",
  "ShelleyOperationalCertificateIssuerVkey": "/var/lib/yggdrasil/keys/cold.vkey"
}
```

Or pass via CLI flags:

```bash
$ yggdrasil-node run \
    --network mainnet \
    --database-path /var/lib/yggdrasil/db \
    --shelley-kes-key /var/lib/yggdrasil/keys/kes.skey \
    --shelley-vrf-key /var/lib/yggdrasil/keys/vrf.skey \
    --shelley-operational-certificate /var/lib/yggdrasil/keys/node.opcert \
    --shelley-operational-certificate-issuer-vkey /var/lib/yggdrasil/keys/cold.vkey
```

When all four are present, `ShelleyGenesis.systemStart` is available, and the `active_slot_coeff` config key is valid, the node activates the forge loop. A partial credential set is a startup error; pass `--non-producing-node` only when intentionally running this config as a relay/non-producing node.

### Startup verification

On startup with credentials configured, the node:

1. Loads each key file and parses the text envelope.
2. Computes the cold-key Blake2b-224 hash → derived pool ID.
3. Verifies the OpCert signature against the configured issuer cold-key vkey. **If the OpCert was issued by a different cold key, startup fails with a clear error.**
4. Derives the absolute current slot from `ShelleyGenesis.systemStart` + `slotLength`, matching upstream block-forging slot-clock semantics.
5. Waits for live epoch nonce and active stake-snapshot sigma before attempting leadership checks.
6. Checks the OpCert `kes_period` lies within the valid window for the current slot.
7. Activates the per-slot forge loop in `run_block_producer_loop()`.

### Topology — InitiatorOnly mode

The block producer must NOT accept inbound connections from the public internet. Configure local roots so it only connects out to your relays:

```jsonc
{
  "localRoots": [
    {
      "accessPoints": [
        { "address": "10.0.0.5", "port": 3001 },
        { "address": "10.0.0.6", "port": 3001 }
      ],
      "advertise": false,
      "trustable": true,
      "valency": 2,
      "hotValency": 2,
      "diffusionMode": "InitiatorOnlyDiffusionMode"
    }
  ],
  "publicRoots": [],
  "bootstrapPeers": [],
  "useLedgerAfterSlot": -1
}
```

Do not set `--port` or `inbound_listen_addr` on a block producer. The relays connect inbound to the public network; the producer connects outbound only to its relays.

Network ACL on the producer: **deny all inbound on port 3001 except from your relay IPs**.

For emergency maintenance where the same config file must run without forging, add `--non-producing-node`. This mirrors the upstream cardano-node operator surface and disables the forge loop even if credential paths remain configured.

## Forge-loop trace events

The forge loop emits these per-slot trace events under the `Node.BlockProduction` namespace, mirroring upstream `forkBlockForging`:

| Event                              | Severity | Meaning |
|------------------------------------|----------|---------|
| `TraceStartLeadershipCheck`        | Debug    | Slot tick: VRF check starting. |
| `TraceNodeNotLeader`               | Debug    | VRF declined election for this slot. |
| `TraceSlotIsImmutable`             | Warning  | Local tip is at or past the current slot — node is lagging. |
| `TraceNodeIsLeader`                | Notice   | Won the slot. Block construction starts. |
| `TraceForgedBlock`                 | Info     | Block forged successfully. |
| `TraceForgedInvalidBlock`          | Critical | Self-validation rejected the locally forged block. **Investigate immediately**. |
| `TraceAdoptedBlock` / `TraceDidntAdoptBlock` | Info | ChainDB add result. |

Forge counts are surfaced via the trace events above; a dedicated Prometheus counter for minted blocks is not yet emitted. Until it lands, scrape `TraceForgedBlock` events from the trace stream (or count adopted blocks via `yggdrasil_blocks_synced` cross-referenced with the forge log).

## Operational rhythm

A pool operator's recurring tasks:

| Task                                  | Cadence       | Notes |
|---------------------------------------|---------------|-------|
| KES rotation                          | every 90 days | Generate new KES key, issue new OpCert, deploy, restart node. |
| OpCert counter increment              | every rotation| `cardano-cli node issue-op-cert` increments automatically. |
| Mainnet sync verification             | weekly        | `yggdrasil-node status` vs. explorer tip. |
| Pool registration parameters review   | per epoch     | Pledge, margin, fixed-cost, relay metadata. |
| Backup verification                   | monthly       | Restore-test the chain DB and key bundle (see [Maintenance]({{ "/manual/maintenance/" | relative_url }})). |
| Yggdrasil version upgrade             | as released   | Read release notes, deploy on relays first, then producer. |

## Multi-relay topology guidance

A robust deployment:

- **2 relays minimum.** Place them in different data centres if possible.
- **Block producer connects to BOTH relays as local-root, hotValency=2.**
- **Each relay connects to the other relay as local-root, hotValency=1.**
- **Each relay also connects to bootstrap peers and public roots** to cover network-side reachability.
- The producer's `useLedgerAfterSlot = -1` (off) — it should not discover random peers from the ledger.

## Switching from upstream Haskell node

If you are migrating from `cardano-node` (Haskell) to Yggdrasil:

1. Stop the Haskell node cleanly. Wait for any in-flight forge to complete.
2. Verify the on-disk database is at a recent slot.
3. Yggdrasil cannot reuse the Haskell ChainDB format directly — it has its own immutable+volatile format. Sync from genesis with Yggdrasil pointing at a fresh `--database-path`.
4. Once Yggdrasil is at tip, set up the same key files (Yggdrasil reads the same upstream key envelope formats unchanged).
5. Run both nodes in parallel for one or more KES periods. Compare `yggdrasil_current_slot`, `yggdrasil_current_block_number`, and forge events against the Haskell node.
6. Cut over by stopping the Haskell node.

The chain hashes are byte-identical between implementations, so you can hash-compare blocks at the same slot using [`node/scripts/compare_tip_to_haskell.sh`](https://github.com/yggdrasil-node/Cardano-node/blob/main/node/scripts/compare_tip_to_haskell.sh).

## Where to go next

- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — backups, KES rotation procedure, upgrades.
- [Monitoring]({{ "/manual/monitoring/" | relative_url }}) — alerting on forge-loop events.
- [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}) — common forge errors.
