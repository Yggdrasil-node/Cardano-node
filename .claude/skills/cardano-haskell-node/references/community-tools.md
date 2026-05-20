# Community Tools for Cardano SPOs

This file covers community-maintained tools that complement `cardano-cli`.
For raw source pages, see `references/sources/operator-tools-*.md` and
`references/sources/learn-air-gap.md`.

## Table of Contents
1. [Guild Operators Suite](#guild-operators-suite)
2. [CNCLI](#cncli)
3. [SPO Scripts (gitmachtl)](#spo-scripts)
4. [cardano-signer](#cardano-signer)
5. [Calidus Keys](#calidus-keys)
6. [Mithril](#mithril)
7. [Air-gap environments](#air-gap-environments)
8. [Mainnet key generation tools](#mainnet-key-generation-tools)
9. [Hardware wallet integration](#hardware-wallet-integration)
10. [Block-Notify](#block-notify)
11. [Explorers & dashboards](#explorers--dashboards)
12. [Community resources](#community-resources)

---

## Guild Operators Suite

Repo / docs: <https://cardano-community.github.io/guild-operators>

A community-maintained set of scripts that wrap common SPO chores.

### Installation
```bash
mkdir "$HOME/tmp" && cd "$HOME/tmp"
curl -sS -o guild-deploy.sh \
  https://raw.githubusercontent.com/cardano-community/guild-operators/master/scripts/cnode-helper-scripts/guild-deploy.sh
chmod 755 guild-deploy.sh
./guild-deploy.sh -b master -n mainnet -s pdl
. "$HOME/.bashrc"
```

`-s pdl`:
- **p**rerequisites (OS packages)
- **d**ownload precompiled binaries
- **l**ibsodium (IOG fork compile + install)

Other flags via `./guild-deploy.sh -h`.

### CNTools
Menu-driven Bash UI for pool operations. Wraps `cardano-cli` with guided
prompts for: wallet management, sending ADA / tokens, pool registration /
update / retirement, KES rotation, metadata management. Tracks wallets / pools
in `$CNODE_HOME/priv/`.

Run: `$CNODE_HOME/scripts/cntools.sh`

### gLiveView
Live terminal dashboard for one node. Autodetects relay vs. block producer.
Shows: epoch / slot / block / sync %, peer counts (hot/warm/cold), block
production, KES expiry, CPU / memory / disk.

Run: `$CNODE_HOME/scripts/gLiveView.sh`

### Topology Updater
Pre-P2P workaround that auto-pairs relays with community peers. Still useful as
a supplement to P2P bootstrap peers, especially on networks where ledger peer
discovery is sparse. Run via cron on each relay to fetch and apply the latest
peer set.

Docs: <https://cardano-community.github.io/guild-operators/Scripts/topologyupdater/>

### Koios
Decentralised public Cardano query API run by the Guild. REST endpoints for
blocks, transactions, pool info, governance, addresses. Free tier; community
mirrors. URL: <https://koios.rest>.

### Guild Network
60-minute-epoch testnet operated entirely by the community. Useful for very
fast iteration on pool changes without waiting on the upstream testnets.

---

## CNCLI

Repo: <https://github.com/cardano-community/cncli>

Rust utilities that extend `cardano-cli`.

Highlights:
- **Leader schedule prediction**:
  ```bash
  cncli leaderlog \
      --db cncli.db \
      --pool-id <POOL_ID_HEX> \
      --pool-vrf-skey /etc/cardano/vrf.skey \
      --byron-genesis /etc/cardano/byron-genesis.json \
      --shelley-genesis /etc/cardano/shelley-genesis.json \
      --ledger-state /tmp/ledger-state.json \
      --ledger-set current
  ```
- **sendtip** — pushes your tip to PoolTool for propagation visibility
- **sync / status** — chain-sync utilities

Install from GitHub releases or `cargo build --release`.

---

## SPO Scripts

Repo: <https://github.com/gitmachtl/scripts>

Step-by-step shell scripts by Martin Lang (ATADA pool). Highly readable —
each script is self-documenting and can be used as a tutorial. Covers:

- All key-pair generation (cold / KES / VRF / payment / stake)
- Pool registration / update / retirement
- Multi-owner pools, multi-witness signing
- KES rotation
- Hardware wallet integration
- Native token minting
- Metadata management
- Governance voting (DRep + SPO)
- Calidus key registration

Good for operators who want to **understand** each step rather than rely on a
menu wrapper.

---

## cardano-signer

Repo: <https://github.com/gitmachtl/cardano-signer>

Sign and verify arbitrary data using Cardano keys. Used for:
- **Calidus key registration and rotation** (CIP-88 / CIP-151)
- **CIP-8 / CIP-30 message signing** for dApps and governance platforms
- **Off-chain identity proofs**

```bash
# Generic data signing
cardano-signer sign \
    --data-hex <HEX> \
    --secret-key <KEY_FILE> \
    --out-file signature.json

# Verification
cardano-signer verify \
    --data-hex <HEX> \
    --public-key <VKEY_FILE> \
    --signature <SIGNATURE>

# Generate a Calidus key pair
cardano-signer keygen \
    --out-skey calidus.skey \
    --out-vkey calidus.vkey
```

---

## Calidus Keys

Authoritative source: `references/sources/operator-tools-calidus-keys.md`.

Calidus keys (Latin *calidus*, "hot") are Ed25519 hot keys SPOs register on-chain
to authenticate with explorers, governance tools, and dApps **without ever
touching the cold key again** after the one-time registration.

Defined in [CIP-88 v2](https://cips.cardano.org/cip/CIP-0088) and
[CIP-151](https://cips.cardano.org/cip/CIP-0151). Supported by Koios, Blockfrost,
CNTools, Cardanoscan, AdaStat, Cexplorer.

### Generate
```bash
cardano-signer keygen \
    --out-skey calidus.skey \
    --out-vkey calidus.vkey
```

### Register on-chain (one-time cold-key signature)
**Air-gapped machine:**
```bash
cardano-signer sign \
    --cip88 \
    --calidus-public-key calidus.vkey \
    --secret-key cold.skey \
    --out-file calidus-registration.json
```

Submit the resulting metadata in a transaction from the online machine.

### Update / revoke
The registration carries a **nonce**. A later registration with a higher nonce
supersedes the previous. To revoke: submit an all-zeroes key with a higher nonce.

### Operational considerations
`calidus.skey` is a hot key but represents your pool's online identity. Keep it
on a relatively secured host. If compromised, rotate by submitting a new
registration with a higher nonce.

---

## Mithril

A stake-based threshold multi-signature protocol for certifying snapshots of
the Cardano chain database. Enables fast node bootstrapping.

### Client (any node — fast bootstrap)
```bash
mithril-client cardano-db download latest \
    --download-dir /path/to/cardano-node/db
```
Reduces bootstrap from hours/days to minutes. Verifies the certificate chain
automatically.

### Signer (block producer — opt in to certification)
The signer holds your KES key and OpCert and contributes signatures every
~10 minutes. Run as a systemd service.

Environment file (`/opt/mithril/mithril-signer.env`):
```ini
NETWORK=mainnet
AGGREGATOR_ENDPOINT=https://aggregator.release-mainnet.api.mithril.network/aggregator
DB_DIRECTORY=/var/lib/cardano/db
CARDANO_NODE_SOCKET_PATH=/run/cardano/node.socket
CARDANO_CLI_PATH=/usr/local/bin/cardano-cli
DATA_STORES_DIRECTORY=/opt/mithril/stores
STORE_RETENTION_LIMIT=5
KES_SECRET_KEY_PATH=/etc/cardano/kes.skey
OPERATIONAL_CERTIFICATE_PATH=/etc/cardano/node.cert
RELAY_ENDPOINT=http://<mithril-relay-internal-ip>:3132
```

Resource use: <5% CPU, <200 MB RAM steady state. First launch runs a ~5-hour
pre-loading phase with higher CPU.

### Mithril relay (required on mainnet for signing)
A **Squid forward proxy** on a Cardano relay machine. The signer reaches it
from the BP's internal network; the relay reaches the Mithril aggregator
externally. The block producer is never directly exposed to the public internet.

Key Squid config points:
- Listen on `3132`
- Allow only the BP's internal IP as source
- Allow only HTTPS to `*.mithril.network` as destination
- Strip request headers (no info leakage about the BP)
- Caching disabled

Full guide: <https://mithril.network/doc/manual/operate/run-signer-node>

---

## Air-gap environments

Source: `references/sources/learn-air-gap.md`.

### cardano-airgap (recommended)
Repo: <https://github.com/IntersectMBO/cardano-airgap>

Nix-built bootable ISO maintained by IntersectMBO. Deterministic build, never
makes a network request. Used by SPOs and Constitutional Committee members.
Ships with cardano-cli, cardano-addresses, cardano-signer, and supporting tools
preinstalled.

### Frankenwallet
Encrypted bootable USB approach. Lower-friction setup but you maintain it
yourself. Suitable when you can't use the cardano-airgap ISO directly.

### Manual Ubuntu air-gap
For full control, build your own Ubuntu air-gapped machine. Install
cardano-cli + cardano-addresses (or hardware wallet support), then disconnect
the network adapter permanently. Use encrypted USB sticks (LUKS / VeraCrypt /
GPG-encrypted 7z archives) for transfers.

---

## Mainnet key generation tools

For mainnet payment / stake key generation, the docs explicitly recommend
**not** using raw `cardano-cli` on a live host. Use one of:

### cardano-addresses (offline mnemonic)
Repo: <https://github.com/IntersectMBO/cardano-addresses>

Derives all keys hierarchically from a BIP-39 mnemonic. The mnemonic is the
master backup — encrypt it (GPG, age) and store in ≥2 physical locations.

Typical flow on the air-gapped machine:
```bash
# Generate mnemonic
cardano-address recovery-phrase generate --size 24 > phrase.txt
# Encrypt immediately
gpg --symmetric phrase.txt && shred -u phrase.txt

# Derive keys (later, on-demand)
gpg --decrypt phrase.txt.gpg | \
  cardano-address key from-recovery-phrase Shelley > root.xsk
# Derive payment/stake keys from root.xsk via the derivation paths
```

### Hardware wallet
See [Hardware wallet integration](#hardware-wallet-integration) below.

---

## Hardware wallet integration

Repo: <https://github.com/vacuumlabs/cardano-hw-cli>

`cardano-hw-cli` wraps cardano-cli for Ledger and Trezor devices. Keys never
leave the device. Useful for:
- Pool owner stake keys (sign delegation, withdrawals, registration)
- Payment keys for the pool's operating account
- Voting (DRep delegations, governance votes by the owner)

**Not supported:** cold keys for pool registration cannot live on a hardware
wallet (the device's Cardano app does not currently sign pool registration
certificates as cold key). Use cardano-airgap + cardano-cli for the cold key.

---

## Block-Notify

Repo: <https://github.com/THE-Cardano-PoolPM/Block-Notify>

Pushes block mint events and "next block scheduled" reminders to:
- Telegram, Discord, Slack
- Email
- Webhooks

Useful for offline visibility into block production without staring at a
dashboard.

---

## Explorers & dashboards

- **[PoolTool](https://pooltool.io)** — pool performance, block production,
  network propagation
- **[Cardanoscan](https://cardanoscan.io)** — transaction and pool explorer;
  governance actions
- **[Cexplorer](https://cexplorer.io)** — pool explorer with API; SPO profiles
- **[AdaStat](https://adastat.net)** — pool statistics; governance dashboard
- **[GovTool](https://gov.tools)** — Conway-era governance interface; DRep
  delegation; proposal browsing

---

## Community resources

### Support / help
- **SPO Telegram** — <https://t.me/CardanoStakePoolWorkgroup> — high-traffic
  workgroup; search past answers
- **Cardano Forum (SPO category)** —
  <https://forum.cardano.org/c/staking-delegation/156>
- **Cardano Stack Exchange** — <https://cardano.stackexchange.com>
- **Koios Discussions Telegram** — <https://t.me/CardanoKoios/1>

### Guides
- **CoinCashew "How to Set Up a Cardano Stake Pool"** —
  <https://www.coincashew.com/coins/overview-ada/guide-how-to-build-a-haskell-stakepool-node>
  — comprehensive step-by-step; frequently updated for current cardano-node
- **Guild Operators docs** — <https://cardano-community.github.io/guild-operators>
- **Cardano Node Course (IOG)** — video course for SPOs and CLI users
- **Japanese SPO Guild Guide** — full Japanese setup manual
- **TOPO Guide** — Spanish-language pool setup
