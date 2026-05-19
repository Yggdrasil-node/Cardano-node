# Round 516 — A3 R3c-2: bulk leader credentials

**Date:** 2026-05-19
**Area:** `crates/node/block-producer` + `crates/tools/db-synthesizer`
**Upstream reference:** `readLeaderCredentials`,
`readLeaderCredentialsSingleton`, `readLeaderCredentialsBulk` /
`parseShelleyCredentials`, and `opCertKesKeyCheck` in
`Cardano.Node.Protocol.Shelley`.

## Summary

R3c-2 ports upstream's bulk-credentials path so the db-synthesizer can
build a `Vec<BlockProducerCredentials>` forger set. The decisive
upstream fact (verified against `Shelley.hs:233-266`): the
bulk-credentials file is a JSON array of **inline** `[cert, vrf, kes]`
text-envelope triples — not file paths — so R3c-2 is a small refactor
over R3a's path-based loader, not a new parser.

## Changes

### `crates/node/block-producer/src/lib.rs`

Behavior-preserving refactor (verified — see below), then the new
bulk loader:

- Extracted the envelope-decoding cores `parse_vrf_signing_key`,
  `parse_kes_signing_key`, `parse_operational_certificate_with_issuer`
  (each `(type_tag, cbor, loc)`), plus `decode_envelope_hex`; the
  path-based `load_*` functions are now thin wrappers over them.
- Extracted `check_opcert_kes_key` — the `opCertKesKeyCheck` KES-match
  check plus the opcert internal-consistency verify — as a named step
  the singleton path invokes.
- New `load_bulk_block_producer_credentials` — mirror of
  `readLeaderCredentialsBulk` / `parseShelleyCredentials`. Decodes the
  inline triples in upstream order (cert → kes → vrf) and labels decode
  failures with upstream `mkCredentials`'s `<bulk-file>.<index>{cert,
  vrf,kes}` diagnostic.

### `crates/tools/db-synthesizer`

- `Cargo.toml` — `yggdrasil-node-block-producer` workspace path-dep.
- `run.rs` — `read_leader_credentials` (mirror of `readLeaderCredentials`
  = singleton ∪ bulk); `RunError::Credentials` (`#[from]
  BlockProducerError`) and `RunError::IncompleteCredentials` (mirror of
  the singleton readers' `OCertNotSpecified` / `VRFKeyNotSpecified` /
  `KESKeyNotSpecified`).
- `AGENTS.md` — functional surface refreshed.

## Parity notes

- **Bulk skips the soundness checks.** `load_bulk_block_producer_credentials`
  deliberately does NOT run `check_opcert_kes_key` — upstream
  `parseShelleyCredentials` builds each bulk entry as
  `PraosCredentialsUnsound`, omitting `opCertKesKeyCheck`. (Note: the
  singleton KES-file arm *also* wraps in `PraosCredentialsUnsound`; the
  real singleton-vs-bulk asymmetry is purely whether `opCertKesKeyCheck`
  runs — there is no `PraosCredentialsSound` constructor.)
- **Scope stops at the loader.** Threading the `Vec<BlockProducerCredentials>`
  into `run_forge` is R3c-3/R3c-4; `read_leader_credentials` is `pub`
  and unit-tested directly, with no production caller yet.
- No new `.rs` file → no new strict-mirror stanza; the new symbols' own
  docstrings cite the cross-module upstream sources, and
  `block-producer/src/lib.rs`'s file-level `## Naming parity` stanza was
  extended to name the leader-credential readers.

## Verification

- **Refactor is behavior-preserving:** after extracting the
  `parse_*` cores + `check_opcert_kes_key` (before adding the bulk
  loader), `cargo test -p yggdrasil-node-block-producer --lib` ran the
  26 pre-existing R3a tests unchanged — 26 passed. The bulk loader then
  took the count to 28 (exactly the 2 new bulk tests).
- Four gates green: `cargo fmt --all -- --check`, `cargo check-all`,
  `cargo lint`; `cargo test --workspace --all-features --no-fail-fast`
  — **6,544 passing, 0 failing** (+4 over the R3c-1b baseline of 6,540:
  2 bulk-loader tests in block-producer, 2 `read_leader_credentials`
  tests in db-synthesizer).
- db-synthesizer: 91 lib + 7 integration tests pass.

## Remaining (A3 R3c)

R3c-1 + R3c-2 are complete. Remaining:

- **R3c-3** — thread `InitialForgeState` + the forger set through
  `run_forge`'s `ForgeState` (blocks stay structural — a
  four-gates-green intermediate).
- **R3c-4** — real Praos forge (`check_should_forge` + `forge_block`),
  picking the first slot-leader from the forger set.
- **R3c-5** — epoch-boundary stake-distribution rebuild.
- **R3c-6** — `FileImmutable` → `ChainDb` migration (the `db-analyser`
  exit gate).
