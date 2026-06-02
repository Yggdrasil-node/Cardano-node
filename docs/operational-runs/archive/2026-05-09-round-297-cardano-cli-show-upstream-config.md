# Round 297 — Migrate `ShowUpstreamConfig` into `yggdrasil-cardano-cli`

**Date:** 2026-05-09
**Phase:** F follow-up (concrete-implementation porting)
**Predecessor:** R296 (`docs/operational-runs/2026-05-09-round-296-cardano-cli-version-wiring.md`)

## Scope

Move the `ShowUpstreamConfig` implementation (~70 lines of helpers
+ JSON-emit body) from `node/src/commands/cardano_cli.rs` into
`crates/cardano-cli/src/environment.rs`. The node binary's
dispatcher becomes a thin shell that handles `NetworkPreset →
&str` conversion before delegating to the new crate.

This is the second concrete-implementation slice of the cardano-cli
migration (R296 wired the `Version` banner; R297 ports the path
resolution + JSON output). `QueryTip` remains in the node binary
because it depends on the binary's tokio runtime + the LSQ
`commands::query::run_query` helper; that migration needs a trait-
based abstraction in `yggdrasil-cardano-cli` and is deferred to
R298+.

## Cross-crate dependency strategy

`node/src/commands/cardano_cli.rs::resolve_upstream_reference_paths`
took `network: NetworkPreset` and matched on the enum to derive the
on-disk sub-directory name. Moving the function as-is would require
`yggdrasil-cardano-cli` to import `yggdrasil_node::config::NetworkPreset`,
which inverts the dependency direction (node → cardano-cli, but cardano-
cli → node).

Resolution: refactor the new-crate API to take `network_dir: &str`
(the on-disk sub-directory name: `"mainnet"` / `"preprod"` /
`"preview"`) and `fallback_magic: u32` as parameters. The node
binary's dispatcher converts `NetworkPreset` to `&str` via a small
helper `network_dir(NetworkPreset)` before calling.

This keeps the new crate independent of any node-binary types and
establishes the pattern future migrations will follow: pass plain
data (strings, numbers, paths) across the crate boundary; let the
binary handle enum/preset conversions.

## Files changed

### `crates/cardano-cli/src/environment.rs`

Three new public functions added (migrated from
`node/src/commands/cardano_cli.rs`):

```rust
pub fn resolve_upstream_reference_paths(
    network_dir: &str,
    upstream_config_root: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)>;

pub fn extract_reference_network_magic(
    config_path: &Path,
    fallback_magic: u32,
) -> u32;

pub fn run_show_upstream_config(
    network_name: &str,
    config_path: &Path,
    topology_path: &Path,
    network_magic: u32,
) -> Result<()>;
```

The first two preserve their existing behavior (path resolution,
network-magic extraction with the `TestnetMagic → NetworkMagic →
ShelleyGenesisFile.networkMagic → fallback` precedence). The third
is the JSON-emit body that prints the config snapshot.

### `node/src/commands/cardano_cli.rs`

Reduced from 163 lines to ~85 lines. The dispatcher now:

1. Maps `NetworkPreset` → `&str` via the local `network_dir` helper.
2. Calls `environment::resolve_upstream_reference_paths(dir, …)` to
   get config + topology paths.
3. Calls `environment::extract_reference_network_magic(&config_path,
   network.network_magic())` for the network-magic resolution.
4. Per-arm dispatch:
   - `Version` → `helper::version_info()` (R296).
   - `ShowUpstreamConfig` → `environment::run_show_upstream_config(…)`
     (R297, new).
   - `QueryTip` → still inline (uses tokio runtime + LSQ helper;
     R298+).

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.51s)
cargo lint                          clean (Finished `dev` profile in 5.78s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 dev/test/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 dev/test/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated

cargo build --release -p yggdrasil-node
                                    Finished `release` profile in 1m 01s

$ target/release/yggdrasil-node cardano-cli --network preview show-upstream-config
{
  "config": "node/configuration/preview/config.json",
  "network": "preview",
  "network_magic": 2,
  "topology": "node/configuration/preview/topology.json"
}
```

The JSON wire output is byte-identical to the pre-R297 form — same
4 keys, same alphabetical ordering, same values. The implementation
moved across the crate boundary; the operator-visible behavior did
not change.

## Diff stat

```text
crates/cardano-cli/src/environment.rs   +95 lines (3 functions migrated)
node/src/commands/cardano_cli.rs        -78 lines / +35 lines (-43 net;
                                                                dispatcher
                                                                shrank from
                                                                163 to ~85
                                                                lines)
docs/operational-runs/2026-05-09-round-297-... (new)
```

## Stop point — R297 closed

| Round | Slice | Status |
|---|---|---|
| R296 | Wire helper::version_info into Version arm | ✅ |
| **R297** | **Migrate ShowUpstreamConfig into env helpers** | ✅ |
| R298+ | Migrate QueryTip (needs LSQ-client abstraction) | future |

R298+ — porting `QueryTip` — requires either:

1. A trait `LsqClient` in `yggdrasil-cardano-cli` that the node
   binary implements with its tokio + Unix-socket connector. The
   trait would carry methods like `query_tip(&self) -> Result<…>`,
   and `yggdrasil-cardano-cli::compatible::run::run_query_tip`
   would consume it.
2. Or extracting `commands::query::run_query` itself out of the
   node binary into a shared crate (e.g. `yggdrasil-network` or a
   new `yggdrasil-lsq-client`).

Both are multi-day refactors with broader downstream impact than
the R296/R297 migrations. Deferred until the operator schedules.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R296 (`docs/operational-runs/2026-05-09-round-296-cardano-cli-version-wiring.md`)
- Migrated APIs:
  - `yggdrasil_cardano_cli::environment::resolve_upstream_reference_paths`
  - `yggdrasil_cardano_cli::environment::extract_reference_network_magic`
  - `yggdrasil_cardano_cli::environment::run_show_upstream_config`
