# Round 507 — block-producer A3 R3a slice 3: remove the divergent issuer-vkey input

**Date:** 2026-05-18
**Area:** node sub-crates / `crates/node/{block-producer,yggdrasil-node,config}`
**Upstream reference:** IntersectMBO/cardano-node 11.0.1 —
`Cardano.Node.Protocol.Shelley` (`readLeaderCredentials`);
`OperationalCertificate` (`cardano-api`).

## Summary

Slice 3 — the final slice of A3 R3a — removes yggdrasil's
upstream-divergent separate issuer-vkey input. Upstream block-producer
credentials are KES key + VRF key + operational certificate; the
cold/issuer verification key is carried *inside* the opcert's
`[OCert, cold_vkey]` text-envelope wrapper, not supplied separately.
yggdrasil carried an extra `--shelley-operational-certificate-issuer-vkey`
CLI flag and `ShelleyOperationalCertificateIssuerVkey` config field —
a CLI/config surface absent upstream. This round removes them
end-to-end across three crates and sources the issuer cold key from the
operational certificate (slice 1's `load_operational_certificate_with_issuer`).

## Parity basis

- Upstream's `OperationalCertificate` (`cardano-api`) carries the cold
  verification key as the second field of the `NodeOperationalCertificate`
  text envelope (`[OCert, cold_vkey]`, the `cardano-cli node
  issue-op-cert` output). `Cardano.Node.Protocol.Shelley` derives the
  header issuer key from that field — there is no separate issuer-vkey
  artifact, CLI flag, or config key upstream.
- The bare `OCert` array(4) text-envelope form embeds no cold vkey; it
  is a yggdrasil-internal test-fixture shape, never an operator
  artifact, so it is rejected for block production.

## Changes

`block-producer`:

- `load_block_producer_credentials` — the `issuer_vkey_path` parameter
  is dropped; the issuer cold vkey now comes from
  `load_operational_certificate_with_issuer`. A bare `OCert` envelope
  (no embedded cold vkey) → new `BlockProducerError::OpCertMissingIssuerKey`.
  Check order: extract the embedded issuer (bare-form → error) → the
  `MismatchedKesKey` KES-match check → the opcert self-consistency
  verify.
- `operational_cert.verify` is now an opcert internal-consistency
  (tamper) check — its sigma must verify against its *own* embedded
  cold vkey; the error message is updated to say so.
- tests: `rejects_mismatched_issuer_key` repurposed →
  `rejects_opcert_with_inconsistent_cold_vkey` (a wrapped opcert whose
  embedded cold vkey did not sign it); `rejects_mismatched_kes_key`
  reworked to the wrapped opcert form; new
  `rejects_bare_opcert_without_embedded_cold_vkey`.

`yggdrasil-node`:

- `cli.rs` — `--shelley-operational-certificate-issuer-vkey` removed
  from the `Run` and `ValidateConfig` clap variants.
- `main.rs` — the 4 dispatch lines removed.
- `run.rs` — the `RunCmdArgs` field, its destructure, and the
  override-call argument removed.
- `configuration.rs` — `apply_block_producer_credential_overrides`
  drops its 4th parameter (now overlays 3 credential paths).
- `validate_config.rs` — `load_configured_block_producer_credentials`
  drops the 4th argument; `run_validate_config_subcommand` drops the
  parameter; `block_producer_credential_fields` and the
  `ensure_block_producer_credential_policy` bail message go from 4
  credential fields to 3.
- `main_tests.rs` — the obsolete issuer-field assignment removed.

`config`:

- `NodeConfigFile::shelley_operational_certificate_issuer_vkey` removed;
  the three preset constructors (`mainnet`/`preprod`/`preview`) no
  longer set it; two config-roundtrip test assertions removed.
  `NodeConfigFile` carries no `#[serde(deny_unknown_fields)]`, so an
  existing operator config still carrying the old
  `ShelleyOperationalCertificateIssuerVkey` key continues to parse —
  the key is silently ignored.

`AGENTS.md`: `block-producer`, `crates/node/cardano-node`, and
`crates/node/cardano-node/src` refreshed for the removed surface.

## Verification

- Focused: `cargo test -p yggdrasil-node-block-producer
  -p yggdrasil-node-config` — block-producer 26 lib tests pass
  (the 3 reworked/new credential tests green); config 69 tests pass.
- Full workspace: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint` all green; `cargo test --workspace --all-features
  --no-fail-fast` — **6,529 passing, 0 failing** (+1 over the R506
  baseline of 6,528 — the new bare-opcert test; the two reworked tests
  are net-zero).
- A plain `cargo test-all` aborts early on the documented timing-flaky
  cardano-tracer test (cargo's default fail-fast); `--no-fail-fast`
  ran every crate clean.

## Remaining

- **A3 R3a is complete** (slices 1–3). Next: R3b (consensus config —
  `Run.initProtocol` / `mkConsensusProtocolCardano`) and R3c (the Praos
  forge loop) per `docs/COMPLETION_ROADMAP.md`.
- **Follow-up doc-only round:** `docs/manual/{block-production,
  troubleshooting,docker,configuration,cli-reference}.md` still
  reference the removed `--shelley-operational-certificate-issuer-vkey`
  flag and need a prose sweep — a Phase-1-style doc round, no cargo
  gates.
