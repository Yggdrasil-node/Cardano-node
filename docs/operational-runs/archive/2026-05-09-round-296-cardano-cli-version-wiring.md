# Round 296 — Wire `yggdrasil-cardano-cli` into the node binary

**Date:** 2026-05-09
**Phase:** F follow-up (concrete-implementation kickoff)
**Predecessor:** R294 + R295 (`docs/operational-runs/2026-05-09-round-294-295-cardano-cli-sweeper.md`)

## Scope

Smallest viable migration step demonstrating the new
`yggdrasil-cardano-cli` workspace crate is consumed in production.
Wires `yggdrasil_cardano_cli::helper::version_info()` into the node
binary's existing `cardano-cli version` subcommand handler.

## Why this slice (and not more)

The R295 closure-doc deferred the full implementation port (~150
upstream files of concrete behavior) to multi-week R296+ follow-up
work. The full migration is genuinely substantial because the
existing `node/src/commands/cardano_cli.rs` implementation has
cross-crate dependencies on `yggdrasil_node::config::NetworkPreset`,
the tokio runtime, and the per-binary `commands::query::run_query`
helper. Pulling those into shared crates is invasive.

R296 ships the **smallest viable migration step** that:
- Adds `yggdrasil-cardano-cli` as a `yggdrasil-node` dependency.
- Routes one subcommand path (`Version`) through the new crate.
- Proves the new crate's API is consumed in production rather than
  only declared in source.

This validates the Phase F crate skeleton with real usage and
establishes the import direction (`yggdrasil-node` → 
`yggdrasil-cardano-cli`) that subsequent R297+ rounds will use to
migrate the remaining commands as their cross-crate dependencies
get untangled.

## Files changed

### `Cargo.toml` (workspace deps)

```toml
[workspace.dependencies]
+yggdrasil-cardano-cli = { path = "crates/cardano-cli" }
 yggdrasil-consensus = { path = "crates/consensus" }
 ...
```

### `node/Cargo.toml` (node binary)

```toml
[dependencies]
 tokio.workspace = true
+yggdrasil-cardano-cli.workspace = true
 yggdrasil-consensus.workspace = true
 ...
```

### `node/src/commands/cardano_cli.rs::run_cardano_cli_command`

```rust
match action {
    CardanoCliCommand::Version => {
        // R296: Version output now sources its banner from
        // `yggdrasil_cardano_cli::helper::version_info()` so the
        // pure-Rust subset and any future Phase-F-implemented
        // commands print a consistent version string.
        println!("{}", yggdrasil_cardano_cli::helper::version_info());
        println!("network preset default: {}", network);
        Ok(())
    }
    ...
}
```

The wire output is unchanged:
```text
$ target/debug/yggdrasil-node cardano-cli --network preview version
yggdrasil-cardano-cli (pure-rust) 0.2.0
network preset default: preview
```

The first line is now produced by the new crate's `helper::version_info()`
function (which formats `yggdrasil-cardano-cli (pure-rust) <CARGO_PKG_VERSION>`)
instead of being inlined in the node binary.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (16.99s including yggdrasil-cardano-cli)
cargo lint                          clean (23.96s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated

$ target/debug/yggdrasil-node cardano-cli --network preview version
yggdrasil-cardano-cli (pure-rust) 0.2.0
network preset default: preview
```

The version output is byte-identical to the pre-R296 output (same
two lines, same format) because `helper::version_info()` returns
exactly the same `"yggdrasil-cardano-cli (pure-rust) {version}"`
shape that was previously inlined.

## Diff stat

```text
Cargo.toml                              +1 line  (workspace.dependencies entry)
node/Cargo.toml                         +1 line  (yggdrasil-cardano-cli dep)
node/src/commands/cardano_cli.rs        -3 lines / +5 lines (route Version
                                                              through helper)
docs/operational-runs/2026-05-09-round-296-... (new)
```

## Stop point — R296 closed; future R297+ deferred

| Round | Slice | Status |
|---|---|---|
| R289 | Phase F bootstrap | ✅ |
| R290 | Byron cluster | ✅ |
| R291 | Compatible cluster | ✅ |
| R292 | EraBased cluster | ✅ |
| R293 | EraIndependent cluster | ✅ |
| R294 + R295 | sweeper + read regrade + TopHandler | ✅ |
| **R296** | **Wire helper::version_info into node Version handler** | ✅ |
| R297+ | Migrate remaining ShowUpstreamConfig + QueryTip impls | future |

R297+ will migrate the `ShowUpstreamConfig` (depends on
`yggdrasil_node::config::NetworkPreset`; needs trait-based abstraction
in the new crate) and `QueryTip` (depends on tokio runtime + the
node binary's `commands::query::run_query`; needs interface
extraction) commands. Both are multi-day refactors; deferred until
the operator schedules them.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R294 + R295 (`docs/operational-runs/2026-05-09-round-294-295-cardano-cli-sweeper.md`)
- New crate: `crates/cardano-cli/`
- Wired API: `yggdrasil_cardano_cli::helper::version_info`
