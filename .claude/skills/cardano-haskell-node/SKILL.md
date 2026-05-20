---
name: cardano-haskell-node
description: >
  Full-lifecycle guide for operating an upstream Haskell Cardano stake pool with cardano-node and
  cardano-cli: hardware, P2P topology with Praos bootstrap peers, air-gapped
  cold-key handling (cardano-airgap), KES/VRF/OpCert generation and rotation,
  the KES Agent for forward secrecy, pool registration and retirement, the new
  tracing system (cardano-tracer + Prometheus + Grafana), server hardening
  (SSH, UFW, fail2ban, sysctl, swap/core-dump disabling), Mithril signer +
  relay, Calidus keys (CIP-88/CIP-151), and Conway-era SPO governance voting.
  Also covers Guild Operators (CNTools, gLiveView, Topology Updater, Koios),
  CNCLI, SPO Scripts, cardano-signer, cardano-hw-cli, cardano-addresses. Use
  for upstream Haskell cardano-node operations and stake-pool administration:
  stake pools, block producers, relays, KES rotation, pool registration or
  retirement, cardano-tracer, Grafana for Cardano, SPO hardening, Mithril,
  Calidus, CNTools, gLiveView, CNCLI, Topology Updater, cardano-airgap, SPO
  governance, or hard-fork voting. Do not use this as the primary guide for
  Yggdrasil Rust implementation, file-mirror extraction, crate restructuring,
  or code-level parity work; use the repo parity skills and agents for those.
---

# Cardano Node & Stake Pool Operations

This skill is a current operator reference for upstream Haskell `cardano-node`
stake pools on mainnet or testnet. It tracks the official Cardano Developer
Portal documentation under `/docs/operate-a-stake-pool/` and bundles community
best practices.

Raw markdown snapshots of every source page live under `references/sources/`. The
skill-local `.claude/skills/cardano-haskell-node/scripts/sync_docs.sh` helper
re-fetches them and reports diffs so this skill can be updated whenever the
docs change.

## Quick-reference: when to read which file

| User is asking about | Read this reference file |
|---|---|
| Exact `cardano-cli` commands (keys, certs, transactions, voting) | `references/cli-commands.md` |
| KES expiry, rotation, KES Agent (mlocked RAM, forward secrecy) | `references/kes-rotation.md` |
| Prometheus/Grafana setup with the new tracing system | `references/monitoring.md` |
| SSH, UFW, fail2ban, sysctl, swap/hibernation/core dump hardening | `references/hardening.md` |
| SPO governance — what to vote on, thresholds, voting workflow | `references/governance.md` |
| CNTools, gLiveView, Topology Updater, CNCLI, Mithril, Calidus, cardano-airgap | `references/community-tools.md` |
| Raw source markdown for any page on the Cardano dev portal | `references/sources/<slug>.md` |

To check whether the skill is current with the docs from the repository root,
run `.claude/skills/cardano-haskell-node/scripts/sync_docs.sh`.

---

## 1. Architecture

A minimal stake pool has three live nodes and an offline machine:

```
Internet
  │
  ├── Relay 1  (public IP, accepts peer connections)
  ├── Relay 2  (public IP, geographic redundancy)
  │
  └── Block Producer  (NO public IP — only connects to own relays)

Air-gapped machine  (never online — cold-key signing only)
```

The block producer mints blocks using hot keys (KES, VRF) plus its operational
certificate. The **cold key** authorizes pool registration, parameter updates, KES
rotation, retirement, and governance votes — it must never touch a networked host.

For Mithril signing, add a **Mithril relay** (Squid forward proxy on port 3132) on
one of your Cardano relay machines so the signer can reach the Mithril aggregator
without exposing the block producer.

### Key types at a glance

| Key | Hot/Cold | Lives on | Purpose |
|---|---|---|---|
| `cold.skey` / `cold.vkey` | Cold | Air-gapped only | Pool identity, registration, KES rotation, voting |
| `cold.counter` | Cold | Air-gapped only | Monotonic OpCert issuance counter |
| `kes.skey` / `kes.vkey` | Hot | Block producer (or KES Agent RAM) | Block signing; expires ~90 days |
| `vrf.skey` / `vrf.vkey` | Hot | Block producer | Slot leader election |
| `node.cert` (OpCert) | Hot | Block producer | Binds active KES key to cold key |
| `payment.skey` / `payment.vkey` | Cold | Air-gapped | Controls pool funds |
| `stake.skey` / `stake.vkey` | Cold | Air-gapped | Delegation + reward withdrawal |
| `calidus.skey` / `calidus.vkey` | Hot | Online host (sensitive) | SPO identity for governance/dApps/explorers |

