---
title: CLI Reference
layout: default
parent: User Manual
nav_order: 9
---

# CLI Reference

This is the authoritative reference for the `yggdrasil-node` binary. Every subcommand and flag is listed.

## Synopsis

```
yggdrasil-node <subcommand> [flags]
```

Subcommands:

| Subcommand          | Purpose                                          |
|---------------------|--------------------------------------------------|
| `run`               | Connect, sync, serve peers, optionally produce blocks. |
| `validate-config`   | Operator preflight — config + storage + credentials sanity. |
| `status`            | Inspect on-disk database and report sync state. |
| `default-config`    | Emit the default JSON config to stdout.         |
| `cardano-cli`       | Pure-Rust subset of upstream `cardano-cli`.     |
| `query`             | NtC LocalStateQuery dispatcher (Unix only).     |
| `submit-tx`         | NtC LocalTxSubmission (Unix only).              |

Global flags (apply to most subcommands):

```
--config <path>          Path to JSON/YAML config file (overrides preset).
--network <preset>       mainnet | preprod | preview.
--database-path <path>   Override storage_dir.
```

---

## `run`

Start a long-running node.

### Synopsis

```
yggdrasil-node run [flags]
```

### Flags

| Flag                                      | Type    | Default       | Description |
|-------------------------------------------|---------|---------------|-------------|
| `--config <path>`                         | path    | none          | Override config file. |
| `--network <preset>`                      | string  | none          | Select network preset. |
| `--peer <host:port>`                      | string  | from config   | Single explicit peer; overrides `peer_addr`. |
| `--network-magic <u32>`                   | u32     | from preset   | Override network magic. |
| `--port <u16>`                            | u16     | none          | Inbound listen port. Omit to disable inbound. |
| `--host-addr <ip>`                        | ip      | `0.0.0.0`     | Inbound bind address. |
| `--database-path <path>`                  | path    | `./db`        | Chain DB root. |
| `--topology <path>`                       | path    | preset's      | Override topology file. |
| `--metrics-port <u16>`                    | u16     | none          | Enable HTTP metrics on `127.0.0.1:<port>`. |
| `--batch-size <usize>`                    | usize   | 50            | Sync batch size (blocks per BlockFetch round-trip). |
| `--checkpoint-interval-slots <u64>`       | u64     | 2160          | Minimum slot delta between ledger checkpoint flushes. |
| `--max-ledger-snapshots <usize>`          | usize   | 8             | Number of ledger checkpoints to retain. |
| `--checkpoint-trace-max-frequency <f64>`  | f64     | 1.0           | Hz cap on `Node.Recovery.Checkpoint` trace events. |
| `--checkpoint-trace-severity <severity>`  | string  | `Info`        | Severity threshold for checkpoint traces. |
| `--checkpoint-trace-backend <backend>`    | string  | `Stdout HumanFormatColoured` | Trace backend for checkpoint events. |
| `--no-verify`                             | flag    | off           | **Disable** block verification. Development/testing only. **Never use on mainnet.** |
| `--non-producing-node`                    | flag    | off           | Force relay/non-producing mode even when producer credential paths are present. |
| `--shelley-kes-key <path>`                | path    | none          | KES signing key (block production). |
| `--shelley-vrf-key <path>`                | path    | none          | VRF signing key. |
| `--shelley-operational-certificate <path>`| path    | none          | OpCert. |
| `--shelley-operational-certificate-issuer-vkey <path>`| path | none      | Cold-key vkey for OpCert verification. |

### Behavior

- If `--peer` is omitted, the node tries the preset's primary peer, then falls back to topology-derived peers.
- If all four block-production credentials are present, the forge loop is activated unless `--non-producing-node` is set.
- A partial block-production credential set is a startup error unless `--non-producing-node` is set.
- Graceful shutdown on `SIGINT` or `SIGTERM`.
- Exit status 0 on clean shutdown, non-zero on unrecoverable error.

### Examples

Basic relay sync:

```bash
$ yggdrasil-node run --network mainnet --database-path /var/lib/yggdrasil/db
```

Relay with inbound serving and metrics:

```bash
$ yggdrasil-node run \
    --network mainnet \
    --database-path /var/lib/yggdrasil/db \
    --port 3001 \
    --host-addr 0.0.0.0 \
    --metrics-port 12798
```

