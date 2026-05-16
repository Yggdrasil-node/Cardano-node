# cardano-cli Command Reference

Commands target cardano-cli v11+ (Conway era, post-Chang hard fork).
Confirm the user's version before adapting:
```bash
cardano-cli --version
```

**CLI prefix conventions (current docs):**

| Subcommand area | Prefix | Notes |
|---|---|---|
| Node key generation, OpCert | `cardano-cli node ...` | No era prefix |
| Pool keys, registration certs, metadata, pool id | `cardano-cli stake-pool ...` / `cardano-cli node ...` | No era prefix |
| Address/stake-address key-gen, address build | `cardano-cli address ...` / `cardano-cli stake-address ...` | No era prefix |
| Transactions (build/sign/submit), queries, governance | `cardano-cli conway ...` | Era prefix required |

Older v8/v9 docs use `cardano-cli ... --mary-era` or `--alonzo-era` style flags;
the current docs use the `cardano-cli conway` subcommand path instead.

## Table of Contents
1. [Environment Setup](#environment-setup)
2. [Wallet Key Generation](#wallet-key-generation)
3. [Stake Address Registration](#stake-address-registration)
4. [Pool Key Generation](#pool-key-generation)
5. [Operational Certificate](#operational-certificate)
6. [Pool Metadata](#pool-metadata)
7. [Pool Registration](#pool-registration)
8. [Pool Parameter Updates](#pool-parameter-updates)
9. [Pool Retirement](#pool-retirement)
10. [Reward Withdrawal](#reward-withdrawal)
11. [Governance Voting](#governance-voting)
12. [Useful Queries](#useful-queries)

---

## Environment Setup

```bash
export CARDANO_NODE_SOCKET_PATH=/run/cardano/node.socket
export CARDANO_NODE_NETWORK_ID=mainnet   # or 1 (preprod), 2 (preview)

# Common protocol parameters file used by many commands
cardano-cli conway query protocol-parameters --out-file protocol.json
```

---

## Wallet Key Generation

For **testnet**, the raw CLI is fine. For **mainnet**, prefer cardano-addresses with
a GPG-encrypted mnemonic on the air-gapped machine, or a hardware wallet via
cardano-hw-cli. See `references/community-tools.md`.

```bash
# Payment key pair
cardano-cli address key-gen \
    --verification-key-file payment.vkey \
    --signing-key-file payment.skey

# Stake key pair
cardano-cli stake-address key-gen \
    --verification-key-file stake.vkey \
    --signing-key-file stake.skey

# Stake address
cardano-cli stake-address build \
    --stake-verification-key-file stake.vkey \
    --out-file stake.addr

# Payment address (combined with stake key so funds accrue rewards)
cardano-cli address build \
    --payment-verification-key-file payment.vkey \
    --stake-verification-key-file stake.vkey \
    --out-file payment.addr
```

`cardano-cli` reads the network from `CARDANO_NODE_NETWORK_ID`. For commands that
explicitly need it as a flag, use `--mainnet` or `--testnet-magic <MAGIC>`:
- Preview: magic `2`
- Pre-Production: magic `1`

Fund the address (testnet faucet or transfer from another wallet), then:
```bash
cardano-cli conway query utxo --address $(cat payment.addr)
```

---

## Stake Address Registration

```bash
# Get the current key deposit amount
keyDeposit=$(jq -r '.stakeAddressDeposit' protocol.json)

# Create the registration certificate
cardano-cli conway stake-address registration-certificate \
    --stake-verification-key-file stake.vkey \
    --key-reg-deposit-amt $keyDeposit \
    --out-file stake-reg.cert

# Build, sign, submit
cardano-cli conway transaction build \
    --tx-in <UTXO_TXHASH>#<UTXO_IX> \
    --change-address $(cat payment.addr) \
    --certificate-file stake-reg.cert \
    --witness-override 2 \
    --out-file tx.raw

cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file stake.skey \
    --out-file tx.signed

cardano-cli conway transaction submit --tx-file tx.signed
```

---

## Pool Key Generation

**Air-gapped machine only.**

```bash
# Cold key pair + counter (single command)
cardano-cli node key-gen \
    --cold-verification-key-file cold.vkey \
    --cold-signing-key-file cold.skey \
    --operational-certificate-issue-counter cold.counter

# KES key pair
cardano-cli node key-gen-KES \
    --verification-key-file kes.vkey \
    --signing-key-file kes.skey

# VRF key pair
cardano-cli node key-gen-VRF \
    --verification-key-file vrf.vkey \
    --signing-key-file vrf.skey

# VRF skey must be readable only by owner or cardano-node will refuse to start
chmod 400 vrf.skey
```

---

## Operational Certificate

The OpCert binds the active KES key to the cold key for the current KES period.
Regenerate it on every KES rotation.

### Step 1 — Determine the current KES period (online node)
```bash
slotsPerKESPeriod=$(jq -r '.slotsPerKESPeriod' shelley-genesis.json)
currentSlot=$(cardano-cli conway query tip | jq -r '.slot')
kesPeriod=$(( currentSlot / slotsPerKESPeriod ))
echo "Current KES period: $kesPeriod"
```

### Step 2 — Issue the OpCert (air-gapped machine)

Transfer the `kesPeriod` value to the air-gapped machine, then:

```bash
cardano-cli node issue-op-cert \
    --kes-verification-key-file kes.vkey \
    --cold-signing-key-file cold.skey \
    --operational-certificate-issue-counter cold.counter \
    --kes-period <KES_PERIOD> \
    --out-file node.cert
```

The `cold.counter` file is incremented in-place by this command. Always use the
latest version next time you rotate.

Transfer `node.cert`, `vrf.skey`, and `kes.skey` (omit `kes.skey` if using the KES
Agent) to the block producer. The cold key and counter **stay on the air-gapped
machine**.

---

## Pool Metadata

```bash
cat > poolMetaData.json << 'EOF'
{
  "name": "Your Pool Name",
  "description": "Short description of your pool",
  "ticker": "TICK",
  "homepage": "https://yourpool.example.com"
}
EOF

cardano-cli stake-pool metadata-hash \
    --pool-metadata-file poolMetaData.json \
    --out-file poolMetaDataHash.txt

# Host poolMetaData.json at a public HTTPS URL (≤64 chars, no redirects)
# Verify the hosted file matches local hash
cardano-cli stake-pool metadata-hash \
    --pool-metadata-file <(curl -s -L https://YOUR_URL)
cat poolMetaDataHash.txt
# Both hashes must be identical
```

Rules:
- `ticker`: 3–9 characters, A–Z and 0–9 only
- `description`: 255 characters max
- `homepage`: your pool's website

---

## Pool Registration

**Generate the registration certificate on the air-gapped machine.**

```bash
minPoolCost=$(jq -r '.minPoolCost' protocol.json)
echo "Minimum pool cost: $minPoolCost lovelace"

cardano-cli stake-pool registration-certificate \
    --cold-verification-key-file cold.vkey \
    --vrf-verification-key-file vrf.vkey \
    --pool-pledge 10000000000 \
    --pool-cost 340000000 \
    --pool-margin 0.01 \
    --pool-reward-account-verification-key-file stake.vkey \
    --pool-owner-stake-verification-key-file stake.vkey \
    --single-host-pool-relay relay1.example.com \
    --pool-relay-port 3001 \
    --single-host-pool-relay relay2.example.com \
    --pool-relay-port 3001 \
    --metadata-url https://YOUR_URL \
    --metadata-hash $(cat poolMetaDataHash.txt) \
    --out-file pool.cert

# Delegation certificate (pledge owner's stake to own pool)
cardano-cli conway stake-address stake-delegation-certificate \
    --stake-verification-key-file stake.vkey \
    --cold-verification-key-file cold.vkey \
    --out-file deleg.cert
```

| Flag | Notes |
|---|---|
| `--pool-pledge` | Lovelace you commit to keep delegated. Higher → higher desirability. |
| `--pool-cost` | Fixed fee per epoch in lovelace. Must be ≥ `minPoolCost` (currently 170 ADA on mainnet). |
| `--pool-margin` | Variable fee as a fraction (`0.01` = 1%). |
| `--single-host-pool-relay` + `--pool-relay-port` | Add one pair per relay. For IP-based relays use `--pool-relay-ipv4`. |

**Submit on the online node:**

```bash
# Build (witness-override 3: payment + cold + stake)
cardano-cli conway transaction build \
    --tx-in <UTXO> \
    --change-address $(cat payment.addr) \
    --certificate-file pool.cert \
    --certificate-file deleg.cert \
    --witness-override 3 \
    --out-file tx.raw

# Sign with all three keys (the cold key signing must happen on the air-gapped
# machine, then bring the signed file back online)
cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file cold.skey \
    --signing-key-file stake.skey \
    --out-file tx.signed

cardano-cli conway transaction submit --tx-file tx.signed

# Verify
cardano-cli stake-pool id \
    --cold-verification-key-file cold.vkey \
    --output-format hex > stakepoolid.txt

cardano-cli conway query stake-snapshot \
    --stake-pool-id $(cat stakepoolid.txt)
```

Registration takes effect at the next epoch boundary.

---

## Pool Parameter Updates

To change pledge, cost, margin, relays, or metadata: generate a new pool
registration certificate with updated values and submit. No additional deposit.

```bash
# Same flag set as initial registration, with new values
cardano-cli stake-pool registration-certificate \
    --cold-verification-key-file cold.vkey \
    --vrf-verification-key-file vrf.vkey \
    --pool-pledge <NEW_PLEDGE> \
    --pool-cost <NEW_COST> \
    --pool-margin <NEW_MARGIN> \
    --pool-reward-account-verification-key-file stake.vkey \
    --pool-owner-stake-verification-key-file stake.vkey \
    --single-host-pool-relay relay1.example.com \
    --pool-relay-port 3001 \
    --metadata-url https://YOUR_URL \
    --metadata-hash $(cat poolMetaDataHash.txt) \
    --out-file pool-update.cert

# Submit in a transaction signed with payment + cold keys
```

---

## Pool Retirement

```bash
currentEpoch=$(cardano-cli conway query tip | jq -r '.epoch')

cardano-cli stake-pool deregistration-certificate \
    --cold-verification-key-file cold.vkey \
    --epoch $((currentEpoch + 2)) \
    --out-file pool-retire.cert

cardano-cli conway transaction build \
    --tx-in <UTXO> \
    --change-address $(cat payment.addr) \
    --certificate-file pool-retire.cert \
    --witness-override 2 \
    --out-file tx.raw

cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file cold.skey \
    --out-file tx.signed

cardano-cli conway transaction submit --tx-file tx.signed
```

The 500 ADA pool deposit is returned after the retirement epoch.

---

## Reward Withdrawal

```bash
cardano-cli conway query stake-address-info --address $(cat stake.addr)

cardano-cli conway transaction build \
    --tx-in <UTXO> \
    --withdrawal $(cat stake.addr)+0 \
    --change-address $(cat payment.addr) \
    --witness-override 2 \
    --out-file tx.raw

cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file stake.skey \
    --out-file tx.signed

cardano-cli conway transaction submit --tx-file tx.signed
```

`+0` means "withdraw the entire available reward balance."

---

## Governance Voting

SPOs vote on no-confidence, committee update, hard-fork initiation, and info
actions using the cold key. See `references/governance.md` for the full workflow.

```bash
# Find active proposals
cardano-cli conway query proposals --all-proposals

# Just the hard-fork proposals
cardano-cli conway query proposals --all-proposals \
  | jq '[.[] | select(.proposalProcedure.govAction.tag == "HardForkInitiation")]'

# Verify anchor document hash before voting
cardano-cli conway query proposals --all-proposals \
  | jq '.[] | {id: .actionId, url: .proposalProcedure.anchor.url, hash: .proposalProcedure.anchor.dataHash}'
wget <url> -O proposal.jsonld
b2sum -l 256 proposal.jsonld
# Hash must equal dataHash

# Create the vote file (online — only uses public cold.vkey)
cardano-cli conway governance vote create \
    --yes \
    --governance-action-tx-id "<TX_ID>" \
    --governance-action-index <IX> \
    --cold-verification-key-file cold.vkey \
    --out-file vote.cert

# Build the unsigned transaction
cardano-cli conway transaction build \
    --tx-in <UTXO> \
    --change-address $(cat payment.addr) \
    --vote-file vote.cert \
    --witness-override 2 \
    --out-file tx.raw

# Transfer tx.raw to the air-gapped machine to sign with cold.skey + payment.skey
cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file cold.skey \
    --out-file tx.signed

# Bring tx.signed back online and submit
cardano-cli conway transaction submit --tx-file tx.signed
```

---

## Useful Queries

```bash
# Current tip and sync status
cardano-cli conway query tip

# UTxO at an address
cardano-cli conway query utxo --address $(cat payment.addr)

# Stake address info (delegation + rewards)
cardano-cli conway query stake-address-info --address $(cat stake.addr)

# Pool parameters on-chain
cardano-cli conway query pool-params --stake-pool-id <POOL_ID_HEX>

# Stake snapshot for a pool
cardano-cli conway query stake-snapshot --stake-pool-id <POOL_ID_HEX>

# Protocol parameters
cardano-cli conway query protocol-parameters --out-file protocol.json

# Leadership schedule (requires VRF key on block producer)
cardano-cli conway query leadership-schedule \
    --cold-verification-key-file cold.vkey \
    --vrf-signing-key-file vrf.skey \
    --current

# Governance state (current proposals and vote tallies)
cardano-cli conway query gov-state
```
