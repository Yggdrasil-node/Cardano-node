# Guidance for network protocol, mux, and codec tests.
Keep tests in this directory aligned with official wire behavior and typed protocol state transitions.

## Scope
- Mini-protocol state machine tests.
- Wire codec and segmentation or reassembly coverage.
- Peer lifecycle and driver regressions.

##  Rules *Non-Negotiable*
- Tests here MUST validate wire tags, message ordering, and protocol boundaries explicitly.
- Network tests MUST not hide ledger decode assumptions that belong in other crates.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Ouroboros network repository:](https://github.com/IntersectMBO/ouroboros-network/)
- [Protocol test suites:](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/test)
- [Network framework tests:](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework/test)
- [Shelley networking spec:](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec)

## Current Phase
- Tests in this directory validate mux behavior, mini-protocol message flows, typed client drivers, and large-message SDU segmentation or reassembly.