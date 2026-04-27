# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in Yggdrasil — particularly anything
that could affect chain consensus, transaction validity, peer denial of
service, or block-producer key safety — **please do not file a public issue.**

Email **security@fraction.estate** with the details. Encrypt with this PGP key
if you want end-to-end confidentiality (key fingerprint will be published in
this file once available; until then, plain email is acceptable for
non-critical reports).

We will:

1. Acknowledge receipt within 72 hours.
2. Investigate and reproduce the issue.
3. Coordinate a fix and a disclosure timeline with you.
4. Credit you in the release notes (with your permission) when the fix ships.

## Scope

In scope:

- Consensus rule violations (blocks accepted that the upstream Haskell node
  would reject, or vice versa).
- Wire-format parity bugs that could be exploited to fork the node from the
  honest network.
- Cryptographic implementation flaws (Ed25519, KES, VRF, BLS12-381, hashing).
- Block-producer credential handling (KES, VRF, OpCert key safety).
- Mempool denial of service exceeding upstream-policy bounds.
- Network-layer denial of service via mini-protocol abuse.
- Local privilege escalation through the node binary.
- Dependency vulnerabilities with practical impact on a running node.

Out of scope:

- Unmaintained or experimental crates (`crates/cddl-codegen` test fixtures,
  development-only tooling).
- Issues requiring physical access to the operator's machine.
- Self-DoS by misconfiguration that the operator could correct.
- Reports against vendored upstream test vectors (`specs/upstream-test-vectors/`)
  that originate from upstream and are mirrored unchanged.

## Supported versions

Yggdrasil follows semantic versioning. We provide security fixes for:

| Version    | Supported          |
|------------|--------------------|
| `0.x`      | Latest minor only. Older `0.x` minors receive critical fixes for 30 days after the next minor releases. |
| `1.x` (future) | TBD when `1.0` ships. |

Pre-release tags (`-rc`, `-beta`, `-alpha`) are not supported once a stable
release in the same line is published.

## Disclosure timeline

For confirmed vulnerabilities, our default disclosure timeline is:

- Day 0: report received, acknowledged.
- Day 0–14: triage, reproduce, develop a fix.
- Day 14–30: fix tested, release prepared.
- Day 30: coordinated public disclosure with the fix release.

Critical issues affecting active mainnet operators may warrant faster
disclosure or a private patch distribution to known operators ahead of public
release. We will discuss timing with you.

## Cardano-network-wide vulnerabilities

If your finding affects the Cardano protocol itself rather than a Yggdrasil-
specific implementation issue, please also report to the upstream
[IntersectMBO](https://github.com/IntersectMBO) team — typically through the
`cardano-node`, `ouroboros-consensus`, `ouroboros-network`, or `cardano-base`
repositories' security policies. Many implementation parity bugs are joint
issues.

## Bug bounty

There is no formal bug bounty program at this time. We will discuss good-faith
gratitude payments case by case for material findings.

## Public security advisories

Once a fix ships, the issue is published as a [GitHub Security Advisory](https://github.com/yggdrasil-node/Cardano-node/security/advisories)
and recorded in [`CHANGELOG.md`](CHANGELOG.md).