Block producer with credentials:

```bash
$ yggdrasil-node run \
    --network mainnet \
    --database-path /var/lib/yggdrasil/db \
    --metrics-port 12798 \
    --shelley-kes-key keys/kes.skey \
    --shelley-vrf-key keys/vrf.skey \
    --shelley-operational-certificate keys/node.opcert \
    --shelley-operational-certificate-issuer-vkey keys/cold.vkey
```

---

## `validate-config`

Operator preflight without starting the network. Reports config + storage + credential sanity issues.

### Synopsis

```
yggdrasil-node validate-config [flags]
```

### Flags

Same as `run` for `--config`, `--network`, `--database-path`, `--topology`. Block-production credential flags also apply if you want to validate them.
`--port`, `--host-addr`, and `--non-producing-node` are accepted so the report can show the exact resolved relay/producer role.

### Output

A structured report with sections:

- **Errors** — must be fixed before `run` will succeed.
- **Warnings** — non-fatal but worth investigating.
- **Info** — confirmation of detected state.

Example:

```
$ yggdrasil-node validate-config --network mainnet --database-path /tmp/empty
Config: OK (mainnet preset)
Network magic: 764824073
Genesis hashes: 4/4 verified
Storage: NOT INITIALIZED at /tmp/empty (will be created on `run`)
Peer snapshot: not configured
KES/Praos: no credentials configured (running as relay)
Topology: 3 bootstrap peers, 0 local roots, 2 public roots
Governor sanity: OK (target_active=20 < target_established=50 < target_known=100)

Errors:   0
Warnings: 0
```

### Exit status

- `0` — no errors, no warnings.
- `1` — warnings only.
- `2` — errors present.

This makes `validate-config` script-friendly:

```bash
$ yggdrasil-node validate-config --network mainnet || exit 1
```

---

## `status`

Inspect on-disk state without connecting to the network.

### Synopsis

```
yggdrasil-node status [flags]
```

### Flags

`--database-path`, `--config`, `--network`.

### Output

- Current sync position (latest applied slot, block number, hash).
- Block counts (immutable / volatile / total).
- Checkpoint state (most recent checkpoint slot, count of retained snapshots).
- Ledger counts (UTxO entries, registered pools, DReps).

```
$ yggdrasil-node status --database-path /var/lib/yggdrasil/db
Network: mainnet
Storage: /var/lib/yggdrasil/db
Tip slot: 117425831
Tip block: 11293441
Tip hash: a1b2c3d4...
Immutable blocks: 11290000
Volatile blocks: 3441
Latest checkpoint: slot 117400000 (5 minutes ago)
Snapshots retained: 7 / 10
UTxO entries: 4123857
Registered pools: 3142
DReps: 247
```

---

## `default-config`

Emit the default JSON config (mainnet) to stdout.

### Synopsis

```
yggdrasil-node default-config
```

### Example

```bash
$ yggdrasil-node default-config > /etc/yggdrasil/config.json
```

Edit the file, then run with `--config /etc/yggdrasil/config.json`.

---

## `cardano-cli`

A pure-Rust subset of upstream `cardano-cli`. Useful when you want to drop the Haskell-toolchain dependency for simple checks.

### Subcommands

```
yggdrasil-node cardano-cli version
yggdrasil-node cardano-cli show-upstream-config --network <preset>
yggdrasil-node cardano-cli query-tip --network <preset> [--database-path <path>]
```

| Sub-subcommand           | Purpose |
|--------------------------|---------|
| `version`                | Print binary version. |
| `show-upstream-config`   | Print the resolved preset config to stdout. |
| `query-tip`              | Read tip slot/block/hash from on-disk storage (no network calls). |

For the full upstream `cardano-cli` surface (transaction building, key derivation, governance, etc.), continue using the Haskell `cardano-cli` against Yggdrasil's NtC socket.

---

## `query` (Unix only)

NtC LocalStateQuery dispatcher. Connects to the running node's Unix socket and issues a query.

### Synopsis

```
yggdrasil-node query <query-name> [args] [--socket <path>] [--network-magic <u32>]
```

### Supported queries

All 24 LSQ tags defined by upstream:

