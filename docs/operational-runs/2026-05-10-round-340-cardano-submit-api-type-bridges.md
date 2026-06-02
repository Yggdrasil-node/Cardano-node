---
title: 'R340: cardano-submit-api type bridges — cli/types, cli/parsers, rest/types, rest/parsers'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-340-cardano-submit-api-type-bridges/
---

# Round 340 — cardano-submit-api type bridges

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R339`](2026-05-10-round-339-cardano-submit-api-foundations.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R340 lands the bridge between Yggdrasil's flat `parser::Args` argv
representation and upstream's typed parser surface
(`TxSubmitCommand`/`TxSubmitNodeParams`). Four production modules
graduate from R335 stub-only to full upstream port:

1. **`cli/types.rs`** — full upstream port of `Cardano.TxSubmit.CLI.Types`:
   `ConfigFile`, `GenesisFile`, `SocketPath` (PathBuf newtypes),
   `ConsensusModeParams` (Cardano-mode-only enum, `#[default]`-marked),
   `NetworkId` (Mainnet | Testnet u32), `TxSubmitNodeParams` (6-field
   record), `TxSubmitCommand` (Run | Version sum). `From<NetworkMagic>
   for NetworkId` glues parser surface into typed surface.
2. **`cli/parsers.rs`** — `into_command(args) → Result<TxSubmitCommand,
   CommandError>` mirroring upstream `pTxSubmit envCli`. Per-field
   bridge fns (`config_file_from_args`, `socket_path_from_args`,
   `network_id_from_args`, `metrics_port_from_args`) match upstream's
   per-field parsers (`pConfigFile`, `pSocketPath'`, `pNetworkId`,
   `pMetricsPort`). Defaults: `8090` web port, `8081` metrics port,
   `127.0.0.1` listen address — all matching upstream constants.
3. **`rest/types.rs`** — `WebserverConfig { host, port }` mirroring
   upstream `data WebserverConfig`. `to_socket_addr` mirrors
   `toWarpSettings`'s role of resolving the loose config into a
   server-binding-ready value. Wildcard host strings (`*`, `0.0.0.0`,
   `::`) all resolve to the unspecified IPv4 address.
4. **`rest/parsers.rs`** — `from_args(args, default_port) →
   WebserverConfig` mirroring upstream `pWebserverConfig
   defaultPort`'s parser combinator. Listen-address default
   (`127.0.0.1`) constant exposed as `DEFAULT_LISTEN_ADDRESS`.

`lib.rs::run()` now validates argv → `TxSubmitCommand` before its
sentinel error so missing-flag errors surface clearly to operators
even before R341 lands the actual HTTP listener.

## Carve-outs

- **Cardano.CLI.Environment.EnvCli**: upstream's parser threads
  process-environment defaults (e.g. `CARDANO_NODE_SOCKET_PATH`) into
  network-id selection. Yggdrasil's CLI surface is environment-blind
  for this binary; `--mainnet|--testnet-magic` is a hard requirement.
  Documented in `cli/parsers.rs` strict-mirror docstring.
- **`Options.Applicative.Parser` combinators** (`Opt.flag'`, `<**>`,
  `<|>`, `<*>`): Yggdrasil's flag-level parsing is centralized in
  `crate::parser::parse_args`; the `cli::parsers` module is a
  flat-mapping bridge over the already-parsed `Args` struct. Documented
  in `cli/parsers.rs` strict-mirror docstring.
- **Warp.HostPreference / Warp.Port / Warp.Settings**: under axum the
  bridge type is `std::net::SocketAddr`; the semantic role is the
  same. Documented in `rest/types.rs` strict-mirror docstring.
- **`Cardano.Api.SocketPath`'s polymorphic `File 'Out` envelope**:
  collapsed to a direct `PathBuf` newtype since the polymorphic
  `File 'In/'Out` machinery has no semantic role at this surface.
  Documented in `cli/types.rs` strict-mirror docstring.

## Diff inventory

- `crates/cardano-submit-api/src/cli/types.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/cli/parsers.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/rest/types.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/rest/parsers.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/lib.rs` — `run()` validates argv →
  `TxSubmitCommand` before sentinel error.
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed; `next_milestone` advanced to
  R341.

## Test inventory

| File                                    | New tests | Total |
|-----------------------------------------|-----------|-------|
| `cli/types.rs`                          | 7         | 7     |
| `cli/parsers.rs`                        | 11        | 11    |
| `rest/types.rs`                         | 7         | 7     |
| `rest/parsers.rs`                       | 4         | 4     |
| **Round contribution**                  | **+29**   |       |
| Crate total (incl. R335/R339)           |           | 80 unit + 4 golden + 1 doctest = **85**|

Workspace contribution: 5,023 → 5,052 (+29).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,052 passed
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 dev/test/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 dev/test/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 85 tests pass
```

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | done        |
| R340  | Type bridges: cli/types, cli/parsers, rest/types, rest/parsers     | **this**    |
| R341  | Rest/Web + Web.hs (axum router; CBOR; LocalTxSubmission; full TraceSubmitApi instances) | next        |
| R342  | Metrics.hs Prometheus surface (port-occupied retry)                | scheduled   |
| R343  | Integration: end-to-end soak vs upstream binary                    | scheduled   |
| R344  | Closeout: AGENTS.md + CHANGELOG + parity-matrix `verified_11_0_1`  | scheduled   |

## Notes for future readers

The decision to flatten upstream's polymorphic `File 'In/'Out`
envelope around `SocketPath` to a direct `PathBuf` newtype was made
because the polymorphism carries no semantic information at this
surface — tx-submit only consumes the path as a connect target. If
future rounds need the polymorphism (e.g. for multi-stream socket
APIs), the upgrade path is straightforward (introduce a phantom
`Marker` type-parameter on `SocketPath`).

Wildcard host resolution accepts three sentinels (`*`, `0.0.0.0`,
`::`) all mapping to IPv4 unspecified. This matches upstream's
`Warp.HostPreference` parsing semantics; an IPv6-unspecified
binding (`::` → `[::]:port`) is currently *not* a separate code
path. If R341 needs IPv6-specific binding behavior, that's the
extension point.
