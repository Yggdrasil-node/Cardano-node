# Guidance for the pure-Rust port of upstream `cardano-cli`.

Focus on **strict 1:1 file-mirror parity** with upstream
`cardano-cli/cardano-cli/src/Cardano/CLI/*.hs` and the operator-
tooling subset that `yggdrasil-node` actually needs. The crate
landed across R289–R295 as the Phase F bootstrap; concrete
implementations port over multi-week R296+ follow-up work.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 scripts/check-strict-mirror.py`
(warn-only since R275; fail-build at R288). Allowlist source-of-truth:
[`docs/strict-mirror-audit.tsv`](../../docs/strict-mirror-audit.tsv).

This crate has the densest file-mirror surface in the workspace
(237 Rust files mirror 180 upstream `.hs` files; the +57 are
parent-shell organizers required by the Rust module-tree convention).
Every leaf carries a strict-mirror block; every parent shell carries
a `**Strict mirror:** none.` block naming the sub-tree it
aggregates.

## Scope

Mirrors upstream `Cardano.CLI.*` subtree:

- Byron operator surface (`byron/`, 10 leaf files).
- Era-shared command surface (`compatible/`, 21 leaf files).
- Era-aware commands (`era_based/`, 57 leaf files across 25 sub-directories).
- Era-independent commands (`era_independent/`, 34 leaf files).
- Operator-tooling utilities (`type/` × 33, `legacy/` × 5, plus
  `io/`, `json/`, `os/`, `read/`, `run/`, `option/`).
- Top-level namespace files: `command.rs`, `run.rs`, `parser.rs`,
  `render.rs`, `helper.rs`, `environment.rs`, `option.rs`,
  `orphan.rs`, `top_handler.rs`.

The crate exposes `yggdrasil_cardano_cli::*` as a workspace-internal
library; the `yggdrasil-node` binary consumes specific helpers (see
"Integration with `node` crate" below). There is no separate
`yggdrasil-cardano-cli` binary today — operators continue to use
`yggdrasil-node cardano-cli <subcommand>` until R298+ migrates the
remaining commands and a separate binary is justified.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `Cardano.CLI.*`
  file by snake_case basename or carry a `## Naming parity` block.
  The CI drift-guard (`check-strict-mirror.py --fail-on-violation`)
  enforces this since R288.
- Cross-crate API design: take plain data (`&str` for network names,
  `u32` for network magic, `&Path` for file paths) at the public
  surface. **Do not import** `yggdrasil_node::config::NetworkPreset`
  or any other binary-specific type — that would invert the
  dependency direction. The node binary handles enum/preset
  conversions before calling.
- `QueryTip` and other LSQ-client subcommands must NOT pull the
  tokio runtime or `commands::query::run_query` into this crate
  directly; an `LsqClient` trait abstraction is the migration path.
- Wire-format byte-equivalence with upstream `cardano-cli` is the
  acceptance gate for any concrete implementation. Operators must
  be able to swap the upstream binary for `yggdrasil-cardano-cli`
  for any covered subcommand without a script change.

## Official Upstream References

- [cardano-cli library tree](.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/) — the strict-mirror source.
- [cardano-cli library cabal file](.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/cardano-cli.cabal) — module-list authority.
- [Cardano API serialise text-envelope](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Api/SerialiseTextEnvelope.hs) — text-envelope codec used by key/cert files.
- [cardano-cli release notes / changelog](.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/CHANGELOG.md) — track upstream subcommand additions.

## Current Status (post-R297)

- **R289 — bootstrap.** Workspace crate skeleton + 8 top-level
  namespace files. Ships compileable placeholder enums per leaf.
- **R290–R295 — full strict-mirror coverage.** Every upstream
  `.hs` file under `Cardano/CLI/*` has a Rust counterpart; every
  parent shell carries `## Naming parity` synthesis declarations.
- **R296 — Version banner.** `yggdrasil_cardano_cli::helper::version_info()`
  returns the canonical `"yggdrasil-cardano-cli (pure-rust) {version}"`
  string. The node binary's `cardano-cli version` subcommand calls
  it.
- **R297 — ShowUpstreamConfig migration.** `environment::resolve_upstream_reference_paths`,
  `environment::extract_reference_network_magic`, and
  `environment::run_show_upstream_config` migrated from
  `node/src/commands/cardano_cli.rs`. Wire output byte-identical.
