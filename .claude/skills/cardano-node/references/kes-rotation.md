# KES Key Rotation

KES (Key Evolving Signature) keys are time-limited hot keys used by the block
producer to sign blocks. They expire after `maxKESEvolutions` periods. Missing
rotation = node silently stops minting.

Mainnet: ~90 days validity. Pre-Production: ~62 days. Preview: hours.

## Operational modes

### Standard mode
`kes.skey` lives on disk. Simplest setup. Rotation requires copying a new
signing key onto the block producer.

### KES Agent mode (Node 10.7.1+, recommended)
[KES Agent](https://github.com/input-output-hk/kes-agent) keeps the signing key
in mlocked RAM. The agent evolves the key at each period boundary and wipes the
previous evolution — forward secrecy against later host compromise.

Required hardening for forward secrecy:
- `swapoff -a` and remove the swap entry from `/etc/fstab`
- Disable hibernation
- Disable core dumps (`/etc/security/limits.conf`: `* hard core 0`, plus
  `fs.suid_dumpable = 0` in `/etc/sysctl.conf`)

---

## Understanding KES timing

Key parameters from `shelley-genesis.json`:
- `slotsPerKESPeriod` — slots in one KES period (mainnet: 129600 = 1.5 days)
- `maxKESEvolutions` — periods a key is valid (mainnet: 62)
- Total validity: 129600 × 62 = ~8,035,200 slots ≈ 93 days

### Check current KES status

On the online node:
```bash
slotsPerKESPeriod=$(jq -r '.slotsPerKESPeriod' shelley-genesis.json)
maxKESEvolutions=$(jq -r '.maxKESEvolutions' shelley-genesis.json)
currentSlot=$(cardano-cli conway query tip | jq -r '.slot')
currentKESPeriod=$((currentSlot / slotsPerKESPeriod))

echo "Current slot:       $currentSlot"
echo "Current KES period: $currentKESPeriod"
echo "Max KES evolutions: $maxKESEvolutions"
```

### Watch remaining KES from cardano-tracer's Prometheus endpoint
```bash
curl -s http://<tracer-host>:12789/<node-name>/metrics | grep remainingKESPeriods
```
Set a Grafana alert on `remainingKESPeriods < 15`.

### Compute expiry from the current OpCert
The OpCert's start KES period is embedded in the cert. With the cert in CBOR form:
```bash
cardano-cli node op-cert-info --file node.cert
```
Then `expiry = startKESPeriod + maxKESEvolutions` and
`remaining = expiry - currentKESPeriod`.

---

## Standard rotation procedure

### Step 1 — Generate a new KES key (air-gapped machine)

```bash
cardano-cli node key-gen-KES \
    --verification-key-file kes.vkey \
    --signing-key-file kes.skey
```

### Step 2 — Compute the current KES period on the online node
```bash
slotsPerKESPeriod=$(jq -r '.slotsPerKESPeriod' shelley-genesis.json)
currentSlot=$(cardano-cli conway query tip | jq -r '.slot')
kesPeriod=$((currentSlot / slotsPerKESPeriod))
echo "Current KES period: $kesPeriod"
```
Transfer this number to the air-gapped machine (write it down or USB).

### Step 3 — Issue the new OpCert (air-gapped machine)

```bash
cardano-cli node issue-op-cert \
    --kes-verification-key-file kes.vkey \
    --cold-signing-key-file cold.skey \
    --operational-certificate-issue-counter cold.counter \
    --kes-period <KES_PERIOD> \
    --out-file node.cert
```

Critical:
- `cold.counter` is mutated in-place — increments by 1
- Always use the latest counter file. A stale counter produces an invalid OpCert.

### Step 4 — Deploy to the block producer

Transfer `kes.skey` and `node.cert` to the block producer over encrypted USB.

```bash
# On the block producer
cp /etc/cardano/kes.skey       /etc/cardano/kes.skey.bak
cp /etc/cardano/node.cert      /etc/cardano/node.cert.bak

# Replace with new files (assuming they're staged in /tmp/new-kes/)
sudo install -m 0400 -o cardano /tmp/new-kes/kes.skey  /etc/cardano/kes.skey
sudo install -m 0444 -o cardano /tmp/new-kes/node.cert /etc/cardano/node.cert
```

### Step 5 — Reload (no full restart needed)

```bash
sudo pkill -HUP cardano-node
```

SIGHUP makes cardano-node re-read its KES key and OpCert without dropping peer
connections. A `systemctl restart cardano-node` also works but interrupts service.

### Step 6 — Verify
```bash
journalctl -u cardano-node -f
# Look for "Operational certificate is valid for X more KES periods"

curl -s http://<tracer>:12789/<node>/metrics | grep remainingKESPeriods
# Should be close to maxKESEvolutions (~62 on mainnet)
```

---

## KES Agent rotation procedure (Node 10.7.1+)

### Setup (one-time)

Install `kes-agent` and `kes-agent-control` from the
[releases page](https://github.com/input-output-hk/kes-agent/releases).

Copy `cold.vkey` (public, safe) from the air-gapped machine to the BP — the agent
needs it to validate OpCerts before activating new keys.

Run the agent (production: systemd unit from the kes-agent repo):
```bash
kes-agent run \
  --service-address       /run/kes-agent/service.socket \
  --control-address       /run/kes-agent/control.socket \
  --cold-verification-key /etc/cardano/cold.vkey \
  --genesis-file          /etc/cardano/shelley-genesis.json
```

In your cardano-node systemd unit, replace `--shelley-kes-key` with:
```
--shelley-kes-agent-socket /run/kes-agent/service.socket
```

### Rotation steps

1. **Generate a staged KES key in the agent (block producer):**
   ```bash
   kes-agent-control \
     --control-address /run/kes-agent/control.socket \
     gen-staged-key \
     --kes-verification-key-file kes.vkey
   ```
   Only `kes.vkey` (public) is written to disk. The signing key stays in mlocked RAM.

2. **Transfer `kes.vkey` to the air-gapped machine.**

3. **Compute the current KES period on the online node** (same as Step 2 above).

4. **Issue OpCert on the air-gapped machine** (same as Step 3 above).

5. **Transfer `node.cert` back to the block producer.**

6. **Install the cert** — the agent validates it against `cold.vkey` and activates
   the staged key automatically. No `pkill -HUP`, no restart. The signing key
   never touches disk at any point.

---

## Common mistakes

1. **Using a stale `cold.counter`.** Counter must be strictly increasing. If you
   have copies on multiple USB drives, use the one from the most recent issuance.
   After each rotation, the air-gapped machine's counter file is the canonical
   copy.

2. **Wrong KES period.** Using a period from the past produces a cert that's
   immediately expiring; from the future, a cert the chain rejects. Always
   compute it at issuance time.

3. **Forgetting to copy the updated counter back.** The counter file is mutated
   in place by `issue-op-cert`. The post-rotation version is what you'll need for
   the next rotation.

4. **Mode mismatch.** If you switch to KES Agent mode, the node's startup flag
   changes (`--shelley-kes-key` → `--shelley-kes-agent-socket`). Mixed config
   will fail to start.

5. **Waiting too long.** Rotate with ≥10 KES periods of buffer (~15 days on
   mainnet). Last-minute rotations leave no room for issues.

---

## Automation

- Calendar reminder every 80 days (mainnet) for buffer
- Grafana alert on `remainingKESPeriods < 15`
- CNCLI can monitor KES status and push notifications (Telegram/Discord/email)
- Some operators script the full rotation via SPO Scripts (gitmachtl); see
  `references/community-tools.md`
