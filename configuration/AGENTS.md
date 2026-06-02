# Guidance for node configuration references and preset layout.
This directory contains reference configuration files and per-network presets used to align Yggdrasil configuration handling with the official node.

## Validators that protect this tree

`configuration/` carries vendored operator-config presets, not
Rust code, so the workspace strict-mirror file-policy (R274+) does
not apply directly here. The CI/local validator for this directory
is:

- `python3 dev/test/check-reference-artifacts.py` (local/operator) —
  validates that each per-network share directory under
  `.reference-haskell-cardano-node/install/share/<network>/`
  carries the canonical operator-config bundle (`config.json`,
  `topology.json`, `byron-genesis.json`, `shelley-genesis.json`,
  `alonzo-genesis.json`, `conway-genesis.json`, `peer-snapshot.json`,
  `tracer-config.json`). The Yggdrasil-side mirrors at
  `configuration/<network>/` are operator-facing copies that
  must keep parity with this canonical bundle.
- `python3 dev/test/check-stale-placement.py` (CI) - asserts these
  Yggdrasil-side preset bundles remain under root `configuration/`
  after the node-crate reorganization, including `config-legacy.json`,
  `submit-api-config.json`, `checkpoints.json` where present, and
  `poolMetaData.json`.

## Scope
- `mainnet/`, `preprod/`, and `preview/` preset directories.
- Reference `config.json`, `topology.json`, and genesis files used to mirror official network layout.
- `poolMetaData.json` — sample stake-pool metadata bundle (`name:
  "WORLDS FIRST RUST FULLNODE"`, `ticker: "RUST"`); operator artifact
  relocated here from `docs/` in R298.

##  Rules *Non-Negotiable*
- Treat the vendored configuration files here as reference inputs, not as the source of truth for local runtime configuration code.
- Preserve the official file naming and preset split so parity work stays traceable.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Official configuration tree](../.reference-haskell-cardano-node/configuration/cardano)
- [Mainnet operator bundle](../.reference-haskell-cardano-node/install/share/mainnet)
- [Preprod operator bundle](../.reference-haskell-cardano-node/install/share/preprod)
- [Preview operator bundle](../.reference-haskell-cardano-node/install/share/preview)
- [Node runtime configuration usage](../.reference-haskell-cardano-node/cardano-node/)
- [Cardano Operations Book — mainnet](https://book.world.dev.cardano.org/env-mainnet.html)
- [Cardano Operations Book — preprod](https://book.world.dev.cardano.org/env-preprod.html)
- [Cardano Operations Book — preview](https://book.world.dev.cardano.org/env-preview.html)

## Current Phase
- Yggdrasil exposes `NetworkPreset` values for `Mainnet`, `Preprod`, and `Preview` and now parses the vendored `topology.json` files here as read-only reference inputs for ordered peer selection across `bootstrapPeers`, `localRoots`, `publicRoots`, `useLedgerAfterSlot`, and `peerSnapshotFile`.
- Operations Book 10.7.1 notes apply to all three presets: new tracing is the default, separate forger/non-forger config files are no longer required, legacy non-P2P mode is unavailable, and PeerSharing-enabled relays must not be connected to a block producer through `InitiatorOnlyMode` because that can leak the producer IP.