- **R503 (Phase 5 follow-on, 2026-05) — `run::run_command`
  Version arm wired.** `Command::Version` now prints
  `helper::version_info()` (was R289-stub "skeleton…"
  placeholder). `Command::ShowUpstreamConfig` and
  `Command::QueryTip` arms updated with structured deferral
  messages explaining WHY (variant needs extension; library
  needs tokio+yggdrasil-network deps) rather than the prior
  generic "R29x scheduled" comment. 3 unit tests
  (`run::tests`) cover all three arms — pinning that the
  Version path actually emits the banner and that the
  deferral messages stay stable. Library `run_command` is
  now operational for the simplest of the three commands;
  ShowUpstreamConfig + QueryTip remain blocked on Command-
  variant + deps work tracked in their respective error
  messages.

### Phase F operator surface (2026-05 — landed in the binary)

While THIS crate (the library `yggdrasil-cardano-cli`) is still
in its R289 placeholder state for most subcommand families, the
binary crate (`yggdrasil-node`) ships a `cardano-cli` subcommand
group with 15 operator-essential commands that route through
`crates/node/yggdrasil-node/src/commands/cardano_cli.rs` into
the existing node helpers (`commands/query.rs`,
`commands/submit_tx.rs`, `yggdrasil_crypto`,
`yggdrasil_ledger::compute_tx_id`, the `bech32` workspace crate,
and `/dev/urandom`). The split is intentional: the binary
surface is "operator-ready"; the library surface stays
placeholder-stage and waits for the C-arc migration.

Operator surface mapped to upstream `cardano-cli` (35 subcommands):

| `yggdrasil-node cardano-cli ...`            | `cardano-cli ...`                          |
| ------------------------------------------- | ------------------------------------------ |
| **Introspection**                           |                                            |
| `version`                                   | `version`                                  |
| `show-upstream-config`                      | n/a (Yggdrasil helper)                     |
| **Query (LocalStateQuery)**                 |                                            |
| `query-tip`                                 | `query tip`                                |
| `query-utxo --address` / `--tx-in`          | `query utxo --address` / `--tx-in`         |
| `query-protocol-parameters`                 | `query protocol-parameters`                |
| `query-stake-pools`                         | `query stake-pools`                        |
| `query-stake-distribution`                  | `query stake-distribution`                 |
| `query-current-era`                         | n/a (informational)                        |
| `query-chain-block-no`                      | n/a (introspection)                        |
| `query-system-start`                        | n/a (introspection)                        |
| `query-current-epoch`                       | n/a (informational)                        |
| `query-expected-network-id`                 | n/a (network-preflight check)              |
| `query-era-history`                         | `query era-history` (opaque CBOR)          |
| `query-treasury-and-reserves`               | n/a (Yggdrasil helper for pots)            |
| `query-drep-stake-distr`                    | `query drep-stake-distribution`            |
| `query-constitution`                        | `query constitution`                       |
| `query-gov-state`                           | `query gov-state`                          |
| `query-drep-state`                          | `query drep-state`                         |
| `query-account-state`                       | n/a (Yggdrasil helper)                     |
| `query-genesis-delegations`                 | n/a (Byron-only helper)                    |
| `query-stability-window`                    | n/a (Yggdrasil helper)                     |
| `query-num-dormant-epochs`                  | n/a (Conway dormancy helper)               |
| `query-deposit-pot`                         | n/a (Yggdrasil helper)                     |
| `query-ledger-counts`                       | n/a (Yggdrasil helper)                     |
| `query-reward-balance --account`            | n/a (Yggdrasil helper)                     |
| `query-delegations-and-rewards --credential`| n/a (Yggdrasil helper)                     |
| `query-stake-pool-params --pool-hash`       | `query pool-params`                        |
| **Transaction**                             |                                            |
| `transaction-submit`                        | `transaction submit`                       |
| `transaction-txid`                          | `transaction txid`                         |
| `transaction-sign` (single-signer)          | `transaction sign` (subset)                |
| **Keys + Addresses**                        |                                            |
| `address-key-gen`                           | `address key-gen`                          |
| `address-key-hash`                          | `address key-hash`                         |
| `address-build`                             | `address build`                            |
| `stake-address-key-gen`                     | `stake-address key-gen`                    |
| `stake-address-build`                       | `stake-address build`                      |

