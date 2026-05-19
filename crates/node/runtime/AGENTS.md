# yggdrasil-node-runtime — orchestration layer

## Scope

Wave 5 PR 12. The glue that wires every other node sub-crate
(config, genesis, tracer, plutus-eval, block-producer, sync) into a
running Cardano node. Equivalent to upstream `Cardano.Node.Run`.

The crate ships the governor loop, mempool tx-submission service,
ledger judgement, peer-management, sync session driver,
block-producer loop, and bootstrap helpers. After this extraction
the `yggdrasil-node` binary's `main.rs` is the only file left in the
binary's `src/` aside from a thin CLI dispatcher.

Sync session positioning is explicit: every freshly connected ChainSync client
must send `MsgFindIntersect`, including `[Origin]`, before `MsgRequestNext`.
Skipping the Origin intersection reproduced mainnet relay disconnects before
any blocks were accepted; keep `sync_session::synchronize_chain_sync_to_point`
aligned with upstream cursor-positioning behavior.

## Rules — Non-Negotiable

- **Single integration boundary.** This crate is the only one that
  pulls together all of the node sub-crates. Sister tools MUST NOT
  depend on `yggdrasil-node-runtime` — they consume the individual
  sub-crates directly.
- **No reverse deps.** Sub-crates (`yggdrasil-node-sync`, etc.)
  MUST NOT depend on `yggdrasil-node-runtime`.
- **Tests live here for now.** runtime/tests.rs (1,938 LoC) carries
  the cross-sub-crate integration scenarios. Future PR may move them
  to `crates/node/yggdrasil-node/tests/` if the integration coverage
  gets large enough to merit a dedicated test crate.

## Naming parity

Synthesis crate. The lib.rs (former node/src/runtime.rs) carries the
`## Naming parity` stanza.

## R-arc tracking

Wave 5 PR 12 — the last extraction in the Wave 5 sequence. PR 13
drops the `pub use` shims in the binary's lib.rs once every consumer
has migrated to direct imports.
