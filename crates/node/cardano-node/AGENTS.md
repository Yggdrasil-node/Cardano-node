# yggdrasil-node binary crate

This crate is the active executable crate for the shipped `yggdrasil-node`
binary. It is not a stale location after the Wave 5 reorganization; it owns the
thin CLI/runtime shell. Repository-root `configuration/` owns network
operator bundles, while `dev/{scripts,evidence,reference,test}/` owns
operator helpers, evidence harnesses, reference refresh tooling, and validators.

## Scope

- `Cargo.toml` for the `yggdrasil-node` package and its feature surface.
- `src/` for CLI parsing, command wrappers, startup assembly, runtime entry,
  process handlers, compatibility re-exports, and binary-local tests.
- `tests/` for integration tests at the binary and operator-script boundary.

## Rules

- Keep this crate thin and integration-focused.
- Do not add ledger, consensus, storage, wire-protocol, tracer transport, or
  block-production business logic here.
- Do not add developer diagnostics under this crate; diagnostics and forensic
  tools belong under `crates/tools/` with the upstream sister-tool they mirror.
- Keep reusable `cardano-cli` key, address, TextEnvelope, transaction,
  LSQ query-helper, and subcommand-runner logic in
  `crates/tools/cardano-cli`; this crate may keep only thin compatibility
  wrappers and node-local query/submission bridges.
- Strict transaction `--tx-hex` parsing is owned by
  `yggdrasil_cardano_cli::era_based::transaction::run::decode_tx_hex_arg`;
  `src/commands/submit_tx.rs` may re-export it but must not fork the parser.
- LocalTxSubmission socket driving is owned by
  `yggdrasil_cardano_cli::lsq_tokio::TokioLsqClient`; `src/commands/submit_tx.rs`
  is only the top-level `submit-tx` compatibility adapter.
- Keep pure query helpers such as SystemStart UTC formatting and lenient
  hex argument decoding in `yggdrasil_cardano_cli::lsq`; do not fork them
  back into `src/commands/query.rs`.
- Keep migrated LSQ wire plans in `yggdrasil_cardano_cli::lsq`. The node
  query bridge may map `QueryCommand` variants to `NtcQuery`, but it should
  not duplicate migrated query CBOR encoders or reply decoders. Parameterized
  query envelopes such as `query-utxo`, `query-reward-balance`, and
  `query-stake-pool-params` also belong in the shared LSQ plan.
- Add reusable logic to the sibling owner crate under `crates/node/`:
  `config`, `genesis`, `sync`, `runtime`, `tracer`, `ntc-server`,
  `ntn-server`, `plutus-eval`, or `block-producer`.
- Node-role classification and Shelley credential-field policy are config
  crate APIs; keep only binary-local credential file loading and trace/report
  adaptation here.
- Config-only `validate-config` invariants belong in
  `yggdrasil-node-config::node_config_preflight_report`; this crate may add
  storage recovery, peer-snapshot, and forge-feature diagnostics around that
  shared report.
- Keep operator scripts in sync with `docs/MANUAL_TEST_RUNBOOK.md` and the
  metrics exposed by `yggdrasil-node-tracer`; operator helpers live under
  `dev/scripts/` and evidence harnesses under `dev/evidence/`.
- Keep root `configuration/` presets byte/source traceable to the official
  reference bundle under `.reference-haskell-cardano-node/install/share/<network>/`.
- Keep `--network` preset resolution usable from both source checkouts and
  release installs: `YGGDRASIL_CONFIG_ROOT` overrides, installed binaries
  probe `<prefix>/share/yggdrasil/configuration`, and the source checkout
  falls back to root `configuration/`.
- Preserve the shipped binary name and package name: `yggdrasil-node`.

## Current Layout

- Active workspace member: root `Cargo.toml` lists
  `crates/node/cardano-node`.
- Active binary: Cargo's implicit target is `src/main.rs` for package
  `yggdrasil-node`; release workflows and Docker packaging copy
  `target/release/yggdrasil-node`.
- Active compatibility library: `src/lib.rs` re-exports selected sibling-crate
  symbols for older callers; new code should import sibling crates directly.
- No `src/bin/` diagnostic helpers are expected here; the `dump_block`
  forensic helper lives under `crates/tools/db-analyser`.
- Active operator assets: release/repro workflows and Docker copy root
  `configuration/` plus the required `dev/` helper subsets.

If a file here looks like pre-extraction runtime logic, verify whether it is
only a CLI adapter or startup shell. Real reusable runtime code belongs in
`crates/node/runtime`, sync code in `crates/node/sync`, config in
`crates/node/config`, tracing in `crates/node/tracer`, NtC/NtN serving in
`crates/node/{ntc-server,ntn-server}`, Plutus evaluation in
`crates/node/plutus-eval`, and forging in `crates/node/block-producer`.

## Operator Assets

- `configuration/{mainnet,preprod,preview}/` mirrors the upstream operator
  share bundles.
- `dev/evidence/parallel_blockfetch_soak.sh` is the canonical runbook section 6.5
  multi-peer BlockFetch evidence gate.
- `dev/scripts/run_preview_real_pool_producer.sh`,
  `run_preview_active_pool_signoff.sh`,
  `register_preview_generated_pool.sh`, and
  `preview_pool_activation_status.sh` are preview producer parity helpers.
- `dev/scripts/run_preprod_real_pool_producer.sh` and
  `run_mainnet_real_pool_producer.sh` are operator rehearsal templates.
- `dev/evidence/compare_tip_to_haskell.sh`, `compare_submit_api_to_upstream.sh`, and
  `compare_db_truncater_to_upstream.sh` are parity comparison harnesses.
- `dev/scripts/run-tools.sh` launches sister-tool binaries from `crates/tools/`.

## Official Upstream References

- Node integration repository:
  `.reference-haskell-cardano-node/cardano-node/`
- Default network configuration files:
  `.reference-haskell-cardano-node/configuration/`
- Release operator bundle:
  `.reference-haskell-cardano-node/install/share/`
- Transaction submit API:
  `.reference-haskell-cardano-node/cardano-submit-api/`
- Consensus integration:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/`
- Network diffusion layer:
  `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/`
