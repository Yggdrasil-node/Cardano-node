---
title: Dependencies
layout: default
parent: Reference
nav_order: 7
---

# Dependency Policy

This file defines how dependencies are introduced into Yggdrasil.

## Approved Now
- `blake2`: pure Rust hashing for Blake2b-based primitives.
- `clap`: pure Rust CLI argument parser with derive macros; needed for the node binary's command-line interface. Rejected alternative was manual `std::env::args` parsing which provides no help text, completion, or structured subcommands. No native build requirements.
- `curve25519-dalek`: curve operations used in the crypto foundation.
- `curve25519-elligator2`: pure Rust legacy elligator2 mapping support needed to mirror `cardano-crypto-praos` batch-compatible VRF hash-to-curve behavior; rejected alternatives were FFI bindings to libsodium/cardano C code and ad-hoc local finite-field reimplementations, and this crate introduces no hidden native build requirements.
- `ed25519-dalek`: pure Rust Ed25519 signing and verification built on RustCrypto and dalek primitives.
- `getrandom`: direct access to the host OS CSPRNG for operator key generation in the standalone `cardano-cli` port and the node binary's compatibility wrapper. This replaces Unix-only `/dev/urandom` reads with the same audited RustCrypto ecosystem entropy abstraction already present transitively in the workspace, without adding native build requirements or userspace entropy fallbacks.
- `k256`: pure Rust secp256k1 elliptic curve implementation from RustCrypto; provides ECDSA (`PrehashVerifier`) and Schnorr (BIP-340) signature verification required by PlutusV2 builtins (`VerifyEcdsaSecp256k1Signature`, `VerifySchnorrSecp256k1Signature`). Rejected alternatives were `secp256k1` crate (wraps C libsecp256k1 — forbidden FFI) and `p256`/local reimplementation. Features enabled: `ecdsa`, `schnorr`, `std`. No native build requirements.
- `num-bigint`, `num-integer`, `num-traits`: pure Rust arbitrary-precision integer arithmetic from the `num` crate family; required by the consensus crate for deterministic Praos leader-value check (`check_leader_value`) and by the ledger/Plutus crates for upstream-compatible `Integer` handling in Plutus constants, `PlutusData`, CBOR bignums, and integer builtins. The upstream Haskell node uses `Natural` and `Rational` for the VRF output comparison and unbounded `Integer` for Plutus arithmetic; these crates provide the equivalent Rust primitives. Rejected alternatives were ad-hoc inline 512-bit integer implementations (error-prone and difficult to audit), fixed-width `i128`/`u256` limits (valid on-chain Plutus scripts can exceed them), and f64 floating-point arithmetic (non-deterministic across platforms). No native build requirements.
- `bls12_381`: pure Rust BLS12-381 pairing-friendly elliptic curve implementation from zkcrypto; required by CIP-0381 PlutusV3 BLS builtins (17 primitives: G1/G2 arithmetic, compress/uncompress, hash-to-curve, Miller loop, and final verify). Rejected alternatives were `blst` crate (wraps C code — forbidden FFI) and `ark-bls12-381` (large dependency tree with optional assembly backends). Features enabled: `experimental` (required for `HashToCurve` trait). Depends on `digest 0.9` which requires a companion `sha2 0.9` for hash-to-curve operations (see `sha2_09` below). No native build requirements.
- `sha2_09` (renamed `sha2 = "0.9"`): provides `digest 0.9`-compatible SHA-256 required by `bls12_381 0.8`'s `ExpandMsgXmd` hash-to-curve expander. The workspace's primary `sha2 0.10` uses `digest 0.10`, which is a different major version with incompatible trait signatures. This renamed dependency is scoped to the crypto crate's BLS12-381 module only. No native build requirements.
- `ripemd`: pure Rust RIPEMD-160 hash from RustCrypto; required by PlutusV3 builtin `Ripemd_160`. No alternative exists in the pure Rust ecosystem. No native build requirements.
- `sha2`: pure Rust SHA-512 required for Praos-compatible VRF proof-to-output hashing; rejected alternatives were hidden FFI wrappers and reimplementing SHA-512 locally, and it introduces no native build requirements.
- `sha3`: pure Rust SHA3-256 and Keccak-256 hashes from RustCrypto; required by PlutusV2 builtin `Sha3_256` and PlutusV3 builtin `Keccak_256`. No alternative exists in the pure Rust ecosystem. No native build requirements.
- `serde`: structured data interchange where handwritten or generated types require it.
- `serde_json`: JSON serialization/deserialization for node configuration files; natural companion to `serde` with no additional native requirements. Matches the Cardano node's JSON-based configuration format.
- `serde_yaml`: pure Rust YAML serialization/deserialization used by the node CLI config loader as a fallback alongside JSON (`load_effective_config`), matching operator workflows that use YAML-formatted config files. Rejected alternatives were (1) hard-failing on non-JSON config files despite upstream ecosystem YAML usage and (2) introducing a custom ad-hoc YAML parser. No native build requirements.
- `serde_cbor`: pure Rust CBOR serialization/deserialization for file-backed storage block payloads (`FileImmutable`/`FileVolatile`) to move persisted block blobs from JSON to deterministic binary encoding while retaining legacy JSON read compatibility during migration. Rejected alternatives were (1) adding ad-hoc block CBOR codec duplication inside storage before ledger-level `Block` CBOR traits exist, and (2) keeping JSON block persistence, which is larger/slower and less parity-aligned with upstream binary storage expectations. No native build requirements.
- `thiserror`: library error types.
- `eyre`: binary error reporting.
- `subtle`: constant-time comparisons for secret material.
- `tokio`: async runtime for networking and orchestration work.
- `zeroize`: deterministic zeroing of secret material on drop; already a transitive dependency via `curve25519-dalek` and `ed25519-dalek`, so adding it directly introduces no new supply chain surface.
- `bech32` (R330, added for the R326-R459 sister-tools port arc): pure Rust BIP-0173 / Bech32m encoding from `rust-bitcoin/rust-bech32` v0.11.0. MIT-licensed (allowed by `deny.toml`), zero transitive dependencies (only `std` feature on its own implementation). Foundation for `crates/tools/bech32/` (R447 relocated) which replicates the upstream `IntersectMBO/bech32` binary's CLI surface byte-for-byte; consumed across R331-R334 (Phase A.1 of the sister-tools port arc). Rejected alternatives were (1) reimplementing Bech32m locally — error-prone for a checksum format with subtle BIP-0173/Bech32m polynomial differences, and (2) FFI to the Haskell `bech32` package (forbidden per the no-FFI policy). No native build requirements.

