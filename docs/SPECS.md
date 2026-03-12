# Specification Sources

Yggdrasil is specification-driven. When sources disagree, use them in this order.

## Priority Order
1. Formal ledger specifications and protocol papers.
2. Cardano ledger CDDL schemas.
3. Accepted CIPs that define era or protocol behavior.
4. Haskell node behavior for compatibility checks and fixture validation.

## Core References
- Cardano ledger CDDL schemas: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specifications: <https://github.com/IntersectMBO/formal-ledger-specifications>
- Ouroboros papers: <https://iohk.io/research/papers/>
- Cardano blueprint: <https://cardano-scaling.github.io/cardano-blueprint/>

## Usage Rules
- Pin the exact upstream revision used for generated artifacts.
- Keep generated code reproducible from checked-in source specifications.
- Add fixture provenance for any Haskell parity test data.
