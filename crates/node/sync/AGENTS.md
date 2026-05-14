# yggdrasil-node-sync — chain-sync + block-fetch driver

## Scope

Wave 5 PR 9 — the highest-risk extraction of the wave. Contains the
node's core chain-sync algorithm + the block-fetch driver + the
multi-peer dispatch logic. Sister tools (`db-truncater`, `db-analyser`,
`db-synthesizer`) only consume the public API surface; the runtime
binary drives the actual sync loop.

Modules:
  - `lib.rs` (former `node/src/sync.rs`, 8,615 LoC): the main
    chain-sync state machine, intersect logic, header verification,
    multi-peer dispatch.
  - `chain_sync` + `block_fetch` + `error` + `shelley_decoders`
    (former `node/src/sync/`): mini-protocol-aligned decomposition
    from R500/R501.
  - `chainsync_worker.rs` + `blockfetch_worker.rs`: per-peer worker
    pool helpers, also from R500/R501.

## Rules — Non-Negotiable

- **R500/R501 layering preserved.** The mini-protocol-aligned
  decomposition (`chain_sync` / `block_fetch` / `error`) reflects
  upstream Ouroboros mini-protocols and MUST stay separate.
- **Bridge to consensus is the public surface.** Anything in
  `yggdrasil-consensus` consumed by sync is via the public crate
  API; sync MUST NOT reach into consensus internals.
- **No reverse dep on runtime / server.** Sync is consumed by the
  runtime sub-loop; it MUST NOT depend on the runtime crate.

## Naming parity

The lib.rs (former `node/src/sync.rs`) carries the `## Naming parity`
stanza. Sub-modules carry their own per-file blocks.

## R-arc tracking

Wave 5 PR 9 (highest-risk extraction). R500/R501 prior decomposition
work is preserved verbatim.
