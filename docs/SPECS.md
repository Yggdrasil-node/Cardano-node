---
title: Specifications
layout: default
parent: Reference
nav_order: 6
---

# Specification Sources

Yggdrasil is specification-driven. When sources disagree, use them in this order.

## Priority Order
1. Formal ledger specifications and protocol papers.
2. Cardano ledger CDDL schemas.
3. Accepted CIPs that define era or protocol behavior.
4. Haskell node behavior for compatibility checks and fixture validation.

## Core References (add or update as needed)
- Cardano ledger CDDL schemas: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specifications: <https://github.com/IntersectMBO/formal-ledger-specifications>
- Cardano Improvement Proposals (CIPs): <https://github.com/cardano-foundation/CIPs>
- Ouroboros papers: <https://iohk.io/research/papers/>
- Cardano blueprint: <https://cardano-scaling.github.io/cardano-blueprint/>
- Ouroboros networking specification: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>
- Ouroboros network implementation: <https://github.com/IntersectMBO/ouroboros-network/>
- Cardano node integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Local parity surfaces (machine-readable)

These living artifacts track the spec-to-code mapping at the file
and feature level. Update them in lockstep with code changes:

- [`docs/parity-matrix.json`](parity-matrix.json) — feature-level
  Rust ↔ Haskell parity inventory; `reference.tag` tracks the
  current IntersectMBO/cardano-node release. Validated by
  `python3 scripts/check-parity-matrix.py`.
- [`docs/strict-mirror-audit.tsv`](strict-mirror-audit.tsv) —
  per-file Yggdrasil `.rs` ↔ upstream `.hs` verdict table from R274.
  Every production `.rs` is graded `(a) DIRECT_MIRROR` or
  `(c) NO_MIRROR_NEEDS_DOCSTRING (verified)`. CI gate:
  `python3 scripts/check-strict-mirror.py --fail-on-violation`.
- [`docs/upstream-haskell-files.txt`](upstream-haskell-files.txt) —
  flat-file index of every upstream `.hs` under
  `.reference-haskell-cardano-node/`, rebuilt by
  `bash scripts/setup-reference.sh`.

For end-to-end "what works today" evidence see
[`docs/PARITY_PROOF.md`](PARITY_PROOF.md); for upstream-pin drift
status see [`docs/UPSTREAM_PARITY.md`](UPSTREAM_PARITY.md).

## Usage Rules
- Pin the exact upstream revision used for generated artifacts.
- Keep generated code reproducible from checked-in source specifications.
- Add fixture provenance for any Haskell parity test data.
- The current `crates/crypto` 80-byte Praos VRF fixtures are draft03-era vectors mirrored from `cardano-crypto-praos`; do not treat them as RFC 9381 final-format verification fixtures without explicit translation or replacement.
- For networking behavior, trace message tags/flow to official Ouroboros protocol sources before introducing local terminology.
- Strict 1:1 file-mirror policy (R274+): every new `.rs` under
  `crates/<crate>/src/` and `crates/node/*/src/` either snake-case-mirrors a
  single upstream `.hs` filename or carries a `## Naming parity`
  docstring stanza. Authoring-time guidance lives in
  [`.claude/skills/round-extraction/SKILL.md`](../.claude/skills/round-extraction/SKILL.md);
  the CI counterpart is `python3 scripts/check-strict-mirror.py`.

## Vendored Upstream Test Vectors
- Vendored cryptographic vectors live under `specs/upstream-test-vectors/` with pinned upstream commit provenance.
- Current pinned `cardano-base` source revision: `7a8a991945d401d89e27f53b3d3bb464a354ad4c`.
- Included corpora:
	- `cardano-crypto-praos/test_vectors/` (Praos VRF vectors)
	- `cardano-crypto-class/bls12-381-test-vectors/test_vectors/` (BLS12-381 vectors)
- Crypto integration tests in `crates/crypto/tests/upstream_vectors.rs` validate that the vendored files are present, well-formed, and aligned with embedded standard VRF fixtures where applicable.
