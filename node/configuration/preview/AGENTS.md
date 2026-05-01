# Guidance for vendored preview configuration reference files.
This directory contains preview reference configuration artifacts only.

## Scope
- Preview `config.json`, `topology.json`, and genesis reference files.

##  Rules *Non-Negotiable*
- Do not hand-edit these reference files.
- Use them only as provenance and shape references for Yggdrasil configuration handling.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Preview configuration files](https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/preview/)
- [Cardano Operations Book — preview](https://book.world.dev.cardano.org/env-preview.html)

## Current Phase
- Treat preview as a GenesisMode/P2P preset per the Operations Book; do not reintroduce legacy non-P2P or split forger/non-forger assumptions when using these reference files.
