# SPO Governance (Conway / CIP-1694)

Cardano's Conway era introduced on-chain governance via
[CIP-1694](https://cips.cardano.org/cip/CIP-1694). Three governance bodies share
decision-making power: **Constitutional Committee (CC)**, **Delegated
Representatives (DReps)**, and **Stake Pool Operators (SPOs)**. Each body votes
on a different subset of governance actions.

Authoritative source: `references/sources/governance-spo-governance.md`.

## What SPOs vote on

SPOs vote with their **cold verification key** and require **>51% of active
stake** to ratify an action (unless noted otherwise).

| Governance action | SPO threshold | Notes |
|---|---|---|
| Motion of no-confidence | 51% | Removes the current CC |
| Update committee / threshold | 51% | Adds, removes, or reweights CC members |
| Hard-fork initiation | 51% | Triggers a protocol upgrade |
| Info | 100% | Advisory only — no on-chain effect |

SPOs **do not** vote on protocol parameter changes, treasury withdrawals, or
constitutional amendments. Those need DRep + CC approval.

Hard-fork initiation is the most common action requiring SPO votes. Running the
upgraded node software is **also** required at the fork epoch, but it does not
substitute for the on-chain vote.

---

## Step 1 — Find active proposals

```bash
cardano-cli conway query proposals --all-proposals
```

Filter for hard-fork proposals only:
```bash
cardano-cli conway query proposals --all-proposals \
  | jq '[.[] | select(.proposalProcedure.govAction.tag == "HardForkInitiation")]'
```

Full governance state (includes vote tallies):
```bash
cardano-cli conway query gov-state
```

Browse-friendly: [GovTool](https://gov.tools),
[CardanoScan](https://cardanoscan.io/govActions),
[Adastat](https://adastat.net/governances), [CGOV](https://app.cgov.io/).

---

## Step 2 — Verify the anchor document

Every governance action carries an **anchor**: a URL pointing to a rationale
document plus the document's hash. Verify before voting:

```bash
# Extract anchor URL and hash from the proposal
cardano-cli conway query proposals --all-proposals \
  | jq '.[] | {id: .actionId, url: .proposalProcedure.anchor.url, hash: .proposalProcedure.anchor.dataHash}'

# Download and check the hash
wget <url> -O proposal.jsonld
b2sum -l 256 proposal.jsonld
# Output must equal the dataHash in the proposal
```

If the hashes don't match, **do not vote** — the on-chain action no longer
matches the document it links to.

---

## Cold-key safety

Your `cold.skey` is the most sensitive credential you hold. If compromised,
an attacker can re-register your pool to their reward address.

Hard rules:
- **Never on a live host.** Build the transaction online, sign it offline,
  submit the signed result.
- **Encrypted at rest.** LUKS, GPG, or age. Plaintext on an offline drive is
  still a single point of failure.
- **Backed up in ≥2 independent encrypted locations.**

### Recommended environment: cardano-airgap

[cardano-airgap](https://github.com/IntersectMBO/cardano-airgap) is a Nix-built
bootable ISO maintained by IntersectMBO. Ships pre-loaded with Cardano tooling.
Built deterministically and has never made a network request — not during build,
not during setup. Already the tool of choice for many SPOs and Constitutional
Committee members.

Alternatives covered in `references/sources/learn-air-gap.md`:
- Frankenwallet (encrypted bootable USB)
- Manually configured air-gapped Ubuntu machine

---

## Step 3 — Cast your vote

You need:
- `cold.vkey` (public, safe online) — to create the vote
- `cold.skey` (air-gapped only) — to sign the transaction
- A funded payment key — to cover the ~0.2 ADA transaction fee

### 3a. Online — create the vote file
```bash
cardano-cli conway governance vote create \
    --yes \
    --governance-action-tx-id "<TX_ID>" \
    --governance-action-index <IX> \
    --cold-verification-key-file cold.vkey \
    --out-file vote.cert
```

Use `--no` to vote against, `--abstain` to abstain. The vote can optionally
include an anchor pointing at your own rationale document.

### 3b. Online — build the unsigned transaction
```bash
cardano-cli conway transaction build \
    --tx-in <UTXO> \
    --change-address $(cat payment.addr) \
    --vote-file vote.cert \
    --witness-override 2 \
    --out-file tx.raw
```

### 3c. Transfer `tx.raw` to the air-gapped machine via USB.

### 3d. Air-gapped — sign
```bash
cardano-cli conway transaction sign \
    --tx-body-file tx.raw \
    --signing-key-file payment.skey \
    --signing-key-file cold.skey \
    --out-file tx.signed
```

### 3e. Transfer `tx.signed` back to the online machine.

### 3f. Online — submit
```bash
cardano-cli conway transaction submit --tx-file tx.signed
```

### 3g. Verify
```bash
cardano-cli conway query gov-state \
  | jq '.proposals[] | select(.actionId.txId == "<TX_ID>")'
```

Your pool's vote should appear in the tally.

---

## Identity layer: Calidus keys

For non-voting identity needs — governance tools, explorers, dApps — register a
**Calidus key** (CIP-88 v2, CIP-151). A Calidus key is an Ed25519 hot key
registered on-chain via a one-time cold-key signature. After registration:

- Governance platforms verify SPO identity without asking for cold-key signatures
- Explorers (Cardanoscan, Cexplorer, AdaStat) can update profile information
- APIs (Koios, Blockfrost) authenticate the pool

See `references/community-tools.md` for the cardano-signer commands to generate
and register a Calidus key.

---

## Updating or revoking a Calidus key

The on-chain registration carries a **nonce**. A later registration with a
higher nonce supersedes the previous one. To revoke, register an all-zeroes key
with a higher nonce.

---

## Further reading
- [CIP-1694: Governance](https://cips.cardano.org/cip/CIP-1694)
- [CIP-88 v2: SPO On-Chain Registration](https://cips.cardano.org/cip/CIP-0088)
- [CIP-151: Calidus Keys for Stake Pools](https://cips.cardano.org/cip/CIP-0151)
- [Cardano Governance Actions insight page](https://cardano.org/insights/governance-actions/)
