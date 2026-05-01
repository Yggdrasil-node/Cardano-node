---
title: Networks and Presets
layout: default
parent: User Manual
nav_order: 4
---

# Networks and Presets

Cardano runs three official networks. Yggdrasil ships a vendored configuration preset for each.

| Network    | Network magic | Purpose                                      | Block reward? | Real ADA? |
|------------|--------------:|----------------------------------------------|:-------------:|:---------:|
| `mainnet`  | 764824073     | Production network                           | yes           | yes       |
| `preprod`  | 1             | Pre-production. Tracks mainnet protocol versions and parameters.| yes (testnet ADA) | no |
| `preview`  | 2             | Forward-looking. Tests upcoming protocol changes ahead of preprod.| yes (testnet ADA) | no |

Pick a preset with `--network <preset>`:

```bash
$ yggdrasil-node run --network mainnet ...
$ yggdrasil-node run --network preprod ...
$ yggdrasil-node run --network preview ...
```

The preset selects:

- The genesis files (`byron-genesis.json`, `shelley-genesis.json`, `alonzo-genesis.json`, `conway-genesis.json`) — vendored under [`node/configuration/<preset>/`](https://github.com/yggdrasil-node/Cardano-node/tree/main/node/configuration).
- The network magic — the integer that identifies the network in NtN handshake `version_data`.
- Bootstrap peers — IOG-operated entry points specific to each network.
- Topology defaults — relays curated for each network.
- Genesis-pinned protocol parameters (slot length, epoch length, security parameter `k`, active slot coefficient `f`, KES periods, etc.).

## What gets vendored per preset

Each preset directory contains:

```
node/configuration/<preset>/
├── alonzo-genesis.json    # Plutus V1/V2 cost models, ExUnit prices, collateral parameters
├── byron-genesis.json     # Byron-era genesis: initial UTxO, genesis delegates
├── config.json            # Operator config (logging, metrics, governor targets, genesis hashes)
├── conway-genesis.json    # Conway-era governance: initial committee, constitution, drep deposits
├── shelley-genesis.json   # Shelley-era: epoch length, K, F, KES, network magic, initial pools
└── topology.json          # Initial peer set: bootstrap peers + public roots + (optional) local roots
```

The `config.json` is the operator-facing entry point. The four genesis JSONs are immutable network parameters — never edit them.

## Genesis hash verification

Every preset config carries the canonical Blake2b-256 hash of each genesis file:

```jsonc
{
  "ShelleyGenesisFile": "shelley-genesis.json",
  "ShelleyGenesisHash": "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81",
  "AlonzoGenesisFile": "alonzo-genesis.json",
  "AlonzoGenesisHash": "7e94a15f55d1e82d10f09203fa1d40f8eede58fd8066542cf6566008068ed874",
  "ConwayGenesisFile": "conway-genesis.json",
  "ConwayGenesisHash": "15a199f895e461ec0ffc6dd4e4028af28a492ab4e806d39cb674c88f7643ef62",
  "ByronGenesisFile": "byron-genesis.json",
  "ByronGenesisHash": "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb"
}
```

On startup, the node hashes each genesis file and compares against the declared hash. Mismatch is fatal — the node refuses to start. This catches:

- Corrupt downloads.
- Accidentally edited genesis files.
- Supply-chain tampering between repository checkout and runtime.

Byron genesis hashing uses upstream canonical JSON rendering before Blake2b-256, while Shelley, Alonzo, and Conway hashes use raw file bytes. All four preset genesis hashes are verified at startup.

## Custom networks

Yggdrasil does not ship a built-in custom-network preset, but you can run against any network for which you have:

1. The four genesis JSON files.
2. The network magic value.
3. At least one peer address.

Create a `custom.json` config based on `node/configuration/mainnet/config.json`, change the genesis paths, network magic, and topology. Run with `--config custom.json` instead of `--network <preset>`. See [Configuration]({{ "/manual/configuration/" | relative_url }}) for the full schema.

## Switching networks on a single machine

Each network needs its own database directory. Do **not** point two different networks at the same `--database-path` — the chain state is incompatible.

```bash
$ yggdrasil-node run --network mainnet --database-path /var/lib/yggdrasil/mainnet-db ...
$ yggdrasil-node run --network preprod --database-path /var/lib/yggdrasil/preprod-db ...
```

You can run both at once on different ports (`--port 3001` for mainnet, `--port 3002` for preprod) on the same machine if you have the resources.

## Network upgrade timelines

Cardano protocol upgrades typically arrive on networks in the order **preview → preprod → mainnet**, separated by a few weeks each. To test your operational tooling against an upcoming change before it reaches mainnet, run a preprod or preview node alongside your mainnet node.

The current Cardano protocol version (Conway, major 10 as of 2026) is supported on all three Yggdrasil presets. The `MaxKnownMajorProtocolVersion` config key (default 10) is the hard cap for every network. Conway's `HeaderProtVerTooHigh` ledger check is stricter on mainnet and temporarily relaxed on testnets until Dijkstra protocol major 12, matching upstream `cardano-ledger`.

## Where to go next

- [Configuration]({{ "/manual/configuration/" | relative_url }}) — every config key explained.
- [Running a Node]({{ "/manual/running/" | relative_url }}) — daemonising, log management.