Not yet wired (each gated on a substantive new primitive — full
tx construction with input selection + fees, new LSQ queries
with Rust-side response decoders, per-era certificate encoders,
era-history Interpreter CBOR decoder): `query leadership-schedule`,
`query stake-snapshot`, `query slot-number`, `transaction build`,
`transaction build-raw`, `transaction view`,
`stake-pool registration-certificate`.

The C-arc migration plan (below) is the path that lifts these
binary-side handlers into THIS library crate so a standalone
`cargo install --path crates/tools/cardano-cli` produces a
parity-compatible `cardano-cli` binary independent of the node
runtime.

## Migration roadmap (R298+ deferred)

Concrete subcommand implementations port from upstream over multi-
week follow-up rounds. Each port requires:

1. Read upstream `Cardano.CLI.<Cluster>.<Subcommand>.Run` for the
   semantic body.
2. Implement the corresponding `crates/cardano-cli/src/<cluster>/<subcommand>/run.rs`
   leaf with byte-equivalent output.
3. Migrate the per-subcommand integration from
   `node/src/commands/cardano_cli.rs` (or a sibling node-binary
   module) into the new crate, taking plain-data parameters at the
   crate boundary.
4. Add a parity-matrix entry in `docs/parity-matrix.json` tracking
   the byte-equivalence verification status against the upstream
   binary at `.reference-haskell-cardano-node/install/bin/cardano-cli`.

The pending migrations as of R297 closure:

| Subcommand | Status | Migration shape |
|---|---|---|
| `Version` | ✅ migrated (R296) | direct `helper::version_info()` call |
| `ShowUpstreamConfig` | ✅ migrated (R297) | `environment::run_show_upstream_config` (path resolution + JSON) |
| `QueryTip` | future R298+ | needs `trait LsqClient` abstraction in this crate; node binary supplies the impl with its tokio + Unix-socket connector |

Subcommands beyond the current 3-command surface (the full upstream
`cardano-cli` has hundreds of subcommands across Byron / Compatible
/ EraBased / EraIndependent / Legacy) are out-of-scope until
operator demand prioritizes specific port targets. The leaf
placeholder enums make incremental ports cheap (one subcommand at
a time) without breaking the public path.

## Integration with `node` crate

`node/src/commands/cardano_cli.rs` is the binary-side dispatcher.
It consumes:

- `yggdrasil_cardano_cli::helper::version_info()` — `Version` arm.
- `yggdrasil_cardano_cli::environment::{resolve_upstream_reference_paths,
  extract_reference_network_magic, run_show_upstream_config}` —
  `ShowUpstreamConfig` arm.
- `crate::commands::query::run_query` (binary-local) — `QueryTip`
  arm; pending migration to a trait abstraction.

The dispatcher's `network_dir(NetworkPreset) -> &'static str` helper
maps the binary's `NetworkPreset` enum to the on-disk sub-directory
name (`"mainnet"` / `"preprod"` / `"preview"`) before calling into
this crate.

## Module layout (top-level only; sub-trees track upstream 1:1)

- `command.rs` — top-level dispatch enum (3 variants today; grows
  per R298+ migration).
- `run.rs` — top-level dispatcher (forwards to per-cluster runners).
- `parser.rs` — clap-based parser shell (currently `NotYetMigrated`
  stub; node binary uses its own parser at `node/src/cli.rs`
  pending the full per-cluster sub-parser tree).
- `render.rs` — output formatting (`render_json`, `render_text`).
- `option.rs` — shared option parsers (`parse_socket_path`,
  `parse_network_magic`).
- `helper.rs` — `version_info()` + (future) text-envelope helpers.
- `environment.rs` — env-var fallback chains + the migrated
  `resolve_upstream_reference_paths` /
  `extract_reference_network_magic` / `run_show_upstream_config`.
- `orphan.rs` — strict-mirror placeholder for upstream
  `Cardano.CLI.Orphan.hs` (Rust coherence rules eliminate the
  upstream orphan-instance need; file kept for parity).
- `top_handler.rs` — `top_handler<F>` panic + structured-error
  catch wrapper around a `main`-style entry function. Mirrors
  upstream `Cardano.CLI.TopHandler.toplevelExceptionHandler`.

Sub-trees (`byron/`, `compatible/`, `era_based/`, `era_independent/`,
`type/`, `legacy/`, `io/`, `json/`, `os/`, `read/`, `run/`,
`option/`) follow the upstream directory layout 1:1; see
`docs/strict-mirror-audit.tsv` for the full per-file verdict table.