## Sister-tools port arc — deferred candidates (R340+, R367+)

The R326-R459 sister-tools port arc will eventually need an HTTP server crate for `cardano-submit-api` (the `/api/submit/tx` endpoint) and a log-rotation crate for `cardano-tracer`. Per the R330 dependency-audit policy, those are NOT pre-added to `[workspace.dependencies]` — they'll land at the round that actually consumes them, with the transitive-dep audit done in context.

- **HTTP server for cardano-submit-api (R338-R345 land)**: chose raw `tokio::net::TcpListener` matching `crates/node/tracer/src/metrics_server.rs` — single endpoint, no TLS, no content negotiation. Zero new deps. Closed at R345.
- **HTTP server for cardano-tracer (R406 audit; R408+ land)**: chose `axum` 0.7 (different decision from cardano-submit-api). Driven by the 4-server complexity + per-server TLS termination + content negotiation. See `docs/operational-runs/2026-05-10-round-398-dep-audit-tracerenv-decision.md` D2 audit for the side-by-side justification.
- **Log rotation for cardano-tracer (deferred — R394 shipped pure-Rust rotation policy helpers without any external rotation crate)**: `tracing-appender` not needed; rotation policy is a thin `tokio::time::interval` over the existing `crates/tools/cardano-tracer/src/handlers/logs/rotator.rs` pure helpers (R447 relocated) when the IO orchestration round lands.
- **Optional fuzz-distribution (deferred to R434 — `tx-generator` Tx fuzz)**: candidate is `rand` (already a transitive dep via `ed25519-dalek`/`curve25519-dalek`, so promoting to a direct workspace dep would not add transitive surface).

Each deferred candidate will be added to `[workspace.dependencies]` only when its consumer round lands, with the `cargo deny check` + `cargo audit` checks run against the actual transitive tree rather than against speculative additions.

## Sister-tools port arc — R398 audit (cardano-tracer R398-R410 sub-arc)

Three new dependencies approved at R398 for landing during the
R398-R410 cardano-tracer subsystem build-out. Each has a full
audit-evidence + rejected-alternatives section in
`docs/operational-runs/2026-05-10-round-398-dep-audit-tracerenv-decision.md`;
the summary entries below cite the canonical recommended feature
sets.

- **`lettre` 0.11 (LANDED at R403)**: pure-Rust SMTP client for
  `cardano-tracer/src/handlers/notifications/email::create_and_send_email`
  (closed the R388 `SmtpSendStatus` carve-out). Final pin:
  `default-features = false, features = ["smtp-transport",
  "tokio1-rustls", "ring", "webpki-roots", "builder"]`. The
  recommended R398 audit-document feature list was extended at
  R403 land time with `ring` (rustls crypto provider) +
  `webpki-roots` (Mozilla CA bundle) — both required by lettre's
  `tokio1-rustls` dependency at compile time. License:
  MIT/Apache-2.0 dual. Verified at R403 via
  `cargo tree -p yggdrasil-cardano-tracer | grep -iE "openssl|native-tls"`
  — zero hits, transitive tree clean of all three banned crates
  per `deny.toml:88-91`. Rejected alternatives: hand-rolled SMTP
  client (massive RFC 5321 + STARTTLS + SASL scope, security
  risk); skip SMTP entirely (cardano-tracer never matches upstream's
  email-notification surface).