### Hard safety rules

- `cold.skey` and `payment.skey` **never** touch an internet-connected machine.
- Cold-key signing happens on the air-gapped machine; you transfer the signed
  transaction back via USB. For mainnet, [cardano-airgap](https://github.com/IntersectMBO/cardano-airgap)
  is the recommended bootable ISO (built deterministically, never connects to a network).
- Always use the latest `cold.counter` when issuing a new OpCert — a stale counter
  produces a certificate the chain will reject.
- KES keys expire. Set calendar reminders and Prometheus alerts.

---

## 2. Hardware Requirements

### Mainnet (per node)
- **CPU:** Intel or AMD x86, 2+ cores, 2 GHz+
- **RAM:** 24 GB
- **Storage:** 150 GB free (250 GB recommended for growth)
- **OS:** 64-bit Linux (Ubuntu 22.04+ / Debian 12+ recommended)
- **Network:** ~1 GB/hour, public IPv4, stable uptime
- **Server count:** 1 block producer + ≥2 relays
- **Air-gapped machine:** required

### Testnet (Preview / Pre-Production)
- RAM: 4 GB, Storage: 20 GB
- Air-gapped machine not strictly required (but good practice)
- Always test on Pre-Production before mainnet
- Pool registration costs a 500 ADA deposit on mainnet (returned on retirement)

---

## 3. Installation

Preferred path is Nix:

```bash
git clone https://github.com/IntersectMBO/cardano-node
cd cardano-node
git tag | sort -V          # find latest release
git switch -d tags/<TAG>
nix build .#cardano-node
nix build .#cardano-cli
```

Or without cloning: `nix build github:IntersectMBO/cardano-node/<TAG>`

Set up the IOG binary cache to avoid building everything from scratch. See the IOGX
template documentation.

Cabal builds are also supported. Read the source snapshot at
`references/sources/installing-cardano-node.md` for current dependency versions
(GHC, libsodium, libsecp256k1, libblst).

Verify after building:
```bash
cardano-node --version
cardano-cli --version
```

---

## 4. Configuration

Each node needs:
- `config.json` — main config; logging, versioning, genesis file paths
- Genesis files — Byron, Shelley, Alonzo, Conway
- `topology.json` — peer configuration (Praos mode with bootstrap peers)

### Fetching network configs (mainnet example)

```bash
NETWORK=mainnet
BASE=https://book.play.dev.cardano.org/environments/$NETWORK
for f in config.json topology.json byron-genesis.json shelley-genesis.json \
         alonzo-genesis.json conway-genesis.json; do
  curl -O "$BASE/$f"
done
```

For Pre-Production substitute `preprod`. For Preview, `preview`.

### Relay topology (Praos / P2P)

Relays should connect outward to your block producer (private), bootstrap peers
(trusted seeds maintained by founding orgs), and the wider network via ledger peers
after sync.

```json
{
  "localRoots": [
    {
      "accessPoints": [
        { "address": "YOUR-BLOCK-PRODUCER-IP", "port": 6000 }
      ],
      "advertise": false,
      "hotValency": 1,
      "warmValency": 2,
      "trustable": false
    }
  ],
  "bootstrapPeers": [
    { "address": "backbone.cardano.iog.io",               "port": 3001 },
    { "address": "backbone.mainnet.emurgornd.com",         "port": 3001 },
    { "address": "backbone.mainnet.cardanofoundation.org", "port": 3001 }
  ],
  "useLedgerAfterSlot": 128908821
}
```

Critical points:
- **`advertise: false`** on the block producer entry — its address must never be
  advertised to the network.
- **`useLedgerAfterSlot`** is a specific slot number, taken from the official
  `topology.json` for your network. Do not set it to `-1` on a relay.
- For testnet, use the bootstrap peers and slot value from the corresponding
  `topology.json` (preprod/preview).

### Block producer topology

The block producer connects only to your relays. Use `localRoots` listing each
relay; do not include public bootstrap peers.

### Mithril bootstrap (recommended)

Instead of syncing from genesis (hours/days), use Mithril to download a certified
snapshot of the chain database:

```bash
mithril-client cardano-db download latest \
    --download-dir /path/to/cardano-node/db
```

The client verifies the certificate chain automatically. Once finished, start
cardano-node normally and it resumes from the snapshot state. Full details in the
[Mithril documentation](https://mithril.network/doc/manual/getting-started/bootstrap-cardano-node).

---

## 5. Running the Node

```bash
cardano-node run \
    --topology topology.json \
    --database-path db \
    --socket-path node.socket \
    --config config.json \
    --port 3001
```

Set the CLI socket and network ID once per shell:
```bash
export CARDANO_NODE_SOCKET_PATH=/path/to/node.socket
# Optional, used by some commands:
export CARDANO_NODE_NETWORK_ID=mainnet      # or 1 (preprod), 2 (preview)
```

Verify sync:
```bash
cardano-cli query tip
```

The node is synced when `syncProgress` is `100.00`.

### Systemd unit

```ini
[Unit]
Description=Cardano Node
After=network-online.target
Wants=network-online.target

[Service]
User=cardano
Type=simple
WorkingDirectory=/home/cardano
ExecStart=/usr/local/bin/cardano-node run \
    --topology /home/cardano/topology.json \
    --database-path /home/cardano/db \
    --socket-path /run/cardano/node.socket \
    --config /home/cardano/config.json \
    --port 3001
Restart=on-failure
RestartSec=10
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now cardano-node
journalctl -u cardano-node -f
```

---

## 6. Pool Registration

The full flow lives in `references/cli-commands.md`. Summary:

1. **Generate wallet keys** (payment, stake) — see version-of-the-day notes below
2. **Register stake address** — submit a stake registration certificate
3. **Generate pool keys** (cold, KES, VRF) on the air-gapped machine
4. **Issue OpCert** on the air-gapped machine using cold key + KES vkey + current KES period
5. **Create + host pool metadata** (HTTPS, ≤64 char URL, no redirects) and hash it
6. **Create pool registration certificate** (pledge, cost, margin, relays, metadata) — air-gapped
7. **Create delegation certificate** (delegates owner's stake to own pool) — air-gapped
8. **Build, sign (payment + cold + stake), submit** the transaction containing both certs
9. **Verify** with `cardano-cli stake-pool id` and `cardano-cli query stake-snapshot`

Pool deposit is 500 ADA (mainnet) plus the 2 ADA stake key deposit, both refundable.

### Mainnet key generation — important

The raw `cardano-cli` commands shown in the docs are appropriate for testnet. For
mainnet, **do not generate payment/stake keys on an internet-connected machine**.
Production options:

- **[cardano-addresses](https://github.com/IntersectMBO/cardano-addresses)** with a
  GPG-encrypted mnemonic on the air-gapped machine — fully offline, recoverable.
- **Hardware wallet** (Ledger / Trezor via
  [cardano-hw-cli](https://github.com/vacuumlabs/cardano-hw-cli)) — keys never
  leave the device.

For cold/KES/VRF keys, always generate on the air-gapped machine using `cardano-cli`.

---

## 7. KES Keys: Standard vs. Agent

KES (Key Evolving Signature) keys expire after `maxKESEvolutions` periods
(~90 days mainnet, ~62 days preprod). Two operational modes:

### Standard mode
`kes.skey` lives on disk in the block producer. Simpler to set up; the signing key
exists in persistent storage. This is the default and what most existing operators use.

### KES Agent (Node 10.7.1+, recommended for new mainnet deployments)
The [KES Agent](https://github.com/input-output-hk/kes-agent) holds the signing key
in mlocked RAM and never writes it to disk. When the key evolves at the start of
each KES period, the previous evolution is wiped from memory. This gives **forward
secrecy**: a later host compromise cannot recover past signing keys.

For the forward-secrecy guarantee to hold, the block producer host **must** disable:
- swap (`swapoff -a` plus removing the swap entry from `/etc/fstab`)
- hibernation
- core dumps

Node configuration uses `--shelley-kes-agent-socket` instead of `--shelley-kes-key`.
Full setup in `references/kes-rotation.md` and `references/sources/block-producer-kes-agent.md`.

### Rotation
Both modes follow the same logical flow: generate a new KES key, fetch the current
KES period, issue a new OpCert on the air-gapped machine, deploy the new credentials.
With the agent, no node restart is needed — the agent activates the new key on
receipt of the cert. Without the agent, send `SIGHUP` to reload credentials without
a full restart: `pkill -HUP cardano-node`.

Full procedure in `references/kes-rotation.md`.

---

## 8. Monitoring (New Tracing System)

`cardano-node` 10.2+ ships with a new tracing system. The legacy direct
EKG/Prometheus endpoint on port 12798 is **no longer supported**.

### Modern stack

```
relays + block producer ──► cardano-tracer ──► Prometheus ──► Grafana
                                │
                                exposes /metrics per node
```

`cardano-tracer` is a separate process that aggregates traces and metrics from one
or more nodes over Unix sockets, and exposes a single Prometheus scrape endpoint.

### Quick checklist
1. Enable Forwarder + EKGBackend in each node's `config.json`
2. Set a unique `TraceOptionNodeName` per node
3. Add `--tracer-socket-path-connect /run/cardano/tracer.sock` to each node startup
4. Run `cardano-tracer` on a monitoring host with a `tracer-config.json`
5. Point Prometheus at the tracer's `hasPrometheus` endpoint
6. Import a Cardano dashboard in Grafana
7. Set alerts on `cardano_node_metrics_remainingKESPeriods_int < 15`,
   peer floor, disk usage, sync lag

Full configuration in `references/monitoring.md`. Key per-node metrics include:
slot number, block height, epoch, mempool bytes, **remaining KES periods**,
peer counts (hot/warm/cold), and blocks served.

### Real-time CLI: gLiveView
For quick checks over SSH, [gLiveView](https://cardano-community.github.io/guild-operators/Scripts/gliveview/)
from the Guild Operators suite gives a live terminal dashboard. No alerting, no
history — but excellent for "is the node OK right now" inspection.

### Network-level visibility: openBlockPerf
[openBlockPerf](https://github.com/cardano-foundation/openblockperf) collects block
propagation timing across participating relays — voluntary, gives operators
insight into how their blocks reach the rest of the network.

---

## 9. Security & Hardening

A compromised relay or BP can mean missed blocks at best, lost funds at worst.
Apply the full 10-step hardening checklist in `references/hardening.md` to **every**
host (relays, block producer, monitoring node):

1. Non-root user for the node
2. Disable root login
3. Keep the system patched (`unattended-upgrades`)
4. SSH keys (ed25519), disable password auth
5. Change default SSH port; harden `sshd_config`
6. UFW firewall — relay: SSH + node port public; BP: SSH + node port only from
   relay IPs; monitoring ports only from monitoring host
7. fail2ban (aggressive mode on `[sshd]`)
8. sysctl hardening (syncookies, no redirects, no source routing, no forwarding)
9. Shared memory hardening (`/run/shm` tmpfs noexec)
10. Audit regularly (see the Cardano dev portal's "Audit your node" checklist)

For KES Agent operators, additionally:
- Disable swap, hibernation, and core dumps on the block producer
- Encrypt the LUKS volume holding any persisted keys
- Use the [KES Agent hardening guide](https://github.com/input-output-hk/kes-agent/blob/main/doc/guide.markdown)

For Grafana: read `references/sources/deployment-improve-grafana-security.md` for
the official hardening notes (reverse proxy, basic auth disable, OAuth, etc.).

---

## 10. Pool Updates, Retirement, Maintenance

### Parameter updates
To change pledge, cost, margin, relays, or metadata, build a **new pool
registration certificate** with updated values and submit it. It's an update, not
a re-registration — no extra deposit. Commands in `references/cli-commands.md`.

### Retirement
Submit a deregistration certificate specifying the target epoch (typically
`currentEpoch + 2`). After the retirement epoch passes, the 500 ADA pool deposit
is returned. Until then, the pool keeps minting normally.

### Node upgrades
On a new `cardano-node` release:
1. Check release notes for breaking changes or hard-fork requirements
2. Build/download the new binary
3. Stop the node, replace the binary, restart
4. Verify `cardano-node --version` and `cardano-cli query tip`
5. For hard-fork events, ensure your config and genesis files are current

### Backups
Encrypted backups (LUKS, GPG, age) of: `cold.skey`, `cold.counter`, `vrf.skey`,
payment/stake keys, all metadata files, OpCert. Keep copies in ≥2 independent
locations. Test restores periodically.

---

## 11. Governance (Conway / CIP-1694)

SPOs are one of three governance bodies (with the Constitutional Committee and
DReps). What SPOs vote on, and the thresholds:

| Action | SPO threshold | Notes |
|---|---|---|
| Motion of no-confidence | 51% | Removes the current CC |
| Update committee/threshold | 51% | Adds, removes, or reweights CC members |
| Hard-fork initiation | 51% | Triggers a protocol upgrade |
| Info | 100% | Advisory only |

SPOs **do not** vote on protocol parameter changes, treasury withdrawals, or
constitutional amendments — those need DRep and CC approval.

Voting workflow (full commands in `references/governance.md`):
1. Find proposals: `cardano-cli conway query proposals --all-proposals`
2. Verify the anchor document hash (`b2sum -l 256 proposal.jsonld`)
3. Online: create the vote file using `cold.vkey` (public, safe)
4. Online: build the unsigned transaction
5. Transfer to air-gapped machine via USB
6. Air-gapped: sign with `cold.skey` (and payment key)
7. Transfer signed tx back
8. Online: submit

For SPO identity verification on explorers, governance tools, and APIs, register
a **Calidus key** (CIP-88 v2 / CIP-151) — a hot identity key registered with a
one-time cold-key signature. After that, no further cold-key exposure is needed
for routine identity proofs.

---

## 12. Community Tools

Detail in `references/community-tools.md`. The highlights:

- **Guild Operators suite** — `guild-deploy.sh` for setup, **CNTools** (menu-driven
  pool management), **gLiveView** (live terminal dashboard), **Topology Updater**
  (legacy peer-pairing, useful pre-P2P), **Koios** (decentralised query API)
- **CNCLI** — Rust utility for leader-schedule prediction, block validation, sendtip
- **SPO Scripts (gitmachtl)** — heavily commented step-by-step shell scripts for
  every pool operation, plus the [cardano-signer](https://github.com/gitmachtl/cardano-signer)
  used for Calidus and CIP-8 message signing
- **Mithril signer / Mithril relay** — participate in chain snapshot certification
- **cardano-airgap** — Nix-built bootable ISO; the recommended air-gapped environment
- **cardano-addresses** — hierarchical key derivation from mnemonic (mainnet keygen)
- **cardano-hw-cli** — Ledger / Trezor integration
- **PoolTool, Cardanoscan, Cexplorer, AdaStat, GovTool** — explorers and dashboards

---

## Response Guidelines

1. **Disambiguate mainnet vs testnet first.** Commands, deposits, faucet usage, and
   safety practices differ. For mainnet, default to maximum caution about keys.

2. **Default to air-gapped key generation on mainnet.** If the user is on mainnet and
   asks for a raw `cardano-cli address key-gen` on a live host, push back and
   recommend cardano-airgap + cardano-addresses or a hardware wallet.

3. **Be precise with lovelace.** 1 ADA = 1,000,000 lovelace. Always include units.

4. **Check cardano-node version.** Command syntax changes across versions. The
   modern docs use `cardano-cli node ...` and `cardano-cli stake-pool ...` without
   era prefix; for transactions, governance, and queries that need era awareness,
   use `cardano-cli conway ...`. KES Agent requires Node 10.7.1+. The new tracing
   system requires Node 10.2+.

5. **Warn about timing.** Registration and delegation take effect at epoch
   boundaries (5 days mainnet, 1 day preprod, 24 min preview). KES rotation is
   time-critical — rotate with ≥10 KES periods of buffer.

6. **Generate runnable commands when asked.** Read the appropriate reference file
   and adapt to the user's specific setup (paths, network, version).

7. **Cite the source.** For any specific claim about parameters, deposits, or
   commands, the authoritative source is the snapshot in `references/sources/`.
   When the user asks for a verbatim quote or the exact current value, point them
   to the corresponding source file.

8. **If something feels out of date, run the skill-local sync script.**
   `.claude/skills/cardano-haskell-node/scripts/sync_docs.sh` re-fetches every
   tracked page and reports diffs. The user can then refresh the skill content
   from the updated snapshots.
