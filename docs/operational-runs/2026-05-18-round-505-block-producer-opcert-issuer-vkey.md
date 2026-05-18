# Round 505 — block-producer A3 R3a slice 1: opcert loader carries the cold issuer vkey

**Date:** 2026-05-18
**Area:** node sub-crates / `crates/node/block-producer`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 — `OCert`
(`Cardano.Protocol.TPraos.OCert`), `OperationalCertificate`
(`cardano-api`), leader-credential loading
(`Cardano.Node.Protocol.Shelley`).

## Summary

The block-producer operational-certificate loader (`decode_opcert_cbor`)
already parsed the trailing 32-byte cold issuer verification key from
the upstream `[OCert, cold_vkey]` text-envelope wrapper — it
length-checked the field, then **discarded** it. This round (A3 R3a
slice 1) makes the loader **return** that key: `decode_opcert_cbor` now
yields `(OpCert, Option<VerificationKey>)`, and a new public accessor
`load_operational_certificate_with_issuer` surfaces the pair.

This is the first bounded slice of A3 R3a (block-producer credential
model). It closes the parity gap "loader silently drops the embedded
cold vkey" without touching the 45-site `yggdrasil_consensus::OpCert`
type or the public `load_operational_certificate` signature — zero
blast radius.

## Parity basis

Upstream model:

- The `cardano-cli node issue-op-cert` text envelope
  (`NodeOperationalCertificate`) is CBOR `[OCert, cold_vkey]` — a
  two-element array whose second field is the stake-pool cold (issuer)
  verification key. Upstream's `OperationalCertificate` (`cardano-api`)
  carries that key as its second constructor field.
- Upstream `Cardano.Node.Protocol.Shelley` derives the header
  `issuer_vkey` from the cold key embedded in the operational
  certificate envelope — it does not read it from a separate file.

Yggdrasil before this round: `decode_opcert_cbor`'s `0x82`-wrapped
branch read `cold_vkey_raw`, validated `len() == 32`, then dropped it on
the floor. `load_block_producer_credentials` instead requires a
separate `issuer_vkey_path` argument — a divergence from the upstream
credential model.

Observable difference closed: the loader now retains the embedded cold
issuer vkey instead of discarding it; callers can obtain it via
`load_operational_certificate_with_issuer`. The bare `OCert`-array form
(`0x84`) carries no cold key and yields `None`.

## Changes

- `block-producer/src/lib.rs`:
  - `decode_opcert_cbor` — signature `-> Result<OpCert, _>` →
    `-> Result<(OpCert, Option<VerificationKey>), _>`; the `0x82`
    branch builds `VerificationKey::from_bytes` from the already-parsed
    `cold_vkey_raw` and returns `Some(..)`; the bare `0x84` branch
    returns `None`. Wire-parsing behavior is byte-identical — only the
    previously-discarded value is now retained.
  - `load_operational_certificate` — keeps its public
    `-> Result<OpCert, _>` signature; now delegates to the new
    accessor and projects `.0`.
  - new `load_operational_certificate_with_issuer(path) ->
    Result<(OpCert, Option<VerificationKey>), _>` — the envelope read +
    type-tag check + decode, returning the `(opcert, issuer)` pair.
  - 2 new tests: `load_opcert_with_issuer_recovers_wrapped_cold_vkey`
    (the `0x82` wrapper → `Some(cold_vkey)`),
    `load_opcert_with_issuer_bare_ocert_carries_no_cold_vkey` (the
    `0x84` bare form → `None`).
- `block-producer/AGENTS.md` — "ships" inventory + R-arc tracking
  refreshed for the new accessor and the remaining R3a work.

## Verification

- Focused (`yggdrasil-node-block-producer`): `cargo test` — 24 lib
  tests pass, including the 2 new R3a tests; the prior 22 unchanged
  (no regression).
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test-all` — **6,527 tests passing,
  0 failing** (+2 over the R504 baseline of 6,525 — exactly the 2 new
  tests).
- Note: one parallel `cargo test-all` run showed a transient failure of
  cardano-tracer's `die_on_failure_aborts_loop_when_action_panics` (a
  timing-based async test — 3 s sleep against a 1 s timer interval). It
  passed 5/5 in isolation and the re-run of the full suite was clean.
  Unrelated to this change (block-producer does not link the tracer's
  notification timer).

## Remaining (A3 R3a)

`load_operational_certificate_with_issuer` is the accessor; the next
R3a slices fold the embedded cold vkey into
`load_block_producer_credentials` (dropping the divergent separate
`issuer_vkey_path`) and add the upstream `MismatchedKesKey` cross-check
between the opcert hot vkey and the loaded KES key. R3b (consensus
config) and R3c (Praos forge loop) follow per
`docs/COMPLETION_ROADMAP.md`.
