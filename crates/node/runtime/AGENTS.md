# yggdrasil-node-runtime orchestration crate

## Scope

Wave 5 PR 12 extracted the reusable runtime orchestration from the
`yggdrasil-node` binary. This crate wires the node sub-crates together for a
running Cardano node, matching the role of upstream `Cardano.Node.Run`.

It owns the governor loop, mempool tx-submission service, ledger judgement,
peer management, sync session driver, block-producer loop, bootstrap helpers,
and runtime integration tests.

The `yggdrasil-node` binary's `src/` tree remains the executable shell:
`main.rs`, `cli.rs`, command wrappers, startup assembly, process handlers, and
compatibility re-exports. Reusable runtime behavior belongs in this crate.

Sync session positioning is explicit: every freshly connected ChainSync client
must send `MsgFindIntersect`, including `[Origin]`, before `MsgRequestNext`.
Skipping the Origin intersection reproduced mainnet relay disconnects before
any blocks were accepted; keep `sync_session::synchronize_chain_sync_to_point`
aligned with upstream cursor-positioning behavior.

## Rules

- This crate is the single integration boundary for runtime behavior that
  pulls together the node sub-crates.
- Sister tools must not depend on `yggdrasil-node-runtime`; they consume the
  individual sub-crates directly.
- Sub-crates such as `yggdrasil-node-sync` must not depend on
  `yggdrasil-node-runtime`.
- `runtime/tests.rs` carries cross-sub-crate integration scenarios. Move tests
  to `crates/node/cardano-node/tests/` only if they become binary-boundary
  tests rather than reusable runtime tests.

## Naming Parity

Synthesis crate. The `lib.rs` file, extracted from the former monolithic
binary runtime module, carries the `## Naming parity` stanza.

## R-Arc Tracking

The binary's `lib.rs` keeps symbol-level compatibility re-exports for older
callers. New code should import `yggdrasil-node-runtime` directly.