| Tag | Name                                   | CLI form |
|-----|----------------------------------------|----------|
| 0   | CurrentEra                             | `current-era` |
| 1   | ChainTip                               | `chain-tip` |
| 2   | CurrentEpoch                           | `current-epoch` |
| 3   | ProtocolParameters                     | `protocol-parameters` |
| 4   | UTxOByAddress                          | `utxo-by-address --address <bech32>` |
| 5   | StakeDistribution                      | `stake-distribution` |
| 6   | RewardBalance                          | `reward-balance --reward-account <bech32>` |
| 7   | TreasuryAndReserves                    | `treasury-and-reserves` |
| 8   | GetConstitution                        | `constitution` |
| 9   | GetGovState                            | `gov-state` |
| 10  | GetDRepState                           | `drep-state` |
| 11  | GetCommitteeMembersState               | `committee-members-state` |
| 12  | GetStakePoolParams                     | `stake-pool-params --pool-id <hex>` |
| 13  | GetAccountState                        | `account-state` |
| 14  | GetUTxOByTxIn                          | `utxo-by-tx-in --tx-in <txhash#index>` |
| 15  | GetStakePools                          | `stake-pools` |
| 16  | GetFilteredDelegationsAndRewardAccounts| `filtered-delegations --stake-credential <hex>...` |
| 17  | GetDRepStakeDistr                      | `drep-stake-distr` |
| 18  | GetGenesisDelegations                  | `genesis-delegations` |
| 19  | GetStabilityWindow                     | `stability-window` |
| 20  | GetNumDormantEpochs                    | `num-dormant-epochs` |
| 21  | GetExpectedNetworkId                   | `expected-network-id` |
| 22  | GetDepositPot                          | `deposit-pot` |
| 23  | GetLedgerCounts                        | `ledger-counts` |

Output is JSON. For Conway-specific complex types (constitution, gov state, drep state, committee, genesis delegations, stake-pool params), the output includes a `cbor_hex` field for client-side decoding.

### Examples

```bash
$ yggdrasil-node query chain-tip --socket /var/lib/yggdrasil/db/node.sock
{"slot":117425831,"hash":"a1b2c3d4...","block_no":11293441}

$ yggdrasil-node query ledger-counts --socket /var/lib/yggdrasil/db/node.sock
{"stake_credentials":1843241,"pools":3142,"dreps":247,"committee_members":7,"gov_actions":12,"gen_delegs":7}

$ yggdrasil-node query account-state --socket /var/lib/yggdrasil/db/node.sock
{"treasury_lovelace":1834567000000000,"reserves_lovelace":11823945000000000,"total_deposits_lovelace":4830000000}
```

---

## `submit-tx` (Unix only)

NtC LocalTxSubmission. Submits a CBOR-encoded transaction to the running node's mempool.

### Synopsis

```
yggdrasil-node submit-tx --tx-hex <hex> [--socket <path>] [--network-magic <u32>]
```

### Flags

| Flag              | Type    | Default | Description |
|-------------------|---------|---------|-------------|
| `--tx-hex <hex>`  | string  | (req)   | Hex-encoded CBOR transaction. `0x` prefix tolerated. |
| `--socket <path>` | path    | `<storage_dir>/node.sock` | NtC socket path. |
| `--network-magic <u32>`| u32| from preset | Network magic for the handshake. |

### Output

`{"accepted":true,"tx_id":"<hash>"}` on acceptance.

`{"accepted":false,"reason":"<MempoolAddTxError variant>","details":"..."}` on rejection.

### Example

```bash
$ yggdrasil-node submit-tx \
    --tx-hex 84a30081825820...01ff \
    --socket /var/lib/yggdrasil/db/node.sock
{"accepted":true,"tx_id":"f3b2c4a8..."}
```

---

## Environment variables

| Variable                       | Effect |
|--------------------------------|--------|
| `CARDANO_NODE_NETWORK_MAGIC`   | Default for `--network-magic` on `query` / `submit-tx`. |
| `RUST_LOG`                     | Tokio runtime logging (independent of the trace dispatcher). |

## Exit codes

| Code | Meaning |
|------|---------|
| 0    | Clean shutdown / successful command. |
| 1    | Validation warnings (validate-config). |
| 2    | Validation errors / unrecoverable startup error. |
| 130  | Interrupted by SIGINT (Ctrl-C). |

## Where to go next

- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — operational procedures.
- [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}) — error message catalogue.
