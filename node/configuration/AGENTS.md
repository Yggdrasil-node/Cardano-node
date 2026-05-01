# Guidance for node configuration references and preset layout.
This directory contains reference configuration files and per-network presets used to align Yggdrasil configuration handling with the official node.

## Scope
- `mainnet/`, `preprod/`, and `preview/` preset directories.
- Reference `config.json`, `topology.json`, and genesis files used to mirror official network layout.

##  Rules *Non-Negotiable*
- Treat the vendored configuration files here as reference inputs, not as the source of truth for local runtime configuration code.
- Preserve the official file naming and preset split so parity work stays traceable.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Official configuration tree](https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano)
- [Mainnet configuration](https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/mainnet)
- [Preprod configuration](https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/preprod)
- [Preview configuration](https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/preview)
- [Node runtime configuration usage](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/)
- [Cardano Operations Book — mainnet](https://book.world.dev.cardano.org/env-mainnet.html)
- [Cardano Operations Book — preprod](https://book.world.dev.cardano.org/env-preprod.html)
- [Cardano Operations Book — preview](https://book.world.dev.cardano.org/env-preview.html)

## Current Phase
- Yggdrasil exposes `NetworkPreset` values for `Mainnet`, `Preprod`, and `Preview` and now parses the vendored `topology.json` files here as read-only reference inputs for ordered peer selection across `bootstrapPeers`, `localRoots`, `publicRoots`, `useLedgerAfterSlot`, and `peerSnapshotFile`.
- Operations Book 10.7.1 notes apply to all three presets: new tracing is the default, separate forger/non-forger config files are no longer required, legacy non-P2P mode is unavailable, and PeerSharing-enabled relays must not be connected to a block producer through `InitiatorOnlyMode` because that can leak the producer IP.