- **`axum` 0.8 + `tower` 0.5 + `rustls-pemfile` 2 (LANDED at R408)**:
  HTTP server stack for `cardano-tracer/src/handlers/metrics/{prometheus,
  monitoring, timeseries_server, servers}` — 4 separate HTTP servers
  per upstream's design + per-server TLS termination via
  `tlsSettingsChain` + `Accept`-header content negotiation. Final
  pin: `axum = { version = "0.8", default-features = false,
  features = ["http1", "tokio", "json"] }`; `tower = { version =
  "0.5", default-features = false, features = ["util"] }`;
  `rustls-pemfile = "2"`. License: all MIT. The R398 audit document
  recommended axum 0.7; at R408 land time the workspace landed
  axum 0.8 (the latest stable release; same default-features-off
  + http1+tokio+json feature pin). `hyper` is a transitive
  dependency of axum 0.8 (no direct workspace entry needed).
  Verified at R408 via
  `cargo tree -p yggdrasil-cardano-tracer | grep -iE "openssl|native-tls"`
  — zero hits, transitive tree clean of all three banned crates per
  `deny.toml:88-91`. Rejected alternatives: raw-tokio matching
  `cardano-submit-api/src/rest/web.rs` (rejected because
  cardano-tracer's per-server TLS + 4-route dispatch + content
  negotiation makes hand-rolling rustls integration four times
  structurally wrong; the cardano-submit-api precedent does not
  carry over).
- **(R398 audit version, kept as historical record)**: `axum` 0.7
  land)**: HTTP server stack for
  `cardano-tracer/src/handlers/metrics/{prometheus, monitoring,
  timeseries_server, servers}` — 4 separate HTTP servers per
  upstream's design + per-server TLS termination via
  `tlsSettingsChain` + `Accept`-header content negotiation. Pin
  `axum = { version = "0.7", default-features = false, features =
  ["http1", "tokio", "json"] }`; `hyper = { version = "1", features
  = ["http1", "server"] }`; `tower = { version = "0.5", features =
  ["util"] }`; `rustls-pemfile = "2"`. License: all MIT.
  Combined transitive surface ~10 unique TLS-stack crates after
  rustls deduplication with lettre's `tokio1-rustls`. Rejected
  alternatives: raw-tokio matching `cardano-submit-api/src/rest/web.rs`
  (rejected because cardano-tracer's per-server TLS + 4-route
  dispatch + content negotiation makes hand-rolling rustls
  integration four times structurally wrong; the cardano-submit-api
  precedent does not carry over).
- **`maud` 0.27 (R406 land alongside axum)**: compile-time HTML
  templating for `RouteDictionary::render_html` (closes the R391
  `RenderHtmlStatus` carve-out — replaces upstream's
  `Text.Blaze.Html` `renderListOfConnectedNodes`). License: MIT.
  Zero transitive deps (proc-macro only). Rejected alternative:
  hand-rolled inline renderer (viable since the HTML page is small,
  ≤ 100 LOC, but maud's compile-time template syntax catches typos
  + auto-escapes user content + zero runtime cost). Fallback if
  maud audit fails at R406: hand-rolled inline renderer kept as
  a documented carve-out option.

## Sister-tools port arc — R468 audit (cardano-tracer TLS)

Two new dependencies approved at R468 to close the long-deferred
`tls_bind_plan_status` + `tls_termination_status` descriptors in
`crates/tools/cardano-tracer/src/handlers/`.

- **`axum-server` 0.7 (LANDED at R468)**: TLS-terminated HTTP server
  for cardano-tracer Prometheus + Monitoring endpoints when the
  operator config has `force_ssl: true`. Final pin:
  `default-features = false, features = ["tls-rustls"]`. Pulls
  rustls + tokio-rustls + rustls-pki-types + rustls-webpki. Audited
  against `deny.toml:90` no-openssl ban via `cargo tree -p
  yggdrasil-cardano-tracer`: no `openssl`, `openssl-sys`,
  `native-tls`. License: MIT/Apache-2.0 (dual-licensed). Pure Rust
  transitive tree.

- **`rustls` 0.23 (LANDED at R468)**: required directly so
  `cardano-tracer`'s `serve_router_with_tls` can call
  `rustls::crypto::ring::default_provider().install_default()`
  (rustls 0.23 requires the application to explicitly choose a
  crypto provider before `ServerConfig::builder` runs). Final pin:
  `default-features = false, features = ["ring", "std", "tls12"]`.
  Picked `ring` over `aws-lc-rs` because the latter pulls
  `aws-lc-sys` C bindings (against Yggdrasil's no-FFI policy
  spirit). `ring` is license-clarified in `deny.toml` (MIT AND ISC
  AND OpenSSL). Pure Rust transitive tree (uses assembly internally
  for crypto primitives, no external C libraries linked).

## Review Required
- Any new cryptography crate.
- Any dependency that enables native code, assembly, or bundled C libraries.
- Any storage dependency that constrains on-disk format or migration strategy.
- Any new CBOR encoding/decoding library or framework used by `crates/ledger`.

## Forbidden
- Haskell runtime bindings.
- C-backed cryptography wrappers.
- Dependencies that hide FFI behind default features.

## Process
When adding a new dependency, record why it is needed, what alternatives were rejected, and whether the crate brings in any native toolchain requirements.
