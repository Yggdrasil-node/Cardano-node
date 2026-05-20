# Guidance for the yggdrasil-node binary source tree

Keep this directory as the executable shell for the node. The heavy runtime,
sync, config, tracing, NtC/NtN server, Plutus, and block-producer logic lives
in sibling crates under `crates/node/`.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream `.hs`
file by snake_case basename (with directory-prefix fallback for sibling
collisions) OR carries a `## Naming parity` docstring stanza ending in
`**Strict mirror:** none.` plus the upstream symbol(s)/file(s) the helper
surfaces. CI gate: `python3 scripts/check-strict-mirror.py`. Allowlist
source-of-truth: [`docs/strict-mirror-audit.tsv`](../../../../docs/strict-mirror-audit.tsv).

## Scope

- `main.rs` owns `clap` parsing and top-level subcommand dispatch only.
- `cli.rs` defines the public binary CLI surface.
- `commands/` adapts CLI subcommands into sibling-crate APIs.
- `run_node.rs` is the executable runtime entry shell that assembles shared
  state, metrics, storage, governor, sync, inbound serving, NtC serving, and
  optional forging.
- `startup.rs` and `ledger_peers.rs` hold binary-local startup assembly that
  depends on both operator config and recovered storage state.
- `handlers/` contains top-level process handlers such as shutdown.
- `lib.rs` preserves symbol-level compatibility re-exports for older callers;
  new code should depend on the extracted sibling crates directly.
- `main_tests.rs` and `../tests/` cover the binary integration boundary.

## Rules

- Do not move ledger, consensus, network protocol, tracer transport, or storage
  business logic back into this directory.
- Do not add `src/bin/` developer diagnostics here. Forensic tools belong under
  `crates/tools/` with the closest upstream sister-tool surface.
- Keep `commands/cardano_cli.rs` as a dispatcher and compatibility wrapper.
  Reusable key, address, TextEnvelope, transaction, LSQ query-helper,
  and subcommand-runner logic belongs in `crates/tools/cardano-cli`.
- Keep strict transaction `--tx-hex` parsing in
  `yggdrasil_cardano_cli::era_based::transaction::run::decode_tx_hex_arg`.
  `commands/submit_tx.rs` may re-export it for compatibility, but should not
  carry a forked parser.
- Keep LocalTxSubmission socket driving in
  `yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient`; `commands/submit_tx.rs`
  should stay a thin top-level command adapter.
- Keep `commands/query.rs` as the node-local NtC bridge for query variants
  that still need this binary surface. Pure helpers such as SystemStart UTC
  formatting and lenient hex argument decoding come from
  `yggdrasil_cardano_cli::lsq`.
- For migrated LSQ variants, delegate CBOR query encoding and reply decoding
  through `yggdrasil_cardano_cli::lsq::{encode_query, decode_query_result}`.
  Parameterized query envelopes such as `query-utxo`,
  `query-reward-balance`, and `query-stake-pool-params` live in
  `yggdrasil_cardano_cli::lsq` too; do not reintroduce node-local copies.
- Add reusable behavior to the appropriate sibling crate:
  `config`, `genesis`, `sync`, `runtime`, `tracer`, `ntc-server`,
  `ntn-server`, `plutus-eval`, or `block-producer`.
- Reuse `yggdrasil-node-config` for node-role classification and
  block-producer credential field policy. This source tree may adapt those
  reports for traces/JSON and load credential files behind the forge feature.
- Keep `commands/validate_config.rs` as a report assembler: it calls
  `node_config_preflight_report` for pure config checks, then appends
  diagnostics that require binary-owned storage recovery, peer snapshot, or
  forge-feature behavior.
- Keep this tree focused on CLI ergonomics, path resolution, startup assembly,
  and operator-facing orchestration.
- Preserve installed preset resolution in `commands/configuration.rs`: an
  explicit `YGGDRASIL_CONFIG_ROOT` wins, release installs probe
  `<prefix>/share/yggdrasil/configuration`, and source builds still fall back
  to root `configuration/`.
- When a subcommand grows beyond argument adaptation, move the reusable core to
  the crate that owns the domain and leave a thin command wrapper here.
- Public orchestration helpers must include Rustdocs when startup, shutdown, or
  recovery semantics are non-obvious.

## Current Layout

This directory is not a stale post-reorganization location. It is the active
workspace member for the shipped `yggdrasil-node` binary. The current files are
expected:

- Binary entry: `main.rs`, `cli.rs`, `commands.rs`, `commands/*.rs`
- Runtime assembly: `run_node.rs`, `startup.rs`, `ledger_peers.rs`
- Process handlers: `handlers.rs`, `handlers/shutdown.rs`
- Compatibility exports: `lib.rs`
- Binary-local tests: `main_tests.rs`

If future extractions empty one of these responsibilities, delete or move only
that responsibility; do not remove the crate while the workspace still ships
the `yggdrasil-node` binary.

## Official Upstream References

- Node runtime and top-level wiring:
  `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node`
- Node configuration handling:
  `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Configuration`
- Node tracing system:
  `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Tracing`
- Consensus diffusion integration:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-diffusion`
- Cardano-specific consensus integration:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src`
- Transaction submit API:
  `.reference-haskell-cardano-node/cardano-submit-api`
- Official network configuration files:
  `.reference-haskell-cardano-node/configuration`
