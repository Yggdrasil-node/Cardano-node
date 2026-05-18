# Round 506 — block-producer A3 R3a slice 2: MismatchedKesKey credential check

**Date:** 2026-05-18
**Area:** node sub-crates / `crates/node/block-producer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`opCertKesKeyCheck` / `MismatchedKesKey` (`Cardano.Node.Protocol.Shelley`).

## Summary

`load_block_producer_credentials` loaded the KES signing key and the
operational certificate but never checked that they were *consistent* —
that the KES verification key the operator supplies matches the hot KES
key the operational certificate certifies. Upstream's
`opCertKesKeyCheck` rejects node startup on that mismatch
(`MismatchedKesKey`). This round (A3 R3a slice 2) ports that check.

Observable gap closed: with a mismatched (KES key, opcert) pair,
yggdrasil would previously start forging headers signed by a KES key
the opcert does not certify — every header's KES signature would fail
verification at every peer. The node now refuses to load such
credentials, matching upstream.

## Re-slice note

R3a was roadmapped as slice 2 = "`issuer_vkey_path`-free loader +
`MismatchedKesKey`". Grounding (the `load_block_producer_credentials`
blast-radius map) showed these are two independent changes: the
`MismatchedKesKey` check is additive and block-producer-internal (zero
blast radius), while dropping `issuer_vkey_path` is a breaking
multi-crate change (removes a CLI flag + a config field). They are now
separate slices — slice 2 (this round) is the additive check; slice 3
is the `issuer_vkey_path` removal.

Also corrected: an exploratory note suggested deriving the issuer vkey
from `operational_cert.sigma`. That is not possible — an Ed25519 public
key cannot be recovered from a signature. The issuer key is available
only as the embedded cold vkey that slice 1's
`load_operational_certificate_with_issuer` returns; slice 3 uses that.

## Parity basis

Upstream `Cardano.Node.Protocol.Shelley`:

- `opCertKesKeyCheck` (Shelley.hs:213-224) loads the operational
  certificate and the KES signing key, computes
  `verificationKeyHash (getHotKey opCert)` and
  `verificationKeyHash (getVerificationKey kesSKey)`, and on inequality
  emits `MismatchedKesKey kesFile certFile`.
- `MismatchedKesKey FilePath FilePath` (Shelley.hs:349-354) is a
  `PraosLeaderCredentialsError` constructor; its `prettyError`
  (Shelley.hs:366-368) is "The KES key provided at: <kes> does not
  match the KES key specified in the operational certificate at:
  <cert>".

yggdrasil before this round: `load_block_producer_credentials` verified
the opcert *signature* against the issuer cold key but never compared
the supplied KES key to the opcert's hot vkey.

## Changes

- `block-producer/src/lib.rs`:
  - new `BlockProducerError::MismatchedKesKey { kes_path, opcert_path }`
    — the `#[error]` text mirrors upstream's `prettyError`.
  - `load_block_producer_credentials` — after loading the KES signing
    key and operational certificate, derive
    `derive_sum_kes_vk(&kes_signing_key)` and compare its bytes to
    `operational_cert.hot_vkey`; on mismatch return `MismatchedKesKey`.
    Function signature unchanged — zero caller blast radius. (Comparing
    raw key bytes is equivalent to upstream's `verificationKeyHash`
    comparison and stricter — no intermediate hash.)
  - `derive_sum_kes_vk` added to the `yggdrasil_crypto::sum_kes` import.
  - 1 new test `load_block_producer_credentials_rejects_mismatched_kes_key`;
    the existing `rejects_mismatched_issuer_key` test gains a comment
    recording that its opcert hot_vkey equals its KES key (so the new
    check passes and the test still reaches the issuer-verify failure
    it asserts).
- `block-producer/AGENTS.md` — credential-surface note + R-arc tracking
  refreshed.

## Verification

- Focused (`yggdrasil-node-block-producer`): `cargo test` — 25 lib
  tests pass, including the new `MismatchedKesKey` test; the existing
  `rejects_mismatched_issuer_key` unchanged.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,528 tests passing, 0 failing** (+1 over the
  R505 baseline of 6,527 — exactly the new test).
- Note: a plain `cargo test-all` run aborted early because
  cardano-tracer's timing-flaky `die_on_failure_aborts_loop_when_action_panics`
  failed and cargo's default fail-fast stops at the first failing
  binary. The `--no-fail-fast` run above ran every crate clean; the
  flake is unrelated to block-producer.

## Remaining (A3 R3a slice 3)

Drop `load_block_producer_credentials`'s `issuer_vkey_path` parameter;
source the issuer cold vkey from `load_operational_certificate_with_issuer`
(error on `None` — bare-`OCert` envelopes are test fixtures, not
operator artifacts). Remove the upstream-divergent
`--shelley-operational-certificate-issuer-vkey` CLI flag (`Run` +
`ValidateConfig`), the `NodeConfigFile::shelley_operational_certificate_issuer_vkey`
field, and update `apply_block_producer_credential_overrides` plus the
two call sites. Breaking operator-surface change — its own round. Then
R3b (consensus config) and R3c (Praos forge loop).
